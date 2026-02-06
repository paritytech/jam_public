# Example Service Implementation 

In this document we guide you through the creation of a service by implementing a complete example. We aim to highlight the initial steps of service creation and the basic necessities, so the example will be simple. We hope to evolve it in stages, gradually adding more complex features with time.

This tutorial walks you through creating a JAM service that maintains a ledger of accounts with multi-token balances and accepts transfers between them. The service allows the minting of a token, and then the transfer of tokens from account to account.

We gradually build a complete example, commenting on the basic choices facing the developer. We exemplify with some basic usage via the CLI. In a later stage, we plan to develop a simple client for the service that can replace this interaction.

Note: This tutorial is not production-ready. It serves only as an illustration of basic concepts and choices the developers should make, but little care is given to security or performance. Do not use these examples in practice without dutifully improving them.

## Overview

The **Token Ledger Service** will: 

1. Track balances of multiple tokens for multiple accounts 
2. Accept transfer requests specifying: sender, recipient, token_id, and amount 
3. Validate transfer correctness during refinement (in-core) 
4. Apply state changes during accumulation (on-chain)

## Project Setup

A complete working version of the code is made available together with this document. In the following sections we highlight the most interesting parts.

Start with creating a new service as detailed in the `Services-Intro` document, and add the following dependencies in `Cargo.toml`.
Depending on your code you likely will need to setup more dependencies to your `Cargo.toml` than the bare minimum outlined in that introduction.

In particular, notice the following essentials in any service you write:
* Every service has an attribute specifying the target architecture to be Risc-V, and to compile without the `std` feature. 
* It is necessary to use `jam_pvm_common`
* A struct type can be equipped with service logic by calling the `declare_service!` macro with it.

Pay attention to `std` restrictions. Any dependency that does not specify `no_std` will by default compile in `std` mode. You can try to disable that by disabling default features in the corresponding line in `Cargo.toml`. For example: 

In `[Cargo.toml]`
```toml
jam-types = { version = "0.1.26", default-features = false }
```

In `[lib.rs]`
```rust
#![cfg_attr(any(target_arch = "riscv32", target_arch = "riscv64"), no_std)]

extern crate alloc;

use alloc::{format, vec::Vec};
use codec::{Decode, Encode};
use jam_pvm_common::*;
use jam_types::*;

/// The Token Ledger Service
pub struct TokenLedger;
declare_service!(TokenLedger);
```

In order to build properly at this stage, add this empty implementation of the `Service` trait:

```rust
impl Service for TokenLedger {
       fn refine(
        core_index: CoreIndex,
        item_index: usize,
        service_id: ServiceId,
        payload: WorkPayload,
        package_hash: WorkPackageHash,
    ) -> WorkOutput {
        vec![].into()
    }

    fn accumulate(slot: Slot, id: ServiceId, item_count: usize) -> Option<Hash> {
        None
    }
}

```

### Important notes

* A service must run in a `no_std` environment. This constrains the structures Rust code normally has access to via `std`, so we need to use replacements where possible. `alloc` gives us access to `Vec` and `format!` and if we need replacements for `HashSet` or `HashMap` we can use `alloc::collections::{BTreeMap, BTreeSet}` or eventually `hashbrown::{HashMap, HashSet}`.

    `hashbrown` is an external crate and requires a new dependency, but conserves the same interface of the `std` versions, with `O(1)` access time and keys based on a hash function (they have to implement the `Hash` trait). 
    This is a good option if your queries are of type `key == <value>`.

    On the other hand, the `BTree` types from `alloc` require that the keys must implement the
    `Ord` trait instead, and are good for queries of the type `key >= <value>`. These types are
    ordered, meaning that entries are stored in key order. This can be beneficial for iterating
    over the contents of the structure, but has the drawback of making access time `O(log n)`
    instead.

* The `declare_service!` macro is the fundamental piece that marks this code as a service, and
  defines the required PVM entry points (`refine_ext` and `accumulate_ext`) that the JAM runtime
  calls.
* The entry point to Refinement is the `WorkPayload` type. The result is of the `WorkOutput` type.
  Both are structures wrapped around a vector of bytes, and it is the responsibility of the service
  to map between these types and its domain-oriented types. The developers should make a conscious 
  choice of how their data should be serialized in storage.


Given that there is a stark difference between refinement and accumulation, we will create two modules to encapsulate the concepts required for each. 

