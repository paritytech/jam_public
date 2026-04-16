//! State transition logic for the external client.
//! Functions could be part of state, but we keep it separate
//! to isolate, what is chain logic.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use codec::{Decode, Encode};
use jam_pvm_common::{info, warn};
use jam_types::Hash;
use token_ledger_common::{
    AccountId, Counterparts, Operation, SignedOperation, TokenId, VerificationKey,
    canonical_transfer, verify_signature,
};

/// This is used to exemplify different means of passing data to the service. 
/// The use-case is artificial, but try to cover all the access points we might want to use in a real implementation.
#[derive(Clone, Copy, Debug, Encode, Decode, PartialEq, Eq)]
pub enum DeliveryMode {
    // Directly send witness and operations in the workitem.
    Direct,
    // Send operations in the workitem and witness as an extrinsic.
    Extrinsic,
    // // Witness and operations are read from a preimage.
    // Preimage,
    // // Witness and operations are stored in segment and only processed later.
    // Segment,
    // // Used to trigger a batch of segment processing.
    // ProcessSegments,
}

#[derive(Copy, Clone, Debug, Encode, Decode)]
pub enum ExecutionMode {
    // Execute the operation immediately with the data received in payload and/or extrinsic.
    Immediate,
    // Verify the data received, but do not execute it. Export the verified data to the D3L and defer execution to a later WorkPackage
    Deferring,
    // Read the data stored by a Deferring WorkPackage and complete its execution
    Deferred,
}

pub type Operations = Vec<SignedOperation>;

pub trait StateOps {
    fn known_tokens_contains(&self, token_id: TokenId) -> bool;
    fn known_tokens_push(&mut self, token_id: TokenId);
    fn get_balance(&self, account: AccountId, token_id: TokenId) -> Option<u64>;
    fn set_balance(&mut self, account: AccountId, token_id: TokenId, balance: u64);
    fn root(&self) -> Hash;
}

/// Verifies if the operations are correctly signed and intrinsically correct.
/// This does not check if they're valid in the current state of the chain, 
/// as that can only be done in accumulation, only if their parameters make sense
/// for a valid operation (e.g. transferring a positive value to a different account).
pub fn verify_operations(operations: &Operations) -> bool {
    for op in operations {
        let SignedOperation {
            operation,
            signature,
        } = op;

        match operation {
            Operation::Mint { to: _, token_id: _, amount } => {
                let admin_key: VerificationKey =
                    VerificationKey::try_from(token_ledger_common::admin())
                        .expect("Hard-coded Admin key");

                if verify_signature(&operation, &signature, admin_key).is_err() {
                    warn!("Invalid signature for Mint operation");
                    return false;
                }
                if *amount == 0 {
                    warn!("Mint: Zero amount");
                    return false;
                }
            }
            Operation::Transfer { from, to, token_id: _, amount } => {
                let Ok(signer_key) = VerificationKey::try_from(*from) else {
                    warn!("Invalid 'from' account in transfer operation: {:?}", from);
                    return false;
                };
                if verify_signature(&operation, &signature, signer_key).is_err() {
                    warn!("Invalid signature for Transfer operation");
                    return false;
                }
                if *amount == 0 {
                    warn!("Transfer: Zero amount");
                    return false;
                }
                if from == to {
                    warn!("Transfer: Self-transfer not allowed");
                    return false;
                }
            }
        }
    }
    true
}

/// This function progresses a state with the results of the operations.
/// The state received might not correspond actually to the current state of the chain,
/// but that cannot be known at refinement time and is the responsibility of the accumulation to check.
pub fn state_transition<S: StateOps>(
    state: &mut S,
    operations: &Operations,
) {
    info!("[Refinement] Processing external client state transition.");

    let mut staged_transfers: BTreeMap<(TokenId, Counterparts), i64> = BTreeMap::new();

    for op in operations {
        let SignedOperation {
            operation,
            signature: _,
        } = op;

        match operation {
            Operation::Mint {
                amount,
                to,
                token_id,
            } => {
                process_mint(state, *to, *token_id, *amount)
            }
            Operation::Transfer {
                from,
                to,
                token_id,
                amount,
            } => {
                let transfer = canonical_transfer(*from, *to, *token_id, *amount);
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
            process_transfer(state, from, to, token_id, net_amount as u64);
        } else if net_amount < 0 {
            process_transfer(state, to, from, token_id, (-net_amount) as u64);
        } // if zero, skip
    }
}

fn process_mint<S: StateOps>(state: &mut S, to: AccountId, token_id: TokenId, amount: u64) {
    if state.known_tokens_contains(token_id) {
        warn!("[Refinement] Minting already minted token: {}", token_id);
        return;
    }

    state.known_tokens_push(token_id);

    let current_bal: u64 = state.get_balance(to, token_id).unwrap_or(0);

    let new_bal = current_bal.saturating_add(amount);
    state.set_balance(to, token_id, new_bal);

    info!("[Refinement] Minted {} of token {} to controller account {:?}. New balance: {}",
        amount,
        token_id,
        hex::encode(to),
        new_bal
    );
}

fn process_transfer<S: StateOps>(
    state: &mut S,
    from: AccountId,
    to: AccountId,
    token_id: TokenId,
    amount: u64,
) {
    if !state.known_tokens_contains(token_id) {
        warn!("[Refinement] Trying to transfer unknown token: {}", token_id);
        return;
    }

    let from_bal: u64 = state.get_balance(from, token_id).unwrap_or(0);

    if from_bal < amount {
        warn!(
            "[Refinement] Insufficient balance: account {:?} has {} but tried to send {}",
            hex::encode(from),
            from_bal,
            amount
        );
        return;
    }

    let to_bal: u64 = state.get_balance(to, token_id).unwrap_or(0);

    state.set_balance(from, token_id, from_bal - amount);
    state.set_balance(to, token_id, to_bal.saturating_add(amount));

    info!(
        "[Refinement] Transferred {} of token {} from {:?} to {:?}",
        amount,
        token_id,
        hex::encode(from),
        hex::encode(to)
    );
}
