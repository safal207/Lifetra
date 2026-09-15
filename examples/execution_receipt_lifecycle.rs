use lifetra::{
    ActionId, AuthorityContext, AuthorityTicket, AutonomyLevel, BeadId, BeadScale,
    CorrectionMemory, CorrectionPolicy, DecisionAuthority, EvidenceRef, EvidenceStatus,
    ExecutionOutcome, ExecutionStatus, ExecutionTrace, OrientationDelta, OrientationVector,
    ProvenOrientation, SafetyEnvelope, Timestamp, TrajectoryBead,
};

fn main() {
    let bead = TrajectoryBead::new(
        BeadId::new("agent:execution:1"),
        BeadScale::Event,
        Timestamp::new(100),
        Timestamp::new(110),
    )
    .with_evidence(EvidenceRef::new(
        "proof:movement:1",
        EvidenceStatus::Supported,
        "external movement proof",
    ));

    let commit = bead
        .prove_transition("moved", "verified movement")
        .expect("proof-backed bead should commit");
    let proven =
        ProvenOrientation::from_commit(OrientationVector::new(0.55, 0.60, 0.90, 0.50), &commit)
            .expect("commit carries proof");
    let delta = OrientationDelta::between(OrientationVector::new(0.60, 0.60, 0.90, 0.50), proven);
    let correction = CorrectionPolicy::new(0.50, 0.01, 0.0, 0.05)
        .expect("valid correction policy")
        .propose(&delta, CorrectionMemory::default())
        .expect("proof-backed delta should produce correction");
    let envelope = SafetyEnvelope::new(AutonomyLevel::BoundedAutomatic, 1, 0, false, 0.10, 0.25)
        .expect("valid safety envelope");
    let authority =
        DecisionAuthority::new(envelope).evaluate(&correction, AuthorityContext::default());

    let ticket = AuthorityTicket::issue(
        ActionId::new("action:payout:42").expect("valid action id"),
        &authority,
        Timestamp::new(111),
    )
    .expect("allowed authority should issue ticket");
    let mut trace = ExecutionTrace::new(ticket);

    assert_eq!(trace.status(), ExecutionStatus::AuthorizedNotDispatched);

    trace
        .record_dispatch(Timestamp::new(112), "proof:dispatch:42")
        .expect("dispatch should record");
    assert_eq!(trace.status(), ExecutionStatus::DispatchedEffectUnknown);

    trace
        .record_external_outcome(
            Timestamp::new(116),
            ExecutionOutcome::Succeeded,
            "proof:external:42",
        )
        .expect("external outcome should record");
    assert_eq!(
        trace.status(),
        ExecutionStatus::EffectConfirmed(ExecutionOutcome::Succeeded)
    );

    println!("action_id={}", trace.ticket.action_id.as_str());
    println!("status={:?}", trace.status());
    println!("authority_proofs={:?}", trace.ticket.authority_proof_refs);
    println!(
        "dispatch_proof={:?}",
        trace.dispatch.as_ref().map(|item| &item.proof_ref)
    );
    println!(
        "external_proof={:?}",
        trace.external.as_ref().map(|item| &item.proof_ref)
    );
}
