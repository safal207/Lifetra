# Attempt Ledger

`AttemptLedger` separates one logical external action from the physical network attempts used to execute it.

The logical identity remains stable:

```text
ActionId + IdempotencyBinding
```

Each physical dispatch receives its own append-only attempt identity:

```text
AttemptId { action_id, ordinal }
```

This distinction matters whenever an external response is lost or a retry is considered. A second network request is not a new logical action, but it is a new physical attempt with its own evidence history.

## Lifecycle

```text
AuthorityTicket
    ↓
ActionId + IdempotencyBinding
    ↓
Attempt #0 dispatch
    ↓
UNKNOWN
    ↓
reconciliation → NoEffectConfirmed
    ↓
RetryAuthority → RedispatchAllowed { ordinal: 1 }
    ↓
Attempt #1 dispatch
    ↓
external receipt → Succeeded
```

The ledger retains both attempts. Attempt #0 is not rewritten or deleted when attempt #1 begins.

## Evidence per attempt

Each `AttemptRecord` keeps:

- its `AttemptId` and ordinal;
- dispatch evidence;
- the proof refs that authorized that dispatch;
- zero or more reconciliation receipts;
- an optional external execution receipt.

The current attempt state is one of:

```text
DispatchedEffectUnknown
Reconciled(...)
EffectConfirmed(...)
```

## Retry safety

`AttemptLedger::record_dispatch()` does not trust a `RetryDecision` blindly.

For a redispatch it independently checks that:

1. the `ActionId` matches the ledger;
2. the idempotency key matches the ledger binding;
3. the requested ordinal is exactly the next append-only ordinal;
4. the previous attempt is already in a retry-safe state;
5. the retry decision carries the proof that made the previous attempt retry-safe;
6. the new dispatch does not move backward in time.

Therefore a forged `RedispatchAllowed` cannot convert an unresolved attempt into permission to send another request.

## Derived retry count

`retry_context()` derives `redispatches_used` from the number of recorded attempts:

```text
0 attempts → 0 redispatches
1 attempt  → 0 redispatches
2 attempts → 1 redispatch
3 attempts → 2 redispatches
```

Callers do not need to maintain an independent retry counter that can drift away from the evidence ledger.

## Late evidence and conflicts

Late evidence is preserved instead of being discarded.

Example:

```text
attempt #0 → dispatch → UNKNOWN
attempt #0 → reconciliation says NoEffectConfirmed
attempt #1 → dispatch
attempt #0 → late external SUCCESS
```

The late success contradicts the earlier no-effect conclusion. The ledger reports conflicting evidence and blocks generation of a new retry view.

This is deliberate. The system must reconcile the contradiction before taking another side-effecting action.

## Core invariants

```text
one logical ActionId != one physical attempt
attempt ordinals are append-only
retry count derives from attempt history
UNKNOWN attempt != redispatch permission
retry permission must carry resolution-proof lineage
late success != ignorable noise
conflicting attempt evidence => block further retry
```

## Exactly-once boundary

The attempt ledger provides the audit structure needed to reason about at-most-once and exactly-once protocols, but it does not claim to create an exactly-once guarantee by itself.

Such a guarantee also depends on external properties, including provider idempotency semantics, durable provider-side identity, reconciliation completeness, and recovery behavior.

The ledger's narrower responsibility is to ensure that Lifetra never hides physical attempts or silently converts uncertainty into retry permission.
