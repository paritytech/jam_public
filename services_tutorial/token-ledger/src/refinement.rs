// Code support for the refinement phase, including data types and expensive computations.
use super::*;

/// Operations that can be submitted to the token ledger
#[derive(Clone, Debug, Encode, Decode)]
pub enum Operation {
	Mint {
		to: AccountId,
		token_id: TokenId,
		amount: u64,
	},
	Transfer {
		from: AccountId,
		to: AccountId,
		token_id: TokenId,
		amount: u64,
	},
}

/// A Refinement Operation with its authorization signature
/// For this tutorial, we will define this authorization as a signature by the hard-coded admin
/// account, covering the encoded operation.
/// In real-world cases, developers might specify a specific format to the signature message.
#[derive(Clone, Debug)]
pub struct SignedOperation {
	pub operation: Operation,
	pub signature: Signature,
}

pub fn verify_signature(op: &refinement::Operation, signature: &Signature, key: VerificationKey) -> Result<(), &'static str> {
	let message = op.encode();
	key
		.verify(&signature, &message)
		.map_err(|_| "Signature verification failed")
}

// Orders a transaction between two parties, so that for both possible directions of transfer,
// we always have the same party first, independently from being the sender or the recipient.
// This allows to keep track of the net balance between two parties over several transfers
// without having to keep track of the direction of each individual transfer.
pub fn canonical_transfer(from: AccountId, to: AccountId, token_id: TokenId, amount: u64) -> ((TokenId, Counterparts), i64) {
	let a = &from;
	let b = &to;
	let (counterparts, amount) = if a < b {
		((*a, *b), amount as i64)
	} else {
		((*b, *a), -(amount as i64))
	};
	((token_id, (counterparts.0, counterparts.1)), amount)
}

