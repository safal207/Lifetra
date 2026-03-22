use lifetra::{
    CausalLink, CausalState, EntityId, EntityState, LifecycleStage, OrientationVector,
    ReflectionState, ResonanceState, StateTransition, SynergyState, Timestamp, TrajectoryState,
};

#[test]
fn constructs_complete_entity_state() {
    let transition = StateTransition::new(
        "initialization",
        Timestamp::new(1_710_000_000),
        "concept takes coherent form",
    );

    let mut trajectory = TrajectoryState::new(LifecycleStage::Emerging, 0.64, 0.51)
        .with_history(vec![transition.clone()]);
    trajectory.push_transition(StateTransition::new(
        "synchronization",
        Timestamp::new(1_710_000_600),
        "entity resonates with adjacent ideas",
    ));

    let entity = EntityState::new(
        EntityId::new("idea:lifetra"),
        CausalState::new(
            vec![
                CausalLink::new("systems thinking", 0.86),
                CausalLink::new("temporal modeling", 0.79),
            ],
            0.82,
        ),
        OrientationVector::new(0.91, 0.62, 0.88, 0.77),
        trajectory,
        ReflectionState::new(
            0.68,
            0.73,
            vec!["unknown external constraints".into()],
            vec!["ambition exceeds current implementation".into()],
        ),
        ResonanceState::new(0.8, 0.74, 0.69),
        SynergyState::new(0.83, 0.71),
    );

    assert_eq!(entity.id.as_str(), "idea:lifetra");
    assert_eq!(entity.trajectory.history.len(), 2);
    assert_eq!(
        entity.trajectory.history[0].occurred_at,
        Timestamp::new(1_710_000_000)
    );
    assert_eq!(entity.causality.influence_balance, 0.82);
    assert!(entity.resonance.is_aligned(0.69));
    assert!(entity.synergy.is_productive(0.7));
}
