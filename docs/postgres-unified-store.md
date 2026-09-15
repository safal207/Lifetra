# PostgreSQL Unified Fenced Store

Layer 15 turns the storage contract from Layer 14 into a shared PostgreSQL-backed coordination boundary.

The goal is not to claim that PostgreSQL makes an arbitrary external side effect transactional. The goal is narrower and concrete: make Lifetra's action binding, lease/fencing authority, prepared attempts, execution evidence, reconciliation evidence, and projection markers durable and mutually serialized across independent processes.

## Storage model

`PostgresUnifiedFencedStore` stores one complete `UnifiedActionRecord` per logical `ActionId`.

```text
lifetra_unified_actions
┌────────────────────────────────────┐
│ action_id  PRIMARY KEY             │
│ revision                           │
│ payload                            │
│ updated_at                         │
└────────────────────────────────────┘
```

The payload preserves the Layer 14 record:

```text
UnifiedActionRecord
├─ ActionId + idempotency key + operation_ref
├─ current lease owner / epoch / revision / expiry
├─ prepared physical attempts
├─ local dispatch evidence
├─ external or reconciliation evidence
└─ proof projection markers
```

The reference codec is versioned as `PGU1`. It is deliberately simple and inspectable; future schema evolution should add explicit codec/schema migration rather than silently changing existing records.

## Transactional CAS

A record mutation with an existing revision follows this shape:

```text
BEGIN
  SELECT revision, payload
  FROM lifetra_unified_actions
  WHERE action_id = $1
  FOR UPDATE

  compare expected revision
  read clock_timestamp()
  validate control transition against DB time

  UPDATE ...
  WHERE action_id = $1 AND revision = expected
COMMIT
```

The entire replacement record is committed together. Two workers that start from the same revision cannot both commit a different next state.

```text
worker A reads revision 12
worker B reads revision 12

A locks row → validates → commits revision 13
B then locks row → sees revision 13 → CAS false
```

`CAS contention != partial success`.

## PostgreSQL time is the lease time authority

`PostgresUnifiedFencedRuntime` obtains lifecycle timestamps from:

```sql
clock_timestamp()
```

More importantly, the store repeats lease-sensitive validation *inside the locked transaction* using PostgreSQL time.

That closes this race:

```text
worker reads DB time while lease valid
        ↓
lease expires
        ↓
worker reaches CAS later
```

The CAS transaction must reject a prepare/dispatch mutation if the lease is expired at commit-time validation.

```text
process clock != lease authority
preflight DB-time check != commit-time DB-time check
```

## Execution authority and evidence admission remain different

A stale or expired lease cannot create new execution intent.

```text
expired epoch → no PREP
stale epoch   → no new DISPATCH state
```

But a receipt that truthfully describes a side effect performed under a previously recorded attempt can arrive after ownership changes. That historical evidence remains admissible if its stable identities match the recorded attempt.

```text
current fence != ownership of historical truth
```

This is intentional: fencing limits future authority; it does not erase facts about the past.

## Migration concurrency

Multiple processes may start at once. PostgreSQL can race in system catalogs even when each session uses `CREATE TABLE IF NOT EXISTS` for the same not-yet-created relation.

`migrate()` therefore takes a transaction-scoped PostgreSQL advisory lock before the DDL. The lock is released automatically when the migration transaction commits or rolls back.

The migration lock is only a schema-startup boundary; normal action coordination uses row locking and record revisions.

## Real integration tests

GitHub Actions starts a PostgreSQL 17 service and provides `LIFETRA_TEST_POSTGRES_URL` to the Rust checks job.

The integration suite exercises:

- payload round-trip through PostgreSQL;
- authoritative DB time;
- concurrent CAS from independent TCP connections;
- DB-time rejection of an execution mutation after real lease expiry;
- late external success evidence;
- persistence of `NoEffectConfirmed` proof lineage into a retry attempt;
- concurrent startup-safe migration.

Tests skip only when `LIFETRA_TEST_POSTGRES_URL` is absent, allowing ordinary local builds without requiring a database.

## What PostgreSQL does not make atomic

The following is still two systems:

```text
PostgreSQL transaction
        ↓
commit PREP / fence / identity
        ↓
remote payment / chain / HTTP side effect
```

Unless that external resource participates in the same transactional mechanism, a crash can still occur between PostgreSQL commit and the remote side effect, or between the side effect and the next PostgreSQL receipt commit.

That is why the previous layers remain necessary:

- stable `ActionId` and `operation_ref`;
- downstream fencing;
- provider idempotency;
- external reconciliation;
- proof-preserving recovery.

PostgreSQL removes process-local coordination from the critical state-store guarantee. It does **not** by itself provide end-to-end exactly-once execution.

## Production boundaries still open

The current backend intentionally remains small. Production hardening can add:

- a connection pool instead of one fresh client per operation;
- TLS and secret-managed credentials;
- versioned SQL migrations;
- retry/backoff classification for deadlock/serialization/network failures;
- HA/failover testing and connection-loss injection;
- PostgreSQL-native typed columns/indexes where operational querying justifies them;
- metrics and tracing around lock wait, contention, transaction latency, and fence rejection;
- provider-specific fenced actuators and reconcilers.

The key invariant for this layer is:

```text
shared PostgreSQL transaction semantics
    replace process-local mutex semantics

but

internal SQL transaction
    != arbitrary remote side-effect transaction
```
