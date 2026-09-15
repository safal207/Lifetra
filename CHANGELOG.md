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
- add `SafetyEnvelope` and `DecisionAuthority` to separate correction proposals from execution permission;
- add explicit autonomy levels, three-valued human approval, quorum requirements, soft autonomous limits, and non-overridable hard limits;
- block insufficient proof even when approval exists, preserving the invariant that authority cannot manufacture evidence;
- distinguish `Allow(Automatic)`, `Allow(ApprovedManual)`, `RequireApproval`, and `Block` authority outcomes;
- add stable `ActionId`, `AuthorityTicket`, `DispatchReceipt`, `ExternalExecutionReceipt`, and `ExecutionTrace`;
- keep authorization, dispatch, and externally confirmed effect as separate proof boundaries;
- preserve `DispatchedEffectUnknown` so missing external acknowledgement is neither success nor failure;
- reject duplicate receipts and temporally invalid receipt ordering instead of silently rewriting execution history;
- add recovery, bead-chain, orientation-delta, correction-loop, authority-gate, and execution-receipt examples plus architecture documentation.

## 0.1.0

- establish the Lifetra Rust workspace and six coupled domain dimensions;
- add aggregate `EntityState` composition and coherence helpers;
- add Python bindings with PyO3/maturin;
- add Colab demos and CI validation.
