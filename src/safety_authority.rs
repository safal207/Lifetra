use lifetra_bead::BeadId;
use lifetra_core::Scalar;

use crate::CorrectionDecision;

/// How much execution authority an agent is granted by policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyLevel {
    /// The system may inspect and explain decisions but may not apply them.
    ObserveOnly,
    /// The system may recommend a correction, but execution requires approval.
    RecommendOnly,
    /// Small proof-backed corrections may execute automatically inside the envelope.
    BoundedAutomatic,
}

/// Explicit approval state. Unknown approval is not denial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ApprovalState {
    #[default]
    Unknown,
    Approved,
    Denied,
}

/// Runtime authority context supplied by the surrounding system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AuthorityContext {
    pub human_approval: ApprovalState,
    pub quorum_approvals: usize,
}

/// Static safety bounds for applying a correction decision.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SafetyEnvelope {
    pub autonomy: AutonomyLevel,
    pub min_proof_refs: usize,
    pub quorum_required: usize,
    pub require_human_approval: bool,
    pub max_autonomous_adjustment: Scalar,
    pub hard_max_adjustment: Scalar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyConfigBlock {
    InvalidEnvelope,
}

/// Why authority was reduced or denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityReason {
    InsufficientProof,
    ObserveOnly,
    RecommendOnly,
    QuorumPending,
    HumanApprovalRequired,
    HumanApprovalDenied,
    OutsideAutonomousEnvelope,
    OutsideHardEnvelope,
}

/// How an allowed correction may be applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionMode {
    Automatic,
    ApprovedManual,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityVerdict {
    Allow(ExecutionMode),
    RequireApproval,
    Block,
}

/// Auditable authority result for one proof-linked correction proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthorityDecision {
    pub source_bead: BeadId,
    pub proof_refs: Vec<String>,
    pub verdict: AuthorityVerdict,
    pub reasons: Vec<AuthorityReason>,
    pub max_abs_adjustment: Scalar,
}

/// Evaluates whether a correction proposal may be executed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DecisionAuthority {
    pub envelope: SafetyEnvelope,
}

impl SafetyEnvelope {
    pub fn new(
        autonomy: AutonomyLevel,
        min_proof_refs: usize,
        quorum_required: usize,
        require_human_approval: bool,
        max_autonomous_adjustment: Scalar,
        hard_max_adjustment: Scalar,
    ) -> Result<Self, SafetyConfigBlock> {
        let normalized = [max_autonomous_adjustment, hard_max_adjustment]
            .into_iter()
            .all(|value| (0.0..=1.0).contains(&value));

        if !normalized || max_autonomous_adjustment > hard_max_adjustment {
            return Err(SafetyConfigBlock::InvalidEnvelope);
        }

        Ok(Self {
            autonomy,
            min_proof_refs,
            quorum_required,
            require_human_approval,
            max_autonomous_adjustment,
            hard_max_adjustment,
        })
    }
}

impl DecisionAuthority {
    pub fn new(envelope: SafetyEnvelope) -> Self {
        Self { envelope }
    }

