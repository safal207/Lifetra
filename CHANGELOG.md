# Changelog

## Unreleased (v0.2 development)

### Trajectory Bead Graph

- add `lifetra-bead` as a bounded local-reality layer over persistent trajectories;
- add explicit `Unknown`, `Supported`, and `Contradicted` evidence semantics;
- add proof-gated `BeadCommit` transitions back into `TrajectoryState`;
- add `BeadChain` for linear proof-carrying trajectory continuity;
- preserve temporal gaps explicitly and reject overlap on one linear chain;
- add `ProofCarry` receipts and proof-continuity validation;
- add provenance-preserving causal zoom from finer beads into coarser temporal beads;
- preserve unresolved unknowns during aggregation so zoom cannot manufacture certainty;
- add proof-backed `ProvenOrientation` and signed `OrientationDelta` at the top-level orchestration layer;
- keep intention separate from evidence: orientation delta can only be derived from a commit that carries proof references;
- add bounded `CorrectionPolicy` with proportional gain, per-axis hysteresis, deadband, and maximum step;
- add `CorrectionMemory`, `OrientationAdjustment`, and proof-linked `CorrectionDecision` for the next planning step;
- keep correction separate from evidence: control can change the next intention but cannot rewrite proven movement;
- add recovery, bead-chain, orientation-delta, and correction-loop examples plus architecture documentation.

## 0.1.0

- establish the Lifetra Rust workspace and six coupled domain dimensions;
- add aggregate `EntityState` composition and coherence helpers;
- add Python bindings with PyO3/maturin;
- add Colab demos and CI validation.
