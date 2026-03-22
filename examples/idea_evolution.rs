use lifetra::{
    CausalLink, CausalState, EntityId, EntityState, LifecycleStage, OrientationVector,
    ReflectionState, ResonanceState, StateTransition, SynergyState, Timestamp, TrajectoryState,
};

fn main() {
    let causality = CausalState::new(
        vec![
            CausalLink::new("founder frustration", 0.82),
            CausalLink::new("customer interviews", 0.91),
            CausalLink::new("adjacent tooling gaps", 0.74),
        ],
        0.81,
    );

    let mut trajectory = TrajectoryState::new(LifecycleStage::Evolving, 0.79, 0.58);
    trajectory.push_transition(StateTransition::new(
        "spark",
        Timestamp::new(1_710_000_000),
        "the idea appears as a response to repeated friction",
    ));
    trajectory.push_transition(StateTransition::new(
        "prototype",
        Timestamp::new(1_710_086_400),
        "a first implementation validates the core workflow",
    ));
    trajectory.push_transition(StateTransition::new(
        "signal",
        Timestamp::new(1_710_172_800),
        "early users begin to describe clear value in their own words",
    ));

    let entity = EntityState::new(
        EntityId::new("idea:adaptive-workbench"),
        causality,
        OrientationVector::new(0.93, 0.55, 0.87, 0.79),
        trajectory,
        ReflectionState::new(
            0.76,
            0.72,
            vec!["pricing is still uncertain".into()],
            vec![],
        ),
        ResonanceState::new(0.84, 0.78, 0.73),
        SynergyState::new(0.88, 0.69),
    );

    println!("== Lifetra idea evolution ==");
    println!("{}", entity.summary());
    println!("total influence: {:.2}", entity.causality.total_influence());

    if let Some(link) = entity.causality.strongest_link() {
        println!(
            "strongest causal source: {} ({:.2})",
            link.source, link.influence
        );
    }

    let (axis, score) = entity.orientation.dominant_axis();
    println!("dominant orientation: {:?} ({:.2})", axis, score);
    println!(
        "transitions recorded: {}",
        entity.trajectory.transition_count()
    );

    if let Some(transition) = entity.trajectory.latest_transition() {
        println!(
            "latest transition: {} at {}",
            transition.label,
            transition.occurred_at.epoch_seconds()
        );
    }

    println!(
        "resonance avg: {:.2}, synergy avg: {:.2}, synergy gap: {:.2}",
        entity.resonance.average_alignment(),
        entity.synergy.combined_score(),
        entity.synergy.synergy_gap()
    );
    println!(
        "health score: {:.2}, coherent at 0.70: {}",
        entity.health_score(),
        entity.is_coherent(0.70)
    );
}