    /// Evaluates execution authority without mutating the correction or trajectory.
    ///
    /// Epistemic failures and hard safety violations are blocking: approval cannot
    /// turn missing proof into proof or override the hard envelope.
    pub fn evaluate(
        &self,
        correction: &CorrectionDecision,
        context: AuthorityContext,
    ) -> AuthorityDecision {
        let max_abs_adjustment = max_abs_adjustment(correction);
        let proof_count = correction.proof_refs.len();

        let base = |verdict, reasons| AuthorityDecision {
            source_bead: correction.source_bead.clone(),
            proof_refs: correction.proof_refs.clone(),
            verdict,
            reasons,
            max_abs_adjustment,
        };

        if proof_count < self.envelope.min_proof_refs {
            return base(
                AuthorityVerdict::Block,
                vec![AuthorityReason::InsufficientProof],
            );
        }

        if max_abs_adjustment > self.envelope.hard_max_adjustment {
            return base(
                AuthorityVerdict::Block,
                vec![AuthorityReason::OutsideHardEnvelope],
            );
        }

        if context.human_approval == ApprovalState::Denied {
            return base(
                AuthorityVerdict::Block,
                vec![AuthorityReason::HumanApprovalDenied],
            );
        }

        if self.envelope.autonomy == AutonomyLevel::ObserveOnly {
            return base(
                AuthorityVerdict::Block,
                vec![AuthorityReason::ObserveOnly],
            );
        }

        let quorum_pending = context.quorum_approvals < self.envelope.quorum_required;
        let outside_auto = max_abs_adjustment > self.envelope.max_autonomous_adjustment;
        let policy_needs_human = self.envelope.require_human_approval
            || self.envelope.autonomy == AutonomyLevel::RecommendOnly
            || outside_auto;
        let human_pending = policy_needs_human && context.human_approval != ApprovalState::Approved;

        let mut reasons = Vec::new();
        if self.envelope.autonomy == AutonomyLevel::RecommendOnly {
            reasons.push(AuthorityReason::RecommendOnly);
        }
        if quorum_pending {
            reasons.push(AuthorityReason::QuorumPending);
        }
        if outside_auto {
            reasons.push(AuthorityReason::OutsideAutonomousEnvelope);
        }
        if human_pending {
            reasons.push(AuthorityReason::HumanApprovalRequired);
        }

        if quorum_pending || human_pending {
            return base(AuthorityVerdict::RequireApproval, reasons);
        }

        let execution = if policy_needs_human {
            ExecutionMode::ApprovedManual
        } else {
            ExecutionMode::Automatic
        };

        base(AuthorityVerdict::Allow(execution), reasons)
    }
}

fn max_abs_adjustment(correction: &CorrectionDecision) -> Scalar {
    [
        correction.adjustment.growth.abs(),
        correction.adjustment.stability.abs(),
        correction.adjustment.truth.abs(),
        correction.adjustment.connection.abs(),
    ]
    .into_iter()
    .fold(0.0, f32::max)
}

#[cfg(test)]
mod tests {
    use crate::{
        BeadId, CorrectionMemory, CorrectionPolicy, EvidenceRef, EvidenceStatus, OrientationDelta,
        OrientationVector, ProvenOrientation, Timestamp, TrajectoryBead,
    };
    use lifetra_bead::BeadScale;

    use super::*;

    fn correction_with(proof_count: usize, max_step: Scalar) -> CorrectionDecision {
        let mut bead = TrajectoryBead::new(
            BeadId::new("bead:authority:1"),
            BeadScale::Event,
            Timestamp::new(0),
            Timestamp::new(1),
        );

        for index in 0..proof_count {
            bead = bead.with_evidence(EvidenceRef::new(
                format!("proof:authority:{index}"),
                EvidenceStatus::Supported,
                "independent external proof",
            ));
        }

        let commit = if proof_count == 0 {
            crate::BeadCommit {
                bead_id: bead.id.clone(),
                proof_refs: Vec::new(),
                transition: crate::StateTransition::new("claim", Timestamp::new(1), "no proof"),
            }
        } else {
            bead.prove_transition("moved", "verified")
                .expect("proof-backed bead should commit")
        };

        let proven = if proof_count == 0 {
            crate::ProvenOrientation {
                bead_id: commit.bead_id.clone(),
                vector: OrientationVector::new(0.2, 0.5, 0.5, 0.5),
                proof_refs: Vec::new(),
            }
        } else {
            ProvenOrientation::from_commit(
                OrientationVector::new(0.2, 0.5, 0.5, 0.5),
                &commit,
            )
            .expect("commit carries proof")
        };

        let delta = OrientationDelta::between(
            OrientationVector::new(0.9, 0.5, 0.5, 0.5),
            proven,
        );
        let policy = CorrectionPolicy::new(1.0, 0.01, 0.0, max_step).expect("valid policy");
        policy
            .propose(&delta, CorrectionMemory::default())
            .unwrap_or_else(|_| CorrectionDecision {
                source_bead: commit.bead_id,
                proof_refs: commit.proof_refs,
                adjustment: crate::OrientationAdjustment {
                    growth: max_step,
                    ..crate::OrientationAdjustment::default()
                },
                next_orientation: OrientationVector::new(1.0, 0.5, 0.5, 0.5),
                memory: CorrectionMemory::default(),
            })
    }

    fn envelope(autonomy: AutonomyLevel) -> SafetyEnvelope {
        SafetyEnvelope::new(autonomy, 1, 0, false, 0.10, 0.25).expect("valid envelope")
    }

