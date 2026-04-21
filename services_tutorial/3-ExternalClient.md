# Transactions Rollup Model 
## Moving data out of JAM database


The previous tutorial sections described how to mint tokens and write account balances directly into Jam state.
This is fine as a tutorial to show how to define a service, and how to access Jam storage within it.
This accumulation centric design is very similar to the way ethereum did work at launch, with a single state shared by all peers, 
but it defeats the general Jam design by polluting Jam storage with service specific data.
It is also not very efficient, as each individual balance has to be specifically accessed during accumulation and this does not scale.
Remember, refinement is generally cheap, accumulation is expensive.

So, we prefer to move processing, as much as possible, out of this accumulation process and into refinement, and we will do so by treating balance changes in batches that we can verify in refinement with the aid of proofs and witnesses. Accumulation, then, will have the simple logic of updating the commitment to the resulting state.

This section of the tutorial will illustrate how to implement this model, and serve as an example for more sophisticated use-cases.

## Overview

This tutorial extends previous tutorials (a token ledger storing data during accumulation) with a focus on:
- designing a client external state: accounts are not stored on Jam state, only the state merkle root.
- discussing the costs of such design.
- developing a minimal external client state code example for educational purpose.

This tutorial will not attempt to:
- be secure, we keep skipping signature checks in refinement.
- be optimal, we use a very simple bounded, unoptimal, merkle state and proofs. For real use a proper third party implementation of state and state storage should be used (eg polkadot sdk).
- implement state distribution: each client should synch upon the last state root finalized in Jam state. A disconnected client will lose ability to synch state if work items and work reports get pruned (the Graypaper defines two retention periods: short-lived _auditable bundles_ are kept in the Audit DA store by assurers only until the relevant block is finalized, and a long-lived data lake, where exported segments and their page-proofs can be retained for 28 days or more). Here we do not implement such client, but simply launch all clients from a disk directory where we keep the successive database states stored in files named according to the state's root hash. A single file called `HEAD` keeps track of which is the current state. This enables us to always sync the witness generation with the current valid state, but of course in case we restart the testnet, or deploy a new service the current state will be reset to genesis. At that moment, the image on disk becomes irrelevant and all such files should be deleted, to avoid submitting inconsistent work packages. This persisted state is used both for interaction via the command line and direct connection to an RPC node.
- define proper role for distribution: every client represents a validator with direct access to the Jam datalake and work items. In a real implementation, distribution strategy must fit the usecase.
- show how to handle errors. External clients must handle the failure or success of service processing, but our dummy client implementation will assume success and try to update its state directly when producing the workitem. If the refinement panics, accumulation will not proceed.
- parallelize execution, since our usecase can fail (multiple parallel transaction overspending an account) and handling parallel refinement is rather complex. Even if part of the tutorial (see segments) lay a foundation to try to resolve multiple parallel state transitions, this is too involved for a tutorial. Therefore every state transition must run sequentially, which means every state transition must start from the state of the previous one.  

## Partial state

In this model, the full service state is managed by external clients, not by Jam service storage. Refinement therefore executes against a partial state carried in the Work Item payload.
We call that partial state a Witness. It is the data needed for refinement to replay and verify a proposed state transition against a committed prior root (often called a state proof in other ecosystems).

Important nuance for this implementation: the Witness is built from state accesses performed during transition execution (reads and writes), not only from values that end up modified.

The Jam's service storage maintains a cryptographic commitment to the global state, so accumulation is limited to updating this commitment after refinement confirms the purported state transition is the correct outcome of all the submitted operations, and it is built on the currently stored state.

Note that refinement, by itself, can ascertain validity of operations only in relation to the partial state communicated with them. Where we have several clients, it may happen that one of them submits a partial state that is not in sync with (or is outright malicious) what the others have submitted to the service and so it may happen that a state transition is considered valid in refinement but corresponds to a global state that is no longer up to date. For this reason, accumulation ensures that the state only changes if its current value corresponds to the initial state confirmed by the Witness, that is, the batch of operations was applied to the service's current state.

In summary, clients store and share a common state, then submit state-transition proofs to the service. Refinement validates the transition from the witness data, and accumulation only stores and updates the state commitment.

## External State Representation

The external state is the full client-side database used to process operations and compute state evolution. We keep it intentionally simple for tutorial purposes.

