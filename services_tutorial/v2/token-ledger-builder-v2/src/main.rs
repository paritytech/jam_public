//! client exposed operations.
//! - opening state from file and others.
//! - produce refinement payload from json.

use clap::{arg, command, value_parser};
use codec::Encode;
use std::path::Path;

use bytes::Bytes;
use jam_std_common::Service;
use jam_std_common::{Node, NodeError, NodeExt, hash_raw};
use jam_tooling::CommonArgs;
use jam_types::{
    AuthConfig, Authorization, Authorizer, CodeHash, CoreIndex, ExtrinsicSpec, HeaderHash,
    ImportSpec, RefineContext, RootIdentifier, Slot, VALS_PER_CORE, ValIndex, WorkItem,
    WorkPackage, WorkPackageHash, max_accumulate_gas, max_refine_gas, val_count,
};
use jsonrpsee::ws_client::WsClient;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use token_ledger_builder_v2::state::State;
use token_ledger_common::SignedOperation;
use token_ledger_service_v2::RefinePayload;
use token_ledger_state_v2::{DeliveryMode, ExecutionMode};
use token_ledger_state_v2::{Hash, merkle::Witness, state_transition};

const BASE_NODE_PORT: u16 = 19800;
const DEFAULT_NODE_INDEX: ValIndex = 0;
const DEFAULT_CORE: CoreIndex = 0;
const BOOTSTRAP_SERVICE_ID: u32 = 0;

fn main() {
    // Assumes we are running against the polkajam-testnet,
    // in the tiny config
    let parameters = jam_types::ProtocolParameters::tiny();
    parameters
        .apply()
        .expect("Built-in parameters should always be valid; qed");

    println!(
        "Set tiny config. Val count: {}, Vals per core: {}, Core count: {}",
        val_count(),
        VALS_PER_CORE,
        val_count() / VALS_PER_CORE as CoreIndex
    );

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
                --extrinsic  "Send the witness to the service as an extrinsic, and not in the WorkItem payload"
            )
            .required(false),
        )
        .arg(
            arg!(
                --"connect-rpc"  "Connect to a running RPC node. Submit work-packages directly to it instead of writing payload to file"
            )
            .required(false),
        )
        .arg(
            arg!(
                --service <SERVICE> "Required if --connect-rpc is used. Hex-string of the service ID (eg. 0x1234abcd)"
            )
            .required(false)
            .value_parser(value_parser!(String)),
        )
        .arg(
            arg!(
			    --head <String> "Starting state root hash for this state transition, if undefined, latest written state is used (referenced in HEAD file)" 
            )
        )
        .arg(
            arg!(
                --port <PORT> "Specify the port number for the RPC connection"
            )
            .value_parser(value_parser!(u16))
            .required(false),
        )
        .get_matches();

    let Some(input_path) = matches.get_one::<PathBuf>("input") else {
        println!("Missing input param");
        return;
    };
    println!("Input: {}", input_path.display());

    let connect_rpc = matches.get_flag("connect-rpc");

    let rpc_port = if connect_rpc {
        Some(
            matches
                .get_one::<u16>("port")
                .copied()
                .unwrap_or(BASE_NODE_PORT + DEFAULT_NODE_INDEX),
        )
    } else {
        None
    };

    let output_path = matches.get_one::<PathBuf>("output");
    if !connect_rpc && output_path.is_none() {
        println!("Missing output param, or use --connect_rpc to submit directly to a running node");
        return;
    }

    let mut opt_service: Option<u32> = None;
    if connect_rpc {
        if matches.get_one::<String>("service").is_none() {
            println!("Missing required --service param when using --connect_rpc");
            return;
        }

        opt_service = matches
            .get_one::<String>("service")
            .map(|s| parse_service_id_hex(s).unwrap());
    }

    let mut override_head: Option<Hash> = None;
    if let Some(head_str) = matches.get_one::<String>("head") {
        let hash = hex::decode(head_str).unwrap();
        override_head = Some(hash.try_into().unwrap());
    }

    let extrinsic_mode = matches.get_flag("extrinsic");

    let preimage_steps = matches.get_flag("preimage");
    let with_segments = matches.get_flag("segment");
    if preimage_steps && with_segments {
        println!(
            "Incompatible options selected: 'segment' and 'preimage' should not be specified together"
        );
        return;
    }

    let operations = read_ops_from_file(input_path);

    let db_path = std::path::PathBuf::new();
    let witness = compute_transition_witness(&db_path, override_head, &operations);

    let delivery = if extrinsic_mode {
        dbg!("Submitting in extrinsic mode");
        DeliveryMode::Extrinsic
    } else {
        dbg!("Submitting in direct mode");
        DeliveryMode::Direct
    };

    let connection_details = connect_rpc.then(|| ConnectionDetails {
        rpc_port: rpc_port.expect("Must be defined if connect_rpc is true"),
        service_id: opt_service.expect("Must be defined if connect_rpc is true"),
    });

    if !with_segments {
        dbg!("Submitting package with Immediate execution");
        let (payload, extrinsics) = match delivery {
            DeliveryMode::Extrinsic => {
                println!(
                    "Submitting in extrinsic mode, witness will be sent as extrinsic data in the first package"
                );
                (
                    RefinePayload {
                        delivery,
                        execution: ExecutionMode::Immediate,
                        operations: operations.clone(),
                        witness: None,
                    },
                    Some(witness),
                )
            }
            DeliveryMode::Direct => {
                println!(
                    "Submitting in direct mode, witness will be included in WorkItem payload of the first package"
                );
                (
                    RefinePayload {
                        delivery,
                        execution: ExecutionMode::Immediate,
                        operations: operations.clone(),
                        witness: Some(witness.clone()),
                    },
                    None,
                )
            }
        };

        if let Some(conn) = connection_details {
            let package_hash =
                create_and_submit_package(output_path, &payload, extrinsics, 0, conn, None);
        }
    } else {
        dbg!("Submitting two packages with segments, with Deferring and Deferred execution");

        let (first_payload, extrinsics) = match delivery {
            DeliveryMode::Extrinsic => {
                println!(
                    "Submitting in extrinsic mode, witness will be sent as extrinsic data in the first package"
                );
                (
                    RefinePayload {
                        delivery,
                        execution: ExecutionMode::Deferring,
                        operations: operations.clone(),
                        witness: None,
                    },
                    Some(witness),
                )
            }
            DeliveryMode::Direct => {
                println!(
                    "Submitting in direct mode, witness will be included in WorkItem payload of the first package"
                );
                (
                    RefinePayload {
                        delivery,
                        execution: ExecutionMode::Deferring,
                        operations: operations.clone(),
                        witness: Some(witness.clone()),
                    },
                    None,
                )
            }
        };

        let second_payload = RefinePayload {
            delivery: DeliveryMode::Direct,
            execution: ExecutionMode::Deferred,
            witness: None,
            // In real code, we should read the verified data stored by Deferring execution and include it in the payload of Deferred execution, for tutorial we just put the same operations and witness for simplicity.
            operations: Vec::new(), // we do not need to include operations in the second package as they are already included in the first package, but for simplicity we include them again.
        };

        if let Some(conn) = connection_details {
            let first_package_hash =
                create_and_submit_package(output_path, &first_payload, extrinsics, 1, conn, None);
            let _ = create_and_submit_package(
                output_path,
                &second_payload,
                None,
                0,
                conn,
                Some(first_package_hash),
            );
        }
    }
}

