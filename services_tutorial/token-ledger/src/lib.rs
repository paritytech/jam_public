#![cfg_attr(any(target_arch = "riscv32", target_arch = "riscv64"), no_std)]

/// This is a simple implementation of an example, as part of a tutorial on how to build services
/// for JAM. Although it demonstrates the basic concepts and techniques, it should is not
/// production-ready and should not be used as is in production.
extern crate alloc;

use alloc::{string::String, vec::Vec};
use alloc::{format, collections::BTreeMap};

use codec::{Decode, Encode};
use ed25519_consensus::{Signature, VerificationKey, VerificationKeyBytes};
use jam_pvm_common::{accumulate, declare_service, error, info, warn, Service};
use jam_types::{
	AccumulateItem, CoreIndex, Hash, ServiceId, Slot, TransferRecord, WorkItemRecord, WorkOutput,
	WorkPackageHash, WorkPayload,
};

mod refinement;
mod accumulation;
// An auxiliary module for handling JSON-encoded data.
mod json;

use refinement::SignedOperation;

/// For demonstration only: a hard-coded admin account that must authorise every operation submitted
/// to the service. In a real-world scenario, developers must decide on a proper authorization
/// layer, deciding who can authorize operations and how the service would manage their identities
/// and keys.
fn admin() -> VerificationKeyBytes {
	[0_u8; 32].into()
}

/// The Token Ledger Service
pub struct TokenLedger;
declare_service!(TokenLedger);

/// A unique identifier for a token type
pub type TokenId = u32;

/// An account identifier (32-byte public key)
pub type AccountId = [u8; 32];

/// A set of two counterparts of a single transfer. 
/// There is no implication of who the sender is, because we 
/// want to accumulate several operations, in either direction,
/// between the same accounts.
/// The final net balance will determine the resultant transfer direction.
type Counterparts = (AccountId, AccountId);

impl Service for TokenLedger {
	fn refine(
		_core_index: CoreIndex,
		item_index: usize,
		service_id: ServiceId,
		payload: WorkPayload,
		package_hash: WorkPackageHash,
	) -> WorkOutput {
		// TODO: by casting a transfer's u64 to i64, to preserve the direction, 
		// we are reducing 2-fold the effective maximum amount. This can be dealt in a few different ways,
		// be it by keeping track of a direction flag, or even by keeping one cumulative balance
		// for each possible direction. For now, we just assume that all transfers are up to i64::MAX.
		let mut staged_transfers: BTreeMap<(TokenId, Counterparts), i64> = BTreeMap::new();
		info!("TokenLedger refine on service {service_id:x}h for package/item {package_hash} / {item_index}");

		// Parse the incoming payload as a JSON array of signed operations
		let operations: Vec<refinement::SignedOperation> = match json::parse_signed_operations(&payload) {
			Ok(ops) => ops,
			Err(e) => {
				error!("Failed to parse signed operations: {}", e);
				return Vec::new().into();
			},
		};

		let mut validated: Vec<accumulation::Operation> = Vec::new();

		for signed_op in operations {
			let SignedOperation { operation, signature } = signed_op;

			match operation {
				refinement::Operation::Mint {to, token_id, amount } => {
		
					let admin_key: VerificationKey =	
						VerificationKey::try_from(admin()).expect("Hard-coded Admin key");

					if refinement::verify_signature(&operation, &signature, admin_key).is_err() {
						warn!("Invalid signature for operation");

						// For the sake of the tutorial, and ease of use, we don't reject if the signature
						// is invalid. We do compute the verification here to show that expensive
						// computation should go in refine(). But skipping actual validation frees us from 
						// having to create actual signatures when passing test data to the service.
					}

					if amount == 0 {
						warn!("Mint: Zero amount");
						continue;
					}
					validated.push(accumulation::Operation::Mint {
						to,
						token_id,
						amount,
					});
				},
				refinement::Operation::Transfer { from, to, token_id, amount } => {
					let Ok(signer_key) =	
						VerificationKey::try_from(from) else {
							warn!("Invalid 'from' account in transfer operation: {:?}", from);
							continue;
						};
					if refinement::verify_signature(&operation, &signature, signer_key).is_err() {
						warn!("Invalid signature for operation");

						// For the sake of the tutorial, and ease of use, we don't reject if the signature
						// is invalid. We do compute the verification here to show that expensive
						// computation should go in refine(). But skipping actual validation frees us from 
						// having to create actual signatures when passing test data to the service.
					}

					// Validate transfer request
					if amount == 0 {
						warn!("Transfer: Zero amount");
						continue;
					}
					if from == to {
						warn!("Transfer: Self-transfer not allowed");
						continue;
					}
					let transfer = refinement::canonical_transfer(from, to, token_id, amount);
					staged_transfers.entry(transfer.0)
						.and_modify(|e| *e += transfer.1)
						.or_insert(transfer.1);
				},
			}
		}
		for entries in staged_transfers {
			let ((token_id, (from, to)), net_amount) = entries;
			if net_amount > 0 {
				validated.push(accumulation::Operation::Transfer {
					from,
					to,
					token_id,
					amount: net_amount as u64,
				});
			} else if net_amount < 0 {
				validated.push(accumulation::Operation::Transfer {
					from: to,
					to: from,
					token_id,
					amount: (-net_amount) as u64,
				});
			} // if zero, skip
		}

		info!("TokenLedger refine: Validated {} operations for accumulation", validated.len());

		// Encode and return for accumulation
		let encoded = validated.encode();
		info!("Refinement total output size: {} bytes", encoded.len());
		encoded.into()
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
