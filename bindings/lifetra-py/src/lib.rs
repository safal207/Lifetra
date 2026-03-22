use lifetra_core::Timestamp as CoreTimestamp;
use lifetra_resonance::ResonanceState as CoreResonanceState;
use lifetra_synergy::SynergyState as CoreSynergyState;
use lifetra_trajectory::{
    LifecycleStage, StateTransition as CoreStateTransition, TrajectoryState as CoreTrajectoryState,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyType;

fn parse_stage(stage: &str) -> PyResult<LifecycleStage> {
    match stage.trim().to_ascii_lowercase().as_str() {
        "emerging" => Ok(LifecycleStage::Emerging),
        "stabilizing" => Ok(LifecycleStage::Stabilizing),
        "evolving" => Ok(LifecycleStage::Evolving),
        "transforming" => Ok(LifecycleStage::Transforming),
        "dormant" => Ok(LifecycleStage::Dormant),
        _ => Err(PyValueError::new_err(format!(
            "invalid lifecycle stage `{stage}`; expected one of: emerging, stabilizing, evolving, transforming, dormant"
        ))),
    }
}

#[pyclass(name = "Timestamp")]
#[derive(Clone)]
pub struct PyTimestamp {
    inner: CoreTimestamp,
}

#[pymethods]
impl PyTimestamp {
    #[new]
    fn new(epoch_seconds: i64) -> Self {
        Self {
            inner: CoreTimestamp::new(epoch_seconds),
        }
    }

    #[getter]
    fn epoch_seconds(&self) -> i64 {
        self.inner.epoch_seconds()
    }

    fn __repr__(&self) -> String {
        format!("Timestamp(epoch_seconds={})", self.epoch_seconds())
    }
}

#[pyclass(name = "StateTransition")]
#[derive(Clone)]
pub struct PyStateTransition {
    inner: CoreStateTransition,
}

#[pymethods]
impl PyStateTransition {
    #[new]
    fn new(label: String, occurred_at: PyRef<'_, PyTimestamp>, note: String) -> Self {
        Self {
            inner: CoreStateTransition::new(label, occurred_at.inner, note),
        }
    }

    #[getter]
    fn label(&self) -> String {
        self.inner.label.clone()
    }

    #[getter]
    fn occurred_at(&self) -> PyTimestamp {
        PyTimestamp {
            inner: self.inner.occurred_at,
        }
    }

    #[getter]
    fn note(&self) -> String {
        self.inner.note.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "StateTransition(label=\"{}\", occurred_at={}, note=\"{}\")",
            self.inner.label,
            self.inner.occurred_at.epoch_seconds(),
            self.inner.note,
        )
    }
}

#[pyclass(name = "ResonanceState")]
#[derive(Clone)]
pub struct PyResonanceState {
    inner: CoreResonanceState,
}

#[pymethods]
impl PyResonanceState {
    #[new]
    fn new(self_alignment: f32, world_alignment: f32, temporal_alignment: f32) -> Self {
        Self {
            inner: CoreResonanceState::new(self_alignment, world_alignment, temporal_alignment),
        }
    }

    #[getter]
    fn self_alignment(&self) -> f32 {
        self.inner.self_alignment
    }

    #[getter]
    fn world_alignment(&self) -> f32 {
        self.inner.world_alignment
    }

    #[getter]
    fn temporal_alignment(&self) -> f32 {
        self.inner.temporal_alignment
    }

    fn average_alignment(&self) -> f32 {
        self.inner.average_alignment()
    }

    fn is_aligned(&self, threshold: f32) -> bool {
        self.inner.is_aligned(threshold)
    }

    fn __repr__(&self) -> String {
        format!(
            "ResonanceState(self_alignment={:.3}, world_alignment={:.3}, temporal_alignment={:.3})",
            self.self_alignment(),
            self.world_alignment(),
            self.temporal_alignment(),
        )
    }
}

#[pyclass(name = "SynergyState")]
#[derive(Clone)]
pub struct PySynergyState {
    inner: CoreSynergyState,
}

#[pymethods]
impl PySynergyState {
    #[new]
    fn new(cooperative_potential: f32, emergent_value: f32) -> Self {
        Self {
            inner: CoreSynergyState::new(cooperative_potential, emergent_value),
        }
    }

    #[getter]
    fn cooperative_potential(&self) -> f32 {
        self.inner.cooperative_potential
    }

    #[getter]
    fn emergent_value(&self) -> f32 {
        self.inner.emergent_value
    }

    fn combined_score(&self) -> f32 {
        self.inner.combined_score()
    }

