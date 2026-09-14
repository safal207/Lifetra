use lifetra::{
    BeadId, BeadScale, EvidenceRef, EvidenceStatus, OrientationDelta, OrientationVector,
    ProvenOrientation, Timestamp, TrajectoryBead,
};

fn main() {
    let intended = OrientationVector::new(0.90, 0.60, 0.95, 0.50);

    let bead = TrajectoryBead::new(
        BeadId::new("agent:step:42"),
        BeadScale::Event,
        Timestamp::new(100),
        Timestamp::new(110),
    )
    .with_evidence(EvidenceRef::new(
        "receipt:step:42",
        EvidenceStatus::Supported,
        "external receipt verifies the state transition",
    ));

    let commit = bead
        .prove_transition("tool-result-applied", "verified external movement")
        .expect("proof-backed bead should commit");

    let proven = ProvenOrientation::from_commit(
        OrientationVector::new(0.55, 0.72, 0.90, 0.40),
        &commit,
    )
    .expect("commit must carry proof");

    let delta = OrientationDelta::between(intended, proven);
    let (axis, signed_gap) = delta.dominant_gap();

    println!("alignment={:.3}", delta.alignment_score());
    println!("dominant_gap={} signed_delta={:.3}", axis, signed_gap);
    println!("proofs={:?}", delta.proven.proof_refs);
}
