# PostgreSQL network fault recovery

Layer 17 validates the Layer 16 transaction-recovery contract against real TCP interruption and a real PostgreSQL service restart.

The goal is not to claim that a single Docker PostgreSQL container is highly available. The goal is narrower: prove that Lifetra distinguishes a connection loss before COMMIT from a lost acknowledgement after PostgreSQL has completed COMMIT, and prove that the unified action record survives a server restart and can be loaded through a new connection.

## Exact COMMIT-boundary fault

`tests/postgres_network_fault.rs` runs a test-only TCP relay between `PostgresUnifiedFencedStore` and PostgreSQL 17.

The relay does not arm the post-COMMIT fault by searching for an arbitrary ASCII word. It waits for the exact PostgreSQL protocol frames:

```text
frontend Simple Query:
Q + length(11) + "COMMIT\0"

backend CommandComplete:
C + length(11) + "COMMIT\0"
```

The proxy follows this order:

```text
Lifetra
   |
   | exact Query("COMMIT")
   v
TCP fault proxy
   |
   | forwards complete COMMIT request
   v
PostgreSQL
   |
   | commits transaction
   | emits CommandComplete("COMMIT")
   v
TCP fault proxy
   |
   X drops the COMMIT acknowledgement
   |
Lifetra sees connection loss
```

Only after the complete backend `CommandComplete("COMMIT")` frame has been read from the server socket does the proxy close the client path. This establishes a concrete server-side post-COMMIT boundary before the acknowledgement is deliberately withheld.

The resilient CAS path then opens a fresh connection and calls `resolve_commit_outcome`. If the durable row equals the intended replacement exactly, the result is `PostgresCommitResolution::Applied` and the CAS returns success without issuing the mutation again.

Core invariant:

```text
server completed COMMIT + client lost ACK
    !=
transaction failure
```

## Pre-COMMIT connection loss

A second proxy mode closes the connection while the transaction is still open, before COMMIT. PostgreSQL therefore cannot later commit that abandoned transaction.

The test verifies that the connection-class failure is treated as `RetryableBeforeCommit` and the whole CAS transaction is retried from the prior durable revision.

Core invariant:

```text
connection lost before COMMIT
    =>
old transaction cannot later become committed
    =>
fresh whole-transaction retry is allowed
```

This retry applies only to the PostgreSQL state mutation. It is not permission to blindly replay an external payment, HTTP request, blockchain transaction, or other side effect.

## Restart durability probe

`examples/postgres_restart_probe.rs` is used by CI in two processes:

```text
seed process
    |
    | create action + acquire epoch 1
    v
PostgreSQL data directory
    |
    X docker restart
    |
    v
verify process
```

The verification process establishes a new PostgreSQL connection and confirms that the same action binding, operation identity, revision, and fencing epoch survive the restart.

This proves single-node restart durability and reconnect behavior for this CI environment.

It does **not** prove:

- replicated PostgreSQL HA;
- automatic primary election;
- synchronous-replication durability across node loss;
- failover through a connection pooler or cluster endpoint;
- zero RPO/RTO guarantees.

Those require a multi-node PostgreSQL topology and a separate failover test layer.

## What Layer 17 proves

```text
real TCP cut before COMMIT
    -> bounded fresh transaction retry

real TCP cut after backend CommandComplete("COMMIT")
    -> commit outcome reconciliation
    -> exact durable replacement => Applied

real PostgreSQL service restart
    -> new connection
    -> action record and fence survive
```

The network fault harness remains in `tests/` and is not exported by the Lifetra runtime API.

## Boundary

Layer 17 strengthens the PostgreSQL durability evidence but does not move arbitrary external side effects into PostgreSQL. End-to-end execution safety still requires stable action/operation identity, provider idempotency, downstream fencing, durable external proof, and reconciliation.
