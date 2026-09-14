use lifetra_bead::BeadId;
use lifetra_core::Scalar;
use lifetra_orient::OrientationVector;

use crate::OrientationDelta;

/// Signed correction applied to the next intended orientation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OrientationAdjustment {
    pub growth: f32,
    pub stability: f32,
    pub truth: f32,
    pub connection: f32,
}

/// Hysteresis memory for each orientation axis.
///
/// An engaged axis remains active until its absolute gap falls below the
/// release threshold. This avoids rapid on/off switching around one boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CorrectionMemory {
    pub growth_engaged: bool,
    pub stability_engaged: bool,
    pub truth_engaged: bool,
    pub connection_engaged: bool,
}

/// Conservative bounded correction policy for the next intended orientation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorrectionPolicy {
    pub gain: Scalar,
    pub engage_threshold: Scalar,
    pub release_threshold: Scalar,
    pub max_step: Scalar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorrectionBlock {
    InvalidPolicy,
    MissingProof,
}

/// Proof-linked proposal for the next orientation.
///
/// This is a control proposal, not a claim about what actually happened.
#[derive(Debug, Clone, PartialEq)]
pub struct CorrectionDecision {
    pub source_bead: BeadId,
    pub proof_refs: Vec<String>,
    pub adjustment: OrientationAdjustment,
    pub next_orientation: OrientationVector,
    pub memory: CorrectionMemory,
}

impl CorrectionPolicy {
    /// Creates a policy with bounded proportional gain and per-axis hysteresis.
    ///
    /// Requirements:
    /// - every scalar is in `0.0..=1.0`;
    /// - `release_threshold <= engage_threshold`.
    pub fn new(
        gain: Scalar,
        engage_threshold: Scalar,
        release_threshold: Scalar,
        max_step: Scalar,
    ) -> Result<Self, CorrectionBlock> {
        let values_are_normalized = [gain, engage_threshold, release_threshold, max_step]
            .into_iter()
            .all(|value| (0.0..=1.0).contains(&value));

        if !values_are_normalized || release_threshold > engage_threshold {
            return Err(CorrectionBlock::InvalidPolicy);
        }

        Ok(Self {
            gain,
            engage_threshold,
            release_threshold,
            max_step,
        })
    }

    /// Produces the next intended orientation from a proof-backed delta.
    ///
    /// The policy never alters the proven movement stored in the delta. It only
    /// computes a bounded adjustment for the next planning step.
    pub fn propose(
        &self,
        delta: &OrientationDelta,
        previous: CorrectionMemory,
    ) -> Result<CorrectionDecision, CorrectionBlock> {
        if delta.proven.proof_refs.is_empty() {
            return Err(CorrectionBlock::MissingProof);
        }

        let (growth, growth_engaged) = self.axis_adjustment(delta.growth, previous.growth_engaged);
        let (stability, stability_engaged) =
            self.axis_adjustment(delta.stability, previous.stability_engaged);
        let (truth, truth_engaged) = self.axis_adjustment(delta.truth, previous.truth_engaged);
        let (connection, connection_engaged) =
            self.axis_adjustment(delta.connection, previous.connection_engaged);

        let adjustment = OrientationAdjustment {
            growth,
            stability,
            truth,
            connection,
        };

        let next_orientation = OrientationVector::new(
            clamp_unit(delta.intended.toward_growth + growth),
            clamp_unit(delta.intended.toward_stability + stability),
            clamp_unit(delta.intended.toward_truth + truth),
            clamp_unit(delta.intended.toward_connection + connection),
        );

        Ok(CorrectionDecision {
            source_bead: delta.bead_id.clone(),
            proof_refs: delta.proven.proof_refs.clone(),
            adjustment,
            next_orientation,
            memory: CorrectionMemory {
                growth_engaged,
                stability_engaged,
                truth_engaged,
                connection_engaged,
            },
        })
    }

    fn axis_adjustment(&self, gap: f32, was_engaged: bool) -> (f32, bool) {
        let magnitude = gap.abs();
        let engaged = if was_engaged {
            magnitude > self.release_threshold
        } else {
            magnitude >= self.engage_threshold
        };

        if !engaged {
            return (0.0, false);
        }

        let raw = self.gain * gap;
        (raw.clamp(-self.max_step, self.max_step), true)
    }
}