    fn is_productive(&self, threshold: f32) -> bool {
        self.inner.is_productive(threshold)
    }

    fn __repr__(&self) -> String {
        format!(
            "SynergyState(cooperative_potential={:.3}, emergent_value={:.3})",
            self.cooperative_potential(),
            self.emergent_value(),
        )
    }
}

#[pyclass(name = "TrajectoryState")]
#[derive(Clone)]
pub struct PyTrajectoryState {
    inner: CoreTrajectoryState,
}

#[pymethods]
impl PyTrajectoryState {
    #[new]
    #[pyo3(signature = (stage, momentum, stability, history=None))]
    fn new(
        stage: &str,
        momentum: f32,
        stability: f32,
        history: Option<Vec<PyStateTransition>>,
    ) -> PyResult<Self> {
        let mut inner = CoreTrajectoryState::new(parse_stage(stage)?, momentum, stability);
        if let Some(history) = history {
            inner = inner.with_history(history.into_iter().map(|item| item.inner).collect());
        }

        Ok(Self { inner })
    }

    #[getter]
    fn stage(&self) -> String {
        self.inner.stage.to_string()
    }

    #[getter]
    fn momentum(&self) -> f32 {
        self.inner.momentum
    }

    #[getter]
    fn stability(&self) -> f32 {
        self.inner.stability
    }

    #[getter]
    fn history(&self) -> Vec<PyStateTransition> {
        self.inner
            .history
            .iter()
            .cloned()
            .map(|inner| PyStateTransition { inner })
            .collect()
    }

    fn add_transition(&mut self, transition: PyStateTransition) {
        self.inner.push_transition(transition.inner);
    }

    fn transition_count(&self) -> usize {
        self.inner.transition_count()
    }

    fn latest_transition(&self) -> Option<PyStateTransition> {
        self.inner
            .latest_transition()
            .cloned()
            .map(|inner| PyStateTransition { inner })
    }

    fn summary(&self) -> String {
        let latest = self
            .inner
            .latest_transition()
            .map(|transition| transition.label.as_str())
            .unwrap_or("none");

        format!(
            "TrajectoryState(stage={}, momentum={:.3}, stability={:.3}, transitions={}, latest={})",
            self.inner.stage,
            self.inner.momentum,
            self.inner.stability,
            self.inner.transition_count(),
            latest,
        )
    }

    #[classmethod]
    fn stages(_cls: &Bound<'_, PyType>) -> Vec<&'static str> {
        vec![
            "emerging",
            "stabilizing",
            "evolving",
            "transforming",
            "dormant",
        ]
    }

    fn __repr__(&self) -> String {
        format!(
            "TrajectoryState(stage=\"{}\", momentum={:.3}, stability={:.3}, history={})",
            self.stage(),
            self.momentum(),
            self.stability(),
            self.transition_count(),
        )
    }
}

#[pyfunction]
fn average_alignment(state: PyRef<'_, PyResonanceState>) -> f32 {
    state.average_alignment()
}

#[pyfunction]
fn is_aligned(state: PyRef<'_, PyResonanceState>, threshold: f32) -> bool {
    state.is_aligned(threshold)
}

#[pyfunction]
fn combined_score(state: PyRef<'_, PySynergyState>) -> f32 {
    state.combined_score()
}

#[pyfunction]
fn is_productive(state: PyRef<'_, PySynergyState>, threshold: f32) -> bool {
    state.is_productive(threshold)
}

#[pyfunction]
fn transition_count(state: PyRef<'_, PyTrajectoryState>) -> usize {
    state.transition_count()
}

#[pymodule]
fn lifetra_py(_py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyTimestamp>()?;
    module.add_class::<PyStateTransition>()?;
    module.add_class::<PyResonanceState>()?;
    module.add_class::<PySynergyState>()?;
    module.add_class::<PyTrajectoryState>()?;
    module.add_function(wrap_pyfunction!(average_alignment, module)?)?;
    module.add_function(wrap_pyfunction!(is_aligned, module)?)?;
    module.add_function(wrap_pyfunction!(combined_score, module)?)?;
    module.add_function(wrap_pyfunction!(is_productive, module)?)?;
    module.add_function(wrap_pyfunction!(transition_count, module)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_lifecycle_stages() {
        assert!(matches!(
            parse_stage("Emerging"),
            Ok(LifecycleStage::Emerging)
        ));
        assert!(matches!(
            parse_stage("dormant"),
            Ok(LifecycleStage::Dormant)
        ));
    }

    #[test]
    fn rejects_invalid_lifecycle_stages() {
        assert!(parse_stage("unknown").is_err());
    }
}
