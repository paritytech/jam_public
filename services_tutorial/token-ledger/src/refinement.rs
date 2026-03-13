// Code support for the refinement phase, including data types and expensive computations.
use crate::api::{Counterparts, TokenId};
use alloc::{collections::BTreeMap, vec::Vec};
use codec::Encode;
use jam_pvm_common::{error, info, warn};
use jam_types::{ServiceId, WorkOutput, WorkPackageHash, WorkPayload};

// ledger api directly used by refine.
pub use crate::api::{
    Operation, SignedOperation, VerificationKey, canonical_transfer, verify_signature,
};

pub fn refine(
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
    info!(
        "TokenLedger refine on service {service_id:x}h for package/item {package_hash} / {item_index}"
    );

    // Parse the incoming payload as a JSON array of signed operations
    let operations: Vec<SignedOperation> = match crate::json::parse_signed_operations(&payload) {
        Ok(ops) => ops,
        Err(e) => {
            error!("Failed to parse signed operations: {}", e);
            return Vec::new().into();
        }
    };

    let mut validated: Vec<crate::accumulation::ValidatedOperation> = Vec::new();

    for signed_op in operations {
        let SignedOperation {
            operation,
            signature,
        } = signed_op;

        match operation {
            Operation::Mint { amount, .. } => {
                let admin_key: VerificationKey =
                    VerificationKey::try_from(crate::api::admin()).expect("Hard-coded Admin key");

                if verify_signature(&operation, &signature, admin_key).is_err() {
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
                validated.push(crate::accumulation::ValidatedOperation(operation));
            }
            Operation::Transfer {
                from,
                to,
                token_id,
                amount,
            } => {
                let Ok(signer_key) = VerificationKey::try_from(from) else {
                    warn!("Invalid 'from' account in transfer operation: {:?}", from);
                    continue;
                };
                if verify_signature(&operation, &signature, signer_key).is_err() {
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
                let transfer = canonical_transfer(from, to, token_id, amount);
                staged_transfers
                    .entry(transfer.0)
                    .and_modify(|e| *e += transfer.1)
                    .or_insert(transfer.1);
            }
        }
    }
    for entries in staged_transfers {
        let ((token_id, (from, to)), net_amount) = entries;
        if net_amount > 0 {
            validated.push(crate::accumulation::ValidatedOperation(
                Operation::Transfer {
                    from,
                    to,
                    token_id,
                    amount: net_amount as u64,
                },
            ));
        } else if net_amount < 0 {
            validated.push(crate::accumulation::ValidatedOperation(
                Operation::Transfer {
                    from: to,
                    to: from,
                    token_id,
                    amount: (-net_amount) as u64,
                },
            ));
        } // if zero, skip
    }

    info!(
        "TokenLedger refine: Validated {} operations for accumulation",
        validated.len()
    );

    // Encode and return for accumulation
    let encoded = validated.encode();
    info!("Refinement total output size: {} bytes", encoded.len());
    encoded.into()
}
