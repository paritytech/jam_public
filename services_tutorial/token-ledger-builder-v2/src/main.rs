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
                -i --input <FILE> "Input refinement json file"
            )
            .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(
                -o --output <FILE> "Output refinement payload file"
            )
            .value_parser(value_parser!(PathBuf)),
        )
        .arg(
            arg!(
                -t --two  "Two steps use"
            )
            .required(false),
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
    println!("Output: {}", output_path.display());

    if matches.get_flag("two") {
        println!("Running two steps");
    }

    if matches.get_flag("two") {
        println!("Running two steps");
    }

    let mut input = std::fs::File::open(&input_path).unwrap();
    let mut input_vec = Vec::new();
    input.read_to_end(&mut input_vec).unwrap();
    let operations = token_ledger::json::parse_signed_operations(input_vec.as_slice()).unwrap();
    dbg!(operations.len());
    let mut output = std::fs::File::create(&output_path).unwrap();
    let mut opt_db = std::fs::OpenOptions::new();
    opt_db.read(true).write(true);
    let db_path = std::path::PathBuf::new();
    let mut state = token_ledger_builder_v2::state::State::from_db_path(db_path);
    dbg!(state.get_root());
    let version = token_ledger_state_v2::Version::NoParallel;
    token_ledger_state_v2::state_transition(&mut state, &operations, false);
    dbg!(state.get_root());
    let witness = state.take_witness();
    dbg!(&witness);

    let refine_payload = token_ledger_service_v2::RefinePayload {
        version,
        operations,
        witness,
    };
    refine_payload.encode_to(&mut output);
}