fn clamp_unit(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use crate::{
        BeadId, BeadScale, EvidenceRef, EvidenceStatus, OrientationVector, ProvenOrientation,
        Timestamp, TrajectoryBead,
    };

    use super::*;

    fn delta(intended: OrientationVector, proven: OrientationVector) -> OrientationDelta {
        let bead = TrajectoryBead::new(
            BeadId::new("bead:control:1"),
            BeadScale::Event,
            Timestamp::new(0),
            Timestamp::new(1),
        )
        .with_evidence(EvidenceRef::new(
            "proof:control:1",
            EvidenceStatus::Supported,
            "external movement proof",
        ));
        let commit = bead
            .prove_transition("moved", "verified movement")
            .expect("proof-backed bead should commit");
        let proven = ProvenOrientation::from_commit(proven, &commit).expect("commit carries proof");

        OrientationDelta::between(intended, proven)
    }

    #[test]
    fn rejects_invalid_hysteresis_configuration() {
        assert_eq!(
            CorrectionPolicy::new(0.5, 0.1, 0.2, 0.1),
            Err(CorrectionBlock::InvalidPolicy)
        );
    }

    #[test]
    fn deadband_ignores_small_noise() {
        let policy = CorrectionPolicy::new(0.5, 0.10, 0.05, 0.20).expect("valid policy");
        let delta = delta(
            OrientationVector::new(0.50, 0.50, 0.50, 0.50),
            OrientationVector::new(0.46, 0.54, 0.50, 0.50),
        );

        let decision = policy
            .propose(&delta, CorrectionMemory::default())
            .expect("proof-backed delta should produce decision");

        assert_eq!(decision.adjustment, OrientationAdjustment::default());
        assert_eq!(decision.next_orientation, delta.intended);
    }

    #[test]
    fn applies_gain_and_caps_each_axis_step() {
        let policy = CorrectionPolicy::new(0.8, 0.10, 0.05, 0.15).expect("valid policy");
        let delta = delta(
            OrientationVector::new(0.70, 0.50, 0.50, 0.50),
            OrientationVector::new(0.20, 0.90, 0.50, 0.50),
        );

        let decision = policy
            .propose(&delta, CorrectionMemory::default())
            .expect("proof-backed delta should produce decision");

        assert!((decision.adjustment.growth - 0.15).abs() < 0.000_1);
        assert!((decision.adjustment.stability + 0.15).abs() < 0.000_1);
        assert!((decision.next_orientation.toward_growth - 0.85).abs() < 0.000_1);
        assert!((decision.next_orientation.toward_stability - 0.35).abs() < 0.000_1);
    }

    #[test]
    fn hysteresis_keeps_axis_engaged_between_thresholds() {
        let policy = CorrectionPolicy::new(0.5, 0.10, 0.04, 0.20).expect("valid policy");
        let delta = delta(
            OrientationVector::new(0.50, 0.50, 0.50, 0.50),
            OrientationVector::new(0.44, 0.50, 0.50, 0.50),
        );

        let idle = policy
            .propose(&delta, CorrectionMemory::default())
            .expect("valid decision");
        assert_eq!(idle.adjustment.growth, 0.0);

        let engaged = policy
            .propose(
                &delta,
                CorrectionMemory {
                    growth_engaged: true,
                    ..CorrectionMemory::default()
                },
            )
            .expect("valid decision");
        assert!((engaged.adjustment.growth - 0.03).abs() < 0.000_1);
        assert!(engaged.memory.growth_engaged);
    }

    #[test]
    fn next_orientation_is_clamped_to_unit_interval() {
        let policy = CorrectionPolicy::new(1.0, 0.01, 0.0, 0.50).expect("valid policy");
        let delta = delta(
            OrientationVector::new(0.95, 0.05, 0.50, 0.50),
            OrientationVector::new(0.40, 0.80, 0.50, 0.50),
        );

        let decision = policy
            .propose(&delta, CorrectionMemory::default())
            .expect("valid decision");

        assert_eq!(decision.next_orientation.toward_growth, 1.0);
        assert_eq!(decision.next_orientation.toward_stability, 0.0);
    }
}
