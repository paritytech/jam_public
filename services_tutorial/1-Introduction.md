# Jam Services

This is an introduction to building Services for use in the JAM protocol. JAM is a protocol for building blockchains, and it draws some inspiration from Ethereum and Polkadot, so it will be convenient to use those two as references for metaphors to explain the novelties of JAM.

JAM also draws some base inspiration from the vision of a multicore computer in some respects, transposed to a Web3 world. That is, it simulates a Web3 multicore computer by allowing the block-chain to execute several different computations at the same time, each on its own subset of validators. This idea is not original to Jam, as Polkadot already supported different cores that could execute different computations simultaneously. 

Polkadot cores enable in effect a sharding of the blockchain's computation, splitting it across multiple parachains and avoiding the need for each state change to be verified by _all_ chain validators. Instead, only by the core's validator set have to agree on the state transition of each para-chain. Security is guaranteed by the ELVES protocol, which prescribes a core's work is then verified by a random set of validators to ensure the core behaved honestly, but crucially still requiring much fewer nodes than the whole chain's contingent.

Polkadot's cores effectively improve the chain's global throughput, but Polkadot is still very restricted in what kind of computations it allows. Each core computation basically corresponds to a change of state in a subsidiary blockchain (eg a parachain). JAM, instead, introduces the ability for this computation to be nearly arbitrary, delegating to services the specification of this computation in a very general manner. In fact, one of its services (CoreVM) gives the ability to run legacy (even pre-web) code with the Web3 without traditional blockchain limitations (like block computation limits) while providing all the Web3 guarantees of censorship-resistance via massive distribution, correctness of computation, and immutability of history. This effectively abstracts away the blockchain and creates an illusion of continuous execution.

Without Services, or without work for those services, there will be little point to JAM and its state will not be very interesting. It is services that provide the business logic and the meaningful changes to the JAM blockchain state, and even if there is nothing like JAM Services in Ethereum, they fulfill the same basic role of Ethereum Smart Contracts. JAM provides blockspace for application developers by letting anyone create and deploy services, and letting their customers submit work to these services.

One of the main roles of JAM is to coordinate how and when these services execute, making use of the resources made available by JAM itself, namely, the computational capacity of all the cores and the available storage to support that computation. In some sense, we can view JAM as a massive distributed world-computer, and Services as interesting applications that run on it.

## Service Structure

Service logic runs inside a Polkadot Virtual Machine (PVM) instance after the service is compiled to PVM-bytecode. This is modelled after RISC-V instruction set architecture, and therefore limited to a set of available instructions with specific parameters.

Some desirable operations are more complex than what can be achieved by combinations of these opcodes, and for those Jam makes it possible to invoke specific host-calls, in a similar way that the EVM allows the invocation of pre-compiled contracts. 

Services are free to define the state data important for their own computation. This state evolves by two basic steps: 

* Refinement, which is performed by a single core (a small set of validators) and proposes summaries of state changes in Work Digests.
* Accumulation, which takes these Work Digests and fuses them to the global chain-state. This computation is performed on-chain by all validators.


## Refinement and Accumulation

Service state evolves in two very distinct phases, Refinement and Accumulation. 
The objective of JAM is to divert as much work as possible to Refinement, which being executed only in the core, is cheaper and so can use more resources. Because this computation happens in-core, it has access to higher resources and performance, as well as access to IO-bound resources like the 'Distributed Decentralized Data-Lake' (aka D3L). The execution in refinement is considered to be stateless, in that it generally can not depend on chain state (aside from some guarantees about preimage availability). Instead, during refinement, there must be a well-defined finalized block called the `lookup-anchor` that all validators know, so they can refer to that state in order to resolve preimage requests and determine whether the refinement was correctly executed.

In summary, refinement has a high gas limit, but cannot access state. It can access all of the package and import data and registered pre-images. This includes the following hard-limits: 
* at most 3072 imported segments of 684 bytes each
* at most 128 elements of extrinsic data of variable size.

In total, a work-package's size, together with all the associated segments and extrinsics, can not exceed 13,791,360 bytes, leaving about 12MB for input data.

On the other hand, Accumulation happens on-chain, and the service's accumulation logic must be executed by all validators during block importing. This computation is stateful, since the state update entirely depends on the current state of the chain at that time, and is the only way that the state of the chain can change. In particular, this is how the changes computed by a service (in refinement) can be merged with the chain state.
Accumulation does not have access to any of the Package data, but only to  the Refinement's output, which is limited to about 48KB. However, we are also able to access the same pre-images as Refinement.


## Coding Support

The main support for writing service code is in the crate `jam-pvm-common`. 
A service must invoke the `declare_service!` macro, which marks the invoking crate as a service and provides it with the entry points for executing the refinement and accmulation service logic. This is done via the `Service` trait, that the parameter of the macro must implement.
This trait provides two hooks, `fn refine` and `fn accumulate`, that host the service logic.

