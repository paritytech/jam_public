//! State logic for state transition.
//! Will be use during refinement or by client (builder and chain sync),
//! this is the core of the service.

#![no_std]

extern crate alloc;

pub use blake2b_simd as blake2b;
pub use jam_types::Hash;

pub mod merkle;

mod transition;

pub use transition::{Operations, state_transition, StateOps, Mode};

pub use token_ledger::api::Solicit;

pub type TreeIndex = u16;

// Empty hash is all 0, so we just use `Default` trait for most init.
// Hash of two empty hash is the empty hash and hashing against empty hash
pub const EMPTY_HASH: Hash = [0u8; 32];

// only 15 to be able to index hashes with u16
pub const TREE_DEPTH: usize = 15;

pub fn hash_multiple(m: &[&[u8]]) -> Hash {
    if m.is_empty() || m.iter().map(AsRef::as_ref).all(<[u8]>::is_empty) {
        return EMPTY_HASH;
    }
    let mut hasher = blake2b::State::new();
    for v in m {
        hasher.update(v);
    }
    let mut res = EMPTY_HASH;
    res.copy_from_slice(&hasher.finalize().as_bytes()[0..32]);
    res
}

pub fn hash_pair(h1: &Hash, h2: &Hash) -> Hash {
    if h1 == &[0u8; 32] && h2 == &[0u8; 32] {
        return EMPTY_HASH;
    }
    hash_multiple(&[&h1[..], &h2[..]])
}

pub fn tree_index_from_key(k: &[u8]) -> TreeIndex {
    let hash = hash_multiple(&[k]);
    // u15
    let b: [u8; 2] = [hash[0], hash[1] & (255 << 1)];
    TreeIndex::from_le_bytes(b)
}

/// Hash to include in merkle structure for a given value.
pub trait MerkleValue {
    fn merkle_value(&self) -> Hash;
}

fn hash_sequence<V: MerkleValue>(values: &[V]) -> Hash {
    let mut hasher = blake2b::State::new();
    for v in values {
        hasher.update(v.merkle_value().as_slice());
    }
    let mut res = EMPTY_HASH;
    res.copy_from_slice(&hasher.finalize().as_bytes()[0..32]);
    res
}
