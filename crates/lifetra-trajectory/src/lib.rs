use lifetra_core::{Scalar, Timestamp};

/// High-level lifecycle phase of an entity.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleStage {
    Emerging,
    Stabilizing,
    Evolving,
    Transforming,
    Dormant,
}

/// A named change in the trajectory history of an entity.
#[derive(Debug, Clone, PartialEq)]
pub struct StateTransition {
    pub label: String,
    pub occurred_at: Timestamp,
    pub note: String,
}

impl StateTransition {
    /// Creates a transition event with a label, timestamp, and note.
    pub fn new(label: impl Into<String>, occurred_at: Timestamp, note: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            occurred_at,
            note: note.into(),
        }
    }
}

/// Temporal evolution state including lifecycle, momentum, stability, and history.
#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryState {
    pub stage: LifecycleStage,
    pub momentum: Scalar,
    pub stability: Scalar,
    pub history: Vec<StateTransition>,
}

impl TrajectoryState {
    /// Creates a new trajectory state with no recorded history.
    pub fn new(stage: LifecycleStage, momentum: Scalar, stability: Scalar) -> Self {
        debug_assert!((0.0..=1.0).contains(&momentum));
        debug_assert!((0.0..=1.0).contains(&stability));

        Self {
            stage,
            momentum,
            stability,
            history: Vec::new(),
        }
    }

    /// Replaces the full transition history.
    pub fn with_history(mut self, history: Vec<StateTransition>) -> Self {
        self.history = history;
        self
    }

    /// Appends a single transition to the history.
    pub fn push_transition(&mut self, transition: StateTransition) {
        self.history.push(transition);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_transition_history() {
        let transition = StateTransition::new("shift", Timestamp::new(42), "entered a new phase");
        let state =
            TrajectoryState::new(LifecycleStage::Evolving, 0.7, 0.5).with_history(vec![transition]);

        assert_eq!(state.history.len(), 1);
        assert!(matches!(state.stage, LifecycleStage::Evolving));
    }

    #[test]
    fn pushes_transition_incrementally() {
        let transition =
            StateTransition::new("reflection", Timestamp::new(99), "tracked a fresh update");
        let mut state = TrajectoryState::new(LifecycleStage::Stabilizing, 0.5, 0.8);

        state.push_transition(transition.clone());

        assert_eq!(state.history, vec![transition]);
    }
}