Balances are stored in a fixed-size binary Merkle tree. The address space is bounded to 2^15 leaves; if two different keys map to the same leaf index, this demo implementation fails on collision.

Token IDs are tracked separately (as a list) and hashed together with the balances-tree root to form the overall state root commitment.

In this implementation, the Witness is built from state accesses performed while executing the transition on the client side.
For each balance key that is read or written, the client records:
- the accessed key/value pair (when present), so refinement can reconstruct the touched leaves;
- the sibling hashes along the leaf-to-root path in the balances Merkle tree.

The witness format is therefore access-based (all keys touched by transition logic), not strictly "all keys that ended up changed". In practice this must include values that were read for validation (for example checking balances before a transfer).

The collected tree hashes are deduplicated by node index before encoding, so overlapping paths do not duplicate the same hash entry. However, this remains a simple tutorial design and does not try to minimize proof size aggressively.

Token IDs are handled separately from balances: the full token-id list is currently included in each witness.

At this point we have described the state model (full external state + partial witness + on-chain commitment). The next sections discuss a different axis: how this same payload is delivered to refinement (directly, via preimage, or via segments).

Full state serializing is done after each state transition (usually done by the command line producing the jam workitem), with no recovery of errors.

Full state deserializing is done on each command call.

## Jam Work-Packages

In Jam, we use Work-Packages to organize the work that the services perform. The Graypaper specifies precisely the constituents of a Work-Package, and it is useful to recapitulate some basic definitions for what follows.
A Work-Package is, essentially, a grouping of several Work-Items, that encapsulate the work to be done by a service within a core within a specific package Context. The Package includes some more information, in order for the execution to be properly authorized, but that is not important for this tutorial.

The Work-Item is the more granular unit of work: every work-item in a Work-Package is targeted at a specific service (different items in the same package can target different services) and when the package is processed, the service's refinement code is run for each work-item independently in order to produce a Work-Digest. The various Work-Digests are then made available to the service's accumulation logic, that can make the resulting on-chain state updates.
The main constituents of a Work-item are a blob of payload data, that is passed as input to the refinement logic, a gas limit for each of the service's phases (refinement and accumulation) and a manifest that includes the following:
- a reference to data supplied extrinsically with the Work-package (ie 'extrinsics') 
- the number of segments exported by this work-item into the Data Lake (also called the D3L - Distributed Decentralized Data Lake)
- a series of identifiers for segments previously exported by other packages

This tutorial will give brief demonstrations of how to make use of each of these pieces of data, but keep in mind the use-cases shown here are completely artificial and serve only to illustrate Jam's capabilities.

We will often refer to data types in the code representing these concepts, namely: `WorkPackage`, `WorkItem` (with obvious meanings) and the main fields: `payload`, `extrinsic`, etc.

## Work-Item Submission

Jam applications must have a means to relay work from their users to the network. In the introductory documents of this tutorial, we showed how to create Work-Items and send them to a running node using the command-line tool `jamt`. But in a realistic scenario it is more likely that a builder application will be responsible for creating Work-Packages and their Work-Items and send them to a chain node. The accompanying code to this tutorial provides a very simple builder that optionally illustrates this scenario. The user can choose to generate a binary file with an encoded Work-item, to be submitted via `jamt`, or to connect directly to a default RPC node and let the builder create and send the package. 

The instructions for the service operation are normally passed in the WorkItem's `payload` field. But a WorkItem can also receive data through associated extrinsics, and even have access to the Data-Lake or Pre-images.
In this tutorial, the data is divided into a list of Operations (with their state transitions), and a Witness to ensure that these are consistent with the chain's current state. 

We use the Witness to demonstrate the usage of extrinsics. When creating a binary for sending with `jamt`, we put the Witness in the item's payload, and so provide a default (an empty list of) set of extrinsic data. When we connect to an RPC Node instead, and create the Work Package, we move the Witness from the payload to the extrinsic data.

### Execution mode
We define in the code an enumeration to represent when a Work-package can be processed. 
In the simplest case, the execution of a service's logic invocation is done within the scope of a single Work Item's refinement, that is, the Work Item is self-sufficient and has all the data needed for the completion of one operation from beginning to end. But a Work Item (and in general a Work Package) may depend on data produced by other Work Packages, and require data exported by them. We illustrate this in a very simple scenario by creating sets of two paired Work Packages where the first one exports data for the second one to execute.
We stress this is an artificial use-case, merely for the illustration of this sort of dependency and essentially splits the work normally done in a single call across two Work-Package. 

