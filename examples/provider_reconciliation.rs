use std::fs;

use lifetra::{
    ActionId, AuthorityTicket, BeadId, DurableJournal, ExecutionMode, IdempotencyBinding,
    ProviderObservation, ProviderReconciler, ProviderReconciliationAdapter,
    ProviderReconciliationQuery, ReconciliationOutcome, RetryAuthority, RetryDecision, RetryPolicy,
    RetryReason, RetryVerdict, Timestamp,
};

struct ProviderProof;

impl ProviderReconciliationAdapter for ProviderProof {
    type Error = &'static str;

    fn reconcile(
        &self,
        query: &ProviderReconciliationQuery,
    ) -> Result<ProviderObservation, Self::Error> {
        println!(
            "lookup action={} key={} attempt={} phase={:?}",
            query.action_id.as_str(),
            query.idempotency_key,
            query.attempt_ordinal,
            query.phase
        );

        Ok(ProviderObservation {
            observed_at: Timestamp::new(120),
            outcome: ReconciliationOutcome::NoEffectConfirmed,
            proof_ref: "proof:provider:no-effect:0".into(),
        })
    }
}

fn main() {
    let path = std::env::temp_dir().join(format!(
        "lifetra-provider-reconciliation-{}.journal",
        std::process::id()
    ));
    fs::remove_file(&path).ok();

    let ticket = AuthorityTicket {
        action_id: ActionId::new("action:payout:provider:42").expect("valid action id"),
        source_bead: BeadId::new("bead:payout:provider:42"),
        execution_mode: ExecutionMode::Automatic,
        authority_proof_refs: vec!["proof:authority:payout:42".into()],
        issued_at: Timestamp::new(100),
    };
    let binding =
        IdempotencyBinding::new(&ticket, "idem:payout:provider:42").expect("valid binding");
    let initial = RetryDecision {
        action_id: ticket.action_id.clone(),
        idempotency_key: binding.key.clone(),
        verdict: RetryVerdict::InitialDispatchAllowed,
        reasons: vec![RetryReason::AuthorizedNotDispatched],
        proof_refs: ticket.authority_proof_refs.clone(),
    };

    let mut journal =
        DurableJournal::create(&path, ticket, binding).expect("journal should create");
    journal
        .prepare_attempt(&initial, Timestamp::new(110))
        .expect("write-ahead prepare should persist");

    // Simulate a process crash: there is a durable PREPARE but no local dispatch receipt.
    drop(journal);

    let mut journal = DurableJournal::open(&path).expect("journal should reopen");
    let reconciler = ProviderReconciler::new(RetryAuthority::new(RetryPolicy::new(1, false, true)));
    let result = reconciler
        .reconcile_once(&mut journal, &ProviderProof)
        .expect("provider reconciliation should succeed");

    assert_eq!(
        result.decision.verdict,
        RetryVerdict::RedispatchAllowed { ordinal: 1 }
    );
    assert!(result
        .decision
        .proof_refs
        .contains(&"proof:provider:no-effect:0".to_owned()));

    println!("provider_outcome={:?}", result.observation.outcome);
    println!("retry_verdict={:?}", result.decision.verdict);
    println!("durable_proofs={:?}", result.decision.proof_refs);

    fs::remove_file(path).ok();
}
