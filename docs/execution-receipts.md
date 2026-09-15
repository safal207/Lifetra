# Authority tickets and execution receipts

The execution-receipt layer closes the boundary between **permission to act** and **evidence that an action actually happened**.

The model preserves one stable `ActionId` across the full lifecycle:

```text
AuthorityDecision::Allow
        |
        v
AuthorityTicket(action_id)
        |
        v
DispatchReceipt(action_id)
        |
        v
ExternalExecutionReceipt(action_id)
```

## Core invariants

```text
authorized != dispatched
dispatched != effect confirmed
missing external receipt != failure
missing external receipt != success
approval != execution proof
action identity must remain stable across the lifecycle
```

An `AuthorityTicket` is a permission receipt. It can only be issued from `AuthorityVerdict::Allow`. It preserves the source bead, execution mode, and authority proof lineage, but does **not** claim that a tool call or external side effect was executed.

A `DispatchReceipt` is separate evidence that the action crossed the dispatch boundary. It requires its own non-empty proof reference.

After dispatch and before an externally verifiable outcome, the trace is explicitly:

```text
ExecutionStatus::DispatchedEffectUnknown
```

This state is intentional. A timeout, lost response, silent child process, or missing remote acknowledgement must not be converted into success or failure.

An `ExternalExecutionReceipt` resolves the effect state to either `Succeeded` or `Failed`, using a separate external proof reference.

## Temporal ordering

The MVP enforces monotonic time:

```text
AuthorityTicket.issued_at
    <= DispatchReceipt.dispatched_at
    <= ExternalExecutionReceipt.observed_at
```

Receipts that violate this ordering are rejected.

## Duplicate boundaries

The MVP permits exactly one dispatch receipt and one external outcome receipt per `ExecutionTrace`.

Duplicate dispatch or duplicate outcome attempts are rejected rather than silently replacing evidence. This protects reconciliation and future exactly-once reasoning.

## Relationship to beads

Execution receipts do not automatically rewrite trajectory history.

After an allowed action executes, a higher layer should create a new `TrajectoryBead` containing the newly observed execution evidence. The previous bead remains unchanged.

Conceptually:

```text
Bn
 -> AuthorityDecision
 -> AuthorityTicket(action_id)
 -> DispatchReceipt(action_id)
 -> ExternalExecutionReceipt(action_id)
 -> Bn+1
```

This preserves the existing rule that future evidence adds a new local truth context instead of retroactively editing an earlier one.

## Current boundary

The current implementation models one linear action lifecycle. It does not yet define:

- provider-specific idempotency keys;
- retries or redispatch authority;
- reconciliation against an external ledger;
- expiry or revocation of authority tickets;
- multi-step transactions;
- compensating actions or rollback receipts;
- cryptographic signatures over receipts;
- distributed consensus over action identity.

Those can be layered on without weakening the core distinction between authorization, dispatch, and externally confirmed effect.
