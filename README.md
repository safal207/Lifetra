# Lifetra

**Lifetra** is a Rust workspace for modeling the living trajectories of ideas and entities through six coupled dimensions:

- **Causality** — what produced the current state
- **Orientation** — where the entity is directed in conceptual or goal space
- **Trajectory** — how the entity evolves over time
- **Reflection** — how it observes and interprets itself
- **Resonance** — how aligned it is internally and with the world
- **Synergy** — what emerges through interaction and collaboration

This v0.1 foundation provides a clean, composable domain model rather than a full framework. It is intended to be a semantic seed for future simulation, analysis, and higher-level orchestration.

## Workspace layout

- `lifetra` — top-level crate that re-exports the public API
- `lifetra-core` — foundational primitives such as `EntityId`, `Timestamp`, and `Scalar`
- `lifetra-entity` — aggregated `EntityState` built from the six domain dimensions
- `lifetra-causal` — causal links and causal state
- `lifetra-orient` — orientation vectors across conceptual tendencies
- `lifetra-trajectory` — lifecycle stages and transition history
- `lifetra-reflect` — self-observation and contradictions
- `lifetra-resonance` — alignment with self, world, and time
- `lifetra-synergy` — collaborative potential and emergent value

## Example

```rust
use lifetra::{
    CausalLink, CausalState, EntityId, EntityState, LifecycleStage, OrientationVector,
    ReflectionState, ResonanceState, StateTransition, SynergyState, Timestamp, TrajectoryState,
};

let mut trajectory = TrajectoryState::new(LifecycleStage::Emerging, 0.64, 0.51);
trajectory.push_transition(StateTransition::new(
    "initialization",
    Timestamp::new(1_710_000_000),
    "concept takes coherent form",
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
    ReflectionState::default(),
    ResonanceState::new(0.8, 0.74, 0.69),
    SynergyState::new(0.83, 0.71),
);

assert!(entity.resonance.is_aligned(0.69));
assert!(entity.synergy.is_productive(0.7));
```

## Notes

- `lifetra-core` is now a true foundational crate, while `lifetra-entity` owns the aggregate `EntityState`.
- `Scalar` is used consistently across normalized domain measurements.
- Constructors include `debug_assert!` guards for normalized `0.0..=1.0` values in debug builds.
- `TrajectoryState`, `ResonanceState`, and `SynergyState` include small domain helpers for common checks and updates.

## Status

Lifetra v0.1 focuses on conceptual clarity, a compilable workspace architecture, and domain types that are minimal but meaningful.