In the accompanying code, the payload is characterized by an execution mode. This can have three settings, that cover the cases we introduced above:

- `Immediate`: the Work Item is fully self-sufficient, and the service logic can be executed immediately. The necessary data is all extracted from the payload or the extrinsics, the refinement logic is executed to check the operations are valid and pass a summary of their execution to accumulation. Finally, the accumulation logic takes this summary and updates the chain state. 
- `Deferring` and `Deferred`: these two variants cover the scenario of paired Work-Packages, and split the responsibility of the Immediate case between them. 
     - `Deferring`: The first Work-Package is created with the `Deferring` mode. It extracts the data and validates the result, but instead of passing the summary to accumulation, it exports the operation and the witness into a D3L segment.
     - `Deferred`: The second Work-Package is created with the `Deferred` mode, and its aim is to complete the execution. Its WorkItem has an empty payload, as everything it needs is already in the D3L. So it imports the segment from the D3L, extracts the Witness and the data, and basically continues the full process (that can be seen in an `Immediate` package) where it was interrupted in the `Deferring` step.

### Handling D3L Segments

Work-Packages can use the Jam data-lake (D3L) to store data for at least 28 days in fixed-size segments. This is a relatively long-term storage, that allows packages to process large amounts of data decoupled it from the construction of the Work-Package itself, which merely keeps a reference to the data segments and where to find them. This allows Work-Packages to be small enough for guarantors and auditors to reconstruct and evaluate them when needed.

This may also be useful in scenarios with large computation requirements, for example, to implement a MapReduce framework. WorkItems can be used to implement the parallel processing and exporting their (partial) results to D3L segments. Once this phase is finished, a second-layer operation can reduce these intermediate datasets to the final result.

A Work-Item can access the D3L during the refinement phase, which includes the ability to export segments, and to do so, it must declare in its manifest the number of segments it exports. The hashes of each of the exported segments are gathered in a Merkle-Tree and the resulting root, known as the segment-root, can be used as an identifier for this segment set.
A segment can be exported with the method `refine::export` or `refine::export_slice` with the segment data. The number of such invocations must match the number declared by the WorkItem in field `export_count`, or else this will return an error.

Segments exported into the D3L are accessible for importing by other packages. A package can read D3L data by invoking the `refine::import` method, and passing a single index. This allows it to specify which segment it wants to load (a package can import at most 3072 segments). However, D3L segments are not identified by a simple index. 
Segments are identified by a pair of values: a Root Identifier and an index into the list of segments associated to that identifier. The Work-Item includes in its manifest a list of these tuples, encapsulated in a type called `ImportSpec`, so that the refinement code can refer to the segments by their index in this list. 

Root Identifiers can be of two kinds: a segment-root, corresponding to the root hash of a Merkle-Tree of all the segments exported in a Work-Package, or the WorkPackageHash itself. A Work-Item that does not need to import anything will have in its manifest an empty list of `ImportSpec`s.

In this example, the data we export is small, and should always fit in a single Segment, so the code does not include any error checking. Each segment has exactly 4,104 bytes. Coupled with the maximum number of segments, a service can receive a maximum of 12,607,488 bytes in segments, so the example we give here, which places only a few bytes in a segment, is probably not justified.
In real-world projects, developers should consider whether they need such long-term storage, and plan the best way of using it, avoiding creating unnecessary segments. Conversely, they also have to plan how to split their data if it is longer than what a single segment can hold. The allowance of nearly 12Mb is very generous, but of course it all depends on the problem at hand.

### Sidenotes on design

This is largely a demo, and so the design was sacrificed in several ways:

- we create packages with segments in a very controlled way, and so we only have to keep track of a single package for definition of the `ImportSpec` and export only one segment at a time. In a broader scenario, we might need to export several of these, and keep track of all the exported segments before advancing to a summary step before accumulation (keep in mind exports can not be imported in accumulation). 