```rust
mod refinement;
mod accumulation;
```

## Define Data Types

Ultimately, the purpose of a service would be to change some chain state, adding some data to chain storage.
It is likely one of the most fundamental decisions of the developer will be the data structures needed
to represent this data. Bear in mind they may be different in Accumulation and Refinement.

Most likely, Accumulation data structures will be close to how the data is stored on chain, 
but keep in mind Refinement allows for far more complex calculation than can be done in the accumulation stage.
Refinement can be used, for example, to whittle down a complex amount of input data into a very compressed 
set of accumulation changes.

In this example, the core concepts we need to represent are: a holder Account, a TokenId, and
two basic Operations: Mint and Transfer.

For this simple example, we will use `u32` for a `TokenId`, but it could be something more general
like a string representing a ticker or some other kind of descriptor. For account, we will use a 32-byte descriptor
that can double as an Ed25519 public key.

```rust
/// A unique identifier for a token type
pub type TokenId = u32;

/// An account identifier (32-byte public key)
pub type AccountId = [u8; 32];
```

We differentiate between operation requests sent to Refinement, and the data necessary to carry out these operations in accumulation. 
In particular, refinement operations carry an authorisation signature so the service can verify they're genuine. Accumulation operations do not need this: they have been validated already in refinement so they only require the essential data for a mint or transfer.

In `[refinement.rs]`:

```rust
/// Operations that can be submitted to the token ledger
#[derive(Clone, Debug, Encode, Decode)]
pub enum Operation {
    Mint {
        to: AccountId,
        token_id: TokenId,
        amount: u64,
    },
    Transfer {
        from: AccountId,
        to: AccountId,
        token_id: TokenId,
        amount: u64,
    },
}
```

In `[accumulation.rs]`:

```rust
/// Validated operations to apply in accumulation
#[derive(Clone, Debug, Encode, Decode)]
pub enum Operation {
    Mint(MintData),
    Transfer(TransferData),
}

/// Validated mint to apply in accumulation
#[derive(Clone, Debug, Encode, Decode)]
pub struct MintData {
    pub to: AccountId,
    pub token_id: TokenId,
    pub amount: u64,
}

/// Validated transfer to apply in accumulation
#[derive(Clone, Debug, Encode, Decode)]
pub struct TransferData {
    pub from: AccountId,
    pub to: AccountId,
    pub token_id: TokenId,
    pub amount: u64,
}
```

## Operation Authorisation

We demonstrate authorization by implementing a simple check that an authorised party has allowed the operation. For Minting, which creates a whole Token's balance out of thin air, we posit that the service is controlled by a single administrator account who must sign the operation. For transfers, we require the signature of the sending party.

We use Ed25519 cryptography as an example. It is for the developers to decide on the best verification/authorisation model for their use-case. This verification is expensive, and is a good example of what should go in the Refinement phase.

We use a wrapper struct that adds a signature to the actual operation.

In `[refinement.rs]`:

```rust
#[derive(Clone, Debug)]
pub struct SignedOperation {
    pub operation: Operation,
    pub signature: Signature,
}

pub fn verify_signature(op: &refinement::Operation, signature: &Signature, key: VerificationKey) -> Result<(), &'static str> {
    let message = op.encode();
    key
        .verify(&signature, &message)
        .map_err(|_| "Signature verification failed")
}
```


## Refinement

Refinement happens **in-core** and is executed off-chain by a small subset of 3 validators. This is
practically stateless, which means it can't use chain state for its computation. In particular, we
can't check if the sender has enough balance to fund a transfer.

Instead, Refinement should verify the correctness of all requests, and that they're properly
authorized. Also, since refinement is not subject to as strict limits in computation time as accumulation is, we
can engage here in heavier computation, and this is a good place to batch similar transfer requests
if possible and create summaries of the changes sent to state. In a scenario where we have many transfers between 
frequently corresponding accounts, we can greatly reduce the corresponding data sent to accumulation
and increase the number of real transfers executed.

A service's payload is just a sequence of bytes. It is the service logic that confers meaning to
this, by decoding it into the format it expects, and in particular to appropriate types.
The first task of a service's Refinement logic should be to decode the payload:

