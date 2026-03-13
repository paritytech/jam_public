//! client exposed operations.
//! - opening state from file and others.
//! - produce refinement payload from json.

use codec::Encode;
use std::env;
use std::io::Read;

const HELP: &str = {
    "Build a refinement payload: 
		input_json_file_path output_payload_file_path
		balance.db and "
};

fn main() {
    let args: Vec<String> = env::args().collect();
    dbg!(args.clone());
    if args.len() != 3 || &args[1] == "--help" {
        println!("{}", HELP);
        return;
    }

    let mut input = std::fs::File::open(&args[1]).unwrap();
    let mut input_vec = Vec::new();
    input.read_to_end(&mut input_vec).unwrap();
    let operations = token_ledger::json::parse_signed_operations(input_vec.as_slice()).unwrap();
    dbg!(operations.len());
    let mut output = std::fs::File::create(&args[2]).unwrap();
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