    #[test]
    fn rejects_invalid_envelope_ordering() {
        assert_eq!(
            SafetyEnvelope::new(AutonomyLevel::BoundedAutomatic, 1, 0, false, 0.30, 0.20),
            Err(SafetyConfigBlock::InvalidEnvelope)
        );
    }

    #[test]
    fn insufficient_proof_blocks_even_with_human_approval() {
        let correction = correction_with(0, 0.05);
        let authority = DecisionAuthority::new(envelope(AutonomyLevel::BoundedAutomatic));
        let result = authority.evaluate(
            &correction,
            AuthorityContext {
                human_approval: ApprovalState::Approved,
                quorum_approvals: 10,
            },
        );

        assert_eq!(result.verdict, AuthorityVerdict::Block);
        assert_eq!(result.reasons, vec![AuthorityReason::InsufficientProof]);
    }

    #[test]
    fn hard_envelope_blocks_even_when_approved() {
        let correction = correction_with(1, 0.30);
        let authority = DecisionAuthority::new(envelope(AutonomyLevel::BoundedAutomatic));
        let result = authority.evaluate(
            &correction,
            AuthorityContext {
                human_approval: ApprovalState::Approved,
                quorum_approvals: 10,
            },
        );

        assert_eq!(result.verdict, AuthorityVerdict::Block);
        assert_eq!(result.reasons, vec![AuthorityReason::OutsideHardEnvelope]);
    }

    #[test]
    fn approval_unknown_is_not_denial() {
        let correction = correction_with(1, 0.05);
        let authority = DecisionAuthority::new(envelope(AutonomyLevel::RecommendOnly));

        let unknown = authority.evaluate(&correction, AuthorityContext::default());
        assert_eq!(unknown.verdict, AuthorityVerdict::RequireApproval);
        assert!(unknown
            .reasons
            .contains(&AuthorityReason::HumanApprovalRequired));

        let denied = authority.evaluate(
            &correction,
            AuthorityContext {
                human_approval: ApprovalState::Denied,
                quorum_approvals: 0,
            },
        );
        assert_eq!(denied.verdict, AuthorityVerdict::Block);
        assert_eq!(denied.reasons, vec![AuthorityReason::HumanApprovalDenied]);
    }

    #[test]
    fn bounded_small_correction_can_run_automatically() {
        let correction = correction_with(1, 0.05);
        let authority = DecisionAuthority::new(envelope(AutonomyLevel::BoundedAutomatic));
        let result = authority.evaluate(&correction, AuthorityContext::default());

        assert_eq!(
            result.verdict,
            AuthorityVerdict::Allow(ExecutionMode::Automatic)
        );
    }

    #[test]
    fn soft_envelope_requires_human_then_allows_manual_execution() {
        let correction = correction_with(1, 0.20);
        let authority = DecisionAuthority::new(envelope(AutonomyLevel::BoundedAutomatic));

        let pending = authority.evaluate(&correction, AuthorityContext::default());
        assert_eq!(pending.verdict, AuthorityVerdict::RequireApproval);
        assert!(pending
            .reasons
            .contains(&AuthorityReason::OutsideAutonomousEnvelope));

        let approved = authority.evaluate(
            &correction,
            AuthorityContext {
                human_approval: ApprovalState::Approved,
                quorum_approvals: 0,
            },
        );
        assert_eq!(
            approved.verdict,
            AuthorityVerdict::Allow(ExecutionMode::ApprovedManual)
        );
    }

    #[test]
    fn quorum_is_independent_from_human_approval() {
        let correction = correction_with(1, 0.05);
        let envelope = SafetyEnvelope::new(
            AutonomyLevel::BoundedAutomatic,
            1,
            2,
            false,
            0.10,
            0.25,
        )
        .expect("valid envelope");
        let authority = DecisionAuthority::new(envelope);

        let pending = authority.evaluate(
            &correction,
            AuthorityContext {
                human_approval: ApprovalState::Approved,
                quorum_approvals: 1,
            },
        );
        assert_eq!(pending.verdict, AuthorityVerdict::RequireApproval);
        assert!(pending.reasons.contains(&AuthorityReason::QuorumPending));

        let allowed = authority.evaluate(
            &correction,
            AuthorityContext {
                human_approval: ApprovalState::Unknown,
                quorum_approvals: 2,
            },
        );
        assert_eq!(
            allowed.verdict,
            AuthorityVerdict::Allow(ExecutionMode::Automatic)
        );
    }
}
