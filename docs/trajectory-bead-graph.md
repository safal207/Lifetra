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

## Proof continuity

A later bead should be able to inherit proof references from an earlier committed bead rather than trusting a textual summary.

Conceptually:

```text
Proof(Bn) -> admissible input evidence for Bn+1
```

This is the basis for **proof continuity**: each externally verifiable proof can reduce the amount of trust required by the next step.

## Zoomable causality

Beads can be aggregated hierarchically:

```text
minute beads -> hour bead -> day bead -> week bead
```

Aggregation should preserve provenance so that a high-level claim can be traced back to the lower-level bead, sector edge, action, and evidence reference that produced it.

The long-term property is therefore not just summarization but **causal zoom**.

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

The v0.2 seed introduces:

- `BeadId`;
- `BeadScale`;
- `SectorKind`;
- `SectorNode` / `SectorEdge` / `SectorGraph`;
- `EvidenceStatus` / `EvidenceRef`;
- `TrajectoryBead`;
- `CommitBlock`;
- `BeadCommit`.

`TrajectoryBead::prove_transition()` is intentionally conservative. Future versions may replace the simple gate with configurable proof policies, quorum rules, confidence models, or domain-specific validators.

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

A useful mathematical analogy is a trajectory carrying a structured local fiber at each bounded context. Lifetra adds operational semantics that the analogy alone does not provide: evidence state, uncertainty, agent decisions, causal relations, and proof-gated commits.