```rust
impl Service for TokenLedger {
    fn refine(
        _core_index: CoreIndex,
        _item_index: usize,
        service_id: ServiceId,
        payload: WorkPayload,
        _package_hash: WorkPackageHash,
    ) -> WorkOutput {
        info!("TokenLedger refine on service {service_id:x}h");
        
        // Parse the incoming payload as operations
        let operations: Vec<Operation> = match Decode::decode(&mut &payload[..]) {
            Ok(ops) => ops,
            Err(_) => {
                panic!("Failed to decode operations");
            }
        };

        [...]

    }
```

Note: in the companion code we've implemented parsing from JSON. That allows for easier interaction in tests, 
but the code is more complex and beyond the focus of this tutorial. The above works if the data is submitted
in SCALE-encoded binary data and serves as illustration.

Next we verify the signatures and package the resulting operations for accumulation:

```rust      
fn refine(...) {

    [...]

    let mut validated: Vec<accumulation::Operation> = Vec::new();

    for signed_op in operations {
        let SignedOperation { operation, signature } = signed_op;

        match operation {
            refinement::Operation::Mint {to, token_id, amount } => {

                let admin_key: VerificationKey =	
                    VerificationKey::try_from(admin()).expect("Hard-coded Admin key");

                if refinement::verify_signature(&operation, &signature, admin_key).is_err() {
                    warn!("Invalid signature for operation");
                    continue;
                }

                if amount == 0 {
                    warn!("Mint: Zero amount");
                    continue;
                }
                validated.push(accumulation::Operation::Mint(accumulation::MintData {
                    to,
                    token_id,
                    amount,
                }));
            },
            refinement::Operation::Transfer { from, to, token_id, amount } => {
                
                let signer_key: VerificationKey =	
                    VerificationKey::try_from(from).expect("AccountIds are of the right size");

                if refinement::verify_signature(&operation, &signature, signer_key).is_err() {
                    warn!("Invalid signature for operation");
                    continue;
                }

                // Validate transfer request
                if amount == 0 {
                    warn!("Transfer: Zero amount");
                    continue;
                }
                if from == to {
                    warn!("Transfer: Self-transfer not allowed");
                    continue;
                }
                validated.push(accumulation::Operation::Transfer(accumulation::TransferData {
                    from: from,
                    to: to,
                    token_id,
                    amount,
                }));
            },
        }
    }
}
```

Finally, we return the result by encoding it into a sequence of bytes:

```rust
fn refine(
    _core_index: CoreIndex,
    _item_index: usize,
    service_id: ServiceId,
    payload: WorkPayload,
    _package_hash: WorkPackageHash,
) -> WorkOutput {
    [...]
    
    // Encode and return for accumulation
    validated.encode().into()
}
```

## Accumulation

This is the second phase of service execution. It happens **on-chain**, is executed by all
validators, and can access (read and write) storage to check the integrity of the changes against
the current chain state.

The input to Accumulation is made available by the function `accumulate_items` from
`jam-pvm-common::accumulate`.

The nature of the accumulation phase is two-fold: we can alter state as a consequence of a
Work-Report or the result of a transfer deferred from the previous accumulation round. Therefore,
typically we match each item to the correct type `AccumulateItem::WorkItem<WorkItemRecord>` or
`AccumulateItem::Transfer<TransferRecord>` and then invoke the suitable code for each. A useful
pattern is to extract the logic for either type into separate local functions and keep this
matching simple:

```rust
impl Service for TokenLedger {
    [...]

    fn accumulate(slot: Slot, service_id: ServiceId, _item_count: usize) -> Option<Hash> {
        info!("TokenLedger accumulate on service {service_id:x}h @{slot}");

        for item in accumulate::accumulate_items() {
            match item {
                AccumulateItem::WorkItem(r) => on_work_item(r),
                AccumulateItem::Transfer(t) => on_transfer(t),
            }
        }

        None
    }
}
```

and outside the `Service` implementation add this
```rust
fn on_work_item(item: WorkItemRecord) {

}

fn on_transfer(item: TransferRecord) {

}
```

### Note: Service Balance and Token Balance

Keep in mind that the deferred transfers deal with the chain's native balance only, which is
relevant for example for handling gas costs. In the context of this service, it might cause
confusion, so remember that the tokens held and represented in this service are totally separate
from the service's token balance. They only exist in the state defined by this service, whereas the token balance
is a more fundamental property of a service itself.

## Accessing Storage