- delayed transaction: this example merely breaks down the execution of a transaction into two different parts, and artificially delays it. A more realistic case will delay execution only until some point where some necessary work has been done. A classic example is sorting of large data sets that have to be partitioned into smaller bits than can be effectively handled, and then combined. 

- data copying: the example here stores in a segment a copy of the data it received. The only advantage is that it advances some of the work (validation) in the first phase, which can be avoided in the second phase. In a more realistic scenario, we should also attempt to save on the data is stored (that is, exported) for the second phase. Exported data should, as much as possible, be transformed data, and where possible smaller than the original input, with just the information needed for the next step.

The model proposed in this tutorial can be viewed as the basic building blocks to creating more complex designs to solve real complex problems.

## Building and testing

We now describe how to build and use the example code presented in this tutorial

### Example code

The code under the git repository is split between three crates:
- `token-ledger-service-v2`: this implements the Jam service code, namely the refine and accumulation logic. It is largely dependent on the state transition of `token-ledger-state-v2`. Must be compiled in `no_std` mode and pre-compiled to PVM before it can be used.
- `token-ledger-builder-v2`: this implements the client part, including constructing encoded payloads and submitting them to Jam. It depends on the state transition from `token-ledger-state-v2`, but can be run in `std` mode.
- `toker-ledger-state-v2`: library crate implementing the merkle state and state transition logic. This is used both in the service and the builder and so must be compiled in `no_std` mode.

Code running direct workitme is executed when `Mode` enum is set to `Direct`.


### Prepare a workitem payload for refinement
There are two kinds of operations the user can create:
- Mint: creates a single token with a certain balance that is fully assigned to a seed account. The service only tries to transfer balances of tokens previously minted.
- Transfer: send some amount of tokens from a source account to a destination account.

For ease of use, we let the user specify a series of operations in a friendly way by creating Json files with minimum information for each operation, namely: amounts, token Ids and account descriptors. The latter are a seed to specify valid cryptographic account Ids, that can't be easily generated by hand, and free the user from having to generate valid keypairs for the sake of test examples. You can see an example in `token-ledger-builder-v2/example_payloads/unsigned_ops_mixed.json`.

We can't use Json directly as payload, so we encode it first in binary. The encoded payload run contains both the input operations and the state witness necessary for refinement to verify the correctness of the state transition.

1. Create a list of operations without cryptographic material (no signatures nor account IDs): <unsigned_ops.json>
2. From `token-ledger-builder-v2`, convert this list to a Json file with full information in Json with 

```
cargo run --bin sign_ops <unsigned_ops.json> <signed_ops.json>
```

3. Transform the list of operations into an encoded payload suitable for submission with
```
cargo run <signed_ops.json> <output_payload>
```

The client can generate several transitions in sequence while keeping consistency across runs: it loads the latest state from disk, applies operations, then persists the resulting state. By default, each run starts from the hash stored in the `HEAD` file (unless a specific head is provided) and builds the next transition on top of that root. The `HEAD` file stores only the latest root hash, while state snapshots are stored in separate files named by their root hash.


### Submit payload to Jam

You can simply use `just` as in previous tutorial but with `token-ledger-service-v2` as service crate, and use the `submit-file` command to submit the file produced (ie <output_payload> )in previous step.

Alternatively, you can also use the builder to connect directly to an RPC node and submit the Work Package without making use of `jamt`.
In this mode, you pass the Json list of files, and not the encoded payload. 
Note: For this tutorial, the builder assumes you are connecting to a testnet in the Tiny configuration. 

1. Compile the service into PVM with, which outputs a compiled `<service.jam>` file:
```
just build-service <service folder>
```

2. Start a testnet and locate a suitable RPC node message similar to 
`node0: 2026-04-06 16:44:45 main INFO polkajam  RPC listening on [::]:19800`
```
just start-testnet
```

3. Deploy the service on the testnet with and copy the resulting service id (ie `94560b8f`)
```
just create-service <service.jam>
```

4. Submit a work item to an RPC node. By default, this tutorial will try to connect to a node on port `19800`. Adjust as needed.
```
cargo run -- --connect-rpc --service 94560b8f <signed_ops.json>
```

### Specifying different payload modes

When using the builder to connect to an RPC node and submit the Work Package, you can pass in the `--extrinsic` option to specify the `Extrinsic` delivery mode, and you can pass the `--segment` option to have the builder produce two packages in the `Deferring / Deferred` mode.

