//! refinement

use alloc::vec::Vec;
use codec::{Decode, Encode};
use jam_pvm_common::refine::lookup;
use jam_pvm_common::{error, info};
use token_ledger_state_v2::Hash;
use token_ledger_state_v2::Version;

#[derive(Encode, Decode)]
pub struct Payload {
    pub version: Version,
    pub operations: token_ledger_state_v2::Operations,
    pub witness: token_ledger_state_v2::merkle::Witness,
}

pub fn refine_payload(payload: &[u8]) -> (Vec<u8>, usize) {
    let mut to_solicit = Vec::new();
    let mut exported_segments = Vec::new();
    let mut processed_segments = Vec::new();
    let (previous_root, new_root, operations_len, version) =
        refine_transition(payload, &mut to_solicit, &mut exported_segments, true, true);
    info!("to root: {:?}", new_root);
    (
        crate::accumulation::Operation {
            version,
            previous_root,
            new_root,
            to_solicit,
            exported_segments,
            processed_segments,
        }
        .encode(),
        operations_len,
    )
}

fn refine_transition(
    mut payload: &[u8],
    to_solicit: &mut Vec<token_ledger_state_v2::Solicit>,
    exported_segment: &mut Vec<u64>,
    allow_preimage: bool,
    export_segment: bool,
) -> (Hash, Hash, usize, Version) {
    let Payload {
        version,
        operations,
        witness,
    } = match Payload::decode(&mut payload) {
        Ok(ops) => ops,
        Err(e) => {
            error!("Failed to parse signed operations: {}", e);
            // TODO noops but should not forward this to accumulate
            return (Default::default(), Default::default(), 0, Version::Direct);
        }
    };

    info!("witness {:?}", &witness);
    let mut operations_len = operations.len();
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

    if version == Version::Segment {
        // put in segment

        if export_segment {
            // here we should large payload put in multiple segments, but for tutorial we only use one and panic when payload too big.
            let exported = jam_pvm_common::refine::export_slice(payload).unwrap();
            exported_segment.push(exported);
        }

        // TODO import payloads and call refine transition on them with export_segmonet false.

        return (previous_root, previous_root, operations_len, version);
    }

    let transition_result =
        token_ledger_state_v2::state_transition(&mut partial_state, &operations, false);

    let mut new_root = partial_state.get_root();
    for solicit in transition_result.to_solicit {
        if solicit.on_root != previous_root {
            error!(
                "Skip a solicit preimage on non current root: {:?}, {:?}",
                solicit.hash, solicit.on_root
            );
            continue;
        }
        info!("looking up preimage");
        if let Some(preimage) = jam_pvm_common::refine::lookup(&solicit.hash) {
            info!("got  preimage");
            if !allow_preimage {
                continue;
            }
            info!("loading transition from preimage");
            let (proot, nroot, ops, _) =
                refine_transition(&preimage, to_solicit, exported_segment, false, false);

            // Note this force to run preimage in sequence at this point
            if proot != new_root {
                error!("processing preimage witness fail due to updated root");
            }
            new_root = nroot;
            operations_len += ops;
        } else {
            // solicit
            info!("solliciting  preimage");
            to_solicit.push(solicit);
        }
    }

    return (previous_root, new_root, operations_len, version);
}
