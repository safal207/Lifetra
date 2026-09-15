use std::collections::HashSet;
use std::fmt;

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

impl fmt::Display for LifecycleStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Emerging => "emerging",
            Self::Stabilizing => "stabilizing",
            Self::Evolving => "evolving",
            Self::Transforming => "transforming",
            Self::Dormant => "dormant",
        };

        f.write_str(label)
    }
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

    /// Returns the number of recorded transitions in the trajectory history.
    pub fn transition_count(&self) -> usize {
        self.history.len()
    }

    /// Returns the most recent transition based on timestamp ordering.
    pub fn latest_transition(&self) -> Option<&StateTransition> {
        self.history
            .iter()
            .max_by_key(|transition| transition.occurred_at)
    }
}

/// Time scale represented by one bead on a trajectory thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemporalGranularity {
    Frame,
    Iteration,
    Session,
    Hour,
    Day,
    Week,
    Custom(String),
}

impl fmt::Display for TemporalGranularity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Frame => f.write_str("frame"),
            Self::Iteration => f.write_str("iteration"),
            Self::Session => f.write_str("session"),
            Self::Hour => f.write_str("hour"),
            Self::Day => f.write_str("day"),
            Self::Week => f.write_str("week"),
            Self::Custom(label) => f.write_str(label),
        }
    }
}

/// Closed temporal interval covered by a trajectory bead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeWindow {
    pub start: Timestamp,
    pub end: Timestamp,
}

impl TimeWindow {
    pub fn new(start: Timestamp, end: Timestamp) -> Self {
        debug_assert!(start <= end, "time window must not run backwards");
        Self { start, end }
    }

    pub fn duration_seconds(self) -> i64 {
        self.end.epoch_seconds() - self.start.epoch_seconds()
    }
}

/// A node inside one sector-local graph carried by a trajectory bead.
#[derive(Debug, Clone, PartialEq)]
pub struct SectorNode {
    pub id: String,
    pub label: String,
    pub signal: Scalar,
    pub note: String,
}

impl SectorNode {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        signal: Scalar,
        note: impl Into<String>,
    ) -> Self {
        debug_assert!((0.0..=1.0).contains(&signal));
        Self {
            id: id.into(),
            label: label.into(),
            signal,
            note: note.into(),
        }
    }
}

/// Directed relation inside a sector-local graph.
#[derive(Debug, Clone, PartialEq)]
pub struct SectorEdge {
    pub from: String,
    pub to: String,
    pub relation: String,
    pub strength: Scalar,
}

impl SectorEdge {
    pub fn new(
        from: impl Into<String>,
        to: impl Into<String>,
        relation: impl Into<String>,
        strength: Scalar,
    ) -> Self {
        debug_assert!((0.0..=1.0).contains(&strength));
        Self {
            from: from.into(),
            to: to.into(),
            relation: relation.into(),
            strength,
        }
    }
}

/// One sector inside a bead. Each sector may carry its own small graph.
///
/// Sectors are intentionally generic: a caller can use them for spatial zones,
/// causal mechanisms, visual criteria, evidence, interaction states, or another
/// decomposition that is meaningful for the modeled system.
#[derive(Debug, Clone, PartialEq)]
pub struct BeadSector {
    pub id: String,
    pub label: String,
    pub spatial_scope: Option<String>,
    pub weight: Scalar,
    pub nodes: Vec<SectorNode>,
    pub edges: Vec<SectorEdge>,
}

impl BeadSector {
    pub fn new(id: impl Into<String>, label: impl Into<String>, weight: Scalar) -> Self {
        debug_assert!((0.0..=1.0).contains(&weight));
        Self {
            id: id.into(),
            label: label.into(),
            spatial_scope: None,
            weight,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn with_spatial_scope(mut self, spatial_scope: impl Into<String>) -> Self {
        self.spatial_scope = Some(spatial_scope.into());
        self
    }

    pub fn push_node(&mut self, node: SectorNode) {
        self.nodes.push(node);
    }

    pub fn push_edge(&mut self, edge: SectorEdge) {
        self.edges.push(edge);
    }

    pub fn average_signal(&self) -> Scalar {
        if self.nodes.is_empty() {
            return 0.0;
        }

        self.nodes.iter().map(|node| node.signal).sum::<Scalar>() / self.nodes.len() as Scalar
    }

    /// Checks whether every sector edge points to node IDs present in this sector.
    pub fn graph_is_consistent(&self) -> bool {
        let ids = self
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<HashSet<_>>();

        self.edges
            .iter()
            .all(|edge| ids.contains(edge.from.as_str()) && ids.contains(edge.to.as_str()))
    }
}

/// One time-bounded "bead" threaded onto a longer living trajectory.
///
/// A bead is a compact state slice: it has a time window, an alignment to the
/// thread's orientation center, several sector-local graphs, and evidence refs.
#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryBead {
    pub id: String,
    pub window: TimeWindow,
    pub granularity: TemporalGranularity,
    pub alignment_to_thread: Scalar,
    pub sectors: Vec<BeadSector>,
    pub evidence: Vec<String>,
}

impl TrajectoryBead {
    pub fn new(
        id: impl Into<String>,
        window: TimeWindow,
        granularity: TemporalGranularity,
        alignment_to_thread: Scalar,
    ) -> Self {
        debug_assert!((0.0..=1.0).contains(&alignment_to_thread));
        Self {
            id: id.into(),
            window,
            granularity,
            alignment_to_thread,
            sectors: Vec::new(),
            evidence: Vec::new(),
        }
    }

