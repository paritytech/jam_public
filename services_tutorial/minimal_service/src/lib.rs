#![cfg_attr(any(target_arch = "riscv32", target_arch = "riscv64"), no_std)]

extern crate alloc;
use alloc::format;
use jam_pvm_common::*;
use jam_types::*;

#[allow(dead_code)]
struct Service;
declare_service!(Service);

impl jam_pvm_common::Service for Service {
	fn refine(
		_core_index: CoreIndex,
		_item_index: usize,
		service_id: ServiceId,
		payload: WorkPayload,
		_package_hash: WorkPackageHash,
	) -> WorkOutput {
		info!(
			"This is Refine in the Minimal Service {service_id:x}h with payload len {}",
			payload.len()
		);
		[&b"Hello "[..], payload.take().as_slice()].concat().into()
	}
	fn accumulate(slot: Slot, id: ServiceId, item_count: usize) -> Option<Hash> {
		info!("This is Accumulate in the Minimal Service {id:x}h with {} items", item_count);
		for item in accumulate::accumulate_items() {
			match item {
				AccumulateItem::WorkItem(w) =>
					if let Ok(out) = w.result {
						accumulate::set_storage(b"last", &out).expect("balance low");
					},
				AccumulateItem::Transfer(t) => {
					let msg = format!(
						"Transfer at {slot} from {:x}h to {id:x}h of {} memo {}",
						t.source, t.amount, t.memo,
					);
					info!("{}", msg);
					accumulate::set_storage(b"lasttx", msg.as_bytes()).expect("balance low");
				},
			}
		}
		None
	}
}

pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
