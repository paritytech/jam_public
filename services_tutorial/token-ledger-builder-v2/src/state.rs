//! client state is simply using the shared state code, and
//! adding to it a simple persistence system  (unsafe, breaks on any crash), and always register witnesses.

use std::cell::RefCell;
use std::collections::BTreeMap;

use codec::{Decode, Encode};
use std::io::{Read, Seek, Write};
use token_ledger::api::{AccountId, TokenId};
use token_ledger_state_v2::merkle::{
    Balance, MerkleTree, Value, ValueTraits, Witness, TREE_HASHES,
};
use token_ledger_state_v2::{
    hash_pair, tree_index_from_key, Hash, MerkleValue, StateOps, TreeIndex, EMPTY_HASH, TREE_DEPTH,
};

#[derive(Default)]
pub struct State {
    balances: StateTree<Balance>,
    known_tokens: KnownTokens,
    persist: Option<std::path::PathBuf>,
}

impl State {
    fn read_head(mut db_location: std::path::PathBuf) -> Hash {
        db_location.push("HEAD");

        if let Ok(mut db_file) = std::fs::File::open(db_location) {
            let mut hash_str = String::new();
            db_file.read_to_string(&mut hash_str).unwrap();
            let hash_vec = hex::decode(hash_str).unwrap();
            let mut hash = EMPTY_HASH;
            hash.copy_from_slice(&hash_vec);
            hash
        } else {
            EMPTY_HASH
        }
    }

    pub fn from_db_path(db_location: std::path::PathBuf, head: Option<Hash>) -> Self {
        let head_hash = if let Some(h) = head {
            h
        } else {
            Self::read_head(db_location.clone())
        };
        if head_hash != EMPTY_HASH {
            let mut state_path = db_location.clone();
            state_path.push(&hex::encode(&head_hash));
            let mut db_file = std::fs::File::open(state_path).unwrap();
            let mut reader = codec::IoReader(&mut db_file);
            State {
                balances: StateTree::<Balance>::from_stream(&mut reader),
                known_tokens: KnownTokens::from_stream(&mut reader),
                persist: Some(db_location),
            }
        } else {
            let mut state = State::default();
            state.set_new_persist_file(db_location);
            state
        }
    }

    pub fn set_new_persist_file(&mut self, db_location: std::path::PathBuf) {
        self.persist = Some(db_location);
    }

    pub fn take_witness(&mut self) -> Witness {
        let (hashes, values) = self.balances.take_witness();
        return Witness {
            hashes: hashes.into_iter().collect(),
            key_value_balances: values.into_iter().collect(),
            token_ids: self.known_tokens.take_witness(),
        };
    }

    pub fn from_witness(witness: Witness) -> Option<Self> {
        let mut result = Self::default();
        if let Some(balances) =
            StateTree::init_from_witness(&witness.hashes, witness.key_value_balances)
        {
            result.balances = balances;
        } else {
            return None;
        }

        result.known_tokens.merkle.token_ids = witness.token_ids;

        // init balances witness tokerns id initial hashes
        let _ = result.take_witness();

        return Some(result);
    }

    pub fn get_root(&self) -> Hash {
        return hash_pair(
            self.balances.root(),
            &self.known_tokens.merkle.merkle_value(),
        );
    }

    pub fn serialize(&mut self) {
        let Some(db_location) = self.persist.clone() else {
            return;
        };
        let hash = self.get_root().clone();
        let mut state_path = db_location.clone();
        state_path.push(&hex::encode(&hash));
        if let Ok(_) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&state_path)
        {
            // ignore already a state with tihs content
        } else {
            let mut file = std::fs::File::create_new(&state_path).unwrap();
            file.seek(std::io::SeekFrom::Start(0)).unwrap();
            self.balances.serialize(&mut file);
            self.known_tokens.serialize(&mut file);
            file.flush().unwrap();
        }
        // update head
        let mut head_path = db_location.clone();
        head_path.push("HEAD");
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&head_path)
        {
            file.write_all(hex::encode(&hash).as_bytes()).unwrap();
            file.flush().unwrap();
        } else {
            let mut file = std::fs::File::create_new(&head_path).unwrap();
            file.write_all(hex::encode(&hash).as_bytes()).unwrap();
            file.flush().unwrap();
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        self.serialize();
    }
}

impl StateOps for State {
    fn get_balance(&self, account: AccountId, token_id: TokenId) -> Option<u64> {
        let to_key = token_ledger::api::balance_key(token_id, &account);
        self.balances.get(to_key.as_slice()).cloned()
    }