pub fn export_payload(output_path: Option<&PathBuf>, payload: &RefinePayload) {
    if let Some(output_path) = output_path {
        println!("Output: {}", output_path.display());

        // Create the output file. In direct and extrinsic mode, this is the end result.
        // In preimage mode, we use this to compute a hash, and then
        // include it as the corresponding pre-image to a Solicit operation.

        let _output = export_direct_payload(output_path, &payload);
    } else {
        println!("No output file specified, skipping writing payload to file");
    }
}

#[derive(Copy, Clone)]
pub struct ConnectionDetails {
    rpc_port: u16,
    service_id: u32,
}

pub fn create_and_submit_package(
    output_path: Option<&PathBuf>,
    payload: &RefinePayload,
    extrinsic: Option<Witness>,
    export_count: u16,
    conn: ConnectionDetails,
    prev_wp_hash: Option<WorkPackageHash>,
) -> WorkPackageHash {
    export_payload(output_path, &payload);

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(error) => {
            println!("⚠️  Failed to create Tokio runtime: {}", error);
            std::process::exit(1);
        }
    };

    let rpc_port = conn.rpc_port;
    println!("Submitting to RPC node at port {rpc_port}...");

    match rt.block_on(submit_to_node(
        rpc_port,
        Some(conn.service_id),
        &payload,
        extrinsic,
        export_count,
        prev_wp_hash,
    )) {
        Ok(package_hash) => {
            println!(
                "✅ RPC submission successful: {package_hash} - payload execution type: {:?}",
                payload.execution
            );
            package_hash
        }
        Err(error) => {
            println!("⚠️  RPC submission failed: {}", error);
            std::process::exit(1);
        }
    }
}

pub(crate) fn parse_service_id_hex(input: &str) -> Option<u32> {
    println!("Parsing service ID from input: '{}'", input);
    let trimmed = input.trim();
    let hex_part = trimmed
        .strip_prefix("0x")
        .or_else(|| trimmed.strip_prefix("0X"))
        .unwrap_or(trimmed);

    if hex_part.is_empty() {
        println!("Service ID cannot be empty");
        return None;
    }

    match u32::from_str_radix(hex_part, 16) {
        Ok(service_id) => Some(service_id),
        Err(e) => {
            println!("Invalid service ID hex '{}': {}", input, e);
            None
        }
    }
}

