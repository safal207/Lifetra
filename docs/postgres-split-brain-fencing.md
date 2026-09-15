# PostgreSQL split-brain fencing and old-primary rejoin

Layer 19 extends the PostgreSQL HA evidence from successful promotion into the next failure boundary: what happens when the former primary can come back.

The core invariant is:

```text
promotion != proof the old primary is fenced forever
```

A database failover is safe only if the former primary cannot return as an independent writable authority. In the CI topology, the old primary is therefore power-fenced before promotion and remains stopped until its data directory has been rewound against the promoted leader.

## Stable client endpoint

Lifetra uses one client DSN for leader traffic throughout the scenario:

```text
postgresql://...@localhost:55434/lifetra
```

A test-only HAProxy TCP endpoint initially routes that DSN to the original primary. After the primary is fenced and the standby is promoted, the control plane replaces the backend with the promoted standby while keeping the client DSN unchanged.

```text
Lifetra
   |
   | stable DSN :55434
   v
leader endpoint
   |
   +--> primary A        (before failover)
   |
   +--> promoted B       (after failover)
```

The endpoint switch is explicit CI control-plane behavior. It is not an automatic production election system and does not establish an RTO SLA.

## Failure and fencing sequence

The split-brain job performs the following order:

1. Start a checksummed PostgreSQL 17 primary.
2. Start the stable leader endpoint pointing to that primary.
3. Bootstrap a physical standby with `pg_basebackup`.
4. Require `streaming:sync` and `synchronous_commit=remote_apply`.
5. Persist the Lifetra action/fence/attempt record through the stable endpoint.
6. Verify the same record is already readable on the standby.
7. Power-fence the old primary with `docker kill`.
8. Confirm the old primary is no longer running.
9. Promote the standby.
10. Repoint the same client endpoint to the promoted leader.
11. Renew the same Lifetra fencing epoch through the unchanged DSN.
12. Keep the old primary stopped while preparing rejoin.
13. Run `pg_rewind` against the promoted leader.
14. Configure the rewound node with `standby.signal`, a primary connection, and a physical replication slot.
15. Start the former primary only after the rewind has completed.
16. Verify `pg_is_in_recovery() = true` on that node.
17. Attempt a direct write and require PostgreSQL to reject it as read-only.
18. Verify the rewound node is streaming from the promoted leader.
19. Verify the stable client endpoint still resolves the current leader and the same Lifetra record.

## Why `pg_rewind`

The original primary and the promoted standby now have different timelines. Restarting the old primary directly would allow an unsafe divergent writable database.

`pg_rewind` synchronizes the old data directory with the promoted leader before the node is allowed to start again. The CI primary is initialized with data checksums so the rewind precondition is satisfied.

The workflow deliberately keeps the old PostgreSQL process stopped while rewind operates on its volume. An unclean old-primary shutdown is handled by `pg_rewind`'s normal crash-recovery preparation before the timeline rewind.

## Observed state continuity

The first complete split-brain run produced:

```text
stable endpoint -> old primary
seed       revision=2 epoch=1 attempt=0
standby    revision=2 epoch=1 lease_revision=0

power-fence old primary
promote standby
stable endpoint -> promoted leader
promoted   revision=3 epoch=1 lease_revision=1

pg_rewind old primary
start old primary as standby
rejoined   revision=3 epoch=1 lease_revision=1
leader     revision=3 epoch=1 lease_revision=1
```

The database role transition did not mint a new Lifetra authority epoch. The same application epoch continued on the promoted leader, while the returned old node received no execution authority at all; it came back only as a read-only physical standby.

## Direct split-brain rejection proof

After rejoin, CI executes a write directly against the former primary:

```sql
CREATE TABLE lifetra_split_brain_violation(id integer);
```

The required result is a PostgreSQL read-only error. A successful write fails the CI job.

This is stronger than merely checking that the stable endpoint points elsewhere: the resurrected node itself must no longer be writable.

## Authority boundaries

```text
database promotion != new application authority
stable client endpoint != automatic consensus
power fence != proof of permanent hardware isolation
pg_rewind completed != permission to start as primary
rejoined node != writable leader
read-only standby != external side-effect fencing
```

Lifetra's `ActionId`, idempotency key, operation identity, fencing epoch, and proof lineage remain the application-side authority contract. PostgreSQL leadership and node lifecycle are a separate deployment control plane.

## What this layer proves

Inside the controlled CI topology:

- the old primary is stopped before promotion;
- clients keep one DSN while the leader backend changes;
- the promoted standby continues the same Lifetra fencing state;
- the old primary remains stopped until its divergent timeline is rewound;
- the rewound node starts in recovery, not as a writable primary;
- a direct write to the resurrected node is rejected;
- the resurrected node streams from the promoted leader;
- the leader and rejoined standby expose the same Lifetra revision and application fencing epoch.

## What this layer does not prove

The CI `docker kill` is a controlled STONITH analogue, not a production hardware fencing service. The layer does not yet prove:

- BMC/cloud-API power fencing under network partitions;
- automatic leader election;
- automatic stable-endpoint failover;
- quorum-based multi-node fencing;
- prevention of operator bypass around the fencing controller;
- cross-region replication/fencing behavior;
- bounded RTO;
- external actuator fencing during a simultaneous database failover.

A production system should require a real fencing authority before promotion and should never allow a previously failed primary to restart writable until it has been proven safe to rejoin.
