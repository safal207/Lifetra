use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use lifetra_core::Timestamp;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct CoordinatorId(String);

impl CoordinatorId {
    pub fn new(value: impl Into<String>) -> Result<Self, QuorumFencingError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(QuorumFencingError::EmptyIdentity("coordinator"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct DatabaseNodeId(String);

impl DatabaseNodeId {
    pub fn new(value: impl Into<String>) -> Result<Self, QuorumFencingError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(QuorumFencingError::EmptyIdentity("database node"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct LeadershipGeneration(u64);

impl LeadershipGeneration {
    pub fn new(value: u64) -> Result<Self, QuorumFencingError> {
        if value == 0 {
            return Err(QuorumFencingError::InvalidGeneration(value));
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Self, QuorumFencingError> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(QuorumFencingError::GenerationOverflow)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromotionRequest {
    pub request_id: String,
    pub generation: LeadershipGeneration,
    pub failed_primary: DatabaseNodeId,
    pub candidate: DatabaseNodeId,
}

impl PromotionRequest {
    pub fn new(
        request_id: impl Into<String>,
        generation: LeadershipGeneration,
        failed_primary: DatabaseNodeId,
        candidate: DatabaseNodeId,
    ) -> Result<Self, QuorumFencingError> {
        let request_id = request_id.into();
        if request_id.trim().is_empty() {
            return Err(QuorumFencingError::EmptyIdentity("promotion request"));
        }
        if failed_primary == candidate {
            return Err(QuorumFencingError::SameFailedAndCandidate);
        }
        Ok(Self {
            request_id,
            generation,
            failed_primary,
            candidate,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QuorumVoteDecision {
    Approve,
    Reject,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuorumVote {
    pub coordinator: CoordinatorId,
    pub request_id: String,
    pub generation: LeadershipGeneration,
    pub failed_primary: DatabaseNodeId,
    pub candidate: DatabaseNodeId,
    pub decision: QuorumVoteDecision,
    pub proof_ref: String,
}

impl QuorumVote {
    pub fn for_request(
        coordinator: CoordinatorId,
        request: &PromotionRequest,
        decision: QuorumVoteDecision,
        proof_ref: impl Into<String>,
    ) -> Result<Self, QuorumFencingError> {
        let proof_ref = proof_ref.into();
        if proof_ref.trim().is_empty() {
            return Err(QuorumFencingError::EmptyProofRef);
        }
        Ok(Self {
            coordinator,
            request_id: request.request_id.clone(),
            generation: request.generation,
            failed_primary: request.failed_primary.clone(),
            candidate: request.candidate.clone(),
            decision,
            proof_ref,
        })
    }

    fn matches(&self, request: &PromotionRequest) -> bool {
        self.request_id == request.request_id
            && self.generation == request.generation
            && self.failed_primary == request.failed_primary
            && self.candidate == request.candidate
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FenceGrant {
    pub request: PromotionRequest,
    pub approving_coordinators: Vec<CoordinatorId>,
    pub approval_proof_refs: Vec<String>,
    pub quorum_size: usize,
    pub issued_at: Timestamp,
    pub grant_ref: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FenceOutcome {
    ConfirmedFenced,
    StillReachable,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FenceReceipt {
    pub generation: LeadershipGeneration,
    pub target: DatabaseNodeId,
    pub observed_at: Timestamp,
    pub outcome: FenceOutcome,
    pub proof_ref: String,
}

impl FenceReceipt {
    pub fn new(
        generation: LeadershipGeneration,
        target: DatabaseNodeId,
        observed_at: Timestamp,
        outcome: FenceOutcome,
        proof_ref: impl Into<String>,
    ) -> Result<Self, QuorumFencingError> {
        let proof_ref = proof_ref.into();
        if proof_ref.trim().is_empty() {
            return Err(QuorumFencingError::EmptyProofRef);
        }
        Ok(Self {
            generation,
            target,
            observed_at,
            outcome,
            proof_ref,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromotionPermit {
    pub request: PromotionRequest,
    pub approving_coordinators: Vec<CoordinatorId>,
    pub quorum_size: usize,
    pub fence_proof_ref: String,
    pub issued_at: Timestamp,
    pub permit_ref: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QuorumFencingError {
    EmptyIdentity(&'static str),
    EmptyProofRef,
    InvalidGeneration(u64),
    GenerationOverflow,
    NotEnoughMembers { members: usize },
    DuplicateMember(String),
    SameFailedAndCandidate,
    GenerationNotCurrent {
        current: Option<u64>,
        requested: u64,
    },
    GenerationNotMonotonic {
        current: u64,
        requested: u64,
    },
    UnknownCoordinator(String),
    DuplicateVote(String),
    VoteIdentityMismatch(String),
    NoQuorum { approvals: usize, required: usize },
    CompetingProposal {
        generation: u64,
        locked_request_id: String,
        competing_request_id: String,
    },
    FenceReceiptMismatch,
    FenceNotConfirmed(FenceOutcome),
    PermitNotCurrent {
        current: Option<u64>,
        permit: u64,
    },
    PermitIdentityMismatch,
}

impl fmt::Display for QuorumFencingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for QuorumFencingError {}

#[derive(Clone, Debug)]
pub struct QuorumFencingAuthority {
    members: BTreeSet<CoordinatorId>,
    quorum_size: usize,
    current_generation: Option<LeadershipGeneration>,
    locked_request: Option<PromotionRequest>,
}

impl QuorumFencingAuthority {
    pub fn new(members: Vec<CoordinatorId>) -> Result<Self, QuorumFencingError> {
        if members.len() < 3 {
            return Err(QuorumFencingError::NotEnoughMembers {
                members: members.len(),
            });
        }
        let mut unique = BTreeSet::new();
        for member in members {
            if !unique.insert(member.clone()) {
                return Err(QuorumFencingError::DuplicateMember(
                    member.as_str().to_owned(),
                ));
            }
        }
        let quorum_size = unique.len() / 2 + 1;
        Ok(Self {
            members: unique,
            quorum_size,
            current_generation: None,
            locked_request: None,
        })
    }

    pub fn quorum_size(&self) -> usize {
        self.quorum_size
    }

    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    pub fn current_generation(&self) -> Option<LeadershipGeneration> {
        self.current_generation
    }

    pub fn begin_generation(
        &mut self,
        generation: LeadershipGeneration,
    ) -> Result<(), QuorumFencingError> {
        if let Some(current) = self.current_generation {
            if generation <= current {
                return Err(QuorumFencingError::GenerationNotMonotonic {
                    current: current.get(),
                    requested: generation.get(),
                });
            }
        }
        self.current_generation = Some(generation);
        self.locked_request = None;
        Ok(())
    }

    pub fn issue_fence_grant(
        &mut self,
        request: &PromotionRequest,
        votes: &[QuorumVote],
        issued_at: Timestamp,
        grant_ref: impl Into<String>,
    ) -> Result<FenceGrant, QuorumFencingError> {
        self.require_current(request.generation)?;
        self.lock_or_match_request(request)?;
        let approvals = self.validated_approvals(request, votes)?;
        if approvals.len() < self.quorum_size {
            return Err(QuorumFencingError::NoQuorum {
                approvals: approvals.len(),
                required: self.quorum_size,
            });
        }
        let grant_ref = grant_ref.into();
        if grant_ref.trim().is_empty() {
            return Err(QuorumFencingError::EmptyProofRef);
        }
        let (approving_coordinators, approval_proof_refs): (Vec<_>, Vec<_>) = approvals
            .into_iter()
            .map(|vote| (vote.coordinator.clone(), vote.proof_ref.clone()))
            .unzip();
        Ok(FenceGrant {
            request: request.clone(),
            approving_coordinators,
            approval_proof_refs,
            quorum_size: self.quorum_size,
            issued_at,
            grant_ref,
        })
    }

    pub fn issue_promotion_permit(
        &self,
        grant: &FenceGrant,
        receipt: &FenceReceipt,
        issued_at: Timestamp,
        permit_ref: impl Into<String>,
    ) -> Result<PromotionPermit, QuorumFencingError> {
        self.require_current(grant.request.generation)?;
        if self.locked_request.as_ref() != Some(&grant.request) {
            return Err(QuorumFencingError::PermitIdentityMismatch);
        }
        if grant.approving_coordinators.len() < self.quorum_size
            || grant.quorum_size != self.quorum_size
        {
            return Err(QuorumFencingError::NoQuorum {
                approvals: grant.approving_coordinators.len(),
                required: self.quorum_size,
            });
        }
        if receipt.generation != grant.request.generation
            || receipt.target != grant.request.failed_primary
        {
            return Err(QuorumFencingError::FenceReceiptMismatch);
        }
        if receipt.outcome != FenceOutcome::ConfirmedFenced {
            return Err(QuorumFencingError::FenceNotConfirmed(receipt.outcome));
        }
        let permit_ref = permit_ref.into();
        if permit_ref.trim().is_empty() {
            return Err(QuorumFencingError::EmptyProofRef);
        }
        Ok(PromotionPermit {
            request: grant.request.clone(),
            approving_coordinators: grant.approving_coordinators.clone(),
            quorum_size: self.quorum_size,
            fence_proof_ref: receipt.proof_ref.clone(),
            issued_at,
            permit_ref,
        })
    }

    pub fn validate_promotion_permit(
        &self,
        permit: &PromotionPermit,
    ) -> Result<(), QuorumFencingError> {
        if self.current_generation != Some(permit.request.generation) {
            return Err(QuorumFencingError::PermitNotCurrent {
                current: self.current_generation.map(LeadershipGeneration::get),
                permit: permit.request.generation.get(),
            });
        }
        if self.locked_request.as_ref() != Some(&permit.request)
            || permit.approving_coordinators.len() < self.quorum_size
            || permit.quorum_size != self.quorum_size
        {
            return Err(QuorumFencingError::PermitIdentityMismatch);
        }
        Ok(())
    }

    fn require_current(
        &self,
        generation: LeadershipGeneration,
    ) -> Result<(), QuorumFencingError> {
        if self.current_generation != Some(generation) {
            return Err(QuorumFencingError::GenerationNotCurrent {
                current: self.current_generation.map(LeadershipGeneration::get),
                requested: generation.get(),
            });
        }
        Ok(())
    }

    fn lock_or_match_request(
        &mut self,
        request: &PromotionRequest,
    ) -> Result<(), QuorumFencingError> {
        match &self.locked_request {
            Some(locked) if locked != request => Err(QuorumFencingError::CompetingProposal {
                generation: request.generation.get(),
                locked_request_id: locked.request_id.clone(),
                competing_request_id: request.request_id.clone(),
            }),
            Some(_) => Ok(()),
            None => {
                self.locked_request = Some(request.clone());
                Ok(())
            }
        }
    }

    fn validated_approvals<'a>(
        &self,
        request: &PromotionRequest,
        votes: &'a [QuorumVote],
    ) -> Result<Vec<&'a QuorumVote>, QuorumFencingError> {
        let mut by_coordinator: BTreeMap<&CoordinatorId, &QuorumVote> = BTreeMap::new();
        for vote in votes {
            if !self.members.contains(&vote.coordinator) {
                return Err(QuorumFencingError::UnknownCoordinator(
                    vote.coordinator.as_str().to_owned(),
                ));
            }
            if !vote.matches(request) {
                return Err(QuorumFencingError::VoteIdentityMismatch(
                    vote.coordinator.as_str().to_owned(),
                ));
            }
            if by_coordinator.insert(&vote.coordinator, vote).is_some() {
                return Err(QuorumFencingError::DuplicateVote(
                    vote.coordinator.as_str().to_owned(),
                ));
            }
        }
        Ok(by_coordinator
            .into_values()
            .filter(|vote| vote.decision == QuorumVoteDecision::Approve)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coordinator(value: &str) -> CoordinatorId {
        CoordinatorId::new(value).expect("coordinator")
    }

    fn node(value: &str) -> DatabaseNodeId {
        DatabaseNodeId::new(value).expect("database node")
    }

    fn authority() -> QuorumFencingAuthority {
        QuorumFencingAuthority::new(vec![
            coordinator("q1"),
            coordinator("q2"),
            coordinator("q3"),
        ])
        .expect("three-node authority")
    }

    fn request(generation: LeadershipGeneration) -> PromotionRequest {
        PromotionRequest::new("promote:b", generation, node("db-a"), node("db-b"))
            .expect("request")
    }

    fn vote(
        coordinator_id: &str,
        request: &PromotionRequest,
        decision: QuorumVoteDecision,
    ) -> QuorumVote {
        QuorumVote::for_request(
            coordinator(coordinator_id),
            request,
            decision,
            format!("proof:vote:{coordinator_id}"),
        )
        .expect("vote")
    }

    #[test]
    fn three_member_authority_requires_two_votes() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(1).unwrap();
        authority.begin_generation(generation).unwrap();
        let request = request(generation);
        let err = authority
            .issue_fence_grant(
                &request,
                &[vote("q1", &request, QuorumVoteDecision::Approve)],
                Timestamp::new(1),
                "grant:1",
            )
            .unwrap_err();
        assert_eq!(
            err,
            QuorumFencingError::NoQuorum {
                approvals: 1,
                required: 2
            }
        );
    }

    #[test]
    fn quorum_plus_confirmed_fence_issues_promotion_permit() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(1).unwrap();
        authority.begin_generation(generation).unwrap();
        let request = request(generation);
        let grant = authority
            .issue_fence_grant(
                &request,
                &[
                    vote("q1", &request, QuorumVoteDecision::Approve),
                    vote("q2", &request, QuorumVoteDecision::Approve),
                ],
                Timestamp::new(2),
                "grant:1",
            )
            .unwrap();
        let receipt = FenceReceipt::new(
            generation,
            node("db-a"),
            Timestamp::new(3),
            FenceOutcome::ConfirmedFenced,
            "proof:fence:db-a:off",
        )
        .unwrap();
        let permit = authority
            .issue_promotion_permit(&grant, &receipt, Timestamp::new(4), "permit:1")
            .unwrap();
        authority.validate_promotion_permit(&permit).unwrap();
        assert_eq!(permit.request.candidate.as_str(), "db-b");
        assert_eq!(permit.approving_coordinators.len(), 2);
    }

    #[test]
    fn unknown_fence_never_becomes_promotion_permission() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(1).unwrap();
        authority.begin_generation(generation).unwrap();
        let request = request(generation);
        let grant = authority
            .issue_fence_grant(
                &request,
                &[
                    vote("q1", &request, QuorumVoteDecision::Approve),
                    vote("q2", &request, QuorumVoteDecision::Approve),
                ],
                Timestamp::new(2),
                "grant:1",
            )
            .unwrap();
        let receipt = FenceReceipt::new(
            generation,
            node("db-a"),
            Timestamp::new(3),
            FenceOutcome::Unknown,
            "proof:fence:unknown",
        )
        .unwrap();
        assert_eq!(
            authority
                .issue_promotion_permit(&grant, &receipt, Timestamp::new(4), "permit:1")
                .unwrap_err(),
            QuorumFencingError::FenceNotConfirmed(FenceOutcome::Unknown)
        );
    }

    #[test]
    fn still_reachable_primary_blocks_promotion() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(1).unwrap();
        authority.begin_generation(generation).unwrap();
        let request = request(generation);
        let grant = authority
            .issue_fence_grant(
                &request,
                &[
                    vote("q1", &request, QuorumVoteDecision::Approve),
                    vote("q3", &request, QuorumVoteDecision::Approve),
                ],
                Timestamp::new(2),
                "grant:1",
            )
            .unwrap();
        let receipt = FenceReceipt::new(
            generation,
            node("db-a"),
            Timestamp::new(3),
            FenceOutcome::StillReachable,
            "proof:fence:reachable",
        )
        .unwrap();
        assert!(matches!(
            authority.issue_promotion_permit(&grant, &receipt, Timestamp::new(4), "permit:1"),
            Err(QuorumFencingError::FenceNotConfirmed(
                FenceOutcome::StillReachable
            ))
        ));
    }

    #[test]
    fn competing_candidate_is_rejected_in_same_generation() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(1).unwrap();
        authority.begin_generation(generation).unwrap();
        let first = request(generation);
        authority
            .issue_fence_grant(
                &first,
                &[
                    vote("q1", &first, QuorumVoteDecision::Approve),
                    vote("q2", &first, QuorumVoteDecision::Approve),
                ],
                Timestamp::new(1),
                "grant:first",
            )
            .unwrap();
        let second = PromotionRequest::new(
            "promote:c",
            generation,
            node("db-a"),
            node("db-c"),
        )
        .unwrap();
        assert!(matches!(
            authority.issue_fence_grant(
                &second,
                &[
                    vote("q1", &second, QuorumVoteDecision::Approve),
                    vote("q2", &second, QuorumVoteDecision::Approve),
                ],
                Timestamp::new(2),
                "grant:second",
            ),
            Err(QuorumFencingError::CompetingProposal { .. })
        ));
    }

    #[test]
    fn duplicate_coordinator_vote_fails_closed() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(1).unwrap();
        authority.begin_generation(generation).unwrap();
        let request = request(generation);
        assert_eq!(
            authority
                .issue_fence_grant(
                    &request,
                    &[
                        vote("q1", &request, QuorumVoteDecision::Approve),
                        vote("q1", &request, QuorumVoteDecision::Reject),
                        vote("q2", &request, QuorumVoteDecision::Approve),
                    ],
                    Timestamp::new(1),
                    "grant:1",
                )
                .unwrap_err(),
            QuorumFencingError::DuplicateVote("q1".into())
        );
    }

    #[test]
    fn non_member_vote_is_rejected() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(1).unwrap();
        authority.begin_generation(generation).unwrap();
        let request = request(generation);
        assert_eq!(
            authority
                .issue_fence_grant(
                    &request,
                    &[
                        vote("q1", &request, QuorumVoteDecision::Approve),
                        vote("outsider", &request, QuorumVoteDecision::Approve),
                    ],
                    Timestamp::new(1),
                    "grant:1",
                )
                .unwrap_err(),
            QuorumFencingError::UnknownCoordinator("outsider".into())
        );
    }

    #[test]
    fn advancing_generation_stales_old_permit() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(1).unwrap();
        authority.begin_generation(generation).unwrap();
        let request = request(generation);
        let grant = authority
            .issue_fence_grant(
                &request,
                &[
                    vote("q1", &request, QuorumVoteDecision::Approve),
                    vote("q2", &request, QuorumVoteDecision::Approve),
                ],
                Timestamp::new(1),
                "grant:1",
            )
            .unwrap();
        let receipt = FenceReceipt::new(
            generation,
            node("db-a"),
            Timestamp::new(2),
            FenceOutcome::ConfirmedFenced,
            "proof:fence",
        )
        .unwrap();
        let permit = authority
            .issue_promotion_permit(&grant, &receipt, Timestamp::new(3), "permit:1")
            .unwrap();
        authority.begin_generation(generation.next().unwrap()).unwrap();
        assert_eq!(
            authority.validate_promotion_permit(&permit).unwrap_err(),
            QuorumFencingError::PermitNotCurrent {
                current: Some(2),
                permit: 1
            }
        );
    }

    #[test]
    fn generations_are_strictly_monotonic() {
        let mut authority = authority();
        let generation = LeadershipGeneration::new(2).unwrap();
        authority.begin_generation(generation).unwrap();
        assert_eq!(
            authority.begin_generation(generation).unwrap_err(),
            QuorumFencingError::GenerationNotMonotonic {
                current: 2,
                requested: 2
            }
        );
    }
}
