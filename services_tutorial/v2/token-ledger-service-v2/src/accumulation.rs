//! accumulation

// use alloc::collections::btree_map::BTreeMap;
use alloc::vec::Vec;
use codec::{Decode, Encode};
use jam_pvm_common::accumulate::{get, set};
use jam_pvm_common::{error, info, warn};
use jam_types::AccumulateItem;
use jam_types::{WorkItemRecord, WorkPackageHash};
use token_ledger_state_v2::DeliveryMode;
use token_ledger_state_v2::Hash;
// use token_ledger_state_v2::Solicit;

#[derive(Clone, Debug, Encode, Decode)]
pub struct Operation {
    pub delivery: DeliveryMode,
    pub previous_root: Hash,
    pub new_root: Hash,
    pub exported_segments: Vec<Hash>,
}

// previous item step
struct PendingAccumulateResult {
    // if an error do not attempt to advance next state transition.
    previous_state_root: Result<Option<Hash>, ()>,
}

pub fn on_accumulate_items(items: Vec<AccumulateItem>) {
    let mut items_result = PendingAccumulateResult {
        previous_state_root: Ok(None),
    };

    let mut package_hash = None;
    // Update the state with each transition in the work item, verifying that the operations source state
    // matches the current state.
    for item in items {
        match item {
            AccumulateItem::WorkItem(r) => {
                if let None = package_hash {
                    package_hash = Some(r.package);
                }
                on_work_item_record(
                    r,
                    &mut items_result,
                    package_hash.clone().unwrap_or_default(),
                );
            }
            AccumulateItem::Transfer(_) => {
                info!("[Accumulation (---)] Transfer not used in this example");
                continue;
            }
        }
    }
    jam_pvm_common::accumulate::checkpoint();
    let package_hash = package_hash.unwrap_or_default();

    // after all transitions have been computed, update the JAM state with the new root.
    match items_result.previous_state_root {
        Ok(Some(new_root)) => {
            let current_root: Hash = get("client_root").unwrap_or_default();
            if current_root != new_root {
                set("client_root", new_root).unwrap();
                info!(
                    "[Accumulation {package_hash}] External client state transition success. New root: {:?}",
                    hex::encode(new_root)
                );
            } else {
                info!(
                    "[Accumulation {package_hash}] External client state unchanged. Skipping root update."
                );
            }
        }
        Ok(None) => {
            info!("[Accumulation {package_hash}] External client state unchanged");
        }
        Err(()) => {
            error!("[Accumulation {package_hash}] Mismatch root, skipping all transitions");
        }
    }
}

fn on_work_item_record(
    record: WorkItemRecord,
    acc: &mut PendingAccumulateResult,
    package_hash: WorkPackageHash,
) {
    let output = match &record.result {
        Ok(output) => {
            info!(
                "[Accumulation {package_hash}] Work item record successful: output {:?}",
                output.0
            );
            output
        }
        Err(e) => {
            warn!("[Accumulation {package_hash}]: Work item failed: {:?}", e);
            return;
        }
    };

    let Ok(op) = Operation::decode(&mut &output[..]) else {
        warn!(
            "[Accumulation {package_hash}]: Failed to decode record output as Operation: {:?}",
            output
        );
        return;
    };

    on_work_item_record_single_step(op, &mut acc.previous_state_root, package_hash);
}

fn on_work_item_record_single_step(
    op: Operation,
    acc: &mut Result<Option<Hash>, ()>,
    package_hash: WorkPackageHash,
) {
    if acc.is_err() {
        return;
    }
    info!("[Accumulation {package_hash}] Processing external state transition operations");
    let current_root = if let Ok(Some(r)) = acc {
        *r
    } else {
        get("client_root").unwrap_or_default()
    };
    if op.previous_root == current_root {
        *acc = Ok(Some(op.new_root));
    } else {
        error!("[Accumulation {package_hash}] Mismatch root, skipping all transitions");
        error!("[Accumulation {package_hash}] expected {:?}", current_root);
        error!("[Accumulation {package_hash}] have {:?}", op.previous_root);
        *acc = Err(());
        return;
    }
}
