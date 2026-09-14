use std::collections::BTreeSet;

use crate::{
    BeadCommit, BeadId, BeadScale, EvidenceRef, EvidenceStatus, TrajectoryBead,
};

/// A proof-preserving link between two consecutive beads on one linear trajectory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofCarry {
    pub from_bead: BeadId,
    pub to_bead: BeadId,
    pub proof_refs: Vec<String>,
}

/// Reasons why a bead cannot be appended to the current linear chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainBlock {
    NoPreviousBead,
    TemporalOverlap {
        previous_bead: BeadId,
        next_bead: BeadId,
    },
    CommitSourceMismatch {
        expected_bead: BeadId,
        commit_bead: BeadId,
    },
    ConflictingCarriedProof {
        proof_ref: String,
    },
}

/// Reasons why a set of lower-level beads cannot be aggregated into a zoomed bead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AggregationBlock {
    EmptyWindow,
    InvalidWindow,
    TargetScaleNotCoarser {
        source_scale: BeadScale,
        target_scale: BeadScale,
    },
}

/// A provenance-preserving result of causal zoom.
///
/// `bead` can participate in a higher-level chain while `source_beads` and
/// `source_proofs` retain the drill-down path to the lower-level evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct BeadAggregate {
    pub bead: TrajectoryBead,
    pub source_beads: Vec<BeadId>,
    pub source_proofs: Vec<String>,
}

impl BeadAggregate {
    pub fn into_bead(self) -> TrajectoryBead {
        self.bead
    }
}

/// A linear proof-carrying sequence of bounded local realities.
///
/// The chain permits temporal gaps but rejects overlap. A gap is an explicit
/// absence of modeled context; it is not silently rewritten as continuity.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BeadChain {
    beads: Vec<TrajectoryBead>,
    carries: Vec<ProofCarry>,
}

