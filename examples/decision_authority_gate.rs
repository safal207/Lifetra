use lifetra::{
    ApprovalState, AuthorityContext, AuthorityVerdict, AutonomyLevel, BeadId, BeadScale,
    CorrectionMemory, CorrectionPolicy, DecisionAuthority, EvidenceRef, EvidenceStatus,
    ExecutionMode, OrientationDelta, OrientationVector, ProvenOrientation, SafetyEnvelope,
    Timestamp, TrajectoryBead,
};

fn main() {
    let intended = OrientationVector::new(0.80, 0.60, 0.90, 0.55);
    let bead = TrajectoryBead::new(
        BeadId::new("agent:authority:1"),
        BeadScale::Event,
        Timestamp::new(100),
        Timestamp::new(110),
    )
    .with_evidence(EvidenceRef::new(
        "receipt:authority:1",
        EvidenceStatus::Supported,
        "external receipt verifies the movement",
    ));

    let commit = bead
        .prove_transition("tool-result-applied", "verified movement")
        .expect("proof-backed bead should commit");
    let proven =
        ProvenOrientation::from_commit(OrientationVector::new(0.52, 0.66, 0.88, 0.51), &commit)
            .expect("commit carries proof");
    let delta = OrientationDelta::between(intended, proven);
    let correction = CorrectionPolicy::new(0.50, 0.10, 0.04, 0.12)
        .expect("valid correction policy")
        .propose(&delta, CorrectionMemory::default())
        .expect("proof-backed delta should produce correction");

    let envelope = SafetyEnvelope::new(AutonomyLevel::BoundedAutomatic, 1, 0, false, 0.08, 0.20)
        .expect("valid safety envelope");
    let authority = DecisionAuthority::new(envelope);

    let pending = authority.evaluate(&correction, AuthorityContext::default());
    assert_eq!(pending.verdict, AuthorityVerdict::RequireApproval);

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

    println!("pending={:?}", pending.verdict);
    println!("approved={:?}", approved.verdict);
    println!("reasons={:?}", pending.reasons);
    println!("proofs={:?}", approved.proof_refs);
}