    pub fn push_sector(&mut self, sector: BeadSector) {
        self.sectors.push(sector);
    }

    pub fn add_evidence(&mut self, evidence_ref: impl Into<String>) {
        self.evidence.push(evidence_ref.into());
    }

    pub fn sector(&self, id: &str) -> Option<&BeadSector> {
        self.sectors.iter().find(|sector| sector.id == id)
    }

    /// Weighted summary signal across sector-local graphs.
    pub fn weighted_signal(&self) -> Scalar {
        let total_weight = self
            .sectors
            .iter()
            .map(|sector| sector.weight)
            .sum::<Scalar>();
        if total_weight <= Scalar::EPSILON {
            return 0.0;
        }

        self.sectors
            .iter()
            .map(|sector| sector.average_signal() * sector.weight)
            .sum::<Scalar>()
            / total_weight
    }

    pub fn sector_graphs_are_consistent(&self) -> bool {
        self.sectors.iter().all(BeadSector::graph_is_consistent)
    }
}

/// Kind of movement from one bead to another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeadTransitionKind {
    Observation,
    Refinement,
    Divergence,
    Recovery,
    Branch,
    Merge,
    Custom(String),
}

/// Causal transition connecting two beads through time and optional space.
#[derive(Debug, Clone, PartialEq)]
pub struct BeadTransition {
    pub from_bead: String,
    pub to_bead: String,
    pub occurred_at: Timestamp,
    pub kind: BeadTransitionKind,
    pub cause: String,
    pub effect: String,
    pub spatial_scope: Option<String>,
    pub confidence: Scalar,
}

impl BeadTransition {
    pub fn new(
        from_bead: impl Into<String>,
        to_bead: impl Into<String>,
        occurred_at: Timestamp,
        kind: BeadTransitionKind,
        cause: impl Into<String>,
        effect: impl Into<String>,
        confidence: Scalar,
    ) -> Self {
        debug_assert!((0.0..=1.0).contains(&confidence));
        Self {
            from_bead: from_bead.into(),
            to_bead: to_bead.into(),
            occurred_at,
            kind,
            cause: cause.into(),
            effect: effect.into(),
            spatial_scope: None,
            confidence,
        }
    }

    pub fn with_spatial_scope(mut self, spatial_scope: impl Into<String>) -> Self {
        self.spatial_scope = Some(spatial_scope.into());
        self
    }
}

/// The thread is the continuity of orientation through many time-bounded beads.
///
/// In the bead-on-a-thread metaphor, the thread is not just chronology. It is
/// the persistent orientation center that lets different local states remain
/// comparable even when the system changes, branches, recovers, or re-plans.
#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryThread {
    pub id: String,
    pub orientation_center: String,
    pub beads: Vec<TrajectoryBead>,
    pub transitions: Vec<BeadTransition>,
}