`jam-pvm-common` also provides access to the host-calls allowed by jam. The list of calls available is too big for this document, but can be found at the [crate's documentation](https://docs.rs/jam-pvm-common/latest/jam_pvm_common/).
The set of host-calls available in refinement is different from those available in accumulation, and these lists are available through the modules [refine](https://docs.rs/jam-pvm-common/latest/jam_pvm_common/refine/index.html) and [accumulate](https://docs.rs/jam-pvm-common/latest/jam_pvm_common/accumulate/index.html).  

The API made available by `jam-pvm-common` covers all the host-calls defined in the graypaper, and it further provides convenient wrappers and variants of these. We can group them in some broad themes.

In refinement, services can invoke host-calls for:
* exporting items to the Jam Data Lake, 
* managing Virtual machine lifetime, 
* reading the gas meter
* managing a PVM instance, creating, removing
* managing a PVM instance's memory pages and their access rights
* managing a PVM instance's memory with low granularity, reading and writing into it
* making preimage lookups
* fetching numerous types of data, including the current work-package, work-items and their constituents
* importing segments referred in work-items

In accumulation, services can invoke host-calls for:
* manipulating storage, where the chain state is kept
* creating services, managing their lifestyle and changing their privileges
* changing the validators keyset
* managing preimages, soliciting, querying and forgetting them
* obtaining service information
* transferring data and funds to another service, creating a deferred transfer
* fetching items ready for accumulation, chain entropy or protocol parameters
* reading the gas meter

# Interacting with services via the CLI

In this section we demonstrate creating a simple JAM service and interacting with it via the CLI. This will serve as a brief introduction to the code. Then, in another document, we will dive deep into the writing of such services.

## Setup an example service

We outline a set of instructions to create a very basic exemplary service.

* Create a new directory to host the service source code. In this example, we'll use `tutorial-service`.

* Initialise the service with `cargo init --lib tutorial_service`

* Modify `Cargo.toml` as necessary 
	- to include necessary dependencies:
	```
	[dependencies]
	jam-pvm-common = { version = "0.1.26", default-features = false, features = ["service", "logging"] }
	polkavm-derive = "0.31.0"
	```
	- to include a license, which is necessary for a service to be built
	```
	[package]
	[...]
	license = "Apache-2.0"
	```

* Copy the following source code onto the `lib.rs` file of the service.

```
#![cfg_attr(any(target_arch = "riscv32", target_arch = "riscv64"), no_std)]

extern crate alloc;
use alloc::format;
use jam_pvm_common::*;
use jam_types::*;

#[allow(dead_code)]
struct Service;
declare_service!(Service);

impl jam_pvm_common::Service for Service {
	fn refine(
		_core_index: CoreIndex,
		_item_index: usize,
		service_id: ServiceId,
		payload: WorkPayload,
		_package_hash: WorkPackageHash,
	) -> WorkOutput {
		info!(
			"This is Refine in the Test Service {service_id:x}h with payload len {}",
			payload.len()
		);
		[&b"Hello "[..], payload.take().as_slice()].concat().into()
	}
	fn accumulate(slot: Slot, id: ServiceId, item_count: usize) -> Option<Hash> {
		info!("This is Accumulate in the Test Service {id:x}h with {} items", item_count);
		for item in accumulate::accumulate_items() {
			match item {
				AccumulateItem::WorkItem(w) =>
					if let Ok(out) = w.result {
						accumulate::set_storage(b"last", &out).expect("balance low");
					},
				AccumulateItem::Transfer(t) => {
					let msg = format!(
						"Transfer at {slot} from {:x}h to {id:x}h of {} memo {}",
						t.source, t.amount, t.memo,
					);
					info!("{}", msg);
					accumulate::set_storage(b"lasttx", msg.as_bytes()).expect("balance low");
				},
			}
		}
		None
	}
}

pub const MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");
```

## Install necessary tools

Most of the tools needed below can be obtained with a Polkajam release. 

* Download an up to date [release from Parity](https://github.com/paritytech/polkajam-releases/releases). 
	- At the time of writing, the latest release is 0.1.27 implementing version 0.7.2 of the Graypaper. 
	- Version 0.1.26 is also available and implements version (0.7.1) of the Graypaper.
	- However, the latest version of `jam-pvm-common` available in [crates.io](https://crates.io/search?q=jam-pvm-common), needed to compile the service, is 0.1.26, so you might prefer to stay on this version for full compatibility.
* Extract the code from the downloaded package and add the resulting directory to your `PATH` variable
* In MacOS, authorise the execution of the following binaries if needed, which are all available in the release:
	- `polkajam-testnet`
	- `polkajam`
	- `jamt`
	- `jamtop`
* Install the service builder with `cargo install jam-pvm-build`
	
## Building and interacting with the service

Services need to be installed in a Jam chain. For tests, we can use a small version of test network 
with the following command.

* Start a test chain: `polkajam-testnet`

Services are meant to be run inside a PVM when executing. Therefore they need to be compiled to PVM
bytecode. We can easily check correctness of the code by running `cargo build` as usual, but this
won't compile to our target. For that, we should use instead `jam-pvm-build`:

* Generate a compiled `<service>.jam` file in the current directory:
	`jam-pvm-build -m service <service-crate-path>`

This creates in the local directory the file `jam-token-ledger.jam` with the compiled bytecode.
The next step is to deploy this bytecode on the testnet.

* Deploy an instance of the service on this testnet: `jamt create-service <service>.jam <amount> <Optional memo>`

	This command indicates the path to the compiled service, the service's initial endowment and a memo describing the endowment. The argument is a file with extension `.jam` that represents a service compiled for the PVM.

	Other possible settings include the ability to register this service with the Bootstrap service (`--register <service name>`), and to specify the minimum amount of gas given to each work item (`min-item-gas`) and each transfer item (`min-memo-gas`) if none is specified on creation (see below).

	The command takes a while to complete, until it eventually prints something like this:
	`Service b5ef19da created at slot 5428812` 
	returning the service identifier.

* (Optional) Track this service resource usage on chain: `jamtop`

## Basic operations

### Monitor execution of this service in the testnet logs, by searching for its Service Id.

For example, you should see messages similar to these, repeated by all validator nodes:

	node0: 2026-01-13 12:01:12 tokio-runtime-worker INFO boot  #0 Created service b5ef19dah with code_hash 0x69b9298123b20fa1...
	
	node0: 2026-01-13 12:01:12 tokio-runtime-worker INFO   #b5ef19da Transfer at 5428812 from 0h to b5ef19dah of 1000000000 memo "My memo"
	
	node0: 2026-01-13 12:01:12 tokio-runtime-worker INFO   #b5ef19da This is Accumulate in the Test Service b5ef19dah with 1 items

So far, aside from these there should be no more activity, since we have not given any work for this service to do.

Be aware that the testnet logs ignore leading 0s when presenting the service Id. For example, the default bootstrap service's id appears as `0h` instead of `00000000`, which is how it appears in `jamtop`. Likewise, a service with id `009ab3e2` will instead appear as `9ab3e2` so discard leading zeroes when searching for a given service id.

### Submit new work items for this service

We use `jamt` to submit a Work Item to a service: `jamt item <service id> <payload>`

Example:

	jamt item <service id> "World."

There is not a prescribed format to `payload`, aside from it being a sequence of bytes, as that is dictated by your own service's code.
In the case of this tutorial service, the payload is interpreted as a string that is appended to "Hello " and the result is passed to the Accumulation phase via `accumulate_items` which stores it under the key `last`. 

We can submit work packages with more than one item.

This can be done by delaying the submission of a work item, and adding it to the queue instead.
We prefix the `item` command with the option `--queue`, with the result that the work item is to the queue but not sent to the chain. This queue is global, and therefore all the queue items, irrespective of their service, are added to it in the order of introduction. 

Once all the items are added, we can push them to refinement and empty the whole queue with

	jamt pack

This creates a single package with all the corresponding items, including multiple services. Each item is refined individually, although Refinement receives the package identifier and the item's index within it. 

The accumulation execution happens individually per service, but multiple work items (naturally all belonging to the same service) may be accumulated at the same time. A full package is accumulated at once, but if the gas is insufficient to accumulate the entire package, it is added to the ready queue for accumulation in a subsequent block.

### Find information about services running on chain including its balance

We can obtain only very basic information about a service via `jamt`.
This can be obtained by inspecting the service:

	jamt inspect service <service id>

### Inspecting service state

Service state is stored and modified in the accumulation phase. This is in essence a database of key/value pairs without any more structure. We can set and get the value at specified location keys (sequences of bytes). There is some support for interacting with values that know how to Encode and Decode themselves. However, 'Jamt' is very basic in what it can do: 1) it can accept arbitrary data as a plain string or a hex-string (in `jamt item`); 2) it can display the contents of a certain storage location in a compressed hex-string (in `jamt inspect storage`). 

The data passed to `jamt item` is UTF-encoded into bytes if it is presented as a quoted string. On the other hand, if it is an _unquoted_ hex string, this will be parsed and converted to bytes representing of the hex-string (ie not its UTF encoding).

The above considerations are only about the transport of the data between the user and the service. For the service itself, the data encoding is transparent to JAM. The service is fully responsible for deciding how that data is serialized: it could be a sequence of SCALE-encoded bytes, a JSON file, or even raw binary (eg encrypted) data. 

To exemplify with the tutorial service, this sets keys `last` and `lasttx`. After we submit the item "World." to service `ef1505fe`, we can type 

	jamt inspect storage ef1505fe last 
	
and obtain

	"Hello World."

Note: the changes to chain state do not happen immediately after we submit a Work Item. This Item first has to be refined and processed into a Work Digest inside a Work Report. Only at the end of the accumulation is the result present in state. It is quite possible that if you query the state right after you submit the work item you will get some old version, and not the result of this work item. Developers should take this asynchronicity in mind when developing their code and devise some mechanism to check when their work items have been accumulated.

### Transfer Funds from the Bootstrap service to a target service

	jamt transfer <destination service> <amount> <memo>

A memo is an arbitrary field of at most 128 bytes. Every balance transfer between services carries an optional Memo. It can serve as a description to the purpose of the transfer.


# To come later:

* How to create dependent packages and pass data from one to the next
	
* Using the D3L layer

* CoreVM: what it is, how to build and execute programs for it

