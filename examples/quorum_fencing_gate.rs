use lifetra::{
    CoordinatorId, DatabaseNodeId, FenceOutcome, FenceReceipt, LeadershipGeneration,
    PromotionRequest, QuorumFencingAuthority, QuorumFencingError, QuorumVote, QuorumVoteDecision,
    Timestamp,
};

fn coordinator(value: &str) -> CoordinatorId {
    CoordinatorId::new(value).expect("coordinator")
}

fn node(value: &str) -> DatabaseNodeId {
    DatabaseNodeId::new(value).expect("database node")
}

fn main() {
    let mut authority = QuorumFencingAuthority::new(vec![
        coordinator("q1"),
        coordinator("q2"),
        coordinator("q3"),
    ])
    .expect("three-node quorum authority");
    let generation = LeadershipGeneration::new(1).expect("generation");
    authority
        .begin_generation(generation)
        .expect("begin first leadership generation");

    let request = PromotionRequest::new(
        "promote:db-b",
        generation,
        node("db-a"),
        node("db-b"),
    )
    .expect("promotion request");

    let q1 = QuorumVote::for_request(
        coordinator("q1"),
        &request,
        QuorumVoteDecision::Approve,
        "proof:vote:q1",
    )
    .expect("q1 vote");
    let no_quorum = authority
        .issue_fence_grant(
            &request,
            std::slice::from_ref(&q1),
            Timestamp::new(10),
            "grant:must-not-exist",
        )
        .expect_err("one of three votes must not authorize fencing");
    assert_eq!(
        no_quorum,
        QuorumFencingError::NoQuorum {
            approvals: 1,
            required: 2,
        }
    );

    let q2 = QuorumVote::for_request(
        coordinator("q2"),
        &request,
        QuorumVoteDecision::Approve,
        "proof:vote:q2",
    )
    .expect("q2 vote");
    let grant = authority
        .issue_fence_grant(
            &request,
            &[q1, q2],
            Timestamp::new(11),
            "grant:g1:db-a",
        )
        .expect("two of three votes authorize the fencing attempt");

    let unknown = FenceReceipt::new(
        generation,
        node("db-a"),
        Timestamp::new(12),
        FenceOutcome::Unknown,
        "proof:fence:unknown",
    )
    .expect("unknown receipt");
    assert!(matches!(
        authority.issue_promotion_permit(
            &grant,
            &unknown,
            Timestamp::new(13),
            "permit:must-not-exist"
        ),
        Err(QuorumFencingError::FenceNotConfirmed(FenceOutcome::Unknown))
    ));

    let confirmed = FenceReceipt::new(
        generation,
        node("db-a"),
        Timestamp::new(14),
        FenceOutcome::ConfirmedFenced,
        "proof:fence:db-a:off",
    )
    .expect("confirmed fence receipt");
    let permit = authority
        .issue_promotion_permit(&grant, &confirmed, Timestamp::new(15), "permit:g1:db-b")
        .expect("confirmed fencing plus quorum may authorize promotion");
    authority
        .validate_promotion_permit(&permit)
        .expect("permit is current");

    println!(
        "PROMOTION_PERMIT generation={} failed={} candidate={} approvals={}/{} fence_proof={} permit_ref={}",
        permit.request.generation.get(),
        permit.request.failed_primary.as_str(),
        permit.request.candidate.as_str(),
        permit.approving_coordinators.len(),
        authority.member_count(),
        permit.fence_proof_ref,
        permit.permit_ref
    );

    authority
        .begin_generation(generation.next().expect("next generation"))
        .expect("advance generation");
    assert!(matches!(
        authority.validate_promotion_permit(&permit),
        Err(QuorumFencingError::PermitNotCurrent {
            current: Some(2),
            permit: 1
        })
    ));
}
