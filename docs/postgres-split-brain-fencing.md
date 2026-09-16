# PostgreSQL split-brain fencing and old-primary rejoin

Layer 19 extends the PostgreSQL HA proof from failover into the old-primary resurrection boundary.

The problem is not merely whether a standby can be promoted. After promotion, the former primary may later become reachable again. If that node can return writable before it is reconciled with the new leader, the database layer can split into two independent writers even though Lifetra's application-level fencing record was preserved correctly.

This layer therefore treats old-primary exclusion and rejoin as a deployment authority boundary around `PostgresUnifiedFencedStore`.

## Tested topology

The integration workflow creates:

```text
Lifetra
   |
   | one stable PostgreSQL DSN
   v
HAProxy endpoint
   |
   +----> primary A
             |
             | synchronous physical replication
             | synchronous_commit = remote_apply
             v
          standby B
```

The client DSN remains stable throughout the test. The endpoint initially routes new connections to A and, after failover, routes them to B.

The endpoint is test infrastructure. It demonstrates that Lifetra does not need a node-specific DSN, but it is not an automatic production leader-election service.

## Initial replicated authority

Before failure, CI requires B to be `streaming:sync` and A to use `synchronous_commit=remote_apply`.

Through the stable endpoint, Lifetra persists:

- one stable `ActionId`;
- one idempotency key;
- one `operation_ref`;
- lease/fencing epoch `1`;
- prepared attempt `0`.

The same record must be readable from the recovery-mode standby before A is lost.

Observed initial state:

```text
leader endpoint / A: revision=2 epoch=1 attempt=0
B standby:           revision=2 epoch=1 lease_revision=0
```

## Power-fence simulation before promotion

The old primary is not merely stopped and left restartable. The test:

1. kills its PostgreSQL container;
2. removes the runnable container while preserving the data volume;
3. verifies the old host port is closed;
4. only then promotes B.

Within the Docker CI topology, removing the runnable container models a STONITH-style exclusion: the former primary cannot resume as a PostgreSQL server until the harness deliberately reconstructs it.

This is **not** a claim of production hardware/cloud fencing. Real deployments need an independent fencing mechanism whose failure modes are outside the failed database node itself.

## Stable endpoint after failover

After B is promoted, the HAProxy backend is changed from A to B while Lifetra continues to use the same DSN.

Lifetra then renews the existing replicated lease:

```text
promoted leader B: revision=3 epoch=1 lease_revision=1
```

Database leadership changed; application authority did not. Promotion does not manufacture a new Lifetra fencing epoch.

## Making the old data directory rewind-safe

PostgreSQL requires the `pg_rewind` target to be cleanly shut down. A killed primary is not guaranteed to satisfy that condition.

The workflow therefore mounts A's preserved data volume into a maintenance-only PostgreSQL container with `--network none`:

```text
old A data volume
      |
      | crash recovery
      | no network interface
      | clean fast shutdown
      v
rewind-safe target
```

This lets PostgreSQL complete crash recovery and create a clean shutdown state without restoring any client or replication authority to the old primary.

The cluster is initialized with data checksums, satisfying one of PostgreSQL's prerequisites for `pg_rewind`.

## Rewind before resurrection

B creates a dedicated physical replication slot for the returning node. A's stopped data directory is then reconciled against B using `pg_rewind --write-recovery-conf`.

The test observed an actual timeline divergence and successful rewind:

```text
pg_rewind: servers diverged ...
pg_rewind: rewinding from last common checkpoint ...
pg_rewind: Done!
```

The target is configured with `standby.signal` and the rejoin replication slot before it receives a network listener again.

## Resurrection is standby-only

Only after rewind does the old data volume return as a PostgreSQL process. The workflow requires:

```sql
SELECT pg_is_in_recovery();
-- true
```

A direct write is then attempted and must fail. The observed PostgreSQL error is:

```text
cannot execute CREATE TABLE in a read-only transaction
```

The rewound node also reads the same Lifetra authority state:

```text
rejoined former A: revision=3 epoch=1 lease_revision=1
```

So the former primary does not return as an independent writer and does not reset the application fencing history.

## Restoring synchronous redundancy

Rejoin is not considered complete merely because A is read-only. B is configured to use the rewound former A as its synchronous standby, and CI waits for:

```text
pg_stat_replication => streaming:sync
synchronous_commit  => remote_apply
```

Lifetra then performs another lease renewal through the **same stable client DSN**:

```text
B leader:          revision=4 epoch=1 lease_revision=2
rewound former A:  revision=4 epoch=1 lease_revision=2
```

The second observation is made from the recovery-mode former primary, proving the post-rejoin mutation was remote-applied to the restored synchronous standby.

## Invariants demonstrated

```text
promotion != proof old primary is fenced
old primary data volume != old primary execution authority
old primary must be fenced before promotion/rejoin work
maintenance crash recovery != network resurrection
pg_rewind precedes old-primary listener restoration
rewound old primary starts in recovery mode
recovery-mode former primary rejects writes
database leadership change != new Lifetra fencing epoch
stable client endpoint != node identity
rejoin != redundancy restored
streaming:sync + remote_apply must be restored before claiming synchronous redundancy
```

## What this layer proves

Inside the controlled Docker topology:

1. Lifetra writes through one stable client DSN rather than a node-specific connection string.
2. A `remote_apply`-acknowledged authority record is present on the synchronous standby.
3. The old primary is removed from runnable/network authority before promotion.
4. The promoted standby continues the same application fencing epoch.
5. Old-primary crash recovery occurs with no network access.
6. `pg_rewind` reconciles the divergent former-primary data directory against the new leader.
7. The former primary receives a listener only after standby recovery configuration exists.
8. The resurrected former primary is recovery-mode read-only and rejects direct writes.
9. The original action identity, prepared attempt, and fencing epoch survive rewind/rejoin.
10. Synchronous streaming is re-established using the former primary as the new standby.
11. A new fenced mutation through the unchanged stable DSN is remote-applied to that rejoined standby.

## Boundary of the claim

The test deliberately does **not** claim a complete production HA control plane.

Still outside the proof:

- automatic leader election and quorum arbitration;
- real cloud/hypervisor/PDU/network STONITH;
- proof that fencing itself remains available during a control-plane partition;
- preventing two independent automation controllers from issuing conflicting promotions;
- transparent migration of already-open client TCP sessions;
- multi-standby quorum behavior;
- cross-host lease-clock assumptions;
- measured RTO/SLA;
- automatic `pg_rewind` orchestration and rollback if rewind fails.

A production next step is an external quorum/fencing coordinator whose authority survives the loss or partition of either PostgreSQL node, with tests that deliberately fail the fencing action itself.