impl BeadChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn beads(&self) -> &[TrajectoryBead] {
        &self.beads
    }

    pub fn carries(&self) -> &[ProofCarry] {
        &self.carries
    }

    pub fn latest(&self) -> Option<&TrajectoryBead> {
        self.beads.last()
    }

    pub fn append(&mut self, bead: TrajectoryBead) -> Result<(), ChainBlock> {
        self.validate_temporal_append(&bead)?;
        self.beads.push(bead);
        Ok(())
    }

    /// Appends a bead while carrying the previous bead's verified proof refs
    /// forward as explicit supported evidence.
    ///
    /// Existing supported evidence with the same id is reused. Existing
    /// unknown or contradicted evidence with the same id blocks the carry
    /// instead of being overwritten.
    pub fn append_with_commit(
        &mut self,
        mut bead: TrajectoryBead,
        previous_commit: &BeadCommit,
    ) -> Result<(), ChainBlock> {
        let previous = self.beads.last().ok_or(ChainBlock::NoPreviousBead)?;

        if previous_commit.bead_id != previous.id {
            return Err(ChainBlock::CommitSourceMismatch {
                expected_bead: previous.id.clone(),
                commit_bead: previous_commit.bead_id.clone(),
            });
        }

        self.validate_temporal_append(&bead)?;

        for proof_ref in &previous_commit.proof_refs {
            if let Some(existing) = bead.evidence.iter().find(|item| item.id == *proof_ref) {
                if existing.status != EvidenceStatus::Supported {
                    return Err(ChainBlock::ConflictingCarriedProof {
                        proof_ref: proof_ref.clone(),
                    });
                }
            } else {
                bead.evidence.push(EvidenceRef::new(
                    proof_ref.clone(),
                    EvidenceStatus::Supported,
                    format!("carried from {}", previous.id.as_str()),
                ));
            }
        }

        self.carries.push(ProofCarry {
            from_bead: previous.id.clone(),
            to_bead: bead.id.clone(),
            proof_refs: previous_commit.proof_refs.clone(),
        });
        self.beads.push(bead);

        Ok(())
    }

    /// Returns the unmodeled time between consecutive beads, in seconds.
    /// Zero means the beads touch exactly.
    pub fn temporal_gaps(&self) -> Vec<i64> {
        self.beads
            .windows(2)
            .map(|window| {
                window[1].starts_at.epoch_seconds() - window[0].ends_at.epoch_seconds()
            })
            .collect()
    }

    /// Verifies that every recorded carried proof still exists as supported
    /// evidence in its destination bead.
    pub fn proof_continuity_is_intact(&self) -> bool {
        self.carries.iter().all(|carry| {
            self.beads
                .iter()
                .find(|bead| bead.id == carry.to_bead)
                .map(|bead| {
                    carry.proof_refs.iter().all(|proof_ref| {
                        bead.evidence.iter().any(|item| {
                            item.id == *proof_ref && item.status == EvidenceStatus::Supported
                        })
                    })
                })
                .unwrap_or(false)
        })
    }

    /// Aggregates a half-open range `[start, end)` into a coarser bead while
    /// preserving source bead ids, supported proof refs, and unresolved unknowns.
    pub fn aggregate_window(
        &self,
        aggregate_id: BeadId,
        target_scale: BeadScale,
        start: usize,
        end: usize,
    ) -> Result<BeadAggregate, AggregationBlock> {
        if start >= end {
            return Err(AggregationBlock::EmptyWindow);
        }
        if end > self.beads.len() {
            return Err(AggregationBlock::InvalidWindow);
        }

        let source = &self.beads[start..end];
        for bead in source {
            if !scale_is_coarser(&target_scale, &bead.scale) {
                return Err(AggregationBlock::TargetScaleNotCoarser {
                    source_scale: bead.scale.clone(),
                    target_scale: target_scale.clone(),
                });
            }
        }

        let mut proof_refs = BTreeSet::new();
        let mut unknowns = BTreeSet::new();
        let mut source_beads = Vec::with_capacity(source.len());

        for bead in source {
            source_beads.push(bead.id.clone());
            for evidence in &bead.evidence {
                if evidence.status == EvidenceStatus::Supported {
                    proof_refs.insert(evidence.id.clone());
                }
            }
            unknowns.extend(bead.unresolved_unknowns.iter().cloned());
        }

        let source_proofs: Vec<String> = proof_refs.into_iter().collect();
        let mut aggregate = TrajectoryBead::new(
            aggregate_id,
            target_scale,
            source.first().expect("non-empty source").starts_at,
            source.last().expect("non-empty source").ends_at,
        );

        for proof_ref in &source_proofs {
            aggregate.evidence.push(EvidenceRef::new(
                proof_ref.clone(),
                EvidenceStatus::Supported,
                "preserved by causal zoom",
            ));
        }
        aggregate.unresolved_unknowns.extend(unknowns);

        Ok(BeadAggregate {
            bead: aggregate,
            source_beads,
            source_proofs,
        })
    }

    fn validate_temporal_append(&self, bead: &TrajectoryBead) -> Result<(), ChainBlock> {
        if let Some(previous) = self.beads.last() {
            if bead.starts_at < previous.ends_at {
                return Err(ChainBlock::TemporalOverlap {
                    previous_bead: previous.id.clone(),
                    next_bead: bead.id.clone(),
                });
            }
        }
        Ok(())
    }
}

fn scale_is_coarser(target: &BeadScale, source: &BeadScale) -> bool {
    match (temporal_rank(target), temporal_rank(source)) {
        (Some(target_rank), Some(source_rank)) => target_rank > source_rank,
        _ => true,
    }
}

