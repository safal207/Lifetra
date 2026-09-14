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
- add recovery and bead-chain examples plus architecture documentation.

## 0.1.0

- establish the Lifetra Rust workspace and six coupled domain dimensions;
- add aggregate `EntityState` composition and coherence helpers;
- add Python bindings with PyO3/maturin;
- add Colab demos and CI validation.
