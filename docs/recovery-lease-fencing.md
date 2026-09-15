# Recovery Lease and Fencing

The recovery lease layer prevents a restarted or delayed worker from retaining execution authority forever.

It addresses the classic split-brain recovery case:

```text
worker A owns action
        ↓
worker A stalls / crashes
        ↓
lease expires
        ↓
worker B acquires a newer epoch
        ↓
worker A wakes up late
```

A lease timeout alone is not sufficient. Worker A may still be alive after its lease expires. Lifetra therefore separates **lease ownership** from **fencing identity**.

## Core model

`RecoveryLeaseStore` is a provider-neutral store contract with an atomic compare-and-swap operation.

```text
RecoveryLeaseStore
  load(ActionId)
  compare_and_swap(ActionId, expected_version, replacement)
```

The CAS operation must be atomic across all workers that share the lease backend. Suitable production implementations include conditional database updates, Redis transactions/scripts, etcd compare transactions, or an equivalent primitive.

A plain read-then-write implementation is not sufficient.

Each lease has:

```text
ActionId
owner / RecoveryWorkerId
LeaseVersion { epoch, revision }
expires_at
```

`epoch` changes when an expired lease is acquired again. `revision` changes when the current owner renews the same epoch.

```text
worker A acquire  → epoch 1, revision 0
worker A renew    → epoch 1, revision 1
worker A expires
worker B acquire  → epoch 2, revision 0
```

The monotonic `epoch` is the fencing value.

## Fencing token

The owner receives:

```text
FencingToken {
  ActionId,
  owner,
  epoch
}
```

A stale token is rejected when the authoritative store contains a newer epoch.

```text
epoch 1 worker wakes up
        ↓
lease store says epoch 2
        ↓
StaleFence { presented_epoch: 1, current_epoch: 2 }
```

The important invariant is:

```text
lease expired != old process stopped
newer epoch => older epoch has no execution authority
```

## Fenced attempt permit

A proof-backed retry/dispatch decision can be converted into a `FencedAttemptPermit` only while the lease token is current.

```text
RetryDecision
      ↓
current FencingToken
      ↓
FencedAttemptPermit {
  ActionId,
  owner,
  fencing_epoch,
  attempt_ordinal,
  idempotency_key
}
```

The permit is **not** dispatch evidence.

```text
fenced permit != dispatch
```

`FencedDurableJournal::prepare_attempt` persists the existing write-ahead `PreparedAttempt` and returns the fencing permit. `record_dispatch`, reconciliation writes, and external-outcome writes validate the current lease before mutating through the fenced wrapper.

## Why the downstream resource must see the epoch

A stale process can pass a lease check and then pause before the external call. Another worker may later acquire a newer epoch. When the old process resumes, a purely local lease check cannot physically prevent it from sending a network request.

Therefore strong end-to-end fencing requires the external actuator/provider to propagate and enforce the epoch whenever the downstream system supports such a primitive.

Conceptually:

```text
request(action_id, idempotency_key, fencing_epoch=7)

provider/resource remembers highest epoch = 8
        ↓
epoch 7 request rejected as stale
```

If a provider cannot enforce fencing epochs, its idempotency semantics and reconciliation evidence remain essential. Lifetra must not claim fencing guarantees that the downstream system cannot actually enforce.

## Lease time

The current core API accepts an explicit `Timestamp` for acquire, renew, and validation so tests and adapters can supply time deterministically.

For a real multi-worker deployment, that time must come from an authoritative or sufficiently bounded source. Independent worker clocks with uncontrolled skew can invalidate lease-expiry assumptions. A production lease-store adapter should normally rely on datastore/server time or native TTL/lease semantics where available.

## File-journal boundary

`DurableJournal` remains a file-backed MVP. `FencedDurableJournal` gates its mutation API with the current token, but lease validation and a filesystem append are not one distributed atomic transaction.

For strict multi-host/multi-process durability, use a lease/WAL backend whose fencing check and state mutation can be made atomic or whose storage system rejects stale epochs. A future database/replicated journal adapter can close that boundary.

This layer therefore establishes the **fencing protocol** without overstating the guarantees of a local file.

## In-memory store

`InMemoryRecoveryLeaseStore` exists for tests and examples. It provides atomic CAS only to users sharing the same in-process `Arc<Mutex<...>>`.

It is explicitly **not** a production cross-process lease store.

## Invariants

```text
lease timeout != proof that old worker stopped
renewal != new ownership epoch
reacquisition after expiry => higher epoch
stale epoch != execution authority
fenced permit != dispatch evidence
idempotency key != fencing token
local lease check != downstream fencing
client clock != authoritative lease time unless explicitly guaranteed
```
