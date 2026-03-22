#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleStage {
    Emerging,
    Stabilizing,
    Evolving,
    Transforming,
    Dormant,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateTransition {
    pub label: String,
    pub occurred_at_epoch_seconds: i64,
    pub note: String,
}

impl StateTransition {
    pub fn new(
        label: impl Into<String>,
        occurred_at_epoch_seconds: i64,
        note: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            occurred_at_epoch_seconds,
            note: note.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryState {
    pub stage: LifecycleStage,
    pub momentum: f32,
    pub stability: f32,
    pub history: Vec<StateTransition>,
}

impl TrajectoryState {
    pub fn new(stage: LifecycleStage, momentum: f32, stability: f32) -> Self {
        Self {
            stage,
            momentum,
            stability,
            history: Vec::new(),
        }
    }

    pub fn with_history(mut self, history: Vec<StateTransition>) -> Self {
        self.history = history;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_transition_history() {
        let transition = StateTransition::new("shift", 42, "entered a new phase");
        let state =
            TrajectoryState::new(LifecycleStage::Evolving, 0.7, 0.5).with_history(vec![transition]);

        assert_eq!(state.history.len(), 1);
        assert!(matches!(state.stage, LifecycleStage::Evolving));
    }
}
