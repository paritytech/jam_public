// This module handles the JSON-related logic for the token ledger service.
use ed25519_consensus::Signature;
use serde::Deserialize;
use super::*;
use crate::refinement::SignedOperation;

/// For the sake of ease of use and demonstration, in this tutorial we let the service
/// receive JSON-encoded operation requests as payloads. This may not be very realistic
/// nor efficient, but it allows us to easily produce test data.
/// As a consequence the service includes a number of types that are only required for
/// parsing the requests. In a real-world implementation, developers could consider
/// more efficient serialization formats, thereby avoiding these structures.

/// JSON representation of essential operation data
#[derive(Debug, Deserialize)]
enum OperationJson {
	Mint {
		token_id: TokenId,
		amount: u64,
		to: String,
		signature: String
	},
	Transfer {
		token_id: TokenId,
		amount: u64,
		from: String,
		to: String,
		signature: String
	},
}

/// Parse JSON payload containing an array of signed operations
pub fn parse_signed_operations(json_bytes: &[u8]) -> Result<Vec<SignedOperation>, String> {
	info!("Parsing JSON payload of {} bytes", json_bytes.len());
	let ops = serde_json::from_slice::<Vec<OperationJson>>(json_bytes)
		.map_err(|e| format!("Failed to parse JSON array: {}", e))?;

	info!("Identified JSON array with {} signed operations", ops.len());

    let operations = ops.into_iter().map(|json_op| {
        match json_op {
            OperationJson::Mint { to, token_id, amount, signature } => {
                Ok(SignedOperation {
                    operation: refinement::Operation::Mint {
                        to: decode_account(&to)?,
                        token_id,
                        amount,
                    }, 
                    signature: decode_signature(&signature)?,
                })
            },
            OperationJson::Transfer { from, to, token_id, amount, signature } => {
                Ok(SignedOperation {
                    operation: refinement::Operation::Transfer {
                        from: decode_account(&from)?,
                        to: decode_account(&to)?,
                        token_id,
                        amount,
                    },
                    signature: decode_signature(&signature)?,
                })
            },
        }
    }).collect::<Result<Vec<SignedOperation>, String>>()?;

    Ok(operations)
}

/// Decode a hex string to a fixed-size byte array, accepting the length parameter
fn decode_hex<const N: usize>(s: &str) -> Result<[u8; N], String> {
	let s = s.strip_prefix("0x").unwrap_or(s);
	if s.len() != N * 2 {
		return Err(format!("Invalid hex length: expected {} characters, got {}", N * 2, s.len()));
	}
	let mut result = [0u8; N];
	for i in 0..N {
		let byte_str = &s[i * 2..(i + 1) * 2];
		result[i] = u8::from_str_radix(byte_str, 16).map_err(|_| format!("Invalid hex character at position {}", i * 2))?;
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
        .map(|bytes| Signature::from(bytes))
        .map_err(|e| format!("Invalid signature hex: {}", e))
}   

// fn parse_operations(json_bytes: &[u8]) {
//     let data = match serde_json::from_slice::<Vec<Operation>>(json_bytes) {
//         Ok(data) => data,
//         Err(e) => {
//             eprintln!("✗ Failed to parse as list of Operation Data: {}", e);
//             return;
//         }
//     };
//     println!("✓ Successfully parsed list of Operation Data ({} elements): {:?}", data.len(), data);
// }

// /// Parse a single JSON SignedOperation object
// fn parse_signed_operation_json(
// 	json_data: SignedOperationJson,
// ) -> Result<SignedOperation, &'static str> {
// 	let operation = match json_data.operation {
// 		OperationJson::Mint { to, token_id, amount } => {
// 			RefinementOperation::Mint(MintRequest { to, token_id, amount })
// 		},
// 		OperationJson::Transfer { from, to, token_id, amount } => {
// 			RefinementOperation::Transfer(TransferRequest { from, to, token_id, amount })
// 		},
// 	};

// 	Ok(SignedOperation { operation, signature: json_data.signature })
// }
