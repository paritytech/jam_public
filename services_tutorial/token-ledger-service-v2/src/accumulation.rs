//! accumulation

use alloc::collections::btree_map::BTreeMap;
use alloc::vec::Vec;
use codec::{Decode, Encode};
use jam_pvm_common::accumulate::{get, set};
use jam_pvm_common::{error, info, warn};
use jam_types::AccumulateItem;
use jam_types::WorkItemRecord;
use token_ledger_state_v2::Hash;
use token_ledger_state_v2::Mode;
use token_ledger_state_v2::Solicit;

#[derive(Clone, Debug, Encode, Decode)]
pub struct Operation {
    pub version: Mode,
    pub previous_root: Hash,
    pub new_root: Hash,
    pub to_solicit: Vec<Solicit>,
    pub exported_segments: Vec<Hash>,
    pub processed_segments: Vec<Hash>,
}

// previous item step
struct ItemAccumulate {
    // if an error do not attempt to advance next state transition.
    previous_state_root: Result<Option<Hash>, ()>,
}

pub fn on_accumulate_items(items: Vec<AccumulateItem>) {
    let mut items_result = ItemAccumulate {
        previous_state_root: Ok(None),
    };

    for item in items {
        info!("Accumulate processing work item record");
        match item {
            AccumulateItem::WorkItem(r) => on_work_item_record(r, &mut items_result),
            AccumulateItem::Transfer(_) => {
                info!("Transfer not used in this example");
                continue;
            }
        }
    }

    // single step resolve
    match items_result.previous_state_root {
        Ok(Some(new_root)) => {
            // TODO manage single step error
            set("client_root", new_root).unwrap();
            info!("External client state transition success");
        }
        Ok(None) => {
            info!("External client state unchanged");
        }
        Err(()) => {
            error!("Mismatch root, skipping all transition");
        }
    }

    // TODO resolve two step on top of Vec conflict or first success
}

fn on_work_item_record(record: WorkItemRecord, acc: &mut ItemAccumulate) {
    info!(
        "Accumulate processing work item record: package {:?}",
        record.package
    );
    let output = match &record.result {
        Ok(output) => output,
        Err(e) => {
            warn!("Work item failed: {:?}", e);
            return;
        }
    };

    let Ok(op) = Operation::decode(&mut &output[..]) else {
        warn!("Failed to decode validated operation");
        return;
    };

    let version = op.version;
    on_work_item_record_single_step(op, &mut acc.previous_state_root, version);
    jam_pvm_common::accumulate::checkpoint();
}

fn on_work_item_record_single_step(
    op: Operation,
    acc: &mut Result<Option<Hash>, ()>,
    version: Mode,
) {
    if acc.is_err() {
        return;
    }
    info!("Processing external state transition operations");
    let current_root = if let Ok(Some(r)) = acc {
        *r
    } else {
        get("client_root").unwrap_or_default()
    };
    if op.previous_root == current_root {
        *acc = Ok(Some(op.new_root));
    } else {
        error!("Mismatch root, skipping all transition");
        error!("expected {:?}", current_root);
        error!("have {:?}", op.previous_root);
        *acc = Err(());
        return;
    }

    match version {
        Mode::Preimage => {
            for solicit in op.to_solicit {
                // check in refine In real code we should not have
                // on_root field at accumulation level.
                assert!(op.previous_root == solicit.on_root);
                if let Err(e) =
                    jam_pvm_common::accumulate::solicit(&solicit.hash, solicit.len as usize)
                {
                    error!("Could not solicit preimage: {:?}, {:?}", solicit.hash, e);
                } else {
                    info!(
                        "Preimage {:?} of len {} has been sollicited",
                        solicit.hash, solicit.len
                    );
                }
            }
        }
        Mode::ProcessSegments | Mode::Segment => {
            // tracking segment so we could attach a proof that a segment is in accumulation for a
            // while in refinement before using import. At this point we just import directly without
            // checks.
            if op.exported_segments.len() > 0 || op.processed_segments.len() > 0 {
                // We store hash of segment content with a reference count.
                let mut buffed_segments: BTreeMap<Hash, u64> =
                    get("buffed_segments").unwrap_or(BTreeMap::new());
                info!("acc loaded buffed of size {}", buffed_segments.len());
                for p in &op.processed_segments {
                    let mut rem_seg = false;
                    if let Some(rc) = buffed_segments.get_mut(p) {
                        *rc -= 1;
                        if *rc == 0 {
                            rem_seg = true;
                        }
                    } else {
                        error!("Non buffered segment {} was refined, dropping all", hex::encode(p));
                        *acc = Err(());
                        return;
                    }
                    if rem_seg {
                        buffed_segments.remove(p);
                    }
                }
                for p in op.exported_segments {
                    info!("Acc exported {}", hex::encode(&p));
                    if let Some(rc) = buffed_segments.get_mut(&p) {
                        *rc += 1;
                    } else {
                        buffed_segments.insert(p, 1);
                    }
                }
                // TODO properly handle error (especially for tutorial)
                set("buffed_segments", &buffed_segments).unwrap();
                info!("Acc updated buffed of size {}", buffed_segments.len());
            }
        }
        Mode::Direct => (),
    }
}