    fn set_balance(&mut self, account: AccountId, token_id: TokenId, balance: u64) {
        let to_key = token_ledger::api::balance_key(token_id, &account);
        if !self.balances.set(to_key.to_vec(), balance) {
            unimplemented!("error on key collision");
        }
    }

    fn known_tokens_contains(&self, token_id: TokenId) -> bool {
        // not registering witness: always pass all tokens to witness
        self.known_tokens.merkle.token_contains(token_id)
    }

    fn known_tokens_push(&mut self, token_id: TokenId) {
        self.known_tokens.merkle.push_token(token_id)
    }
}

#[derive(Default)]
pub struct Tree {
    merkle: MerkleTree,
    initial_hashes: BTreeMap<TreeIndex, Hash>,
    witness: RefCell<BTreeMap<TreeIndex, Hash>>,
}

impl Tree {
    fn get_hash(&self, ix: TreeIndex) -> &Hash {
        let hash = self.merkle.get_hash(ix);
        if hash == &EMPTY_HASH {
            return hash;
        }

        let witness_hash = self.initial_hashes.get(&ix).unwrap_or(&EMPTY_HASH);
        if witness_hash != &EMPTY_HASH {
            self.witness.borrow_mut().insert(ix, *witness_hash);
        }

        return hash;
    }

    fn root(&self) -> &Hash {
        return self.get_hash((TREE_HASHES - 1) as TreeIndex);
    }

    // expect ix of a value
    // Note that if state is modified, we do not need to record,
    // but since this state is append only and modify read existing
    // value, this would just not change the witness state.
    fn record_witness_access(&self, ix: TreeIndex) {
        let mut at = ix;
        let mut offset: TreeIndex = 0;
        for depth in 0..TREE_DEPTH {
            if at % 2 == 0 {
                self.get_hash(offset + at + 1);
            } else {
                self.get_hash(offset + at - 1);
            }
            offset += 1 << (TREE_DEPTH - depth);
            at = at / 2;
        }
    }

    pub fn insert(&mut self, ix: TreeIndex, value_hash: Hash) {
        self.record_witness_access(ix);
        self.merkle.insert(ix, value_hash);
    }

    fn take_witness(&mut self) -> BTreeMap<TreeIndex, Hash> {
        let hashes = std::mem::replace(
            self.witness.get_mut(),
            BTreeMap::<TreeIndex, Hash>::default(),
        );
        // update for next run
        self.initial_hashes = self.merkle.hashes.clone();
        hashes
    }
}

struct StateTree<V: ValueTraits> {
    // TODO rem (TreeIndex is always hashextract of key...), yet avoid checking for existing key
    indexes: BTreeMap<Vec<u8>, TreeIndex>,
    values: BTreeMap<TreeIndex, Value<V>>,
    tree: Tree,
    witness_values: RefCell<BTreeMap<Vec<u8>, V>>,
}

impl<V: ValueTraits> Default for StateTree<V> {
    fn default() -> Self {
        Self {
            indexes: Default::default(),
            values: Default::default(),
            tree: Default::default(),
            witness_values: Default::default(),
        }
    }
}

impl<V: ValueTraits> StateTree<V> {
    fn from_stream<R: codec::Input>(buf_reader: &mut R) -> Self {
        let mut result = Self::default();
        let nb_item = u64::decode(buf_reader).unwrap();
        dbg!("loading {} items", nb_item);

        for _ in 0..nb_item {
            let v = Value::<V>::decode(buf_reader).unwrap();
            result.set(v.key, v.value);
        }
        result.tree.initial_hashes = result.tree.merkle.hashes.clone();
        result
    }
    fn serialize<W: Write>(&mut self, w: &mut W) {
        dbg!("serializing {} items", self.values.len());
        (self.values.len() as u64).encode_to(w);
        for (_, v) in self.values.iter() {
            v.encode_to(w);
        }
    }

    fn get_value(&self, k: &[u8]) -> Option<&Value<V>> {
        let Some(i) = self.indexes.get(k) else {
            {
                let ix = tree_index_from_key(&k);
                self.tree.record_witness_access(ix);
            }
            return None;
        };

        let v = self.values.get(i);
        {
            if let Some(value_v) = v.as_ref() {
                if !self.witness_values.borrow().contains_key(&value_v.key) {
                    self.witness_values
                        .borrow_mut()
                        .insert(value_v.key.clone(), value_v.value.clone());
                }
            }
            self.tree.record_witness_access(*i);
        }
        return v;
    }

    pub fn get(&self, k: &[u8]) -> Option<&V> {
        let v = self.get_value(k)?;
        if v.key.as_slice() != k {
            return None;
        };
        Some(&v.value)
    }

