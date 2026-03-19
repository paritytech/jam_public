//! accumulation

use alloc::collections::btree_set::BTreeSet;
use alloc::vec::Vec;
use codec::{Decode, Encode};
use jam_pvm_common::accumulate::{get, set};
use jam_pvm_common::{error, info, warn, ApiError};
use jam_types::AccumulateItem;
use jam_types::WorkItemRecord;
use token_ledger_state_v2::Hash;
use token_ledger_state_v2::Solicit;
use token_ledger_state_v2::Version;

#[derive(Clone, Debug, Encode, Decode)]
pub struct Operation {
    pub version: Version,
    pub previous_root: Hash,
    pub new_root: Hash,
    pub to_solicit: Vec<Solicit>,
    pub exported_segments: Vec<u64>,
    pub processed_segments: Vec<u64>,
}

// previous item step
struct ItemAccumulate {
    // trace single step version hash from previous calls.
    single_step: Result<Option<Hash>, ()>,
    // preimage persistence: TODO unused, would need better separation with single_step.
    two_step: (),
}

pub fn on_accumulate_items(items: Vec<AccumulateItem>) {
    let mut items_result = ItemAccumulate {
        single_step: Ok(None),
        two_step: (),
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
    match items_result.single_step {
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
    on_work_item_record_single_step(op, &mut acc.single_step, version);
    jam_pvm_common::accumulate::checkpoint();
}

fn on_work_item_record_single_step(
    op: Operation,
    acc: &mut Result<Option<Hash>, ()>,
    version: Version,
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
        Version::Preimage => {
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
        Version::ProcessSegments | Version::Segment => {
            // tracking segment so we could attach a proof that a segment is in accumulation for a
            // while in refinement before using import. At this point we just import directly without
            // checks.
            if op.exported_segments.len() > 0 || op.processed_segments.len() > 0 {
                // TODO this should be index by (WorkpackageHash, ix)
                let mut buffed_segments: BTreeSet<u64> =
                    get("buffed_segments").unwrap_or(BTreeSet::new());
                info!("acc loaded buffed of size {}", buffed_segments.len());
                for p in &op.processed_segments {
                    if !buffed_segments.remove(p) {
                        error!("Non buffered segment {} was refined, dropping all", p);
                        *acc = Err(());
                        return;
                    }
                }
                for p in op.exported_segments {
                    info!("Acc exported {}", p);
                    buffed_segments.insert(p);
                }
                // TODO properly handle error (especially for tutorial)
                set("buffed_segments", &buffed_segments).unwrap();
                info!("Acc updated buffed of size {}", buffed_segments.len());
                for b in &buffed_segments {
                    info!("Curr buffed: {b}");
                }
            }
        }
        Version::Direct => (),
    }
}
