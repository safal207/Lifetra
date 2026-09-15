use lifetra::{
    ActionId, AuthorityContext, AuthorityTicket, AutonomyLevel, BeadId, BeadScale,
    CorrectionMemory, CorrectionPolicy, DecisionAuthority, EvidenceRef, EvidenceStatus,
    ExecutionStatus, ExecutionTrace, IdempotencyBinding, OrientationDelta, OrientationVector,
    ProvenOrientation, ReconciliationOutcome, ReconciliationReceipt, RetryAuthority, RetryContext,
    RetryPolicy, RetryVerdict, SafetyEnvelope, Timestamp, TrajectoryBead,
};

fn main() {
    let bead = TrajectoryBead::new(
        BeadId::new("agent:retry:1"),
        BeadScale::Event,
        Timestamp::new(100),
        Timestamp::new(110),
    )
    .with_evidence(EvidenceRef::new(
        "proof:movement:retry:1",
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
        ActionId::new("action:payout:retry:42").expect("valid action id"),
        &authority,
        Timestamp::new(111),
    )
    .expect("allowed authority should issue ticket");
    let binding = IdempotencyBinding::new(&ticket, "idem:payout:retry:42")
        .expect("valid idempotency binding");
    let retry = RetryAuthority::new(RetryPolicy::new(1, false, true));
    let mut trace = ExecutionTrace::new(ticket);

    let initial = retry
        .evaluate(&trace, &binding, None, RetryContext::default())
        .expect("initial dispatch decision");
    assert_eq!(initial.verdict, RetryVerdict::InitialDispatchAllowed);

    trace
        .record_dispatch(Timestamp::new(112), "proof:dispatch:retry:42")
        .expect("dispatch should record");
    assert_eq!(trace.status(), ExecutionStatus::DispatchedEffectUnknown);

    let before_reconcile = retry
        .evaluate(&trace, &binding, None, RetryContext::default())
        .expect("retry decision");
    assert_eq!(before_reconcile.verdict, RetryVerdict::ReconcileFirst);

    let no_effect = ReconciliationReceipt::new(
        trace.ticket.action_id.clone(),
        Timestamp::new(118),
        ReconciliationOutcome::NoEffectConfirmed,
        "proof:provider:no-effect:42",
    )
    .expect("valid reconciliation receipt");
    let redispatch = retry
        .evaluate(&trace, &binding, Some(&no_effect), RetryContext::default())
        .expect("retry decision");

    assert_eq!(
        redispatch.verdict,
        RetryVerdict::RedispatchAllowed { ordinal: 1 }
    );
    assert_eq!(redispatch.action_id, trace.ticket.action_id);
    assert_eq!(redispatch.idempotency_key, "idem:payout:retry:42");

    println!("action_id={}", redispatch.action_id.as_str());
    println!("idempotency_key={}", redispatch.idempotency_key);
    println!("verdict={:?}", redispatch.verdict);
    println!("proofs={:?}", redispatch.proof_refs);
}