    // fail on key collision by returning false
    pub fn set(&mut self, k: Vec<u8>, v: V) -> bool {
        let ix = tree_index_from_key(&k);
        if let Some(existing) = self.values.get_mut(&ix) {
            if existing.key.as_slice() == k {
                existing.value = v;
                self.tree.insert(ix, existing.merkle_value());
            } else {
                let _ = self.get(k.as_slice()); // register witness (we access value to check key).
                return false;
            }
        } else {
            let value = Value { key: k, value: v };
            self.tree.insert(ix, value.merkle_value());
            self.indexes.insert(value.key.clone(), ix);
            self.values.insert(ix, value);
        }
        true
    }

    pub fn root(&self) -> &Hash {
        self.tree.root()
    }

    fn init_from_witness(
        witness_hashes: &[(TreeIndex, Hash)],
        witness_key_values: Vec<(Vec<u8>, V)>,
    ) -> Option<Self> {
        let token_ledger_state_v2::merkle::StateTree {
            tree,
            values,
            indexes,
        } = token_ledger_state_v2::merkle::StateTree::init_from_witness(
            witness_hashes,
            witness_key_values,
        )?;

        Some(Self {
            indexes,
            values,
            tree: Tree {
                initial_hashes: tree.hashes.clone(),
                witness: Default::default(),
                merkle: tree,
            },
            witness_values: Default::default(),
        })
    }

    fn take_witness(&mut self) -> (BTreeMap<TreeIndex, Hash>, BTreeMap<Vec<u8>, V>) {
        let hashes = self.tree.take_witness();
        let values = std::mem::replace(
            self.witness_values.get_mut(),
            BTreeMap::<Vec<u8>, V>::default(),
        );

        (hashes, values)
    }
}

#[derive(Default)]
pub struct KnownTokens {
    merkle: token_ledger_state_v2::merkle::KnownTokens,
    witness: Vec<TokenId>,
}

impl KnownTokens {
    fn from_stream<R: codec::Input>(buf_reader: &mut R) -> Self {
        let mut result = Self::default();
        result.merkle.token_ids = Decode::decode(buf_reader).unwrap();
        result.witness = result.merkle.token_ids.clone();
        result
    }

    fn serialize<W: Write>(&mut self, w: &mut W) {
        let encoded = self.merkle.token_ids.encode();
        dbg!("serializing {} token bytes", encoded.len());
        w.write_all(encoded.as_slice()).unwrap();
    }

    fn take_witness(&mut self) -> Vec<TokenId> {
        std::mem::replace(&mut self.witness, self.merkle.token_ids.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state() {
        let empty: State = Default::default();

        assert_eq!([0; 32], empty.get_root());
    }
    #[test]
    fn check_constants() {
        assert_eq!(TREE_DEPTH + 1, std::mem::size_of::<TreeIndex>() * 8);
    }
    #[test]
    fn create_token_and_distribute() {
        let mut state: State = Default::default();
        assert!([0; 32] == state.get_root());

        state.set_balance([1; 32], 1, 10);

        assert!([0; 32] != state.get_root());

        let witness_1 = state.take_witness();
        let mut state2 = State::from_witness(witness_1).unwrap();
        state2.set_balance([1; 32], 1, 10);
        assert_eq!(state.get_root(), state2.get_root());

        assert_eq!(Some(10), state.get_balance([1; 32], 1));
        // get balance witness
        let witness_2 = state.take_witness();
        let state3 = State::from_witness(witness_2).unwrap();
        assert_eq!(state.get_root(), state3.get_root());
        assert_eq!(Some(10), state3.get_balance([1; 32], 1));

        // insert token and insert another value
        state.set_balance([8; 32], 2, 32);
        state.known_tokens_push(1);
        state.known_tokens_push(2);
        let witness_3 = state.take_witness();

        let mut state4 = State::from_witness(witness_3).unwrap();
        assert_eq!(state3.get_root(), state4.get_root());
        state4.set_balance([8; 32], 2, 32);
        state4.known_tokens_push(1);
        state4.known_tokens_push(2);
        assert_eq!(state.get_root(), state4.get_root());

        // test serialize
        let dir = tempfile::tempdir().unwrap();
        let dir_path = dir.keep();
        state.set_new_persist_file(dir_path.clone());
        let root_bef_ser = state.get_root();
        core::mem::drop(state);

        let state_from_ser = State::from_db_path(dir_path);

        assert_eq!(root_bef_ser, state_from_ser.get_root());
    }
}
