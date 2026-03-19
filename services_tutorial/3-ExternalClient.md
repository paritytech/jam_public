## Moving data out of JAM database

JAM allows storing data during its accumulation phase, yet this is costy and non scalable, this accumulation centric design is very similar to the way ethereum did work at launch: a single state shared by all peers.

Now we will move as much as possible out of this accumulation process, to run things mostly in refinement. Remember, refinement is generally cheap, accumulation is expensive.
Same for data, data stored in jamt storage is expensive, data stored in data lake is less expensive.

To achieve this, refinement will run over a partial state included in the workitem. In this tutorial, we call this partial state the witness as it is the witness of a given state transition. It is also often refered as a state proof (eg in polkadot).

Therefore, jamt will only ever see partial states needed for state transitions, and the full state used by clients of the service is managed (and stored) externally at service client application level. This is similar to what is done by a polkadot parachain, yet this tutorial will use a simple custom state to stress that jam do not expect specifics data or state transitions.

To sumup, we will have clients that store and share a common state, then send proofs of state transition to jam. Jam refinement will validate those proofs, and jam accumulation will only store and update this common state merkle root.

## Overview

This tutorial extends previous tutorial (a token ledger storing data during accumulation) with a focus on:
- designing a client external state: accounts are not stored on jam state, only the state merkle root.
- discuss cost of such design.
- have  a minimal external client state code example for educational purpose.

This tutorial will not attempt to:
- be secure, we keep skipping signature checks in refinement.
- be optimal, we use a very simple bounded, unoptimal, merkle state and proofs. For real use a proper third party implementation of state and state storage should be use (eg polkadot sdk).
- implement state distribution: each client should synch upon the last state root finalized in jam state. A disconnected client will lose ability to synch state if work items and work reports got pruned (GP 14.3.1 defines two retention periods, short live until finality for auditing and a long live for 28 days (672 slots) the datalake). Here we will not implement such client but simply launch all clients from a disk directory over a single state persistence, totally cheating on state distribution. Generally availability for client can be largely application centric. Work reports in blocks can also be largely used (but in practice only block changing jamt storage for the service are of interest).
- define proper role for distribution: every client are just validators with direct access the jam datalake and work items, on a real implementation, distribution strategy must fit the usecase.
- external client must handle fail or success accumulate processing, here when running test, we assume it will always succeed. A failure will put client in an invalid state. TODO should we backup old persistence files to rollback (sounds simple enough).

So the tutorial still stay mostly at service level.

## Single workitem state transition

This design simply put a batch of operations in a single work item. Processing of the batch is done in a single refinement call. Then refinement can directly transmit both new and old state root to accumulation which only update this root (if old root matches).

JAM persistence is therefore only:
- a key value for the current state root
- work item in the datalake.

### Testing

This tutorial can run the same examples as the token ledger one. One will observe that the logs are slightly different:
- transfer are logged in refine.
- transfer in refine are asociated with a workpackage hash and workitem (we could have a single extrinsic root).
- accumulate advance state root.
- accumulate display workitem processed or failure (can fail if two workpackage try to advance same external client state: only one get processed, failure need to be handled properly though).

### Prepare a workitem payload for refinement

From `token-ledger-builder-v2`
```
cargo run -- -i ./example_payloads/op_mint.json -o refinement_payload
```

This run locally the external client operations, and write a payload for refinement containing both input operations and the state witness to be able to run.
t

The json file shall contain all operation to run for a single slot. `op_mint.json` for instance will involves:  three balance value included of each minted token, and the tokens (as documented in code sample the state is simply includding all tokens everytime).

Codewise, client read full state from local disk persistence, then run operation from json, then extract witness from recorded state access, then binary encode both witness and operation into an external file, finally update persistence so next call will run on an updated state.

### Run on jam

Simply use jst as in previous tutorial but with `token-ledger-service-v2` as service crate, and always use `submit-file` command for work item, the payload must be the `refinement_payload` file produce in previous step).

### Example code

can be found in this git repository under `token-ledger-external-state` crate:
- external_client module is the dummy client external state implementation. Description of this state is out of the scope of this tutorial, but code has been written to be simple and easy to read (serializing deserializing all at once from file, simple binary tree for balances, single out of tree value to store all tokens ids).
- main.rs: produce payload for refinement:  just open external client state from last serializing, process state transition from operations in input json and a jam encoding binary payload in a file 
- lib.rs: the actual service, split into accumalution and refinement modules.

## Using a preimage 

We do the same as previously but do not write witness into the work item.

Instead we will publish a preimage for the service.

Note that this design do not look to good (delay for preimage is rather long and need three calls when a single one was used previously.

### Steps


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

## Exporting segment  (TODO?)

What if we want to use again the witness and operations later. Here we will tell the workitem to record witness and operation in one or multiple segments.

Segments created will be send to accumulation.

Accumulation, will then postpone the processing for any reason (in the code we just add a counter and only the second time accumulation see this item it will process it: obviously this is just a demo and makes little sense).

We then ask again for the segment(s) but do not attach witness, just the segment index (and we need to declare it in the workitem metas). Refinement will then import the segment decode it and send it again to accumulation, this time it will be processed.


### exporting segments

Refinement will just export the payload in a segment.

Putting this in place, when testing, one would observe an `ApiResult::StorageFull`.
This is due to the limit of exports define for the package, define in workitem `export_count`.
Therefore builder should update this value.
Here we just have to pass this info to the command that send this.
Note that if we use a larger number of exports (eg 3 to get margin), then accumulate will fail with a `BadExports` error.
So we use number of exports 1 here.




