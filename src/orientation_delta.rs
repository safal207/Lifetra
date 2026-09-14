use lifetra_bead::{BeadCommit, BeadId};
use lifetra_core::Scalar;
use lifetra_orient::{OrientationAxis, OrientationVector};

/// A proof-backed movement vector attributed to one committed bead.
///
/// The vector is descriptive: it records what the evidence-backed transition
/// actually moved toward. It does not inherit authority from the intended
/// orientation.
#[derive(Debug, Clone, PartialEq)]
pub struct ProvenOrientation {
    pub bead_id: BeadId,
    pub vector: OrientationVector,
    pub proof_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrientationBlock {
    MissingProof,
}

impl ProvenOrientation {
    /// Binds an observed movement vector to a proof-backed bead commit.
    ///
    /// A manually constructed commit with no proof references is rejected so
    /// intention or an unverified observation cannot masquerade as movement.
    pub fn from_commit(
        vector: OrientationVector,
        commit: &BeadCommit,
    ) -> Result<Self, OrientationBlock> {
        if commit.proof_refs.is_empty() {
            return Err(OrientationBlock::MissingProof);
        }

        Ok(Self {
            bead_id: commit.bead_id.clone(),
            vector,
            proof_refs: commit.proof_refs.clone(),
        })
    }
}

/// Difference between intended local orientation and proof-backed movement.
///
/// Signed deltas use `intended - proven`:
/// - positive means the proven movement under-realized the intention;
/// - negative means the proven movement exceeded that intended intensity;
/// - zero means the two match on that axis.
#[derive(Debug, Clone, PartialEq)]
pub struct OrientationDelta {
    pub bead_id: BeadId,
    pub intended: OrientationVector,
    pub proven: ProvenOrientation,
    pub growth: f32,
    pub stability: f32,
    pub truth: f32,
    pub connection: f32,
}

impl OrientationDelta {
    pub fn between(intended: OrientationVector, proven: ProvenOrientation) -> Self {
        let growth = intended.toward_growth - proven.vector.toward_growth;
        let stability = intended.toward_stability - proven.vector.toward_stability;
        let truth = intended.toward_truth - proven.vector.toward_truth;
        let connection = intended.toward_connection - proven.vector.toward_connection;

        Self {
            bead_id: proven.bead_id.clone(),
            intended,
            proven,
            growth,
            stability,
            truth,
            connection,
        }
    }

    /// Mean absolute gap across the four orientation axes, normalized to 0..=1.
    pub fn mean_absolute_gap(&self) -> Scalar {
        (self.growth.abs() + self.stability.abs() + self.truth.abs() + self.connection.abs())
            / 4.0
    }

    /// Simple first-pass alignment score: `1 - mean_absolute_gap`.
    ///
    /// This measures agreement between intended and proven direction vectors;
    /// it is not a truth or success score for the underlying task.
    pub fn alignment_score(&self) -> Scalar {
        1.0 - self.mean_absolute_gap()
    }

    /// Returns the axis with the largest absolute intention-vs-movement gap.
    pub fn dominant_gap(&self) -> (OrientationAxis, f32) {
        let gaps = [
            (OrientationAxis::Growth, self.growth),
            (OrientationAxis::Stability, self.stability),
            (OrientationAxis::Truth, self.truth),
            (OrientationAxis::Connection, self.connection),
        ];

        gaps.into_iter()
            .max_by(|left, right| {
                left.1
                    .abs()
                    .partial_cmp(&right.1.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or((OrientationAxis::Growth, 0.0))
    }
}

#[cfg(test)]
mod tests {
    use lifetra_bead::{BeadScale, EvidenceRef, EvidenceStatus, TrajectoryBead};
    use lifetra_core::Timestamp;
    use lifetra_trajectory::StateTransition;

    use super::*;

    fn committed_bead() -> BeadCommit {
        TrajectoryBead::new(
            BeadId::new("bead:orientation:1"),
            BeadScale::Event,
            Timestamp::new(10),
            Timestamp::new(20),
        )
        .with_evidence(EvidenceRef::new(
            "receipt:movement:1",
            EvidenceStatus::Supported,
            "external transition receipt",
        ))
        .prove_transition("moved", "proof-backed movement")
        .expect("supported bead should commit")
    }

    #[test]
    fn proven_orientation_preserves_commit_proof_identity() {
        let commit = committed_bead();
        let movement = ProvenOrientation::from_commit(
            OrientationVector::new(0.6, 0.7, 0.8, 0.2),
            &commit,
        )
        .expect("commit has proof");

        assert_eq!(movement.bead_id.as_str(), "bead:orientation:1");
        assert_eq!(movement.proof_refs, vec!["receipt:movement:1"]);
    }

    #[test]
    fn proofless_commit_cannot_define_proven_movement() {
        let commit = BeadCommit {
            bead_id: BeadId::new("bead:unproven"),
            proof_refs: Vec::new(),
            transition: StateTransition::new("claim", Timestamp::new(20), "no proof"),
        };

        assert_eq!(
            ProvenOrientation::from_commit(OrientationVector::default(), &commit),
            Err(OrientationBlock::MissingProof)
        );
    }

    #[test]
    fn computes_signed_orientation_delta_and_alignment() {
        let commit = committed_bead();
        let proven = ProvenOrientation::from_commit(
            OrientationVector::new(0.6, 0.7, 0.8, 0.2),
            &commit,
        )
        .expect("commit has proof");
        let delta = OrientationDelta::between(
            OrientationVector::new(0.9, 0.5, 0.8, 0.4),
            proven,
        );

        assert!((delta.growth - 0.3).abs() < 0.000_1);
        assert!((delta.stability + 0.2).abs() < 0.000_1);
        assert!(delta.truth.abs() < 0.000_1);
        assert!((delta.connection - 0.2).abs() < 0.000_1);
        assert!((delta.mean_absolute_gap() - 0.175).abs() < 0.000_1);
        assert!((delta.alignment_score() - 0.825).abs() < 0.000_1);
        assert_eq!(delta.dominant_gap().0, OrientationAxis::Growth);
    }
}
