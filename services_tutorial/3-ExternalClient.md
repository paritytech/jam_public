
## Moving data out of JAM database


Previous tutorial step was writing all account balances and minted token into Jam service step.
This is fine as a tutorial to show how to use service Jam storage and how to define a service, but this defeat general Jam design by chugging Jam storage with service specific data.

This accumulation centric design is very similar to the way ethereum did work at launch: a single state shared by all peers.

Remember, refinement is generally cheap, accumulation is expensive.
So we will move as much as possible processing out of this accumulation process, and run things mostly in refinement.
This means accessing actual balances from refinement.
The trick is to simply move all the storage of the service out of Jam accumulation and attach proofs/witness to refinement. 

Refinement will run over a partial state included in the workitem.
During refinement, operations are run against this partial state.
In this tutorial, we call this partial state the witness as it is the witness of a given state transition, it is also often refered as a state proof (eg in polkadot).

The data will no longer be stored on Jam, but will be external data that clients must manage and track.
So clients store and share a common state, then send proofs of state transition to Jam. Jam refinement will validate those state transition proofs, and Jam accumulation will only store and update the state merkle root.

Note that refinement on itself can run on a valid proof that is not synched with client, things get only fully validated when during accumulation step we check that the transition used in refinement is effectively running against the correct previous root.


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
- external client must handle fail or success accumulate processing, here the dummy client implementation will assume success and update its state directly when producing the workitem (a non dummy client should only be committed after accumulation). So a failure will put client in an invalid state (in practice the storage write a different state serialization for each new state root and it is possible to revert by changing HEAD file pointer to a different serialization file).
- Do real parrallel processing, since our usecase can fail (multiple parallel transaction overspending an account), doing parallel refinement is rather complex. Even if part of the tutorial (see segments) put foundation to try to resolve multiple parallel state transitions, this is too involve for the tutorial. Therefore every state transition must run sequentially: every state transition must start from the state of the previous one.  

So the tutorial still stay mostly at service level but also involve a lot more client dummy implementation.

## Direct workitem state transition

This design simply put a batch of operations in a single work item. Processing of the batch is done in a single refinement call. Then refinement can directly transmit both new and old state root to accumulation which only update this root (if old root matches).

JAM persistence is therefore only:
- a key value for the current state root
- work items shared in the datalake.

### External state

The attached code provide a simple external state. It simply store accounts in a small binary tree of 2^15 keyspace elements, failling on key collision (keys are stored alongside the balance).
Token ids are stored as a single value and just hashed against binary tree root to produce state root.

Witnesses do not attempt to reduce footprint and will store all sibling of every keys with redundancy.
Full state serializing is done after all state transition (usually done by the command line producing the jam workitme), with no recovery of errors.
Full state deserializing is done on each command call.


### Testing

This tutorial run the same examples as the token ledger one. One will observe that the logs are slightly different:
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
This time, since we got this additional processing, we do not deserialize json in refinement but a binary encoded payload.


The json file shall contain all operation to run for a single slot. `op_mint.json` for instance will involves:  three balance value included of each minted token, and the tokens (as documented in code sample the state is simply includding all tokens everytime).

Codewise, client read full state from local disk persistence, then run operation from json, then extract witness from recorded state access, then binary encode both witness and operation into an external file, finally update persistence so next call will run on an updated state.

### Run on jam

Simply use jst as in previous tutorial but with `token-ledger-service-v2` as service crate, and always use `submit-file` command for work item, the payload must be the `refinement_payload` file produce in previous step).

### Example code

The code under the git repository is split between three crates:
- `toker-ledger-state-v2`: merkle state, and state transition logic. Is `no_std`.
- `token-ledger-builder-v2`: the client part: data serializing and can command line to build input payloadata. Depends on the state transition from `token-ledger-state-v2`. Is not `no_std`.
- `token-ledger-service-v2`: the Jam service code, refine and accumulation logic, largely depend on the state transition of `token-ledger-state-v2`. Is `no_std`

Code running direct workitme is executed when `Mode` enum is set to `Direct`.

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

## Exporting segment 


This time we will demonstrate storing payload in a segment. The usecase is to delay processing of state transition, but is also largelly artificial, yet using segment to store service data seems a lot more sound than primage.

Code is run when `Mode` enum is set to `Preimage`.


Segments allows use use the witness and operations in later processing without having to attach them against.

It make a lot of sense in large transformation, eg data mining things over multiple workitem (map), export results in segments and finally reduce these segments.

In our case we cannot really map in parallel because then reduce operation can fail and make things rather impractical, so for the demo we only allow sequential state transitions.

Segments are mainly using two api here:
- refinement export: the segment is created and shared, workitem number of exported segment declaration must match 
- refinement import: the segment is read during refinement, the segment definition is attached to workitem and import will just use an index into these declaration.


## Steps


#### exporting segments


A workitem witness will record state transition in the same way as in our "direct" example and produce a similar payload.

Refinement will just export the payload in a segment.

Refinement run as in `Direct` example: validate all operations, but instead of sending directly the result to accumulation, it will store the payload in a segment by using the `export` host function.

Just using `export` will fail into an `ApiResult::StorageFull` error.

The number of Export must be strictly define in workitem `export_count` description.
Therefore builder should update this value.

In attached code we build payload as before with an additional `--segment` parameter.  

Note that if we use a larger number of exports (eg 3 to get margin), then accumulate will also fail with a `BadExports` error.
So we use number of exports 1 here.

Also note that segment size is fix. Here we assume the payload will fit in a single segment. In real world design, one will allow to fill the segment with data to reduce cost.

On the client side, we must track these exported segments.
Here for testing I just use the exported segment number and workpackage hash from the log.

Following accumulation is will track exported segment by hash in a single service storage value. This tracking allows us to invalidate importing unexpected segments, note that it is a bit overkill and single monotonic counter could be enough.

#### Importing segments

After a few segments are buffed, we would want to actually process them and update the root stored by accumulation 

This is done by asking to process again for the segment(s), but do without attached witnesses. Witnesses will just be read from previously exported segments.

We define a special workitem operation to trigger this. 
Refinement will import all declared segment import for the workitem and process them. This time it will send root update to accumulation as in `Direct` example.
The segments infos must be attached to the workitem desscription.

In attached code, the payload do not contain anything dynamic and is always encoded to "0300000000".
The dynamic data is the list of the workitems to process, this list as the `export_count` is always written in the workitem definition.
With a tool I am using to rpc a new item for my node the syntax is :
```
mytool item --import-package-hash c5d3d11b9163e8f30fb8cb9bb5e06321441dd44686b0983d82d54e297ddb817f --import-package-index 0 97a7303e  0x0300000000
```
With 97a7303e being my service id and 0x03000000 the static workitem payload.


Using segments export and import we manage to buffer some inputs, and delay processing without using very little accumulation storage.

We did not cover the client access to segment.


#### Sidenotes on design

This is largely a demo, and usecase is not so good for the following reason:

- tracking segment in accumulation: the storage in jamt service is way to big. One cound just rely on two monotonic counters for exported segments and processed segments. Then resolve all info from exported segments or even refinement workitem.

- map reduce: the refinement consuming segments is forcing a sequence of segments, so it makes no sense to put the whole witness in segment, only the root change is really needed.
Proper usecase would be on process that refine to smaller exported segment dataset and merge segments through an infaillible refinement processing.

- delay transaction: makes not a lot of sense. Generally data exported in segment should be transformed data, and really make sense when segment get use in a later computation. Here we just directly copy, so one can say that the design is wrong, but it also lay foundation for mork complex state update (one that would require interaction between non sequential segments processing). 
