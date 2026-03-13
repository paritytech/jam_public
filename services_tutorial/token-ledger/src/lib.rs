#![cfg_attr(any(target_arch = "riscv32", target_arch = "riscv64"), no_std)]

/// This is a simple implementation of an example, as part of a tutorial on how to build services
/// for JAM. Although it demonstrates the basic concepts and techniques, it should is not
/// production-ready and should not be used as is in production.
extern crate alloc;

use jam_pvm_common::{Service, accumulate, declare_service, info};
use jam_types::{
    AccumulateItem, CoreIndex, Hash, ServiceId, Slot, WorkOutput, WorkPackageHash, WorkPayload,
};

pub mod api;
pub mod json;

mod accumulation;
mod refinement;

/// The Token Ledger Service
pub struct TokenLedger;
declare_service!(TokenLedger);

impl Service for TokenLedger {
    fn refine(
        _core_index: CoreIndex,
        item_index: usize,
        service_id: ServiceId,
        payload: WorkPayload,
        package_hash: WorkPackageHash,
    ) -> WorkOutput {
        refinement::refine(item_index, service_id, payload, package_hash)
    }

    fn accumulate(slot: Slot, service_id: ServiceId, item_count: usize) -> Option<Hash> {
        info!("TokenLedger accumulate on service {service_id:x}h @{slot} with {item_count} items");

        for item in accumulate::accumulate_items() {
            info!("Accumulate processing work item record");
            match item {
                AccumulateItem::WorkItem(r) => accumulation::on_work_item(r),
                AccumulateItem::Transfer(t) => accumulation::on_transfer(t),
            }
        }

        None
    }
}

pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