async fn submit_to_node(
    rpc_port: u16,
    service_id: Option<u32>,
    payload: &RefinePayload,
    extrinsic_witness: Option<Witness>,
    export_count: u16,
    prev_wp_hash: Option<WorkPackageHash>,
) -> Result<WorkPackageHash, NodeError> {
    let node = match connect_to_node(rpc_port).await {
        Ok(node) => node,
        Err(error) => {
            println!("⚠️  Error connecting to RPC node: {}", error);
            std::process::exit(1);
        }
    };
    println!("Connected to RPC node, submitting payload...");

    let (context, _anchor_slot) = create_refine_context(&node).await?;
    println!("Created context for submission");

    let service_id = service_id.expect("Service ID is required to submit to RPC node");

    let Ok((service, null_authorizer_hash)) =
        get_service_data(&node, service_id, context.anchor).await
    else {
        std::process::exit(1);
    };

    let extrinsic_data = extrinsic_witness
        .as_ref()
        .map(|witness| witness.encode())
        .unwrap_or_default();
    let extrinsic_hash = hash_raw(&extrinsic_data).into();
    let extrinsic_specs = ExtrinsicSpec {
        hash: extrinsic_hash,
        len: extrinsic_data.len() as u32,
    };
    let extrinsic_bytes = vec![Bytes::copy_from_slice(&extrinsic_data)];

    let (encoded_package, package_hash) = create_package(
        service_id,
        service,
        payload,
        export_count,
        null_authorizer_hash,
        context,
        extrinsic_specs,
        prev_wp_hash,
    );

    println!("Created work package for submission");
    println!("Submitting work package");
    let max_core = val_count() / VALS_PER_CORE as CoreIndex;
    let mut core = DEFAULT_CORE;
    let submitted_core: Option<CoreIndex>;

    loop {
        match node
            .submit_encoded_work_package(core, encoded_package.clone().into(), &extrinsic_bytes)
            .await
        {
            Ok(_) => {
                submitted_core = Some(core);
                break;
            }
            Err(error) => {
                println!(
                    "submit_encoded_work_package to core {core}/{} failed: {}",
                    max_core, error
                );
                core = (core + 1) % max_core;
                if core == DEFAULT_CORE {
                    return Err(error);
                }
            }
        }
    }

    let submitted_core = submitted_core.expect("submitted core is set when submission succeeds");

    println!(
        "✅ Payload submitted successfully to service {service_id} on core {} with package hash {package_hash}",
        submitted_core
    );

    Ok(package_hash)
}

async fn get_service_data(
    node: &WsClient,
    service_id: u32,
    anchor: HeaderHash,
) -> Result<(Service, CodeHash), NodeError> {
    let service = match node
        .service_data(anchor, service_id)
        .await?
        .ok_or_else(|| println!("Service {service_id} not found at anchor {:?}", anchor))
    {
        Ok(service) => service,
        Err(_) => {
            println!("⚠️  Service {service_id} not found at anchor {:?}", anchor);
            std::process::exit(1);
        }
    };

    let (null_authorizer_hash, auth_code_preimage_available) =
        get_authorizer(&node, anchor).await?;

    let service_code_preimage_available = node
        .service_preimage(anchor, service_id, service.code_hash.0)
        .await?
        .is_some();

    println!(
        "Is authorizer available: {:?}",
        auth_code_preimage_available
    );
    if !service_code_preimage_available || !auth_code_preimage_available {
        println!(
            "Preflight failed before submit: code preimage missing. service_preimage_available={}, authorizer_preimage_available={}\nservice={:08x}, service_code_hash={}, auth_code_host={:08x}, null_authorizer_hash={}, anchor={:?}\nHint: this commonly happens when targeting externally deployed services whose code preimage is not available to this node.",
            service_code_preimage_available,
            auth_code_preimage_available,
            service_id,
            hex::encode(service.code_hash.0),
            BOOTSTRAP_SERVICE_ID,
            hex::encode(null_authorizer_hash.0),
            anchor
        );
        Err(NodeError::Other(
            "Required preimages not available".to_string(),
        ))
    } else {
        Ok((service, null_authorizer_hash))
    }
}

