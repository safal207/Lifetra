# External quorum fencing authority

Layer 20 moves database promotion authority outside the PostgreSQL pair itself.

The previous layers prove that Lifetra can survive PostgreSQL transaction ambiguity, network acknowledgement loss, primary/standby failover, old-primary fencing, `pg_rewind`, and standby rejoin. They do not answer the control-plane question: **who is allowed to decide that a new database writer may exist?**

This layer introduces a provider-neutral quorum contract around that decision.

## Core separation

```text
PostgreSQL role
    !=
quorum leadership generation
    !=
Lifetra action fencing epoch
```

A PostgreSQL promotion changes database topology. A quorum `LeadershipGeneration` identifies one control-plane decision generation. A Lifetra action fencing epoch still governs execution authority for one logical action. None of these counters may be substituted for another.

## Three-node reference authority

The reference contract requires at least three unique coordinator identities. For `N` members, quorum is:

```text
floor(N / 2) + 1
```

For the standard three-member case, two independent approvals are required.

```text
q1 ---- approve ----\
                     +--> quorum --> FenceGrant
q2 ---- approve ----/
q3 ---- absent
```

One reachable coordinator is not enough. Losing quorum therefore reduces availability but does not create a second writer.

## Two-stage authority

Quorum does **not** immediately authorize promotion.

First it authorizes a fencing attempt:

```text
PromotionRequest
      +
quorum votes
      ↓
FenceGrant
```

The failed primary must then be externally fenced and produce a matching receipt:

```text
FenceGrant
   ↓
external STONITH / power / network fence
   ↓
FenceReceipt(ConfirmedFenced)
```

Only then may the authority produce:

```text
PromotionPermit
```

The distinction is deliberate:

```text
quorum thinks fencing should happen
    !=
fencing actually happened
```

`FenceOutcome::Unknown` and `FenceOutcome::StillReachable` both fail closed and cannot become promotion permission.

## Monotonic generation

Every control-plane decision is scoped to a monotonic `LeadershipGeneration`.

```text
generation 1 permit
       ↓
coordinator state advances to generation 2
       ↓
generation 1 permit is stale
```

A stale promotion permit cannot be replayed after a newer generation begins.

## Proposal locking

Within one generation, once a promotion request reaches the fencing-grant stage, the authority locks the exact tuple:

```text
request_id
failed_primary
candidate
leadership_generation
```

A competing candidate in the same generation is rejected. This prevents two controllers from independently turning the same failure observation into two different promotion targets under one generation.

## Vote validation

Votes fail closed when:

- the coordinator is not a configured member;
- the same coordinator appears twice;
- the vote is for a different request, generation, failed primary, or candidate;
- the proof reference is empty;
- approvals are below quorum.

A duplicate coordinator cannot manufacture quorum by voting twice.

## CI integration

The split-brain workflow uses the quorum example as a gate around the promotion sequence.

The intended order is:

```text
1. establish generation
2. collect 2/3 quorum votes
3. issue FenceGrant
4. fence old primary
5. prove old primary is no longer reachable
6. convert the confirmed fence observation into PromotionPermit
7. only then run pg_ctl promote on the candidate
```

The example also proves that:

- one of three votes returns `NoQuorum`;
- quorum plus an `Unknown` fence receipt still cannot issue a permit;
- quorum plus `ConfirmedFenced` can issue a permit;
- beginning generation 2 invalidates the generation 1 permit.

## Invariants

```text
no quorum != degraded permission
no quorum => no new writer
quorum approval != fence completion
UNKNOWN fence != confirmed fence
still reachable primary != promotion permission
duplicate vote != two voters
non-member vote != authority
one generation != multiple competing candidates
new generation stales old promotion permits
database generation != Lifetra action fencing epoch
promotion permit != proof that promotion completed
```

## Boundary of the claim

The Layer 20 Rust authority is a protocol/reference implementation. Its in-process state is not itself a distributed consensus system.

It does **not** yet prove:

- Raft/Paxos durability across coordinator process loss;
- a real three-node etcd/Consul/ZooKeeper quorum;
- authenticated or cryptographically signed votes;
- Byzantine coordinator behavior;
- production BMC/cloud/PDU fencing;
- automatic endpoint update from the same consensus log;
- coordinator state recovery after full quorum restart.

Those are deployment backends for the contract. The next hardening step is to place the generation/proposal/grant state in a real consensus service and inject quorum loss and leader changes while the PostgreSQL failover workflow is active.
