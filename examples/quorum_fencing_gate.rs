use lifetra::{
    CoordinatorId, DatabaseNodeId, FenceGrant, FenceOutcome, FenceReceipt, LeadershipGeneration,
    PromotionRequest, QuorumFencingAuthority, QuorumFencingError, QuorumVote, QuorumVoteDecision,
    Timestamp,
};

fn coordinator(value: &str) -> CoordinatorId {
    CoordinatorId::new(value).expect("coordinator")
}

fn node(value: &str) -> DatabaseNodeId {
    DatabaseNodeId::new(value).expect("database node")
}

fn setup() -> (
    QuorumFencingAuthority,
    LeadershipGeneration,
    PromotionRequest,
    QuorumVote,
    QuorumVote,
) {
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
    let request = PromotionRequest::new("promote:db-b", generation, node("db-a"), node("db-b"))
        .expect("promotion request");
    let q1 = QuorumVote::for_request(
        coordinator("q1"),
        &request,
        QuorumVoteDecision::Approve,
        "proof:vote:q1",
    )
    .expect("q1 vote");
    let q2 = QuorumVote::for_request(
        coordinator("q2"),
        &request,
        QuorumVoteDecision::Approve,
        "proof:vote:q2",
    )
    .expect("q2 vote");
    (authority, generation, request, q1, q2)
}

fn require_no_quorum() {
    let (mut authority, _, request, q1, _) = setup();
    let err = authority
        .issue_fence_grant(&request, &[q1], Timestamp::new(10), "grant:must-not-exist")
        .expect_err("one of three votes must not authorize fencing");
    assert_eq!(
        err,
        QuorumFencingError::NoQuorum {
            approvals: 1,
            required: 2,
        }
    );
    println!("NO_QUORUM approvals=1 required=2");
}

fn issue_grant() -> (QuorumFencingAuthority, LeadershipGeneration, FenceGrant) {
    let (mut authority, generation, request, q1, q2) = setup();
    let grant = authority
        .issue_fence_grant(&request, &[q1, q2], Timestamp::new(11), "grant:g1:db-a")
        .expect("two of three votes authorize the fencing attempt");
    println!(
        "FENCE_GRANT generation={} target={} candidate={} approvals={}/{} grant_ref={}",
        grant.request.generation.get(),
        grant.request.failed_primary.as_str(),
        grant.request.candidate.as_str(),
        grant.approving_coordinators.len(),
        authority.member_count(),
        grant.grant_ref
    );
    (authority, generation, grant)
}

fn require_unknown_fence_blocked() {
    let (authority, generation, grant) = issue_grant();
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
    println!("FENCE_UNKNOWN_BLOCKED generation=1 target=db-a");
}

fn issue_permit() {
    let (mut authority, generation, grant) = issue_grant();
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
    println!("STALE_PERMIT_BLOCKED permit_generation=1 current_generation=2");
}

fn main() {
    match std::env::var("LIFETRA_QUORUM_PHASE")
        .unwrap_or_else(|_| "full".into())
        .as_str()
    {
        "deny-no-quorum" => require_no_quorum(),
        "fence-grant" => {
            issue_grant();
        }
        "deny-unknown-fence" => require_unknown_fence_blocked(),
        "promotion-permit" => issue_permit(),
        "full" => {
            require_no_quorum();
            require_unknown_fence_blocked();
            issue_permit();
        }
        other => panic!("unsupported LIFETRA_QUORUM_PHASE: {other}"),
    }
}
