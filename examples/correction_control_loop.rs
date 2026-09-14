use lifetra::{
    BeadId, BeadScale, CorrectionMemory, CorrectionPolicy, EvidenceRef, EvidenceStatus,
    OrientationDelta, OrientationVector, ProvenOrientation, Timestamp, TrajectoryBead,
};

fn main() {
    let intended = OrientationVector::new(0.80, 0.60, 0.90, 0.55);

    let bead = TrajectoryBead::new(
        BeadId::new("agent:control:1"),
        BeadScale::Event,
        Timestamp::new(100),
        Timestamp::new(110),
    )
    .with_evidence(EvidenceRef::new(
        "receipt:control:1",
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
    let policy = CorrectionPolicy::new(0.50, 0.10, 0.04, 0.12).expect("valid policy");
    let decision = policy
        .propose(&delta, CorrectionMemory::default())
        .expect("proof-backed delta should produce correction");

    println!("alignment={:.3}", delta.alignment_score());
    println!("growth_adjustment={:.3}", decision.adjustment.growth);
    println!(
        "next_growth={:.3}",
        decision.next_orientation.toward_growth
    );
    println!("proofs={:?}", decision.proof_refs);
}
