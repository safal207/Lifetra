use lifetra::{
    BeadId, BeadScale, EvidenceRef, EvidenceStatus, LifecycleStage, SectorEdge, SectorGraph,
    SectorKind, SectorNode, Timestamp, TrajectoryBead, TrajectoryState,
};

fn main() {
    let causal = SectorGraph::new(SectorKind::Causality)
        .with_nodes(vec![
            SectorNode::new("dispatch", "side effect dispatched"),
            SectorNode::new("receipt", "external receipt observed"),
        ])
        .with_edges(vec![SectorEdge::new(
            "dispatch",
            "receipt",
            "confirmed-by",
            0.98,
        )]);

    let bead = TrajectoryBead::new(
        BeadId::new("recovery:42"),
        BeadScale::Event,
        Timestamp::new(1_710_000_000),
        Timestamp::new(1_710_000_030),
    )
    .with_sector(causal)
    .with_evidence(EvidenceRef::new(
        "receipt:provider:abc",
        EvidenceStatus::Supported,
        "provider receipt confirms the external effect",
    ));

    let commit = bead
        .prove_transition("reconciled", "recovery completed with external proof")
        .expect("proof-backed bead should commit");

    let mut trajectory = TrajectoryState::new(LifecycleStage::Evolving, 0.72, 0.66);
    commit.apply_to(&mut trajectory);

    println!("committed bead: {}", commit.bead_id.as_str());
    println!("proof refs: {:?}", commit.proof_refs);
    println!("trajectory transitions: {}", trajectory.transition_count());
}
