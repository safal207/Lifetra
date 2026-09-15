# Crash-Safe Durable Journal

`DurableJournal` closes the process-memory gap between execution authority, physical attempts, and restart recovery.

The journal is an append-only file-backed write-ahead log for one logical `ActionId` and one `IdempotencyBinding`. Every accepted record is followed by `File::sync_all()` before the API returns.

## Core ordering

For a side effect, the safe local order is:

```text
RetryDecision / authority
        ↓
PreparedAttempt
        ↓
append + fsync
        ↓
external dispatch
        ↓
Dispatch record
        ↓
append + fsync
        ↓
external receipt / reconciliation
        ↓
append + fsync
```

The key boundary is:

```text
prepared != not dispatched
```

A process can crash after the external call crossed the boundary but before the process durably records dispatch. Therefore a recovered `PreparedAttempt` without a durable dispatch record is ambiguous.

Lifetra recovers that state as:

```text
RecoveryDirective::ReconcilePreparedAttempt { ordinal }
```

It never converts it back into permission for a blind send.

## Recovery directives

`recover()` replays complete durable records and produces a reconstructed `AttemptLedger` plus one explicit directive:

- `ReadyForInitialPreparation` — no physical attempt has been prepared yet;
- `ReconcilePreparedAttempt` — a write-ahead prepare survived but there is no durable dispatch receipt; dispatch may have happened;
- `ReconcileDispatchedAttempt` — dispatch is durable but its effect is still unknown;
- `EvaluateRetry` — the latest attempt has durable evidence such as `NoEffectConfirmed` or failure and may be evaluated by `RetryAuthority`;
- `CloseSucceeded` — success is durably known;
- `Block` — evidence is contradictory or prior-attempt history makes a new retry unsafe.

A journal reopened with a pending prepared attempt cannot call `record_dispatch()` for that old preparation. This prevents a restart from fabricating a dispatch receipt for work whose process-local dispatch boundary is no longer knowable.

Provider-specific resolution of `ReconcilePreparedAttempt` is intentionally outside this file-backed MVP. A future provider adapter must query an external ledger/idempotency endpoint and produce explicit evidence rather than inferring that the call did not happen.

## Attempt ledger replay

Durable dispatch, reconciliation, and external-outcome records are replayed through the existing `AttemptLedger` APIs. This preserves the same invariants after restart that apply in-process:

```text
one logical ActionId != one physical attempt
UNKNOWN after dispatch != retry permission
StillUnknown != retry permission
late success != ignorable noise
conflicting evidence => block
```

Retry count is reconstructed from durable attempt history rather than a separate mutable counter.

## Torn writes and corruption

Each journal line carries:

- a monotonic sequence number;
- a versioned payload (`LJ1`);
- a 64-bit FNV-1a checksum over the sequence and payload.

If the final record is truncated and does not end in a newline, `open()` trims that incomplete tail back to the last complete record and fsyncs the repaired file. Recovery then proceeds from the last complete durable boundary.

A complete record with a bad checksum, a sequence gap, or invalid encoding is rejected. It is not silently skipped.

The checksum detects accidental corruption/torn-record inconsistencies. It is **not** a cryptographic signature and does not provide authenticity against an attacker.

## Durability boundary

This implementation calls `File::sync_all()` after every journal append and after trimming an incomplete tail. That is a meaningful process-crash durability barrier, but absolute power-loss guarantees remain dependent on the operating system, filesystem, storage hardware, mount options, and directory-entry durability semantics.

This layer is therefore a file-backed durable-journal MVP, not a replicated WAL or consensus log.

## Exactly-once boundary

The durable journal materially improves restart safety, but it does not claim exactly-once execution by itself.

An exactly-once or at-most-once protocol still depends on:

- provider idempotency semantics;
- a durable external operation identity;
- reconciliation completeness;
- crash-safe provider adapters;
- retention and recovery policy;
- correct handling of late and contradictory evidence.

The journal's narrower guarantee is that Lifetra does not intentionally forget its durable local execution boundary and then turn uncertainty into retry permission.

## Example

Run:

```bash
cargo run --example durable_journal_recovery
```

The example durably prepares and records an initial dispatch, simulates a process restart, observes `ReconcileDispatchedAttempt`, records `NoEffectConfirmed`, and then reuses `RetryAuthority` to derive a bounded redispatch decision.
