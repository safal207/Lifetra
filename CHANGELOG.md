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
- add `IdempotencyBinding`, `ReconciliationReceipt`, `RetryPolicy`, and `RetryAuthority` for fail-closed redispatch control;
- require reconciliation before any retry from `DispatchedEffectUnknown` and keep `StillUnknown` non-retryable;
- distinguish `NoEffectConfirmed` from generic not-found/silence so absence of a record cannot silently authorize another side effect;
- preserve one `ActionId` and idempotency binding across redispatch decisions, with explicit retry limits and ordinals;
- add append-only `AttemptLedger`, `AttemptId`, and attempt-scoped dispatch/reconciliation/external receipts so one logical action can preserve multiple physical attempts;
- derive redispatch count from attempt history and require each redispatch decision to carry the previous attempt's retry-safe resolution proof;
- preserve late and contradictory attempt evidence, blocking further retry when an older attempt later succeeds or evidence disagrees;
- add fsync-backed `DurableJournal` with write-ahead `PreparedAttempt` records before external side effects;
- replay durable records into `AttemptLedger` after restart and recover explicit directives for prepared ambiguity, dispatched unknown effects, retry evaluation, success closure, or blocking;
- treat a recovered prepared-only attempt as ambiguous and reconcile-first instead of converting it into blind redispatch permission;
- repair an incomplete trailing journal record while rejecting complete checksum corruption or sequence gaps;
- add provider-neutral `ProviderReconciliationAdapter` and `ProviderReconciler` to turn durable reconcile directives into persisted external proof and retry/close verdicts;
- keep provider observations separate from local dispatch receipts, including proof-backed ordinal gaps after externally resolved prepared attempts;
- require provider observations to be durably recorded before returning retry permission, while adapter failures leave journal history unchanged;
- keep generic timeout, silence, or not-found semantics at `StillUnknown` unless provider evidence actually establishes `NoEffectConfirmed`;
- add CAS-backed `RecoveryLeaseStore`, monotonic `FencingToken` epochs, lease renewal revisions, and `RecoveryLeaseManager` for split-brain recovery authority;
- add `FencedAttemptPermit` and `FencedDurableJournal` so stale epochs cannot continue mutating through the fenced runtime after a newer owner acquires the action;
- keep lease timeout distinct from proof that the old process stopped, and require downstream resources to enforce fencing epochs for end-to-end stale-worker rejection;
- include `InMemoryRecoveryLeaseStore` only as a process-local test/example CAS backend, while production multi-worker deployments require a shared atomic lease store and authoritative lease time;
- add provider-neutral `FencedActuatorAdapter`, `FencedActuatorRequest`, `FencedActuatorReceipt`, and `FencedActuatorController` for side-effect-boundary epoch enforcement;
- bind each fenced request to stable `ActionId`, idempotency key, attempt ordinal, owner, fencing epoch, and `operation_ref`, preventing a newer epoch from changing the semantic operation under an existing action identity;
- distinguish `Applied`, `AlreadyApplied`, and explicit downstream rejections for stale/future epochs, wrong owners, expired authority, and identity conflicts;
- add `InMemoryFencedActuator` as a process-local atomic compare+apply reference implementation that rejects stale requests before effect insertion and prevents duplicate application of the same logical effect;
- keep downstream actuator receipts separate from local dispatch receipts and require production actuators to make fence comparison plus side-effect commit one atomic resource operation;
- add recovery, bead-chain, orientation-delta, correction-loop, authority-gate, execution-receipt, reconciliation-retry, attempt-ledger, durable-journal, provider-reconciliation, recovery-lease-fencing, and fenced-actuator examples plus architecture documentation.

## 0.1.0

- establish the Lifetra Rust workspace and six coupled domain dimensions;
- add aggregate `EntityState` composition and coherence helpers;
- add Python bindings with PyO3/maturin;
- add Colab demos and CI validation.
