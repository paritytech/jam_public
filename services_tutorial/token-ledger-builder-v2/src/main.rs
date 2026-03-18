//! client exposed operations.
//! - opening state from file and others.
//! - produce refinement payload from json.

use clap::{arg, command, value_parser, ArgAction, Command};
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
                -t --two  "Two steps use"
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

    let version = if matches.get_flag("two") {
        dbg!("Running two steps");
        token_ledger_state_v2::Version::TwoStepParallel
    } else {
        token_ledger_state_v2::Version::NoParallel
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
    let mut state = token_ledger_builder_v2::state::State::from_db_path(db_path, overload_head);
    println!("Initial root: {}", hex::encode(state.get_root()));
    token_ledger_state_v2::state_transition(&mut state, &operations, false);
    let witness = state.take_witness();
    println!("Post execution root: {}", hex::encode(state.get_root()));
    dbg!(&witness);

    let refine_payload = token_ledger_service_v2::RefinePayload {
        version,
        operations,
        witness,
    };
    refine_payload.encode_to(&mut output);
}
