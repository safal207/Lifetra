# Proof-backed Orientation Delta

`OrientationDelta` compares an intended local direction with a movement vector that is explicitly bound to a proof-backed `BeadCommit`.

The purpose is to let an agent distinguish:

- what it intended to move toward;
- what the evidence-backed transition actually moved toward;
- where the largest directional mismatch occurred.

## Boundary

Intention is not evidence.

A goal, plan, prompt, or internal preference must not be allowed to rewrite causal truth. For that reason, a `ProvenOrientation` can only be created from a commit that carries at least one proof reference.

Conceptually:

```text
intended orientation
        |
        v
      action
        |
        v
   local bead
        |
   proof gate
        |
        v
   BeadCommit --------> ProvenOrientation
                              |
                              v
                     OrientationDelta
```

If the commit has no proof references, the movement cannot be labeled `ProvenOrientation`.

## Signed delta

For each axis, the MVP computes:

```text
delta = intended - proven
```

Therefore:

- positive delta: proven movement under-realized the intended intensity;
- negative delta: proven movement exceeded the intended intensity;
- zero: intended and proven values match on that axis.

The initial axes are inherited from Lifetra orientation:

- growth;
- stability;
- truth;
- connection.

## Alignment

The first-pass alignment metric is:

```text
mean_absolute_gap = mean(abs(axis_delta))
alignment_score = 1 - mean_absolute_gap
```

Because both orientation vectors are normalized to `0..=1`, the score also remains in `0..=1`.

This score means only **agreement between intended and proof-backed direction vectors**. It is not a truth score, business-success score, or claim that the underlying action was good.

## Agent control loop

The intended use is a closed loop:

```text
orientation
   -> action
   -> bead
   -> external proof
   -> commit
   -> proven movement
   -> orientation delta
   -> next orientation
```

The critical asymmetry is that the next orientation may be corrected by the measured delta, but the intended orientation cannot retroactively change the proven movement.

## Relation to BeadChain

`BeadChain` answers whether proof and temporal continuity survive across local contexts.

`OrientationDelta` answers whether the proof-backed movement through one of those contexts matches the intended local direction.

Together they separate three questions:

1. **What did we want?** — orientation.
2. **What is actually supported?** — evidence / commit.
3. **Did the supported movement follow the intended direction?** — orientation delta.

This creates a first control primitive for reliable agents without collapsing planning, observation, and proof into one state.
