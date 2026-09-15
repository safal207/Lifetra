# Provider reconciliation adapters

`ProviderReconciler` bridges a durable recovery directive to provider-specific external evidence without teaching Lifetra about any particular payment rail, HTTP API, database, blockchain, or queue.

## Boundary

The runtime may recover in either of two ambiguous states:

```text
ReconcilePreparedAttempt
ReconcileDispatchedAttempt
```

The first means a write-ahead `PreparedAttempt` is durable but no local dispatch receipt survived. The second means a local dispatch receipt is durable but the external effect remains unknown.

Both require external reconciliation before another side effect may be authorized.

## Provider-neutral query

The adapter receives:

```text
ActionId
IdempotencyBinding
Attempt ordinal
Phase: PreparedAmbiguous | DispatchedUnknown
```

A concrete adapter may map these values to a provider operation lookup, idempotency-key query, transaction search, settlement ledger, chain transaction, or another authoritative source.

The core does not infer provider semantics.

## Provider observation

An adapter returns a `ProviderObservation`:

```text
observed_at
outcome
proof_ref
```

where `outcome` is one of:

```text
EffectSucceeded
EffectFailed
NoEffectConfirmed
StillUnknown
```

`NoEffectConfirmed` is deliberately strong. A generic 404, timeout, missing response, provider outage, or empty search result must remain `StillUnknown` unless the provider's documented semantics plus external evidence actually establish that the side effect did not occur.

## Durable-before-decision rule

`ProviderReconciler::reconcile_once` persists the provider observation through `DurableJournal::record_reconciliation` before returning a retry or close verdict.

```text
provider lookup
      ↓
external proof
      ↓
durable reconciliation record
      ↓
retry / close verdict
```

Therefore a process crash after the lookup but before the caller receives the verdict can recover the evidence instead of repeating the lookup result from memory.

## Prepared ambiguity does not fabricate dispatch

A provider may discover that a crash-ambiguous prepared operation succeeded even though Lifetra has no local dispatch receipt.

That remains provider reconciliation evidence. Lifetra does not manufacture a historical `DispatchReceipt`.

If the provider proves `NoEffectConfirmed` or a retryable failure for prepared attempt `n`, policy may authorize attempt `n + 1`. `AttemptLedger` permits that proof-backed ordinal gap while preserving the fact that no local dispatch receipt exists for `n`.

Example:

```text
Prepared #0
   ↓ crash
Provider: NoEffectConfirmed #0
   ↓ durable proof
RedispatchAllowed #1
   ↓
Prepared #1
   ↓
DispatchReceipt #1
```

The physical dispatch ledger contains attempt `#1`, not a fabricated attempt `#0`.

## Fail-closed behavior

The adapter boundary preserves these invariants:

```text
provider observation != local dispatch receipt
provider lookup failure != NoEffectConfirmed
timeout / silence / generic not-found != proof of no effect
external proof is durable before retry permission
StillUnknown => reconcile again, never blind retry
prepared resolution may advance ordinal without inventing dispatch history
```

If the adapter itself returns an error, the durable journal is not mutated by `ProviderReconciler`.

## What this layer does not guarantee

The trait makes reconciliation pluggable; it does not make arbitrary provider APIs authoritative. A concrete adapter still has to define:

- which provider endpoint or ledger is authoritative;
- how `ActionId` and the idempotency key map to provider identity;
- what qualifies as positive success/failure proof;
- under exactly which semantics absence can become `NoEffectConfirmed`;
- how stale or eventually-consistent provider reads are handled;
- how proof references can later be inspected or verified.

End-to-end exactly-once behavior still depends on those provider semantics as well as durable local storage and retry policy.
