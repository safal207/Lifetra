# Durable actuator receipt bridge

`DurableActuatorReceiptBridge` closes the evidence gap between a fenced downstream side effect and Lifetra's local durable execution history.

The bridge is intentionally a separate durable sidecar rather than a claim that the external actuator and `DurableJournal` participate in one atomic transaction.

## The crash window

A downstream resource can apply the logical effect and return `Applied`, but the Lifetra process can die before that receipt reaches the main journal:

```text
Durable PREP + operation identity
        ↓
Fenced actuator
        ↓
external effect APPLIED
        ↓
process crash
        ↓
local actuator receipt missing
```

On restart, absence of the local receipt is not interpreted as absence of the external effect.

The bridge has already persisted:

- `ActionId`;
- idempotency key;
- stable `operation_ref`;
- prepared worker identity;
- fencing epoch;
- attempt ordinal.

That identity is sufficient for a provider/resource adapter to reconcile the external side effect without inventing a local dispatch receipt.

## Durable ordering

Normal execution uses this order:

```text
main DurableJournal PREP
        ↓
bridge PREP (operation_ref + fenced permit)
        ↓
caller receives permit
        ↓
external fenced actuator
        ↓
bridge RCPT / RECO
        ↓
main DurableJournal projection
        ↓
bridge SYNC marker
```

The permit is not returned before both preparation records exist. An external effect therefore cannot legitimately be initiated through this API without durable operation identity.

`RCPT`/`RECO` is persisted before projection into the main journal. If the process dies after main-journal projection but before the bridge `SYNC` marker, replay checks the proof reference already present in the main journal and only writes the missing `SYNC` marker.

## Sidecar storage

The reference implementation stores each bridge event as an immutable sequence file:

```text
00000000000000000000.evt   BIND
00000000000000000001.evt   PREP
00000000000000000002.evt   RCPT or RECO
00000000000000000003.evt   SYNC
```

A record is written to a temporary file, `sync_all()` is called, the file is renamed to its final sequence name, and the directory is synced. Temporary files are ignored during replay. Missing sequence numbers fail closed.

This is a local-filesystem durability reference, not a substitute for a replicated production log. Directory durability semantics are platform/filesystem dependent.

## Projection semantics

Positive actuator evidence never fabricates a dispatch that Lifetra did not observe.

| Durable actuator evidence | Main journal projection |
| --- | --- |
| `Applied`, local dispatch exists | `ExecutionOutcome::Succeeded` external receipt |
| `Applied`, no local dispatch | reconciliation `EffectSucceeded` |
| `AlreadyApplied` | reconciliation `EffectSucceeded` |
| rejected actuator request | reconciliation `StillUnknown` |
| crash recovery finds effect | reconciliation `EffectSucceeded` |
| crash recovery finds nothing | reconciliation `StillUnknown` |
| identity conflict | bridge directive `BlockIdentityConflict` and no positive effect proof |

A rejection proves that a particular request was rejected; it does **not** prove that no earlier equivalent side effect exists. Therefore rejection is not converted into `NoEffectConfirmed` and cannot authorize retry by itself.

## Crash-after-apply recovery

```text
BIND(operation_ref)
      ↓
PREP(epoch 1, attempt 0)
      ↓
external APPLIED
      ↓
CRASH before local receipt
      ↓
worker B acquires epoch 2
      ↓
bridge recovery_query
  ActionId + idempotency key
  + operation_ref + ordinal
      ↓
external reconciliation
      ↓
EffectSucceeded + proof_ref
      ↓
bridge RECO (durable)
      ↓
project into DurableJournal
      ↓
CloseSucceeded
```

The external observation may prove that attempt 0's logical effect exists even though no local `DispatchReceipt` was ever recorded. The local history stays honest.

## Returning proof to the trajectory

A positive `DurableActuatorEvidence` can be converted with `as_supported_evidence()` into an `EvidenceRef::Supported` for a later `TrajectoryBead`.

```text
FencedActuatorReceipt / recovery proof
        ↓
DurableActuatorEvidence
        ↓
as_supported_evidence()
        ↓
next TrajectoryBead
```

This closes the evidence loop without rewriting the bead that existed before the external proof arrived.

## Core invariants

```text
operation identity must be durable before side effect
external Applied != local DispatchReceipt
durable receipt precedes main-journal projection
crash between stores != lost evidence
missing local receipt != no external effect
rejection != NoEffectConfirmed
AlreadyApplied != second side effect
positive actuator proof may seed the next bead
future proof does not rewrite past knowledge
```

## Production boundary

This layer does not make two independent storage systems transactionally atomic. It makes their divergence observable and recoverable.

Production deployments still need to choose storage and external reconciliation semantics appropriate to their failure model. A stronger future layer can place lease/fence state, operation binding, journal mutation, and projection markers behind one transactional/replicated store where that is desirable.
