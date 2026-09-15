# Downstream Fenced Actuator

`RecoveryLeaseManager` and `FencedDurableJournal` prevent a stale worker from mutating local execution state after a newer lease epoch takes ownership. That is necessary, but it is not sufficient for a network side effect.

A worker can pass a local lease check, pause, lose the lease, and then resume just before sending a request. If the external resource does not validate the fencing epoch itself, the old worker can still produce a stale effect.

This layer moves the fence to the side-effect boundary.

## Core invariant

```text
local lease check before network call
    !=
protection at side-effect commit time
```

A conforming actuator must compare the request's fencing epoch against authoritative downstream authority in the same atomic operation that commits the side effect, or use an external primitive with equivalent semantics.

A read-current-epoch operation followed by a separate write is still vulnerable to TOCTOU races and does not satisfy the contract.

## Request identity

`FencedActuatorRequest` is built from `FencedAttemptPermit` and contains:

- `ActionId`;
- recovery worker identity;
- fencing epoch;
- physical attempt ordinal;
- idempotency key;
- stable `operation_ref`.

`operation_ref` binds the semantic operation to the logical action. It can be a canonical command identifier, payload digest, immutable provider operation reference, or another stable representation.

```text
same ActionId
+ same IdempotencyKey
+ newer FencingEpoch
    != permission to change the operation
```

If the operation identity changes under an already-used action identity, the downstream boundary must reject it rather than apply a different effect.

## Outcomes

`FencedActuatorOutcome` distinguishes:

```text
Applied
AlreadyApplied { applied_epoch, applied_attempt_ordinal }
Rejected(...)
```

Rejections include:

```text
StaleEpoch
FutureEpoch
WrongOwner
LeaseExpired
IdentityConflict
```

A rejection is evidence that this actuator request was not accepted as a new side effect. It is not automatically evidence that no earlier attempt ever produced an effect.

## Duplicate semantics

If an effect was already applied for the same `ActionId`, idempotency key, and `operation_ref`, a later compatible request returns `AlreadyApplied` instead of applying the effect again.

This makes the distinction explicit:

```text
retry request received
    !=
second side effect applied
```

A newer epoch can therefore observe an effect produced under an older epoch without duplicating it.

## Proof boundaries

`FencedActuatorReceipt` is downstream effect evidence. It remains separate from local dispatch evidence.

```text
FencedAttemptPermit != DispatchReceipt
DispatchReceipt != FencedActuatorReceipt
FencedActuatorReceipt != local journal mutation
```

The provider-neutral controller validates that a receipt echoes the exact request identity and carries a non-empty proof reference, but it deliberately does not fabricate a local `DispatchReceipt` from downstream evidence.

If the local worker loses its lease after sending the request, the new recovery owner should reconcile the external proof through the existing provider-reconciliation path rather than allowing the stale worker to rewrite durable local history.

## Atomic downstream contract

`FencedActuatorAdapter` is a semantic contract. A production implementation must arrange an atomic boundary equivalent to:

```text
BEGIN ATOMIC RESOURCE OPERATION
    read authoritative action fence
    reject if request epoch/owner is not current
    reject if lease authority is expired
    reject if action/idempotency/operation identity conflicts
    if same effect already exists:
        return AlreadyApplied
    else:
        apply side effect
        persist effect identity + accepted fence
        return Applied
END ATOMIC RESOURCE OPERATION
```

Examples of possible concrete mechanisms include a transactional database row/version, compare-and-set resource version, smart-contract nonce/epoch rule, or provider-native conditional operation token.

An ordinary HTTP API that accepts an arbitrary `fencing_epoch` field but does not enforce it atomically does not provide fencing safety.

## In-memory reference actuator

`InMemoryFencedActuator` is a process-local reference implementation for tests and examples. It performs authority comparison, identity validation, and effect insertion inside one mutex critical section.

It is not a distributed production backend.

The reference actuator also exposes deterministic sink time and explicit authority installation so tests can model:

```text
worker A permit: epoch 1
        ↓
authority takeover: epoch 2
        ↓
worker A request arrives late
        ↓
Rejected(StaleEpoch { current_epoch: 2 })
        ↓
no effect inserted
```

## Remaining boundary

This layer closes the conceptual gap between local recovery fencing and downstream side-effect fencing. It still does not claim end-to-end exactly-once execution for arbitrary providers.

A strong production deployment still needs:

- a shared authoritative lease/fence backend;
- trustworthy time/TTL semantics;
- an actuator/resource that can enforce the epoch atomically;
- durable provider operation identity;
- reconciliation for lost responses;
- durable local journal recovery;
- provider-specific treatment of `AlreadyApplied` and rejection receipts.

The guarantee is therefore deliberately scoped:

```text
if the downstream resource enforces the current fence atomically,
a stale worker cannot create a newly accepted side effect with an older epoch.
```
