//! accumulation

use alloc::vec::Vec;
use codec::{Decode, Encode};
use jam_pvm_common::accumulate::{get, set};
use jam_pvm_common::{error, info, warn};
use jam_types::AccumulateItem;
use jam_types::WorkItemRecord;
use token_ledger_state_v2::Hash;

#[derive(Clone, Debug, Encode, Decode)]
pub struct Operation {
    pub version: token_ledger_state_v2::Version,
    pub previous_root: Hash,
    pub new_root: Hash,
}

// previous item step
struct ItemAccumulate {
    // trace single step version hash from previous calls.
    single_step: Result<Option<Hash>, ()>,
    // two step persistence: TODO first item package and conflict list Vec<workitem_preimages>
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
            set("external_client_root_single_step", new_root).unwrap();
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

    match op.version {
        token_ledger_state_v2::Version::NoParallel => {
            on_work_item_record_single_step(op, &mut acc.single_step);
        }
        token_ledger_state_v2::Version::TwoStepParallel => unimplemented!(),
    }
}

fn on_work_item_record_single_step(op: Operation, acc: &mut Result<Option<Hash>, ()>) {
    if acc.is_err() {
        return;
    }
    info!("Processing external state transition operations");
    let current_root = if let Ok(Some(r)) = acc {
        *r
    } else {
        get("external_client_root_single_step").unwrap_or_default()
    };
    if op.previous_root == current_root {
        *acc = Ok(Some(op.new_root));
    } else {
        error!("Mismatch root, skipping all transition");
        *acc = Err(());
    }
}
