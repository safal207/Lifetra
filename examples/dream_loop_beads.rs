use lifetra::{
    BeadSector, BeadTransition, BeadTransitionKind, SectorEdge, SectorNode,
    TemporalGranularity, TimeWindow, Timestamp, TrajectoryBead, TrajectoryThread,
};

fn visual_sector(id: &str, camera: f32, light: f32, material: f32, note: &str) -> BeadSector {
    let mut sector = BeadSector::new(id, "Visual refinement", 1.0).with_spatial_scope("upper-landing");
    sector.push_node(SectorNode::new("camera", "Camera composition", camera, note));
    sector.push_node(SectorNode::new(
        "light",
        "Lighting and exposure",
        light,
        "retain highlight detail and readable depth",
    ));
    sector.push_node(SectorNode::new(
        "material",
        "Material separation",
        material,
        "stone, plaster and timber should remain distinct",
    ));
    sector.push_edge(SectorEdge::new("camera", "light", "changes visible surface balance", 0.72));
    sector.push_edge(SectorEdge::new("light", "material", "reveals", 0.81));
    sector
}

fn main() {
    let mut baseline = TrajectoryBead::new(
        "baseline",
        TimeWindow::new(Timestamp::new(1_000), Timestamp::new(1_060)),
        TemporalGranularity::Iteration,
        0.52,
    );
    baseline.push_sector(visual_sector(
        "visual",
        0.35,
        0.48,
        0.51,
        "dominant wall plane overwhelms the doorway",
    ));
    baseline.add_evidence("capture:upper-landing:baseline");
    baseline.add_evidence("critic:composition=2.4/3");

    let mut camera_fix = TrajectoryBead::new(
        "camera-fix",
        TimeWindow::new(Timestamp::new(1_061), Timestamp::new(1_120)),
        TemporalGranularity::Iteration,
        0.71,
    );
    camera_fix.push_sector(visual_sector(
        "visual",
        0.74,
        0.52,
        0.54,
        "camera is centered on the circulation path and doorway",
    ));
    camera_fix.add_evidence("capture:upper-landing:camera-fix");

    let mut material_fix = TrajectoryBead::new(
        "material-fix",
        TimeWindow::new(Timestamp::new(1_121), Timestamp::new(1_180)),
        TemporalGranularity::Iteration,
        0.86,
    );
    material_fix.push_sector(visual_sector(
        "visual",
        0.79,
        0.76,
        0.83,
        "composition stays legible while lighting reveals material families",
    ));
    material_fix.add_evidence("capture:upper-landing:material-fix");
    material_fix.add_evidence("critic:total=8.4/10");

    let mut thread = TrajectoryThread::new(
        "dream-loop:upper-landing",
        "approach the target image while preserving route legibility, runtime quality and evidence integrity",
    );
    thread.push_bead(baseline);
    thread.push_bead(camera_fix);
    thread.push_bead(material_fix);

    thread.push_transition(
        BeadTransition::new(
            "baseline",
            "camera-fix",
            Timestamp::new(1_061),
            BeadTransitionKind::Refinement,
            "camera placement made a wall plane dominate the frame",
            "camera was moved toward the circulation axis and the doorway became readable",
            0.93,
        )
        .with_spatial_scope("upper-landing"),
    );
    thread.push_transition(
        BeadTransition::new(
            "camera-fix",
            "material-fix",
            Timestamp::new(1_121),
            BeadTransitionKind::Refinement,
            "flat exposure still collapsed plaster, stone and timber into similar tones",
            "lighting and material separation improved while preserving the corrected camera",
            0.88,
        )
        .with_spatial_scope("upper-landing"),
    );

    println!("thread: {}", thread.id);
    println!("orientation: {}", thread.orientation_center);
    println!("beads: {}", thread.bead_count());
    println!("transitions: {}", thread.transition_count());
    println!("mean alignment: {:.2}", thread.mean_alignment());
    println!("graph consistent: {}", thread.graph_is_consistent());

    for bead in &thread.beads {
        println!(
            "{} [{}] alignment={:.2} signal={:.2} evidence={}",
            bead.id,
            bead.granularity,
            bead.alignment_to_thread,
            bead.weighted_signal(),
            bead.evidence.len()
        );
    }
}
