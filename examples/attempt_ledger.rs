use lifetra::{
    ActionId, AttemptLedger, AttemptStatus, AuthorityTicket, BeadId, ExecutionMode,
    ExecutionOutcome, IdempotencyBinding, ReconciliationOutcome, RetryAuthority, RetryPolicy,
    RetryVerdict, Timestamp,
};

fn main() {
    let ticket = AuthorityTicket {
        action_id: ActionId::new("action:payout:ledger:42").expect("valid action id"),
        source_bead: BeadId::new("bead:payout:ledger:42"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:payout:42".into()],
        issued_at: Timestamp::new(100),
    };
    let binding =
        IdempotencyBinding::new(&ticket, "idem:payout:ledger:42").expect("valid binding");
    let mut ledger = AttemptLedger::new(ticket, binding).expect("valid attempt ledger");
    let retry_authority = RetryAuthority::new(RetryPolicy::new(1, false, true));

    let (trace, reconciliation, context) = ledger.retry_inputs().expect("initial retry inputs");
    let initial = retry_authority
        .evaluate(&trace, &ledger.binding, reconciliation.as_ref(), context)
        .expect("initial dispatch decision");
    assert_eq!(initial.verdict, RetryVerdict::InitialDispatchAllowed);

    ledger
        .record_dispatch(&initial, Timestamp::new(101), "proof:dispatch:attempt:0")
        .expect("attempt zero dispatch");
    ledger
        .record_reconciliation(
            0,
            Timestamp::new(105),
            ReconciliationOutcome::NoEffectConfirmed,
            "proof:provider:no-effect:attempt:0",
        )
        .expect("attempt zero reconciliation");

    let (trace, reconciliation, context) = ledger.retry_inputs().expect("redispatch inputs");
    let retry = retry_authority
        .evaluate(&trace, &ledger.binding, reconciliation.as_ref(), context)
        .expect("redispatch decision");
    assert_eq!(
        retry.verdict,
        RetryVerdict::RedispatchAllowed { ordinal: 1 }
    );

    ledger
        .record_dispatch(&retry, Timestamp::new(106), "proof:dispatch:attempt:1")
        .expect("attempt one dispatch");
    ledger
        .record_external_outcome(
            1,
            Timestamp::new(109),
            ExecutionOutcome::Succeeded,
            "proof:external:success:attempt:1",
        )
        .expect("attempt one external outcome");

    assert_eq!(ledger.attempts().len(), 2);
    assert_eq!(
        ledger.latest().expect("latest attempt").status(),
        AttemptStatus::EffectConfirmed(ExecutionOutcome::Succeeded)
    );
    assert!(!ledger.has_conflicting_evidence());

    for attempt in ledger.attempts() {
        println!(
            "attempt={} action={} status={:?}",
            attempt.id.ordinal,
            attempt.id.action_id.as_str(),
            attempt.status()
        );
    }
}
