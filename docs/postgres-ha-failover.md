# PostgreSQL HA Failover Evidence

Layer 18 extends the PostgreSQL-backed unified fenced store from single-node durability into a real primary/standby failover topology in CI.

The goal is narrow and testable: prove that a Lifetra action record acknowledged by the primary under synchronous physical replication is already applied on the standby, survives loss of the primary, and remains writable after the standby is promoted.

## Topology

The CI job starts two PostgreSQL 17 containers on a private Docker network:

```text
PostgreSQL primary
    |
    | physical streaming replication
    | application_name=lifetra_standby
    v
PostgreSQL standby
```

The standby is created with `pg_basebackup` and a physical replication slot. After streaming begins, the primary is configured with:

```text
synchronous_standby_names = FIRST 1 (lifetra_standby)
synchronous_commit = remote_apply
```

The test waits until `pg_stat_replication` reports `streaming:sync` before any Lifetra HA proof is written.

## Why `remote_apply`

For the tested commit, success from the primary must mean more than "WAL reached the primary disk".

With `remote_apply`, PostgreSQL does not acknowledge the commit until the synchronous standby has replayed it. The test therefore treats the acknowledged Lifetra record as RPO-zero evidence for this exact one-primary/one-standby topology and configuration.

This is not a universal PostgreSQL RPO claim. Changing the replication mode, synchronous standby policy, quorum, storage, or acknowledgement setting changes the guarantee.

## Lifetra proof sequence

`examples/postgres_ha_failover_probe.rs` writes more than a marker row. It persists one real unified action with:

- stable `ActionId`;
- idempotency key;
- `operation_ref`;
- recovery lease with fencing epoch `1`;
- prepared physical attempt `0`.

The CI sequence is:

```text
primary is writable
    ↓
create action
    ↓
acquire fencing epoch 1
    ↓
prepare attempt 0
    ↓
COMMIT acknowledged under remote_apply
    ↓
read the same binding / revision / epoch / attempt on standby
    ↓
kill primary
    ↓
promote standby
    ↓
renew the same application fencing epoch on the promoted leader
    ↓
record revision + lease revision advance
```

The read from the standby happens before the primary is killed. That separates "replication had the state" from "promotion happened to recover something later".

## Fencing continuity across database leadership change

Database leadership and Lifetra execution authority are different dimensions.

```text
database primary role changes
    !=
automatic Lifetra fencing epoch change
```

The promoted standby loads the replicated lease and constructs the same `UnifiedFencingToken` for owner + epoch `1`. A successful `renew` proves that the promoted database can continue the existing application authority record without resetting the fencing epoch or losing the prepared attempt.

The renewal increments the lease revision and the enclosing unified record revision. That provides a post-promotion write proof rather than a read-only copy check.

## What this proves

Inside the CI topology, Layer 18 proves:

1. physical streaming replication is actually established;
2. the selected standby reaches synchronous `sync` state;
3. acknowledged Lifetra state is visible on the standby before primary loss;
4. `docker kill` removes the old primary from service;
5. the standby can be promoted to a writable PostgreSQL leader;
6. the same Lifetra action identity, operation identity, fence epoch, and prepared attempt survive promotion;
7. a new fenced mutation succeeds on the promoted leader.

## What this does not prove

This is a controlled two-node failover proof, not a complete production HA system.

It does **not** yet provide:

- automatic leader election;
- a stable client endpoint that routes to the current primary;
- STONITH or proof that an old primary cannot later reappear writable;
- automatic rewind/rejoin of the former primary;
- multi-standby quorum behavior;
- a numeric RTO SLA;
- cross-host clock-skew bounds for lease expiry;
- synchronous-replication availability under standby loss;
- TLS, managed credentials, pooling, or production orchestration.

Two boundaries are especially important:

```text
promotion != fencing a resurrected old primary
```

and

```text
RPO=0 for a remote_apply acknowledged record
    !=
zero data loss under every PostgreSQL configuration
```

The CI intentionally leaves the killed primary down after promotion. A later layer should test old-primary resurrection and cluster-level node fencing before claiming split-brain-safe database leadership.

## Availability tradeoff

Synchronous `remote_apply` reduces the loss window for acknowledged commits, but it also couples primary write availability and latency to the synchronous standby. If the required standby is unavailable, commits can block.

Lifetra should therefore treat replication policy as part of the durability contract, not as a transparent implementation detail.

## CI evidence

The dedicated `postgres-ha` job performs the full topology bootstrap and teardown independently from the ordinary PostgreSQL integration tests. It uses separate Docker volumes and ports so the HA proof does not inherit state from the single-node test database.

The job must pass each boundary separately:

- start primary;
- bootstrap physical standby;
- require synchronous `remote_apply` replication;
- seed fenced state on primary;
- verify committed state on read-only standby;
- kill primary and promote standby;
- continue fenced state on the promoted leader;
- clean up both nodes and volumes.

This keeps the failover evidence inspectable instead of collapsing the whole scenario into one opaque script.
