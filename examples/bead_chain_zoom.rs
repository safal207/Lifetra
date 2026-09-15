use lifetra::{
    BeadChain, BeadId, BeadScale, EvidenceRef, EvidenceStatus, Timestamp, TrajectoryBead,
};

fn verified_minute(id: &str, start: i64, proof: &str) -> TrajectoryBead {
    TrajectoryBead::new(
        BeadId::new(id),
        BeadScale::Minute,
        Timestamp::new(start),
        Timestamp::new(start + 60),
    )
    .with_evidence(EvidenceRef::new(
        proof,
        EvidenceStatus::Supported,
        "external receipt",
    ))
}

fn main() {
    let first = verified_minute("minute:1", 0, "receipt:1");
    let first_commit = first
        .prove_transition("minute-1-verified", "external receipt confirms step 1")
        .expect("verified minute should commit");

    let second = verified_minute("minute:2", 60, "receipt:2");
    let mut minute_chain = BeadChain::new();
    minute_chain.append(first).expect("append minute 1");
    minute_chain
        .append_with_commit(second, &first_commit)
        .expect("carry proof into minute 2");

    let third = verified_minute("minute:3", 120, "receipt:3");
    let second_commit = minute_chain
        .latest()
        .expect("minute 2 exists")
        .prove_transition("minute-2-verified", "proof continuity preserved")
        .expect("minute 2 should commit");
    minute_chain
        .append_with_commit(third, &second_commit)
        .expect("carry proof into minute 3");

    assert!(minute_chain.proof_continuity_is_intact());

    let hour = minute_chain
        .aggregate_window(BeadId::new("hour:1"), BeadScale::Hour, 0, 3)
        .expect("minutes should zoom into hour");

    println!("source beads: {}", hour.source_beads.len());
    println!("source proofs: {}", hour.source_proofs.len());
    println!(
        "hour can commit: {}",
        hour.bead
            .prove_transition("hour-verified", "zoomed proof remains traceable")
            .is_ok()
    );
}