The storage available for the chain state is in essence a database of key-value pairs, where both
keys and values are raw sequences of bytes. For each value we want to store we need to define the
corresponding key and then encode it.

In this first iteration, we want to store very simple data:
- a list of the tokens we have minted
- the token balance of an account for a given token.

The list of known tokens will be kept at a single location in storage, that will never change for
the duration of the service. Therefore, we can simply hard-code the key. For balances, however, we
will have one entry for each non-null (account, token) combination, so we define a function to
generate the key for each entry. We define the key to be a representation of:
"bal:" + <token_id> + <account_id>

Notice the first part of the key defines the storage item itself (ie "token balance"). Always keep
in mind to have a prefix defining what the storage is about, and follow with the variable
parameters after it. Without a prefix, you could have different combinations of parameters for different
state data decoding to the same thing and leading to the same storage key. 
It is more or less indifferent whether we place the token first or the
account first, and rather comes down to preference.

```rust
/// Storage key for an account's token balance
pub fn balance_key(token_id: TokenId, account: &AccountId) -> Vec<u8> {
    let mut key = Vec::with_capacity(4 + 32 + 4);
    key.extend_from_slice(b"bal:");
    key.extend_from_slice(&token_id.to_le_bytes());
    key.extend_from_slice(account);
    key
}
```

In the service code, we can use the methods made available by `jam-pvm-common::accumulate` to interact with storage. The most basic ones are `get_storage` and `set_storage`. These return and receive, respectively, sequences of bytes, leaving all the encoding / decoding work for the caller code.

There are other methods available like `get` and `set` that require respectively a
SCALE `Decode`-able and `Encode`-able type that return and receive instances of that type instead. In
this tutorial, we illustrate both approaches. 

## Handling accumulation inputs

In this example we don't need to do much with service balance transfers. So we present a basic place-holder
implementation:

```rust
fn on_transfer(item: TransferRecord) {
    use crate::alloc::string::ToString;

    let TransferRecord { source, amount, memo, .. } = item;
	info!("Received transfer from {source} of {amount} with memo {}",
		alloc::string::String::from_utf8(memo.to_vec()).unwrap_or("[...]".to_string())
	);
}
```

When we receive a Work-Item, what we do depends on the type of operation:

```rust
fn on_work_item(record: WorkItemRecord) {
    info!("Accumulate processing work item record: package {:?}", record.package);
    let output = match record.result {
        Ok(output) => output,
        Err(e) => {
            warn!("Work item failed: {:?}", e);
            return;
        },
    };

    let Ok(operations) = Vec::<Operation>::decode(&mut &output[..]) else {
        warn!("Failed to decode validated operations");
        return;
    };

    info!("Processing {} validated operations", operations.len());

    for op in operations {
        match op {
            Operation::Mint(mint) => process_mint(mint),
            Operation::Transfer(transfer) => process_transfer(transfer),
        }
    }
}
```

### Implementing a Minting Request

We use a hard-wired storage key `known_token_ids` to keep track of the tokens that we have minted, and
avoid minting the same token twice. Minting creates balance for a new token from scratch, and
assigns it completely to a designated account.

We exemplify use of `set` and `get` to update the known tokens without having to encode or decode
the vector.

Conversely, we illustrate `set_storage` and `get_storage` for manipulating the balance.

We've added `checkpoint()` at the end. This is a host-call which means that once the execution
reaches that stage, the changes made by it become permanent in state. In the event the accumulation
can not finish due to some failure, for example a logic error or lack of gas, the computation is
rolled back and the changes undone, but this reversal is done only up to the last `checkpoint`. In
the absence of any `checkpoint`, the whole accumulation is rolled back.

```rust
fn process_mint(mint: ValidatedMint) {
    use jam_pvm_common::accumulate::{checkpoint, get, get_storage, set, set_storage};

    let mut known_tokens: Vec<TokenId> = get("known_token_ids").unwrap_or_default();

    if known_tokens.contains(&mint.token_id) {
        warn!("Minting already minted token: {}", mint.token_id);
        return;
    }

    known_tokens.push(mint.token_id);
    let _ = set("known_token_ids", &known_tokens);

    let to_key = balance_key(mint.token_id, &mint.to);

    let current_bal: u64 =
        get_storage(&to_key).and_then(|b| u64::decode(&mut &b[..]).ok()).unwrap_or(0);

    let new_bal = current_bal.saturating_add(mint.amount);
    let _ = set_storage(&to_key, &new_bal.encode());

    info!(
        "Minted {} of token {} to account {:?}. New balance: {}",
        mint.amount,
        mint.token_id,
        &mint.to[..],
        new_bal
    );

    checkpoint();
}
```

