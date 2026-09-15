# Bead-Thread Trajectory Model

The bead-thread model extends Lifetra's trajectory dimension from a list of transitions into a multi-scale representation of **state, cause, space, transition, evidence, and time**.

It grew from a simple metaphor:

- the **thread** is the trajectory itself and its persistent center of orientation;
- each **bead** is a bounded slice of lived state, such as a frame, iteration, hour, day, week, or other interval;
- each bead is divided into **sectors**;
- each sector can carry its own local graph;
- causal transitions connect one bead to the next;
- the local graphs are coordinated back into the thread so local changes remain interpretable as part of one trajectory.

## Why this is needed

Many iterative agent loops record only a sequence of outputs:

```text
target -> build -> screenshot -> critique -> fix -> screenshot -> ...
```

That is enough to show chronology, but not enough to answer deeper questions:

- **Cause:** which intervention produced which observed improvement?
- **Space:** where did the problem and the change occur?
- **Transition:** what changed between states, and was it refinement, divergence, recovery, branch, or merge?
- **Time:** is the relevant unit a frame, iteration, session, hour, day, or week?
- **Continuity:** what stable orientation makes two very different states comparable?
- **Evidence:** what screenshot, measurement, receipt, or external proof supports the claimed transition?
- **Recurrence:** is the same defect appearing across several beads, indicating a root architectural cause rather than a local tuning issue?

The bead-thread model makes those questions first-class.

## The model

```text
orientation center
      |
      v
=========================== thread / trajectory ===========================>
       O bead A          O bead B          O bead C
      /|\               /|\               /|\
     / | \             / | \             / | \
  space light cause  space light cause  space light cause
    |     |     |      |     |     |      |     |     |
 local sector graphs   local sector graphs   local sector graphs

          A -- causal transition --> B -- causal transition --> C
```

## Thread

`TrajectoryThread` represents continuity across many local states.

Its `orientation_center` is the semantic center of the trajectory. It is deliberately a stable description rather than a single score. Example:

> Match the target visual while preserving spatial legibility, interaction quality, runtime constraints, and evidence integrity.

This prevents a system from improving one local metric while silently drifting away from the actual direction of travel.

## Bead

`TrajectoryBead` is one bounded state slice.

A bead contains:

- `TimeWindow`
- `TemporalGranularity`
- alignment to the thread
- multiple `BeadSector`s
- evidence references

A bead may represent:

- one rendered frame;
- one agent refinement iteration;
- one user session;
- one hour of activity;
- one day;
- one week;
- a custom domain-specific interval.

This lets the same trajectory be inspected at different temporal scales without pretending that every system evolves at one fixed cadence.

## Sectors and local graphs

Each bead can be divided into sectors. A sector is intentionally generic because different systems need different decompositions.

Examples:

- **space** — rooms, zones, surfaces, camera regions;
- **causality** — observed causes and dependencies;
- **visual** — composition, light, materials, details;
- **interaction** — states, controls, transitions;
- **evidence** — receipts, screenshots, hashes, measurements;
- **orientation** — local goals or directional signals;
- **reflection** — contradictions and interpretations;
- **resonance** — alignment across internal and external signals.

Every `BeadSector` may contain a small directed graph of `SectorNode`s and `SectorEdge`s.

That matters because a bead is not just a bag of scores. It can express structure such as:

```text
camera position --exposes--> white wall plane
white wall plane --causes--> low spatial legibility
master doorway --restores--> route readability
```

inside the `upper-landing` spatial scope.

## Bead-to-bead transitions

`BeadTransition` connects state slices through time.

A transition records:

- source bead;
- destination bead;
- timestamp;
- transition kind;
- cause;
- effect;
- optional spatial scope;
- confidence.

Transition kinds currently include:

- observation;
- refinement;
- divergence;
- recovery;
- branch;
- merge;
- custom.

The model does not claim causality merely because two states are consecutive. The `cause`, `effect`, `confidence`, and evidence references make the causal claim explicit and reviewable.

## Mapping to an agent visual-refinement loop

A visual Dream Loop can be represented as follows:

| Visual loop concept | Lifetra bead-thread concept |
| --- | --- |
| target image | orientation constraint / thread center |
| current live screenshot | evidence attached to a bead |
| critic score | sector node signals |
| critic feedback | candidate causal graph edges |
| code or scene change | bead-to-bead transition |
| changed room / camera region | transition spatial scope |
| next iteration | next bead |
| repeated defect | recurring node/edge pattern across beads |
| re-dreamed target | branch or explicit orientation-center update |
| rollback | recovery transition |
| alternative approach | branch trajectory |

## What this adds beyond a basic refinement loop

A conventional refinement loop is usually linear and screenshot-centric. The bead-thread model adds several missing dimensions:

1. **Causal attribution** — changes are linked to observed effects rather than stored only as chronology.
2. **Spatial locality** — a problem can be scoped to a room, zone, surface, viewport, or other location.
3. **Transition semantics** — refinement, divergence, recovery, branch, and merge are distinct.
4. **Multi-scale time** — frame, iteration, session, hour, day, and week can all be modeled.
5. **Persistent orientation** — local optimization remains tethered to a stable trajectory center.
6. **Evidence continuity** — proofs stay attached to the state in which they were observed.
7. **Recurrence detection** — repeated gaps across beads can reveal a deeper architectural cause.
8. **Branchable trajectories** — the model can represent alternative paths rather than one irreversible loop.

## Example: visual refinement

```rust
use lifetra::{
    BeadSector, BeadTransition, BeadTransitionKind, SectorEdge, SectorNode,
    TemporalGranularity, TimeWindow, Timestamp, TrajectoryBead, TrajectoryThread,
};

let mut visual = BeadSector::new("visual", "Visual fidelity", 1.0)
    .with_spatial_scope("upper-landing");
visual.push_node(SectorNode::new(
    "camera",
    "Camera position",
    0.42,
    "camera sits too close to a dominant wall plane",
));
visual.push_node(SectorNode::new(
    "legibility",
    "Spatial legibility",
    0.38,
    "doorway and route are hard to read",
));
visual.push_edge(SectorEdge::new("camera", "legibility", "reduces", 0.9));

let mut before = TrajectoryBead::new(
    "iteration-1",
    TimeWindow::new(Timestamp::new(100), Timestamp::new(160)),
    TemporalGranularity::Iteration,
    0.55,
);
before.push_sector(visual);
before.add_evidence("screenshot:upper-landing:v1");

let after = TrajectoryBead::new(
    "iteration-2",
    TimeWindow::new(Timestamp::new(161), Timestamp::new(220)),
    TemporalGranularity::Iteration,
    0.82,
);

let mut thread = TrajectoryThread::new(
    "upper-landing-refinement",
    "increase visual fidelity while preserving spatial legibility and interaction",
);
thread.push_bead(before);
thread.push_bead(after);
thread.push_transition(
    BeadTransition::new(
        "iteration-1",
        "iteration-2",
        Timestamp::new(161),
        BeadTransitionKind::Refinement,
        "camera position hides the intended doorway",
        "camera moves toward the corridor center and restores route readability",
        0.9,
    )
    .with_spatial_scope("upper-landing"),
);

assert!(thread.graph_is_consistent());
```

## Design boundary

This model is semantic infrastructure. It does not automatically prove that a causal claim is true, choose the correct time granularity, or infer spatial structure from raw data. Those remain responsibilities of the caller, evidence pipeline, or higher-level agent.

The important change is that Lifetra now has a place to **represent** those relationships without flattening them into a single score or an unordered event log.
