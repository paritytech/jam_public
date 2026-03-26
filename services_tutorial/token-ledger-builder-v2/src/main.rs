//! client exposed operations.
//! - opening state from file and others.
//! - produce refinement payload from json.

use clap::{arg, command, value_parser};
use codec::Encode;
use std::env;
use std::io::Read;
use std::path::PathBuf;
use std::fs::File;
use token_ledger_service_v2::RefinePayload;
use token_ledger_builder_v2::state::State;
use token_ledger_state_v2::{
    Hash,
    merkle::Witness, 
};
use token_ledger_common::{Operation, Signature, SignedOperation, Solicit};

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
               [output] "Output refinement payload file. Optional if --connect_rpc is used, otherwise required"
            )
            .required(false)
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
                --connect_rpc  "Connect to a running RPC node. Submit work-packages directly to it instead of writing payload to file"
            )
            .required(false),
        )
        .arg(
            arg!(
			    --head <String> "Starting state root hash for this state transition, if undefined, latest written state is used (referenced in HEAD file)" 
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

    let connect_rpc = matches.get_flag("connect_rpc");

    let output_path = matches.get_one::<PathBuf>("output");
    if !connect_rpc && output_path.is_none() {
        println!("Missing output param, or use --connect_rpc to submit directly to a running node");
        return;
    }

	let mut overload_head: Option<Hash> = None;
    if let Some (head_str) = matches.get_one::<String>("head")  {
        let hash = hex::decode(head_str).unwrap();
        overload_head = Some(hash.try_into().unwrap());
    }

    if let Some(output_path) = output_path {
        println!("Output: {}", output_path.display());
    }

    let preimage_steps = matches.get_flag("preimage");
    let with_segments = matches.get_flag("segment");
    if preimage_steps && with_segments {
        println!("Incompatible options selected: 'segment' and 'preimage' should not be specified together");
        return;
    }    
    let version = if preimage_steps {
        dbg!("Running preimage steps");
        token_ledger_state_v2::Mode::Preimage
	} else if with_segments {
        dbg!("Running segment steps");
        token_ledger_state_v2::Mode::Segment
    } else {
        dbg!("Running direct steps");
        token_ledger_state_v2::Mode::Direct
    };

    let operations = read_ops_from_file(&input_path);

    if let Some(output_path) = output_path {
        println!("Output: {}", output_path.display());
    }

    let db_path = std::path::PathBuf::new();
    let witness = compute_transition_witness(&db_path, overload_head, &operations);
    
    let refine_payload = RefinePayload {
        version,
        operations,
        witness,
    };

    if let Some(output_path) = output_path {

        // Create the output file. In direct mode, this is the end result.
        // In preimage mode, we use this to compute a hash, and then
        // include it as the corresponding pre-image to a Solicit operation.

        let output = export_direct_payload(&output_path, refine_payload);

		if preimage_steps {
            std::mem::drop(output); 
            export_preimage_payload(output_path, db_path, overload_head, version);
		}
    } else {
        println!("No output file specified, skipping writing payload to file");
    }
}

fn export_direct_payload(output_path: &PathBuf, refine_payload: RefinePayload) -> File {
    let mut output = std::fs::File::create(&output_path).unwrap();
    refine_payload.encode_to(&mut output);
    output
}

fn export_preimage_payload(
        output_path: &PathBuf, 
        db_path: PathBuf, 
        overload_head: Option<Hash>, 
        version: token_ledger_state_v2::Mode
    ) {
    println!("Processing with pre-image steps");

    let (hash, len) = compute_payload_hash(&output_path);

    println!("Preimage hash: {}. Preimage length: {}", hex::encode(hash), len);

    let mut state = State::from_db_path(db_path, overload_head);
    let mut operations: Vec<SignedOperation> = Vec::with_capacity(1);

    operations.push(SignedOperation {
        // Dummy, unchecked in tutorial
        signature: Signature([0; 64].into()),
        operation: Operation::Solicit(Solicit {
            on_root: state.get_root(),
            hash: hash.into(), len,
        }),
    });

    let _ = token_ledger_state_v2::state_transition(&mut state, &operations, false);
    // only root as we only check right root for solicit
    let witness = state.take_witness();
    let solicit_payload = RefinePayload {
        version,
        operations,
        witness,
    };
    let mut prep_path = output_path.clone();
    prep_path.set_extension("prepare");
    let mut output = std::fs::File::create(&prep_path).unwrap();
    solicit_payload.encode_to(&mut output);
}

fn read_ops_from_file(path: &PathBuf) -> Vec<token_ledger_common::SignedOperation> {
    let mut input = std::fs::File::open(&path).unwrap();
    let mut input_vec = Vec::new();
    input.read_to_end(&mut input_vec).unwrap();
    let operations = token_ledger_common::json::parse_signed_operations(input_vec.as_slice()).unwrap();
    dbg!(operations.len());
    operations
}

fn compute_transition_witness(db_path: &std::path::PathBuf, overload_head: Option<Hash>, operations: &Vec<SignedOperation>) -> Witness {
    let mut opt_db = std::fs::OpenOptions::new();
    opt_db.read(true).write(true);
    let mut state = State::from_db_path(db_path.clone(), overload_head);
    println!("Initial root: {}", hex::encode(state.get_root()));
    let _ = token_ledger_state_v2::state_transition(&mut state, operations, false);
    let witness = state.take_witness();
    println!("Post execution root: {}", hex::encode(state.get_root()));
    // dbg!(&witness);
    print_debug(&witness);
    witness
}

fn compute_payload_hash(file_path: &PathBuf) -> (Hash, u64) {
    let mut payload_file = std::fs::File::open(&file_path).unwrap();
    let mut data = Vec::new();
    payload_file.read_to_end(&mut data).unwrap();
    println!("Read {} bytes from file {}", data.len(), file_path.display());
    let hash_r =  blake2b_simd::Params::new().hash_length(32).hash(&data);
    let mut hash: Hash = [0;32];
    hash.copy_from_slice(hash_r.as_bytes());
    let len = data.len() as u64;
    (hash, len)
}

pub fn print_debug(witness: &Witness) {
    println!("Witness:");
    println!("  Hashes:");
    for (index, hash) in witness.hashes.iter() {
        println!("    {}: {}", index, hex::encode(hash));
    }
    println!("  Key value balances:");
    for (key, value) in witness.key_value_balances.iter() {
        println!("    {}: {}", hex::encode(key), value);
    }
    println!("  Token ids:");
    for token_id in witness.token_ids.iter() {
        println!("    {}", token_id);
    }
}
