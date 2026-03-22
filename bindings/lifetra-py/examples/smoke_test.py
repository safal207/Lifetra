from lifetra_py import (
    ResonanceState,
    StateTransition,
    SynergyState,
    Timestamp,
    TrajectoryState,
    average_alignment,
    combined_score,
    is_aligned,
    is_productive,
    transition_count,
)


trajectory = TrajectoryState("emerging", 0.64, 0.51)
trajectory.add_transition(
    StateTransition(
        "initialization",
        Timestamp(1_710_000_000),
        "concept takes coherent form",
    )
)

resonance = ResonanceState(0.80, 0.74, 0.69)
synergy = SynergyState(0.83, 0.71)

assert transition_count(trajectory) == 1
assert abs(average_alignment(resonance) - ((0.80 + 0.74 + 0.69) / 3.0)) < 1e-6
assert abs(combined_score(synergy) - 0.77) < 1e-6
assert is_aligned(resonance, 0.69) is True
assert is_productive(synergy, 0.70) is True

print("lifetra_py smoke test passed")
