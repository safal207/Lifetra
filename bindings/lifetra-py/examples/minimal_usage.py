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


def main() -> None:
    trajectory = TrajectoryState("emerging", momentum=0.64, stability=0.51)
    trajectory.add_transition(
        StateTransition(
            "initialization",
            Timestamp(1_710_000_000),
            "concept takes coherent form",
        )
    )
    trajectory.add_transition(
        StateTransition(
            "prototype",
            Timestamp(1_710_086_400),
            "first external feedback arrives",
        )
    )

    resonance = ResonanceState(0.80, 0.74, 0.69)
    synergy = SynergyState(0.83, 0.71)

    print(trajectory.summary())
    print(f"Transition count: {transition_count(trajectory)}")
    print(f"Average alignment: {average_alignment(resonance):.3f}")
    print(f"Aligned at 0.69: {is_aligned(resonance, 0.69)}")
    print(f"Combined synergy score: {combined_score(synergy):.3f}")
    print(f"Productive at 0.70: {is_productive(synergy, 0.70)}")


if __name__ == "__main__":
    main()
