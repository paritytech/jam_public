// This module handles the JSON-related logic for the token ledger service.
use crate::{Operation, Signature, SignedOperation, TokenId};
use alloc::{format, string::String, vec::Vec};
use jam_pvm_common::info;
use serde::{Deserialize, Serialize};

// For the sake of ease of use and demonstration, in this tutorial we let the service
// receive JSON-encoded operation requests as payloads. This may not be very realistic
// nor efficient, but it allows us to easily produce test data.
// As a consequence the service includes a number of types that are only required for
// parsing the requests. In a real-world implementation, developers could consider
// more efficient serialization formats, thereby avoiding these structures.

/// JSON representation of basic human-friendly operation data,
/// without signatures or real AccountIds. Instead, accounts are 
/// specified by u64 seeds, from which we derive a valid cryptographic Keypair,
/// so we can have access to the private key and with it produce signatures as needed.
/// Using seeds everywhere in these operations also allows users to reason over the accounts
/// to manually produce a consistent test scenario.
/// We stress this is only used to produce test cases for demonstration, and should not be used in production.
#[derive(Debug, Deserialize)]
pub enum UnsignedOperationJson {
    Mint {
        token_id: TokenId,
        amount: u64,
        to_seed: u64,
    },
    Transfer {
        token_id: TokenId,
        amount: u64,
        from_seed: u64,
        to_seed: u64,
    },
}

/// JSON representation of full operation data, ready for encoding and
/// submission to a service. 
#[derive(Debug, Deserialize, Serialize)]
pub enum SignedOperationJson {
    Mint {
        token_id: TokenId,
        amount: u64,
        to: String,
        signature: String,
    },
    Transfer {
        token_id: TokenId,
        amount: u64,
        from: String,
        to: String,
        signature: String,
    },
}


/// Parse JSON payload containing an array of unsigned operations
pub fn parse_unsigned_operations(json_bytes: &[u8]) -> Result<Vec<UnsignedOperationJson>, String> {
    info!("Parsing JSON payload of {} bytes", json_bytes.len());
    let ops = serde_json::from_slice::<Vec<UnsignedOperationJson>>(json_bytes)
        .map_err(|e| format!("Failed to parse JSON array: {}", e))?;

    info!("Identified JSON array with {} unsigned operations", ops.len());

    Ok(ops)
}

/// Parse JSON payload containing an array of fully specified signed operations.
pub fn parse_signed_operations(json_bytes: &[u8]) -> Result<Vec<SignedOperation>, String> {
    info!("Parsing JSON payload of {} bytes", json_bytes.len());
    let ops = serde_json::from_slice::<Vec<SignedOperationJson>>(json_bytes)
        .map_err(|e| format!("Failed to parse JSON array: {}", e))?;

    info!("Identified JSON array with {} signed operations", ops.len());

    let operations = ops
        .into_iter()
        .map(|json_op| match json_op {
            SignedOperationJson::Mint {
                to,
                token_id,
                amount,
                signature,
            } => Ok(SignedOperation {
                operation: Operation::Mint {
                    to: decode_account(&to)?,
                    token_id,
                    amount,
                },
                signature: decode_signature(&signature)?,
            }),
            SignedOperationJson::Transfer {
                from,
                to,
                token_id,
                amount,
                signature,
            } => Ok(SignedOperation {
                operation: Operation::Transfer {
                    from: decode_account(&from)?,
                    to: decode_account(&to)?,
                    token_id,
                    amount,
                },
                signature: decode_signature(&signature)?,
            }),
        })
        .collect::<Result<Vec<SignedOperation>, String>>()?;

    Ok(operations)
}

/// Decode a hex string to a fixed-size byte array, accepting the length parameter
fn decode_hex<const N: usize>(s: &str) -> Result<[u8; N], String> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != N * 2 {
        return Err(format!(
            "Invalid hex length: expected {} characters, got {}",
            N * 2,
            s.len()
        ));
    }
    let mut result = [0u8; N];
    for i in 0..N {
        let byte_str = &s[i * 2..(i + 1) * 2];
        result[i] = u8::from_str_radix(byte_str, 16)
            .map_err(|_| format!("Invalid hex character at position {}", i * 2))?;
    }
    Ok(result)
}

/// Decode a hex string to a 32-byte account
fn decode_account(s: &str) -> Result<[u8; 32], String> {
    decode_hex::<32>(s).map_err(|e| format!("Invalid account hex: {}", e))
}

/// Decode a hex string to a 64-byte signature
fn decode_signature(s: &str) -> Result<Signature, String> {
    decode_hex::<64>(s)
        .map(|bytes| ed25519_consensus::Signature::from(bytes))
        .map(|sig| Signature(sig))
        .map_err(|e| format!("Invalid signature hex: {}", e))
}

