//! refinement

use alloc::vec::Vec;
use codec::{Decode, Encode};
use jam_pvm_common::{error, info};

#[derive(Encode, Decode)]
pub struct Payload {
    pub version: token_ledger_state_v2::Version,
    pub operations: token_ledger_state_v2::Operations,
    pub witness: token_ledger_state_v2::merkle::Witness,
}

pub fn refine_payload(mut payload: &[u8]) -> (Vec<u8>, usize) {
    let Payload {
        version,
        operations,
        witness,
    } = match Payload::decode(&mut payload) {
        Ok(ops) => ops,
        Err(e) => {
            error!("Failed to parse signed operations: {}", e);
            return (Vec::new(), 0);
        }
    };

    info!("witness {:?}", &witness);
    let operations_len = operations.len();
    info!(
        "read payload of size {}, with {} operations",
        payload.len(),
        operations_len
    );
    let opt_partial_state = token_ledger_state_v2::merkle::State::from_witness(witness);
    if opt_partial_state.is_none() {
        error!("error loading state");
        unimplemented!("TODO error report in work output ?");
    }
    let mut partial_state = opt_partial_state.unwrap();
    info!("loaded state from witness");
    let previous_root = partial_state.get_root();
    info!("from root: {:?}", previous_root);
    let to_solicit = token_ledger_state_v2::state_transition(&mut partial_state, &operations, false);
    let new_root = partial_state.get_root();
    info!("to root: {:?}", new_root);

    (
        crate::accumulation::Operation {
            version,
            previous_root,
            new_root,
						to_solicit,
        }
        .encode(),
        operations_len,
    )
}