### Implementing Transfers

The logic for implementing a transfer is similar to that for mint, adding checks that the token has
been minted, and that the sender has enough funds. We now modify two balances instead of one.

```rust
fn process_transfer(t: ValidatedTransfer) {
    use jam_pvm_common::accumulate::{checkpoint, get, get_storage, set_storage};

    let known_tokens: Vec<TokenId> = get("known_token_ids").unwrap_or_default();

    if !known_tokens.contains(&t.token_id) {
        warn!("Trying to transfer unknown token: {}", t.token_id);
        return;
    }

    let from_key = balance_key(t.token_id, &t.from);
    let to_key = balance_key(t.token_id, &t.to);

    let from_bal: u64 =
        get_storage(&from_key).and_then(|b| u64::decode(&mut &b[..]).ok()).unwrap_or(0);

    if from_bal < t.amount {
        warn!(
            "Insufficient balance: account {:?} has {} but tried to send {}",
            &t.from[..],
            from_bal,
            t.amount
        );
        return;
    }

    let to_bal: u64 = get_storage(&to_key).and_then(|b| u64::decode(&mut &b[..]).ok()).unwrap_or(0);

    let _ = set_storage(&from_key, &(from_bal - t.amount).encode());
    let _ = set_storage(&to_key, &(to_bal.saturating_add(t.amount)).encode());

    info!(
        "Transferred {} of token {} from {:?} to {:?}",
        t.amount,
        t.token_id,
        &t.from[..],
        &t.to[..]
    );

    checkpoint();
}
```

# Seeing it in action

Please refer to `Services-intro.md` for guidance on how to interact with your new service.

In this tutorial, we interact with the service only via the CLI. To submit actual work to a testnet
via this way requires encoded `WorkItem`s and that is not feasible to do by hand. 
The companion code can understand JSON-encoded list of commands, which provides a convenient way to 
submit test data. 

We plan to provide an integrated Client in future versions of this tutorial.

This is an example list of commands:

```json
[
{
    "Mint": 
    {
        "token_id": 10,
        "amount": 1000000,
        "to": "0000000044444444222222223333333300000000444444442222222233333333",
        "signature": "00000000000000000101010101010101020202020202020203030303030303030404040404040404050505050505050506060606060606060707070707070707"
    }
},
{
    "Transfer": 
    {
        "token_id": 10,
        "amount": 250000,
        "from": "0000000044444444222222223333333300000000444444442222222233333333",
        "to": "0000000011111111222222223333333300000000111111112222222233333333",
        "signature": "00000000000000001010101010101010202020202020202030303030303030304040404040404040505050505050505060606060606060607070707070707070"
    }
},
{
    "Transfer": 
    {
        "token_id": 10,
        "amount": 10000,
        "to": "0000000011111111222222223333333300000000111111112222222233333333",
        "from": "0000000011111111777777777777777700000000111111117777777777777777",
        "signature": "00000000100000002000000030000000400000005000000060000000700000008000000090000000a0000000b0000000c0000000d0000000e0000000f0000000"
    }
}
]
```

It illustrates one mint and two transfer operations. 

In the companion code we provide an even bigger example (`op_list.json`) which includes some invalid transactions, testing the logic to detect them. It is worth noting that all the items in these documents are processed in the same WorkPackage but that there is no provision in the code consider the impact of mutually affecting transactions on each other. For that reason, some transfers that should logically succeed after another transfer may not execute successfully because the changes in balance were not yet performed.

# Conclusion and Future Work

This first tutorial walked through the creation of a simple service, covering the necessary steps
and noting some aspects developers may want to pay attention to.

It highlights the necessity of some support tools beyond the service itself, and the next logical
step for this would be the creation of an integrated client that would dispense the developer 
from using `jamt` and having to craft carefully encoded commands.

This could also allow for the creation and submission of large quantities of `WorkItem`s and
grouping them in different `WorkPackage`s, or explore the actual limits of how many transfers can
be handled by the service at peak, something we did not explore in here.

This could lead to explore more interesting ways to optimize the service and increase its
throughput.

