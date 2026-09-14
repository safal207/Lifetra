# Evidence-Driven Correction Policy

`CorrectionPolicy` is the fourth experimental control layer in Lifetra's Trajectory Bead Graph work.

It converts a proof-backed `OrientationDelta` into a bounded proposal for the **next intended orientation**. It does not modify the proven movement that already happened.

## Control loop

```text
intended orientation
        |
        v
      action
        |
        v
      bead
        |
        v
 external evidence
        |
        v
   BeadCommit
        |
        v
ProvenOrientation
        |
        v
OrientationDelta
        |
        v
CorrectionPolicy
        |
        v
next intended orientation
```

The core separation is:

```text
past proven movement != future correction proposal
```

A controller may use evidence about the past to alter the next plan, but it cannot rewrite the evidence-backed past.

## Proportional correction

For one orientation axis, the starting control law is:

```text
raw_adjustment = gain * (intended - proven)
```

The adjustment is then capped by `max_step` and the resulting next orientation is clamped to the normalized `0.0..=1.0` range.

Conceptually:

```text
O_next = clamp(O_intended + clamp(K * delta, -max_step, max_step))
```

This is intentionally a small proportional controller, not a general claim that every agent-control problem should use this law.

## Deadband and hysteresis

Small deviations may be noise. Applying a correction on every tiny change can create oscillation.

The policy therefore uses two thresholds:

- `engage_threshold` — an inactive axis starts correcting only when the absolute delta reaches this level;
- `release_threshold` — an already active axis keeps correcting until the absolute delta falls below this lower level.

The required relation is:

```text
release_threshold <= engage_threshold
```

Example:

```text
engage = 0.10
release = 0.04
```

An inactive axis with gap `0.06` stays idle. An axis that was already engaged remains active at gap `0.06`. This is the hysteresis band.

## Per-axis memory

`CorrectionMemory` keeps engagement state independently for:

- growth;
- stability;
- truth;
- connection.

This matters because one part of the orientation may still need control while another has already settled.

## Bounded reaction

`max_step` limits the magnitude of one correction on each axis.

A large observed miss therefore cannot cause an arbitrarily large change to the next intended orientation in one iteration.

This protects the controller from overreaction and makes future higher-level policies possible, such as rate limits, cooldowns, confidence-weighted gain, or domain-specific safety bounds.

## Proof gate

A correction requires a proof-backed `OrientationDelta`. `CorrectionPolicy::propose()` rejects deltas whose proven movement carries no proof references.

The relevant invariant is:

```text
intention is not evidence
correction is not evidence
proof-backed movement remains immutable input to control
```

A `CorrectionDecision` carries the source bead id and proof references forward so the next planning decision remains traceable to the evidence that motivated it.

## What a CorrectionDecision means

A decision contains:

- `source_bead` — which proved transition caused the control response;
- `proof_refs` — evidence identity supporting the observed movement;
- `adjustment` — signed per-axis correction;
- `next_orientation` — bounded proposal for the next planning step;
- `memory` — hysteresis state for the next controller iteration.

It does **not** mean that the proposed next orientation has already been achieved.

## Agent interpretation

An agent can now distinguish four different statements:

1. **Intent** — "I wanted to move toward X."
2. **Proof** — "Evidence confirms movement Y."
3. **Delta** — "X and Y differ by D."
4. **Correction** — "Given D and policy P, the next plan should shift by C."

These statements must remain separate.

## Initial safety properties

The v0.2 correction seed provides:

- proof-linked control input;
- normalized policy parameters;
- proportional gain;
- per-axis deadband;
- per-axis hysteresis;
- per-axis maximum correction step;
- clamping of the next intended orientation to `0.0..=1.0`;
- no mutation of the proven movement.

## Deliberate non-goals

This layer does not yet implement:

- PID or integral control;
- confidence-weighted gains;
- delayed observations;
- multi-agent arbitration;
- branching control trajectories;
- learned policies;
- safety envelopes tied to a specific real-world actuator;
- business-success optimization.

Those can be added later without weakening the evidence boundary.

## Full experimental stack

```text
TrajectoryBead
    -> BeadCommit
    -> BeadChain / ProofCarry
    -> causal zoom
    -> ProvenOrientation
    -> OrientationDelta
    -> CorrectionPolicy
    -> CorrectionDecision
    -> next intended orientation
```

The resulting loop is evidence-driven rather than self-confirming: a plan can influence the future, but only external proof can describe what actually happened.
