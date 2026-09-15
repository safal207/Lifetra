# PostgreSQL Failure Recovery

Layer 16 hardens `PostgresUnifiedFencedStore` against transaction aborts, connection loss, and ambiguous COMMIT acknowledgement without collapsing unknown state into failure.

The core rule is:

```text
transaction error != one generic retry decision
```

The phase and PostgreSQL failure class determine whether a new transaction is safe.

## Failure taxonomy

`PostgresFailurePhase` records where a database error was observed:

- `Connect`
- `Begin`
- `Read`
- `Write`
- `Commit`
- `Reconcile`

`classify_postgres_failure` / `classify_postgres_sqlstate` map failures into four dispositions.

### Retryable aborted transaction

PostgreSQL SQLSTATEs:

- `40001` — serialization failure
- `40P01` — deadlock detected

mean PostgreSQL aborted the transaction. Lifetra may start the whole CAS transaction again from durable state.

```text
40001 / 40P01
      ↓
transaction aborted by PostgreSQL
      ↓
fresh transaction may retry
```

### Retryable before COMMIT

A connection-class failure observed before COMMIT cannot leave that still-open transaction committing later through the lost client connection.

```text
connection loss before COMMIT
      ↓
no successful COMMIT call is being assumed
      ↓
fresh transaction may retry
```

The retry is bounded by `PostgresTransactionRetryPolicy` (three attempts by default).

### COMMIT outcome unknown

A connection-class failure while COMMIT is being observed is different:

```text
COMMIT sent
    ↓
connection / acknowledgement lost
    ↓
UNKNOWN
```

Lifetra does **not** blindly execute the write again. `resolve_commit_outcome` opens a fresh connection and compares the durable record with the intended replacement.

| Durable observation | Resolution |
| --- | --- |
| exact intended replacement is present | `Applied` |
| previous expected revision is still present | `NotApplied` |
| competing payload owns the intended next revision | `Contended` |
| a later revision has already superseded the intended revision | `Unknown` |
| expected existing row disappeared | `Unknown` |

`NotApplied` may re-enter the bounded transaction retry loop. `Applied` returns success without a duplicate write. `Contended` returns CAS loss. `Unknown` remains fail-closed.

The important invariant is:

```text
COMMIT acknowledgement lost != COMMIT failed
```

and, after later revisions exist:

```text
superseded state != proof whether an older ambiguous COMMIT was ours
```

Lifetra therefore refuses to invent an answer from a later revision.

## SERIALIZABLE CAS

The PostgreSQL CAS path now begins each attempt with:

```sql
SET TRANSACTION ISOLATION LEVEL SERIALIZABLE
```

and preserves the existing boundary:

```text
BEGIN
  ↓
SELECT ... FOR UPDATE
  ↓
expected revision check
  ↓
DB-time transition validation
  ↓
whole-record UPDATE
  ↓
COMMIT
```

A serialization/deadlock abort restarts this complete unit rather than retrying only the final SQL statement.

## Retry policy

`PostgresTransactionRetryPolicy` bounds automatic retries. The default is three attempts. A zero-attempt policy normalizes to one attempt so configuration can never create an infinite/no-op retry loop.

When the budget is exhausted Lifetra returns `TransactionRetryExhausted` with the last database error text.

Automatic retry is intentionally narrow:

```text
known aborted transaction -> retry
known pre-COMMIT connection loss -> retry
ambiguous COMMIT -> reconcile first
permanent SQL error -> fail
```

## Fault-injection evidence

The test suite uses two complementary approaches.

1. A deterministic SQLSTATE harness verifies the semantic classification of serialization failure (`40001`), deadlock (`40P01`), connection-class failures, and permanent SQL errors without relying on timing-sensitive destructive database operations.
2. The PostgreSQL 17 integration suite runs the actual CAS path at `SERIALIZABLE`, including independent-connection contention against one revision.

The COMMIT-acknowledgement window is tested by deliberately discarding the caller-side meaning of an already committed replacement and asking `resolve_commit_outcome` to recover from durable state. Separate tests verify `Applied`, `NotApplied`, and the fail-closed `Unknown` result after that revision has been superseded.

## What this does not claim

This layer protects Lifetra's PostgreSQL state transition. It does not make a remote payment, blockchain transaction, HTTP call, or other external side effect part of the SQL transaction.

It also does not yet provide:

- jittered/exponential database retry backoff;
- connection pooling;
- automatic transaction replay across PostgreSQL failover;
- WAL/LSN-based COMMIT reconstruction;
- network-proxy fault injection at the exact COMMIT response byte boundary;
- provider-side exactly-once guarantees.

Those are separate production-hardening layers. External effects still use the existing fenced actuator + reconciliation protocol.
