# Trajectory Bead Graph

The Trajectory Bead Graph (TBG) models a living process as a trajectory crossing a sequence of bounded local realities called **beads**.

A bead is not merely a snapshot. It records what entered a bounded context, which local relations were observed, what remains unknown, which claims are externally supported, and which proof-backed transition may leave the bead and become part of the larger trajectory.

## Core picture

```text
orientation / goal --------------------------------------------->

trajectory ----●----------●----------●----------●--------------->
              B0         B1         B2         B3

Each bead contains sector-local graphs:

                 causality
                    |
          state --- ● --- evidence
                    |
             agent / risk / outcome
```

The trajectory is the persistent path. A bead is one bounded combination of reality intersected by that path. The tangent direction at a bead can be interpreted as local orientation.

## Why beads exist

A global graph tends to mix facts from different moments, scales, and epistemic states. TBG introduces locality so that a decision can be evaluated using only the knowledge that was available inside the relevant bead.

This prevents future evidence from silently rewriting an earlier decision context.

Example:

```text
B3 crash boundary
  dispatch = SUPPORTED
  remote effect = UNKNOWN
  completion = UNKNOWN

B5 reconciliation
  remote effect = SUPPORTED by external receipt
```

B5 adds knowledge. It does not retroactively make B3 certain.

## Bead boundary

A bead can be bounded by:

- time: minute, hour, day, week;
- an event: one transaction, tool call, match, PR, recovery episode;
- a system regime: stable, degraded, recovering;
- an arbitrary domain-specific boundary.

This means the same model can support temporal zoom without forcing one universal granularity.

## Sector graphs

Each bead can contain multiple graph projections over the same underlying reality. The initial sector vocabulary is:

- causality;
- orientation;
- state;
- agent;
- evidence;
- environment;
- risk;
- outcome;
- reflection;
- resonance;
- synergy;
- custom domain sectors.

A sector graph contains local nodes and relations. The same external entity may be represented in several sectors, but higher-level systems should preserve a shared stable identity when correlating those projections.

## Evidence semantics

The MVP deliberately uses three evidence states:

- `Unknown` — the claim has not been resolved;
- `Supported` — available proof supports the claim;
- `Contradicted` — available proof conflicts with the claim.

The critical invariant is:

```text
UNKNOWN != FALSE != FAILURE
```

Silence, timeout, or missing evidence must not be collapsed into a negative fact.

## Proof-gated trajectory commit

A bead does not move the trajectory forward merely because an agent produced a statement.

The MVP permits a `BeadCommit` only when:

1. at least one supporting evidence reference exists;
2. no evidence reference is contradicted;
3. no unresolved unknown remains.

Conceptually:

```text
Bead
  -> sector synthesis
  -> proof gate
  -> BeadCommit
  -> TrajectoryState
```

This creates a distinction between local observation and a proof-backed state transition.

## The thread: `BeadChain`

`BeadChain` turns isolated beads into a linear proof-carrying trajectory.

```text
B0 --proof carry--> B1 --proof carry--> B2 --proof carry--> B3
```

The first implementation deliberately keeps the thread strict and inspectable:

- beads are ordered in time;
- temporal overlap is rejected for one linear thread;
- temporal gaps are allowed but remain explicit;
- proof can move forward only from the immediately previous committed bead;
- an incoming proof never overwrites an `Unknown` or `Contradicted` claim with the same identity.

A gap therefore means "unmodeled interval", not "nothing happened".

## Proof continuity

A later bead can inherit proof references from an earlier committed bead rather than trusting a textual summary.

Conceptually:

```text
Proof(Bn) -> admissible evidence in Bn+1
```

`append_with_commit()` verifies that the commit belongs to the previous bead and then carries its proof references into the destination bead as explicit supported evidence.

The chain records a `ProofCarry` receipt:

```text
ProofCarry {
  from_bead,
  to_bead,
  proof_refs
}
```

`proof_continuity_is_intact()` checks that every recorded carried proof is still present as supported evidence in the destination bead.

This makes the trust-reduction rule operational:

> Each externally verifiable proof can reduce the amount of trust required by the next step.

## Zoomable causality

Beads can be aggregated hierarchically:

```text
minute beads -> hour bead -> day bead -> week bead
```

The implementation exposes `aggregate_window()` and returns a `BeadAggregate` containing:

- the coarser `TrajectoryBead`;
- `source_beads` for drill-down provenance;
- `source_proofs` for evidence provenance.

Supported proof is preserved upward. Unresolved unknowns are also preserved upward.

That yields an important invariant:

```text
aggregation may compress context,
but must not manufacture certainty
```

If one minute bead contains an unresolved settlement status, the hour bead remains unable to produce a proof-gated trajectory commit until that uncertainty is resolved.

Known temporal scales are ordered as:

```text
Minute < Hour < Day < Week
```

A temporal aggregate must move to a strictly coarser scale. Event and custom beads remain domain-defined and are not forced into this ranking.

## Causal zoom

A high-level result should be inspectable in both directions:

```text
week outcome
  -> day aggregate
    -> hour aggregate
      -> minute bead
        -> sector relation
          -> action
            -> external proof
```

The current MVP preserves bead and proof provenance. Future versions can extend the same path down to sector edges, tool calls, receipts, or domain-specific execution identities.

## Mapping to Lifetra

The existing Lifetra dimensions remain meaningful:

- `lifetra-causal` describes causal structure;
- `lifetra-orient` describes direction;
- `lifetra-trajectory` remains the persistent trajectory;
- `lifetra-reflect` evaluates observations and contradictions;
- `lifetra-resonance` measures alignment;
- `lifetra-synergy` models interaction and emergence.

`lifetra-bead` adds the missing local coordination layer between those dimensions and proof-backed trajectory movement.

## Initial API

The current v0.2 development seed introduces:

- `BeadId`;
- `BeadScale`;
- `SectorKind`;
- `SectorNode` / `SectorEdge` / `SectorGraph`;
- `EvidenceStatus` / `EvidenceRef`;
- `TrajectoryBead`;
- `CommitBlock`;
- `BeadCommit`;
- `BeadChain`;
- `ProofCarry`;
- `ChainBlock`;
- `BeadAggregate`;
- `AggregationBlock`.

`TrajectoryBead::prove_transition()` is intentionally conservative. `BeadChain` is likewise intentionally linear for the first version; branching, merging, concurrent beads, quorum policies, confidence models, and domain-specific proof validators can be layered on later without weakening the current invariants.

## Cross-project interpretation

The same primitive can be used in several systems:

- reliable AI agents: one bead per run, action, or recovery boundary;
- ContractGraph-QA: crash/restart/reconciliation beads with exactly-once evidence;
- Liminal evidence systems: one bead as a proof-carrying context;
- multiplayer systems: tick/session/reconnect beads for causal divergence analysis;
- product QA: user-journey beads with UI, network, state, and outcome sectors;
- physical or material simulation: one bead per environmental condition regime, while keeping physical claims separate from model hypotheses.

## Design boundary

TBG is currently an architecture and computational representation, not a claim of a new physical law or new mathematics.

A useful mathematical analogy is a trajectory carrying a structured local fiber at each bounded context. Lifetra adds operational semantics that the analogy alone does not provide: evidence state, uncertainty, agent decisions, causal relations, proof-gated commits, proof continuity, and provenance-preserving causal zoom.