impl TrajectoryThread {
    pub fn new(id: impl Into<String>, orientation_center: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            orientation_center: orientation_center.into(),
            beads: Vec::new(),
            transitions: Vec::new(),
        }
    }

    pub fn push_bead(&mut self, bead: TrajectoryBead) {
        if let Some(latest) = self.latest_bead() {
            debug_assert!(
                bead.window.start >= latest.window.start,
                "beads should normally be appended in temporal order"
            );
        }
        self.beads.push(bead);
    }

    pub fn push_transition(&mut self, transition: BeadTransition) {
        self.transitions.push(transition);
    }

    pub fn bead_count(&self) -> usize {
        self.beads.len()
    }

    pub fn transition_count(&self) -> usize {
        self.transitions.len()
    }

    pub fn latest_bead(&self) -> Option<&TrajectoryBead> {
        self.beads.iter().max_by_key(|bead| bead.window.end)
    }

    pub fn mean_alignment(&self) -> Scalar {
        if self.beads.is_empty() {
            return 0.0;
        }

        self.beads
            .iter()
            .map(|bead| bead.alignment_to_thread)
            .sum::<Scalar>()
            / self.beads.len() as Scalar
    }

    pub fn trajectory_span(&self) -> Option<TimeWindow> {
        let start = self.beads.iter().map(|bead| bead.window.start).min()?;
        let end = self.beads.iter().map(|bead| bead.window.end).max()?;
        Some(TimeWindow::new(start, end))
    }

    /// Validates that transition endpoints refer to beads on this thread and
    /// that all sector-local graphs are internally connected to known node IDs.
    pub fn graph_is_consistent(&self) -> bool {
        let bead_ids = self
            .beads
            .iter()
            .map(|bead| bead.id.as_str())
            .collect::<HashSet<_>>();

        self.beads
            .iter()
            .all(TrajectoryBead::sector_graphs_are_consistent)
            && self.transitions.iter().all(|transition| {
                bead_ids.contains(transition.from_bead.as_str())
                    && bead_ids.contains(transition.to_bead.as_str())
            })
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

    #[test]
    fn exposes_transition_queries() {
        let state =
            TrajectoryState::new(LifecycleStage::Transforming, 0.8, 0.55).with_history(vec![
                StateTransition::new("seed", Timestamp::new(10), "first form"),
                StateTransition::new("pivot", Timestamp::new(30), "changed direction"),
                StateTransition::new("sync", Timestamp::new(20), "reconciled signals"),
            ]);

        assert_eq!(state.transition_count(), 3);
        assert_eq!(
            state
                .latest_transition()
                .map(|transition| transition.label.as_str()),
            Some("pivot")
        );
        assert_eq!(LifecycleStage::Transforming.to_string(), "transforming");
    }

    #[test]
    fn bead_contains_sector_local_graphs() {
        let mut visual =
            BeadSector::new("visual", "Visual fidelity", 0.7).with_spatial_scope("upper-landing");
        visual.push_node(SectorNode::new(
            "camera",
            "Camera",
            0.42,
            "too close to wall",
        ));
        visual.push_node(SectorNode::new(
            "light",
            "Lighting",
            0.58,
            "highlights retain detail",
        ));
        visual.push_edge(SectorEdge::new("camera", "light", "exposes", 0.6));

        let mut bead = TrajectoryBead::new(
            "iteration-1",
            TimeWindow::new(Timestamp::new(100), Timestamp::new(160)),
            TemporalGranularity::Iteration,
            0.64,
        );
        bead.push_sector(visual);
        bead.add_evidence("capture:upper-landing:v1");

        assert!(bead.sector_graphs_are_consistent());
        assert!(bead.weighted_signal() > 0.4);
        assert_eq!(bead.evidence.len(), 1);
    }

    #[test]
    fn thread_links_cause_space_transition_and_time() {
        let baseline = TrajectoryBead::new(
            "baseline",
            TimeWindow::new(Timestamp::new(100), Timestamp::new(120)),
            TemporalGranularity::Iteration,
            0.52,
        );
        let refined = TrajectoryBead::new(
            "refined",
            TimeWindow::new(Timestamp::new(121), Timestamp::new(150)),
            TemporalGranularity::Iteration,
            0.81,
        );

        let mut thread = TrajectoryThread::new(
            "visual-loop",
            "match the target while preserving spatial legibility and runtime constraints",
        );
        thread.push_bead(baseline);
        thread.push_bead(refined);
        thread.push_transition(
            BeadTransition::new(
                "baseline",
                "refined",
                Timestamp::new(121),
                BeadTransitionKind::Refinement,
                "camera intersects the dominant wall plane",
                "camera moved into the corridor center and reveals the doorway",
                0.9,
            )
            .with_spatial_scope("upper-landing"),
        );

        assert_eq!(thread.bead_count(), 2);
        assert_eq!(thread.transition_count(), 1);
        assert!(thread.mean_alignment() > 0.6);
        assert_eq!(thread.trajectory_span().unwrap().duration_seconds(), 50);
        assert!(thread.graph_is_consistent());
    }

    #[test]
    fn detects_broken_sector_or_thread_edges() {
        let mut sector = BeadSector::new("causal", "Causal", 1.0);
        sector.push_node(SectorNode::new("known", "Known node", 0.5, "present"));
        sector.push_edge(SectorEdge::new("known", "missing", "causes", 0.8));

        let mut bead = TrajectoryBead::new(
            "b1",
            TimeWindow::new(Timestamp::new(1), Timestamp::new(2)),
            TemporalGranularity::Frame,
            0.5,
        );
        bead.push_sector(sector);

        let mut thread = TrajectoryThread::new("t", "orientation");
        thread.push_bead(bead);
        thread.push_transition(BeadTransition::new(
            "b1",
            "missing-bead",
            Timestamp::new(2),
            BeadTransitionKind::Divergence,
            "unknown",
            "unknown",
            0.2,
        ));

        assert!(!thread.graph_is_consistent());
    }
}