fn temporal_rank(scale: &BeadScale) -> Option<u8> {
    match scale {
        BeadScale::Minute => Some(0),
        BeadScale::Hour => Some(1),
        BeadScale::Day => Some(2),
        BeadScale::Week => Some(3),
        BeadScale::Event | BeadScale::Custom(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lifetra_core::Timestamp;

    fn supported_bead(
        id: &str,
        scale: BeadScale,
        start: i64,
        end: i64,
        proof: &str,
    ) -> TrajectoryBead {
        TrajectoryBead::new(
            BeadId::new(id),
            scale,
            Timestamp::new(start),
            Timestamp::new(end),
        )
        .with_evidence(EvidenceRef::new(
            proof,
            EvidenceStatus::Supported,
            "external receipt",
        ))
    }

    #[test]
    fn carries_verified_proof_into_next_bead() {
        let first = supported_bead("b1", BeadScale::Minute, 0, 60, "proof:1");
        let commit = first
            .prove_transition("step-1", "verified")
            .expect("first bead should commit");

        let second = TrajectoryBead::new(
            BeadId::new("b2"),
            BeadScale::Minute,
            Timestamp::new(60),
            Timestamp::new(120),
        );

        let mut chain = BeadChain::new();
        chain.append(first).expect("first bead should append");
        chain
            .append_with_commit(second, &commit)
            .expect("proof should carry");

        assert!(chain.proof_continuity_is_intact());
        assert_eq!(chain.carries().len(), 1);
        assert_eq!(chain.beads()[1].supported_evidence_count(), 1);
    }

    #[test]
    fn rejects_temporal_overlap_on_linear_thread() {
        let mut chain = BeadChain::new();
        chain
            .append(supported_bead("b1", BeadScale::Minute, 0, 60, "p1"))
            .expect("first bead should append");

        let overlap = supported_bead("b2", BeadScale::Minute, 50, 90, "p2");
        assert!(matches!(
            chain.append(overlap),
            Err(ChainBlock::TemporalOverlap { .. })
        ));
    }

    #[test]
    fn records_gaps_without_pretending_they_are_continuous() {
        let mut chain = BeadChain::new();
        chain
            .append(supported_bead("b1", BeadScale::Minute, 0, 60, "p1"))
            .expect("first bead should append");
        chain
            .append(supported_bead("b2", BeadScale::Minute, 90, 150, "p2"))
            .expect("gapped bead should append");

        assert_eq!(chain.temporal_gaps(), vec![30]);
    }

    #[test]
    fn minute_beads_zoom_to_hour_with_provenance() {
        let mut chain = BeadChain::new();
        chain
            .append(supported_bead("m1", BeadScale::Minute, 0, 60, "p1"))
            .expect("m1");
        chain
            .append(supported_bead("m2", BeadScale::Minute, 60, 120, "p2"))
            .expect("m2");
        chain
            .append(supported_bead("m3", BeadScale::Minute, 120, 180, "p3"))
            .expect("m3");

        let aggregate = chain
            .aggregate_window(BeadId::new("hour:1"), BeadScale::Hour, 0, 3)
            .expect("minute beads should aggregate to hour");

        assert_eq!(aggregate.source_beads.len(), 3);
        assert_eq!(aggregate.source_proofs, vec!["p1", "p2", "p3"]);
        assert_eq!(aggregate.bead.supported_evidence_count(), 3);
        assert!(aggregate.bead.unresolved_unknowns.is_empty());
    }

    #[test]
    fn causal_zoom_preserves_unknowns() {
        let first = supported_bead("m1", BeadScale::Minute, 0, 60, "p1")
            .with_unknown("remote settlement status");
        let second = supported_bead("m2", BeadScale::Minute, 60, 120, "p2");

        let mut chain = BeadChain::new();
        chain.append(first).expect("first");
        chain.append(second).expect("second");

        let aggregate = chain
            .aggregate_window(BeadId::new("hour:1"), BeadScale::Hour, 0, 2)
            .expect("aggregate should be created");

        assert_eq!(
            aggregate.bead.unresolved_unknowns,
            vec!["remote settlement status".to_string()]
        );
        assert!(aggregate
            .bead
            .prove_transition("hour-summary", "should remain blocked")
            .is_err());
    }

    #[test]
    fn rejects_same_or_finer_temporal_scale() {
        let mut chain = BeadChain::new();
        chain
            .append(supported_bead("m1", BeadScale::Minute, 0, 60, "p1"))
            .expect("m1");

        assert!(matches!(
            chain.aggregate_window(BeadId::new("minute:copy"), BeadScale::Minute, 0, 1),
            Err(AggregationBlock::TargetScaleNotCoarser { .. })
        ));
    }
}
