# Reconciliation and retry authority

This layer closes the unsafe gap between `DispatchedEffectUnknown` and a possible redispatch.

## Core rule

```text
dispatched + unknown effect => reconcile first
```

An idempotency key is necessary for safe provider interaction, but it is not evidence that a retry is safe. The runtime still needs external reconciliation before it can authorize another dispatch of the same logical action.

## Identity

`IdempotencyBinding` permanently binds a provider-facing idempotency key to one `ActionId`.

```text
ActionId(action:42)
    <-> IdempotencyKey(idem:42)
```

The binding cannot be reused for another action identity.

## Reconciliation outcomes

`ReconciliationReceipt` records an externally supported observation after dispatch:

- `EffectSucceeded` — the effect happened successfully; close the action;
- `EffectFailed` — a known failed outcome; retry only if policy explicitly permits it;
- `NoEffectConfirmed` — external proof establishes that the side effect did not occur; retry may be allowed by policy;
- `StillUnknown` — reconciliation did not resolve the effect; keep reconciling and do not redispatch.

`NoEffectConfirmed` is intentionally stronger than a generic HTTP 404, missing row, timeout, or provider silence. Those observations are not sufficient on their own to prove that no side effect occurred.

## Retry verdicts

`RetryAuthority` returns one auditable verdict:

```text
InitialDispatchAllowed
ReconcileFirst
RedispatchAllowed { ordinal }
CloseSucceeded
CloseFailed
Block
```

The retry ordinal counts redispatches, not the initial dispatch.

## Fail-closed state machine

```text
AuthorizedNotDispatched
    -> InitialDispatchAllowed

DispatchedEffectUnknown
    -> no reconciliation      -> ReconcileFirst
    -> StillUnknown           -> ReconcileFirst
    -> EffectSucceeded        -> CloseSucceeded
    -> EffectFailed           -> policy decides retry or close
    -> NoEffectConfirmed      -> policy decides retry or close

EffectConfirmed(Succeeded)
    -> CloseSucceeded

EffectConfirmed(Failed)
    -> policy decides retry or close
```

A configured retry limit can turn an otherwise retryable outcome into `Block`.

## Invariants

```text
UNKNOWN after dispatch != retry permission
idempotency key != proof of no effect
not found != NoEffectConfirmed
reconciliation proof must match ActionId
redispatch preserves ActionId
redispatch preserves IdempotencyBinding
retry policy != external evidence
```

The runtime never turns a timeout, missing response, or unresolved lookup into implicit redispatch authority.

## Relationship to execution receipts

The previous layer establishes:

```text
AuthorityTicket
    -> DispatchReceipt
    -> ExternalExecutionReceipt
```

This layer handles the branch where a `DispatchReceipt` exists but no external outcome receipt is yet available:

```text
DispatchReceipt
    -> DispatchedEffectUnknown
    -> ReconciliationReceipt
    -> RetryAuthority
```

If redispatch is authorized, the same logical action identity and idempotency binding must be reused. A future attempt-ledger layer can record each physical dispatch attempt separately while preserving that one logical identity.

## Scope boundary

This MVP does not yet implement provider adapters, automatic polling, exponential backoff, retry scheduling, cryptographic receipts, attempt ledgers, or cross-provider consensus. It defines the conservative decision boundary those mechanisms must obey.