## Debugging

As with any code, it is possible the service meets unexpected conditions and fails to compute to the end, for example due to insufficient gas or memory for the whole workload. We can simulate the latter by commenting this line `polkavm_derive::min_stack_size!(32 * 1024);` and trying to submit a single operation.

We will likely find an error, where refinement does not finish and then accumulation panics:
`node4: 2026-04-09 15:57:48 tokio-runtime-worker WARN   #ea10d9a7 [Accumulation]: Work item failed: Panic`

This error is raised while executing the service code inside the PVM, and to obtain detailed logs of that we need to execute the testnet in debug mode. That can be achieved with 
`just start-testnet-debug`, but keep in mind this generates a much bigger amount of logs. You can redirect the ouptut to file for easier analysis with
```
just start-testnet-debug  2>&1 | tee log_file
```

If an error occurs during PVM execution, you will get a message in your logs similar to this one, with an indication of the location of the error, the likely cause and a possible solution.
```
node2: 2026-04-09 16:16:33 tokio-runtime-worker DEBUG polkavm::api    Location: #112765: u64 [sp + 0x28] = a0
node2: 2026-04-09 16:16:33 tokio-runtime-worker DEBUG polkavm::api  Trapped when trying to access address: 0xfefdddc0-0xfefdddc8
node2: 2026-04-09 16:16:33 tokio-runtime-worker DEBUG polkavm::api    Current stack range: 0xfefde000-0xfefe0000
node2: 2026-04-09 16:16:33 tokio-runtime-worker DEBUG polkavm::api    Hint: try increasing your stack size with: 'polkavm_derive::min_stack_size'
```

In this case, we can just increase the available memory. In this case the error was caused by external conditions: there is not necessarily an error in the logic, but the environment (ie memory) constraints forced the program to stop prematurely. For a different case, where the error is caused by bad logic, we could have different error messages. For example, add this bit of code that tries to divide by zero:
```
    info!("=== Dividing by zero to create a panic in service code ===");
    let a = 10;
    let b = get("zero_divisor").unwrap_or(0);
    let c = a / b;
    info!("Result of division: {}", c);
```

On execution, his results in a different message, also with a trap location:
```
node0: 2026-04-09 16:25:06 tokio-runtime-worker INFO   #1d9993d1 === Dividing by zero to create a panic in service code ===
[...]
node0: 2026-04-09 16:25:06 tokio-runtime-worker WARN   #1d9993d1 Panic message: panicked at src/accumulation.rs:34:13:
node0: attempt to divide by zero
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter  Starting execution at: 37063 [5736]
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter::raw_handlers  Trap at 37063: explicit trap
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::api    Location: #37063: trap
```

Here, we get a pointer to the actual line that caused the error, which should put you on the path to the cause.

The logs provide a full trace of the execution. For example, just before this message you could have something like

```
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter  Starting execution at: 15071 [4878]
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter  Compiling block:
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4886]: 15085: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4887]: 15085: ra = 0x128
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4888]: 15089: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4889]: 15089: sp = sp + 0xffffffffffffffb0
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4890]: 15092: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4891]: 15092: u64 [sp + 0x48] = ra
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4892]: 15095: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4893]: 15095: u64 [sp + 0x40] = s0
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4894]: 15098: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4895]: 15098: u64 [sp + 0x38] = s1
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4896]: 15101: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4897]: 15101: a2 = 0xc
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4898]: 15104: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4899]: 15104: a1 = 0x1079c
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4900]: 15109: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4901]: 15109: a0 = sp + 0x20
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4902]: 15112: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4903]: 15112: ra = 0x270
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4904]: 15116: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [4905]: 15116: jump 12187
[...]
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter  Compiling block:
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5724]: 37047: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5725]: 37047: a4 = u64 [sp + 0x108]
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5726]: 37051: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5727]: 37051: a0 = 0x1
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5728]: 37054: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5729]: 37054: a3 = sp + 0x8
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5730]: 37057: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5731]: 37057: a1 = 0
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5732]: 37059: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5733]: 37059: a2 = 0
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5734]: 37061: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5735]: 37061: ecalli 100
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5736]: 37063: charge_gas
node0: 2026-04-09 16:25:06 tokio-runtime-worker DEBUG polkavm::interpreter    [5737]: 37063: trap
```

