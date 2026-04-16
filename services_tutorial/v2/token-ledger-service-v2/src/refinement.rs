//! refinement

use alloc::vec::Vec;
use codec::{Decode, Encode};
use jam_pvm_common::{ApiError, error, info};
use token_ledger_state_v2::{Hash, DeliveryMode, ExecutionMode, Operations, merkle::{State, Witness}, hash_multiple, state_transition, verify_operations};
use jam_pvm_common::refine;
use jam_types::WorkPackageHash;

#[derive(Encode, Decode)]
pub struct Payload {
    pub delivery: DeliveryMode,
    pub execution: ExecutionMode,
    pub operations: Operations,
    pub witness: Option<Witness>,
}

#[derive(Encode, Decode)]
pub struct SegmentData {
    pub operations: Operations,
    pub witness: Witness,
}

pub fn refine_payload(payload: &[u8], package_hash: WorkPackageHash) -> (Vec<u8>, usize) {
    let mut exported_segments = Vec::new();

    let Some((previous_root, new_root, operations_len, delivery)) = refine_transition(
        payload,
        package_hash,
        &mut exported_segments,
    ) else {
        return (Vec::new().into(), 0)
    };
    
    info!(
        "[Refinement {package_hash}] Refinement done. Root {:?} -> {:?} --- {} operations, delivery mode {:?}",
        hex::encode(previous_root),
        hex::encode(new_root),
        operations_len,
        delivery
    );
    let output = 
    (
        crate::accumulation::Operation {
            delivery,
            previous_root,
            new_root,
            exported_segments,
        },
        operations_len,
    );

    (output.0.encode(), output.1)
}

fn refine_transition(
    mut payload: &[u8],
    package_hash: WorkPackageHash,
    exported_segments: &mut Vec<Hash>,
) -> Option<(Hash, Hash, usize, DeliveryMode)> {

    let Payload {
        delivery,
        execution, 
        operations,
        witness,
    } = match Payload::decode(&mut payload) {
        Ok(ops) => ops,
        Err(e) => {
            error!("[Refinement {package_hash}] Failed to parse signed operations: {}", e);
            return None
        }
    };
    
    let operations_len = operations.len();
    match execution {
        ExecutionMode::Immediate | ExecutionMode::Deferring => {

            let witness = match delivery {
                DeliveryMode::Extrinsic => {
                    // In this mode, we should not have a witness in the RefinePayload, but we have to read it from extrinsics instead.
                    assert!(witness.is_none());
                    let extrinsic = refine::extrinsic(0);
                    let Some(extrinsic_data) = extrinsic.as_ref() else {
                        error!("[Refinement {package_hash}] No extrinsic data found");
                        return None
                    };
                    info!("[Refinement {package_hash}] Found extrinsic data with {} bytes", extrinsic_data.len());

                    let witness: Witness = match Decode::decode(&mut &extrinsic_data[..]) {
                        Ok(t) => t,
                        Err(e) => {
                            error!("[Refinement {package_hash}] Failed to decode witness from extrinsic: {:?}", e);
                            return None
                        }
                    };

                    witness
                }
                DeliveryMode::Direct => {
                    assert!(witness.is_some());
                    witness.unwrap()
                }
            };

            let Some(mut partial_state) = State::from_witness(witness.clone())
            else {
                error!("[Refinement {package_hash}] Error loading state from witness");
                return None
            };

            info!("[Refinement {package_hash}] Loaded state from witness: {}", partial_state);
            let previous_root = partial_state.get_root();

            if !verify_operations(&operations) {
                error!("[Refinement {package_hash}] Invalid operations, skipping execution");
                return None
            }

            match execution {
                ExecutionMode::Immediate => {
                    info!("[Refinement {package_hash}] Executing operations in immediate mode");
                    state_transition(&mut partial_state, &operations);
                    let new_root = partial_state.get_root();
                    return Some((previous_root, new_root, operations_len, delivery))
                }
                ExecutionMode::Deferring => {
                    info!("[Refinement {package_hash}] Exporting data for later work-package");
                        let segment_data = SegmentData {
                            operations,
                            witness,
                        };
                        let segment_slice = segment_data.encode();

                        match jam_pvm_common::refine::export_slice(segment_slice.as_slice()) {
                            Ok(exported) => {
                                let exported_hash = hash_multiple(&[segment_slice.as_slice()]);
                                info!(
                                    "[Refinement {package_hash}] Inserted segment with hash {}, at index {}",
                                    hex::encode(&exported_hash),
                                    exported
                                );
                                exported_segments.push(exported_hash);
                            }
                            Err(ApiError::StorageFull) => {
                                error!("[Refinement {package_hash}] cannot add segment, storage full, ignoring");
                                return None
                            }
                            Err(e) => {
                                error!("[Refinement {package_hash}] fail pushing segment {:?}",  e);
                                return None
                            }
                        }
                    return Some((previous_root, previous_root, operations_len, delivery)) // in this case we return the same root because we have not executed the operations yet
                }
                ExecutionMode::Deferred => {
                    unreachable!("[Refinement {package_hash}] In this context, we have verified to be only Immediate or Deferring");
                }
            }
        },
        ExecutionMode::Deferred => {

            match jam_pvm_common::refine::import(0) { // We should have a single segment imported
                Some(segment) => {
                    info!("[Refinement {package_hash}] Loading transition from segment {:?}", segment);
                    let segment_data: SegmentData = match Decode::decode(&mut &segment.as_slice()[..]) {
                        Ok(t) => t,
                        Err(e) => {
                            error!("[Refinement {package_hash}] Failed to decode operations and state from segment: {:?}", e);
                            return None
                        }
                    };

                    let Some(mut partial_state) = State::from_witness(segment_data.witness)
                    else {
                        error!("[Refinement {package_hash}] error loading state from witness in deferred execution");
                       return None
                    };

                    info!("[Refinement {package_hash}] Loaded state from witness: {}", partial_state);
                    let previous_root = partial_state.get_root();

                    state_transition::<State>(&mut partial_state, &segment_data.operations);
                    let new_root = partial_state.get_root();

                    return Some((previous_root, new_root, segment_data.operations.len(), delivery))
                }
                None => {
                    error!("[Refinement {package_hash}] No segment found for deferred execution");
                    return None
                },
            }
        }
    }


}

#[test]
fn encode_process_payload() {
    let process_payload = Payload {
        delivery: DeliveryMode::ProcessSegments,
        execution: ExecutionMode::Deferred,
        operations: Default::default(),
        witness: Default::default(),
    };
    let encoded = process_payload.encode();
    let hex_encoded = hex::encode(&encoded);

    assert_eq!(hex_encoded, "0300000000");

    hex::decode(&hex_encoded).unwrap();
    Payload::decode(&mut encoded.as_slice()).unwrap();
}
