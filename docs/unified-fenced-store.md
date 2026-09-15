# Unified Fenced Store

Layer 14 reduces the number of local crash gaps by keeping the control and evidence state for one logical action in one CAS-versioned record.

```text
UnifiedActionRecord
├─ ActionId + idempotency key + operation_ref
├─ current lease owner + fencing epoch
├─ prepared physical attempts
├─ local dispatch evidence
├─ external / reconciliation evidence
└─ projection markers for later bead evidence
```

The store contract is intentionally provider-neutral. A production implementation can map the complete record to a PostgreSQL row/document, a strongly consistent KV item, or another transactional object, provided `compare_and_swap` replaces the record atomically.

## Why this layer exists

The previous runtime deliberately kept several durable boundaries separate:

```text
lease store
DurableJournal
actuator receipt sidecar
external resource
```

That design makes crash windows explicit and recoverable, but local projection can still require convergence between multiple stores. `UnifiedFencedStore` collapses the local control/evidence state for one action into one atomic record replacement.

It does **not** make an arbitrary external provider part of that transaction.

## Authority and evidence are different rights

A current fencing token is required to mutate future execution intent:

```text
current fence => acquire / renew / prepare / dispatch
stale fence   => no new execution authority
```

But valid evidence about a past attempt is still admissible after ownership changes:

```text
worker A epoch 1 applies effect
lease expires
worker B acquires epoch 2
late valid receipt from epoch 1 arrives
        ↓
receipt may be admitted as past evidence
        ↓
receipt does NOT restore epoch-1 execution authority
```

Fencing must not erase reality merely because the worker that produced the evidence is no longer current.

## Atomic success projection

A positive actuator or reconciliation proof is appended to `evidence` together with a `UnifiedProjectionMarker` in the same record CAS:

```text
before revision N
    ↓
EffectSucceeded proof
    ↓
CAS replacement
    ├─ evidence += proof
    └─ projections += marker
    ↓
revision N+1
```

There is no local state where the unified record has accepted a success proof but forgotten whether that proof is eligible to seed a later bead.

`projected_supported_evidence()` converts only positive projected proof into `EvidenceRef::Supported`. It does not mutate any historical bead; the proof is intended for a later bead.

## Retry safety

`NoEffectConfirmed` and confirmed failure may make a previous attempt retry-safe, but the next `RetryDecision` must explicitly carry the resolution proof:

```text
attempt #0
    ↓
NoEffectConfirmed(proof:X)
    ↓
RedispatchAllowed #1
must contain proof:X
```

`StillUnknown`, rejection, and missing evidence cannot authorize a new attempt.

## CAS and contention

Every mutating method loads the action record, derives a replacement, increments its record revision, and commits with:

```text
compare_and_swap(action_id, expected_revision, replacement)
```

If another worker changed the record first, the operation returns `Contended`. It does not partially apply only the lease, attempt, evidence, or projection subset.

The in-memory implementation uses one mutex and exists only as a process-local reference implementation.

## What this layer does not claim

`UnifiedFencedStore` is a transactional **internal-state** boundary. It does not claim that a remote payment processor, blockchain, HTTP API, or other actuator commits inside the same database transaction.

End-to-end safety still requires the external resource to provide its own compatible guarantees, such as:

- atomic fencing / conditional write at the side-effect boundary;
- stable external operation identity;
- idempotency semantics;
- authoritative reconciliation evidence.

For production multi-worker use, the unified store also needs a real shared transactional backend and authoritative or bounded lease time.

## Core invariants

```text
current fence != evidence ownership
stale worker cannot create new execution intent
late valid past receipt can remain admissible evidence
binding + lease + attempt + evidence + projection share one CAS record
CAS contention != partial success
positive proof + projection marker commit together
rejection != NoEffectConfirmed
StillUnknown != retry permission
future proof does not rewrite past bead knowledge
internal transactional state != remote side-effect transaction
```