You can also try to compare this trace to the service's disassembled PVM code.
To do that, first locate the `.polkavm` file (TODO: currently, this seems to be exported to a temporary folder in order to create the .jam file, which is then copied to the current working directory. If there is a way to preserve the `.polkavm` file, I still haven't found it out. For now, I modified the builder logs to also copy the .polkavm together with the .jam file) and disassemble it with
```
polkatool disassemble <file.polkavm>
```

Note: by default, the builder removes some debug symbols from the compiled source, which will not be present in the disassembled file. If you want to have these (indicating the function names), build the service before deploying it on the testnet with the environment PVM_BUILDER_STRIP set to 0.

For example, for the above code, we can find the trap occurred in the following function:
```
<__rustc::da6fc54cdd59cb4]::rust_begin_unwind>:
      : @1831
 36929: sp = sp + 0xfffffffffffffea0
 36933: u64 [sp + 0x158] = ra
 36937: u64 [sp + 0x150] = s0
 36941: a1 = 0
 36943: a2 = 0
 36945: u64 [sp] = a0
 36947: a3 = 0x11570
 36952: a0 = 0x1
 36955: a4 = 0x15
 36958: ecalli 100 // 'log'
 36960: s0 = 0x1
 36963: u64 [sp + 264] = 0
 36967: a1 = 0x11585
 36972: a0 = sp + 0x8
 36975: a2 = 0xf
 36978: ra = 1024, jump @1826
      : @1832 [@dyn 512]
 36983: a0 = sp
 36985: a1 = 0x3f4
 36989: a2 = 0x11598
 36994: u64 [sp + 304] = 0
 36998: u64 [sp + 0x140] = a0
 37002: u64 [sp + 0x148] = a1
 37006: a0 = sp + 0x140
 37010: u64 [sp + 0x110] = a2
 37014: u64 [sp + 0x118] = s0
 37018: u64 [sp + 0x120] = a0
 37022: u64 [sp + 0x128] = s0
 37026: a0 = sp + 0x8
 37029: a1 = sp + 0x110
 37033: ra = 0x402
 37037: a2 = a1
 37039: a1 = 0x132a0
 37044: jump @2582
      : @1833 [@dyn 513]
 37047: a4 = u64 [sp + 0x108]
 37051: a0 = 0x1
 37054: a3 = sp + 0x8
 37057: a1 = 0
 37059: a2 = 0
 37061: ecalli 100 // 'log'
 37063: trap
```

Debugging the PVM code is complex and way beyond the scope of this tutorial. If you need to debug at this level, be prepared to invest some time in understanding the PVM and the various outputs.

# (Future work))

Work Items can also make use of pre-images in refinement. However, the tutorial still does not support such a use case.

## Using a preimage 

This is an experiment to store payload as a preimage, please do not read this as a usecase that make sense, but just as a description about how to add a preimage to service.

Code is run when `Mode` enum is set to `Preimage`.

We do the same as previously but do not write witness into the work item.

Instead we will publish a preimage for the service.

This design is quite bad:
- delay for preimage is rather long and need three calls when a single one was used previously and generally preimage
- preimages are rather expensive and shall not be use for such short duration storage

### Steps

There is two operations: sollicit then publish. Roughly sollicit pay for the preimage and commit to a hash and a lenth of the preimage blob, publish then transmit the preimage to peers.

From `token-ledger-builder-v2`
```
cargo run -- -i ./example_payloads/op_mint.json -o refinement_payload -pi
```

Here `refinement_payload` will be the same file as previously, but will be use as a preimage, not a workitem.
`refinement_payload.prepare` file will also be created, it contais an workitem for the service that sollicit a workitem.

After creating the service we then submit `refinement_payload.prepare` as a workitem.

This when refine does a lookup for the preimage (we couldn’t provide it at this point), but will not find it, then it will pass an item to accumulation.

Accumulation see this item and call `solicite` host function to allow the given preimage (hash and length are used).

Now, we can provide the workimage to jam (just provide command with `refinement_payload` as parameter).

Next we will submit again the workitem `refinement_payload.prepare`, this time (if preimage had time to propagate), the refinement will find the preimage, decode it, obtain witness and operations and then do the same process as in previous example.
