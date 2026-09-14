use lifetra_core::{Scalar, Timestamp};
use lifetra_trajectory::{StateTransition, TrajectoryState};

/// Stable identifier for one bounded trajectory context (a "bead").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BeadId(String);

impl BeadId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Temporal or semantic scale used to bound a bead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeadScale {
    Event,
    Minute,
    Hour,
    Day,
    Week,
    Custom(String),
}

/// A sector is one projection of reality inside a bead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectorKind {
    Causality,
    Orientation,
    State,
    Agent,
    Evidence,
    Environment,
    Risk,
    Outcome,
    Reflection,
    Resonance,
    Synergy,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectorNode {
    pub id: String,
    pub label: String,
}

impl SectorNode {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq)]
pub struct SectorGraph {
    pub kind: SectorKind,
    pub nodes: Vec<SectorNode>,
    pub edges: Vec<SectorEdge>,
}

impl SectorGraph {
    pub fn new(kind: SectorKind) -> Self {
        Self {
            kind,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn with_nodes(mut self, nodes: Vec<SectorNode>) -> Self {
        self.nodes = nodes;
        self
    }

    pub fn with_edges(mut self, edges: Vec<SectorEdge>) -> Self {
        self.edges = edges;
        self
    }
}

/// Evidence remains explicitly three-valued: unknown is not false.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceStatus {
    Unknown,
    Supported,
    Contradicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceRef {
    pub id: String,
    pub status: EvidenceStatus,
    pub note: String,
}

impl EvidenceRef {
    pub fn new(id: impl Into<String>, status: EvidenceStatus, note: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            status,
            note: note.into(),
        }
    }
}

/// One bounded combination of reality crossed by the larger trajectory.
#[derive(Debug, Clone, PartialEq)]
pub struct TrajectoryBead {
    pub id: BeadId,
    pub scale: BeadScale,
    pub starts_at: Timestamp,
    pub ends_at: Timestamp,
    pub sectors: Vec<SectorGraph>,
    pub evidence: Vec<EvidenceRef>,
    pub unresolved_unknowns: Vec<String>,
}

impl TrajectoryBead {
    pub fn new(id: BeadId, scale: BeadScale, starts_at: Timestamp, ends_at: Timestamp) -> Self {
        debug_assert!(starts_at <= ends_at);
        Self {
            id,
            scale,
            starts_at,
            ends_at,
            sectors: Vec::new(),
            evidence: Vec::new(),
            unresolved_unknowns: Vec::new(),
        }
    }

    pub fn with_sector(mut self, sector: SectorGraph) -> Self {
        self.sectors.push(sector);
        self
    }

    pub fn with_evidence(mut self, evidence: EvidenceRef) -> Self {
        self.evidence.push(evidence);
        self
    }

    pub fn with_unknown(mut self, unknown: impl Into<String>) -> Self {
        self.unresolved_unknowns.push(unknown.into());
        self
    }

    pub fn supported_evidence_count(&self) -> usize {
        self.evidence
            .iter()
            .filter(|item| item.status == EvidenceStatus::Supported)
            .count()
    }

    /// Produces a trajectory commit only when the bead has at least one supported proof,
    /// no contradicted proof, and no unresolved unknowns.
    pub fn prove_transition(
        &self,
        label: impl Into<String>,
        note: impl Into<String>,
    ) -> Result<BeadCommit, CommitBlock> {
        if !self.unresolved_unknowns.is_empty() {
            return Err(CommitBlock::UnresolvedUnknowns(
                self.unresolved_unknowns.clone(),
            ));
        }

        let contradicted: Vec<String> = self
            .evidence
            .iter()
            .filter(|item| item.status == EvidenceStatus::Contradicted)
            .map(|item| item.id.clone())
            .collect();

        if !contradicted.is_empty() {
            return Err(CommitBlock::ContradictedEvidence(contradicted));
        }

        let supported: Vec<String> = self
            .evidence
            .iter()
            .filter(|item| item.status == EvidenceStatus::Supported)
            .map(|item| item.id.clone())
            .collect();

        if supported.is_empty() {
            return Err(CommitBlock::NoSupportedEvidence);
        }

        Ok(BeadCommit {
            bead_id: self.id.clone(),
            proof_refs: supported,
            transition: StateTransition::new(label, self.ends_at, note),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitBlock {
    NoSupportedEvidence,
    UnresolvedUnknowns(Vec<String>),
    ContradictedEvidence(Vec<String>),
}

/// The evidence-carrying output that is allowed to move the trajectory forward.
#[derive(Debug, Clone, PartialEq)]
pub struct BeadCommit {
    pub bead_id: BeadId,
    pub proof_refs: Vec<String>,
    pub transition: StateTransition,
}

impl BeadCommit {
    pub fn apply_to(&self, trajectory: &mut TrajectoryState) {
        trajectory.push_transition(self.transition.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lifetra_trajectory::LifecycleStage;

    #[test]
    fn sector_graph_keeps_local_relations() {
        let graph = SectorGraph::new(SectorKind::Causality)
            .with_nodes(vec![
                SectorNode::new("dispatch", "side effect dispatched"),
                SectorNode::new("receipt", "external receipt observed"),
            ])
            .with_edges(vec![SectorEdge::new(
                "dispatch", "receipt", "precedes", 0.95,
            )]);

        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.edges.len(), 1);
    }

    #[test]
    fn unknown_does_not_collapse_into_failure() {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:recovery:42"),
            BeadScale::Event,
            Timestamp::new(10),
            Timestamp::new(20),
        )
        .with_evidence(EvidenceRef::new(
            "dispatch-proof",
            EvidenceStatus::Supported,
            "dispatch is proven",
        ))
        .with_unknown("remote effect completion");

        assert!(matches!(
            bead.prove_transition("recovered", "resume after crash"),
            Err(CommitBlock::UnresolvedUnknowns(_))
        ));
    }

    #[test]
    fn supported_proof_can_commit_back_to_trajectory() {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:hour:1"),
            BeadScale::Hour,
            Timestamp::new(100),
            Timestamp::new(200),
        )
        .with_evidence(EvidenceRef::new(
            "receipt:abc",
            EvidenceStatus::Supported,
            "external state confirms the transition",
        ));

        let commit = bead
            .prove_transition("verified-shift", "proof-backed state delta")
            .expect("supported bead should produce a commit");

        let mut trajectory = TrajectoryState::new(LifecycleStage::Evolving, 0.7, 0.6);
        commit.apply_to(&mut trajectory);

        assert_eq!(trajectory.transition_count(), 1);
        assert_eq!(commit.proof_refs, vec!["receipt:abc".to_string()]);
        assert_eq!(commit.bead_id.as_str(), "bead:hour:1");
    }
}
