#![no_std]

//! This is a simple implementation of an example, as part of a tutorial on how to build services
//! for JAM. Although it demonstrates the basic concepts and techniques, it should is not
//! production-ready and should not be used as is in production.
//!
//! No error management is provided, and no consistency guarantees for persistence as this
//! targets example/tutorial focused on others aspects.
//!
//! Building client or running tests requires to manually set "std" feature.

extern crate alloc;

use jam_pvm_common::{Service, accumulate, declare_service, info};
use jam_types::{CoreIndex, Hash, ServiceId, Slot, WorkOutput, WorkPackageHash, WorkPayload};

pub use refinement::Payload as RefinePayload;
mod accumulation;
mod refinement;

/// The Token Ledger Service
pub struct TokenLedgerExternalClient;
declare_service!(TokenLedgerExternalClient);

impl Service for TokenLedgerExternalClient {
    fn refine(
        _core_index: CoreIndex,
        item_index: usize,
        service_id: ServiceId,
        payload: WorkPayload,
        package_hash: WorkPackageHash,
    ) -> WorkOutput {
        info!(
            "TokenLedger refine on service {service_id:x}h for package/item {package_hash} / {item_index}"
        );

        let (encoded, operations_len) = refinement::refine_payload(payload.0.as_slice());

        info!("Refinement done over {} operations", operations_len);
        encoded.into()
    }

    fn accumulate(slot: Slot, service_id: ServiceId, item_count: usize) -> Option<Hash> {
        info!("TokenLedger accumulate on service {service_id:x}h @{slot} with {item_count} items");

        crate::accumulation::on_accumulate_items(accumulate::accumulate_items());
        None
    }
}

pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
