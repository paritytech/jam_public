//! client exposed operations.
//! - opening state from file and others.
//! - produce refinement payload from json.

use clap::{arg, command, value_parser};
use codec::Encode;
use std::env;
use std::io::Read;
use std::path::PathBuf;

//const HELP: &str = {
//    "Build a refinement payload:
//		input_json_file_path output_payload_file_path
//		balance.db and "
//};

fn main() {
    let matches = command!() // requires `cargo` feature
        .arg(
            arg!(
							[input]  "Input refinement json file"
            )
            .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(
               [output] "Output refinement payload file"
            )
            .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(
                --preimage  "Use a preimage"
            )
            .required(false),
        )
        .arg(
            arg!(
                --segment  "Use a segment"
            )
            .required(false),
        )
        .arg(
            arg!(
							--head <String> "Overload root hash for this state transition (rather than using db head)"
            )
        )
 
        //.subcommand(
        //    Command::new("test")
        //        .about("does testing things")
        //        .arg(arg!(-l --list "lists test values").action(ArgAction::SetTrue)),
        //)
        .get_matches();

    let Some(input_path) = matches.get_one::<PathBuf>("input") else {
			println!("Missing input param");
				return;
    };
    println!("Input: {}", input_path.display());

    let Some(output_path) = matches.get_one::<PathBuf>("output") else {
			println!("Missing output param");
				return;
    };

		let mut overload_head: Option<jam_types::Hash> = None;
    if let Some (head_str) = matches.get_one::<String>("head")  {
			let hash = hex::decode(head_str).unwrap();
			overload_head = Some(hash.try_into().unwrap());
    }
 
    println!("Output: {}", output_path.display());

		let preimage_steps = matches.get_flag("preimage");
		let with_segments = matches.get_flag("segment");
    let version = if preimage_steps {
					if with_segments {
			println!("Either segment or preimage");
				return;
					}
        dbg!("Running preimage steps");
        token_ledger_state_v2::Mode::Preimage
		} else if with_segments {
        dbg!("Running segment steps");
        token_ledger_state_v2::Mode::Segment
    } else {
        dbg!("Running direct steps");
        token_ledger_state_v2::Mode::Direct
    };

    let mut input = std::fs::File::open(&input_path).unwrap();
    let mut input_vec = Vec::new();
    input.read_to_end(&mut input_vec).unwrap();
    let operations = token_ledger::json::parse_signed_operations(input_vec.as_slice()).unwrap();
    dbg!(operations.len());
    let mut output = std::fs::File::create(&output_path).unwrap();
    let mut opt_db = std::fs::OpenOptions::new();
    opt_db.read(true).write(true);
    let db_path = std::path::PathBuf::new();
    let mut state = token_ledger_builder_v2::state::State::from_db_path(db_path.clone(), overload_head);
    println!("Initial root: {}", hex::encode(state.get_root()));
    let _ = token_ledger_state_v2::state_transition(&mut state, &operations, false);
    let witness = state.take_witness();
    println!("Post execution root: {}", hex::encode(state.get_root()));
    dbg!(&witness);

    let refine_payload = token_ledger_service_v2::RefinePayload {
        version,
        operations,
        witness,
    };
    refine_payload.encode_to(&mut output);

		if preimage_steps {

    std::mem::drop(output);

    let mut output = std::fs::File::open(&output_path).unwrap();
			let mut data = Vec::new();
			output.read_to_end(&mut data).unwrap();
			let hash_r =  blake2b_simd::Params::new().hash_length(32).hash(&data);
			let mut hash: [u8; 32] = [0;32];
			hash.copy_from_slice(hash_r.as_bytes());
			let len = data.len() as u64;
		
			let mut state = token_ledger_builder_v2::state::State::from_db_path(db_path, overload_head);
			let mut operations: Vec<token_ledger::api::SignedOperation> = Vec::with_capacity(1);

			operations.push(token_ledger::api::SignedOperation {
				// Dummy, unchecked in tutorial
				signature: token_ledger::api::Signature([0; 64].into()),
				operation: token_ledger::api::Operation::Solicit(token_ledger::api::Solicit {
					on_root: state.get_root(),
					hash: hash.into(), len,
				}),
			});
			let _ = token_ledger_state_v2::state_transition(&mut state, &operations, false);
			// only root as we only check right root for solicit
    let witness = state.take_witness();
    let refine_payload = token_ledger_service_v2::RefinePayload {
        version,
        operations,
        witness,
    };
		let mut prep_path = output_path.clone();
		prep_path.set_extension("prepare");
    let mut output = std::fs::File::create(&prep_path).unwrap();
    refine_payload.encode_to(&mut output);
		}
}