fn create_package(
    service_id: u32,
    service: Service,
    payload: &RefinePayload,
    export_count: u16,
    authorizer_hash: CodeHash,
    context: RefineContext,
    extrinsic: ExtrinsicSpec,
    import_from_package: Option<WorkPackageHash>,
) -> (Vec<u8>, WorkPackageHash) {
    let extrinsics = vec![extrinsic]
        .try_into()
        .expect("We only have one extrinsic, so this should never fail");

    // export_count must be at least as large as the number of segments we export during refinement,
    // or else we will have a StorageFull error.

    let import_segments = if let Some(prev_package_hash) = import_from_package {
        vec![ImportSpec {
            root: RootIdentifier::Indirect(prev_package_hash),
            index: 0,
        }]
    } else {
        Default::default()
    };

    let item = WorkItem {
        service: service_id,
        code_hash: service.code_hash,
        payload: payload.encode().into(),
        refine_gas_limit: max_refine_gas(),
        accumulate_gas_limit: max_accumulate_gas(),
        import_segments: import_segments
            .try_into()
            .expect("We only have one segment to import, so this should never fail"),
        extrinsics,
        export_count,
    };
    println!("Created work item for submission without imports");

    let package = WorkPackage {
        authorization: Authorization::new(),
        auth_code_host: BOOTSTRAP_SERVICE_ID,
        authorizer: Authorizer {
            code_hash: authorizer_hash, // instantiated usually to be null_authorizer_hash
            config: AuthConfig::new(),
        },
        context,
        items: vec![item]
            .try_into()
            .expect("We only have one item, so this should never fail"),
    };

    println!("Created work package for submission");

    let encoded_package = package.encode();
    let package_hash: WorkPackageHash = hash_raw(&encoded_package).into();

    (encoded_package, package_hash)
}

async fn create_refine_context(node: &WsClient) -> Result<(RefineContext, Slot), NodeError> {
    // Match the parent-based anchoring logic used by newer tooling.
    let finalized = node.finalized_block().await?;
    let lookup_anchor = node.parent(finalized.header_hash).await?;

    let best_block = node.best_block().await?;
    let parent = node.parent(best_block.header_hash).await?;
    let anchor = parent.header_hash;

    let state_root = node.state_root(anchor).await?;
    let beefy_root = node.beefy_root(anchor).await?;

    let context = RefineContext {
        anchor,
        state_root,
        beefy_root,
        lookup_anchor: lookup_anchor.header_hash,
        lookup_anchor_slot: lookup_anchor.slot,
        prerequisites: Default::default(),
    };

    Ok((context, parent.slot))
}

async fn connect_to_node(rpc_port: u16) -> Result<WsClient, NodeError> {
    let common_args = CommonArgs {
        rpc: format!("ws://localhost:{}", rpc_port).to_string(),
    };

    let node = match common_args.connect_rpc(DEFAULT_NODE_INDEX).await {
        Ok(node) => {
            let best_block = node.best_block().await?;
            println!(
                "✅ Succeeded connecting to RPC node at {}. Best block: {} at slot {}",
                common_args.rpc, best_block.header_hash, best_block.slot
            );
            node
        }
        Err(error) => {
            println!(
                "⚠️  Startup RPC connection failed for {}: {}",
                common_args.rpc, error
            );
            std::process::exit(1);
        }
    };

    Ok(node)
}

fn export_direct_payload(output_path: &PathBuf, refine_payload: &RefinePayload) -> File {
    let mut output = std::fs::File::create(output_path).unwrap();
    refine_payload.encode_to(&mut output);
    output
}

fn read_ops_from_file(path: &PathBuf) -> Vec<token_ledger_common::SignedOperation> {
    let mut input = std::fs::File::open(path).unwrap();
    let mut input_vec = Vec::new();
    input.read_to_end(&mut input_vec).unwrap();
    let operations =
        token_ledger_common::json::parse_signed_operations(input_vec.as_slice()).unwrap();
    dbg!(operations.len());
    operations
}

fn compute_transition_witness(
    db_path: &Path,
    override_head: Option<Hash>,
    operations: &Vec<SignedOperation>,
) -> Witness {
    let mut opt_db = std::fs::OpenOptions::new();
    opt_db.read(true).write(true);
    let mut state = State::from_db_path(db_path.to_path_buf(), override_head);

    println!("\nInitial root: {}", hex::encode(state.get_root()));
    let _ = state_transition(&mut state, operations);
    let witness = state.take_witness();
    println!("Post execution root: {}", hex::encode(state.get_root()));
    // dbg!(&witness);
    print_debug(&witness);
    witness
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

async fn get_authorizer(node: &WsClient, block: HeaderHash) -> Result<(CodeHash, bool), NodeError> {
    let null_authorizer_hash: CodeHash = hash_raw(jam_null_authorizer_bin::BLOB).into();
    let auth_code_preimage_available = node
        .service_preimage(block, BOOTSTRAP_SERVICE_ID, null_authorizer_hash.0)
        .await?
        .is_some();

    Ok((null_authorizer_hash, auth_code_preimage_available))
}
