use std::collections::HashSet;

use lifetra::{
    ApprovalState, AuthorityContext, AuthorityVerdict, AutonomyLevel, BeadId, BeadScale,
    CorrectionMemory, CorrectionPolicy, DecisionAuthority, EvidenceRef, EvidenceStatus,
    ExecutionMode, OrientationVector, ProvenOrientation, SafetyEnvelope, Timestamp, TrajectoryBead,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const STATION_REQUEST_PROTOCOL: &str = "lifetra.station.request.v0.1";
pub const OBSERVATION_PROTOCOL: &str = "lifetra.observation.v0.1";
pub const DECISION_PROTOCOL: &str = "lifetra.decision.v0.1";

#[derive(Debug, Clone, Deserialize)]
pub struct MetroObservation {
    pub protocol: String,
    pub observation_id: String,
    pub action_id: String,
    pub receipt_id: String,
    pub receipt_status: String,
    pub receipt_hash: String,
    pub hash_algorithm: String,
    pub proof_refs: Vec<String>,
    #[serde(default)]
    pub previous_bead_ref: Option<String>,
    pub observed_at: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct OrientationInput {
    pub growth: f32,
    pub stability: f32,
    pub truth: f32,
    pub connection: f32,
}

impl OrientationInput {
    fn validate(self, name: &str) -> Result<(), String> {
        let values = [self.growth, self.stability, self.truth, self.connection];
        if values
            .into_iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        {
            Ok(())
        } else {
            Err(format!(
                "{name} values must be finite and normalized to 0..=1"
            ))
        }
    }

    fn into_vector(self) -> OrientationVector {
        OrientationVector::new(self.growth, self.stability, self.truth, self.connection)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct CorrectionConfig {
    pub gain: f32,
    pub engage_threshold: f32,
    pub release_threshold: f32,
    pub max_step: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SafetyConfig {
    pub autonomy: String,
    pub min_proof_refs: usize,
    pub quorum_required: usize,
    pub require_human_approval: bool,
    pub max_autonomous_adjustment: f32,
    pub hard_max_adjustment: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthorityContextInput {
    pub human_approval: String,
    pub quorum_approvals: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NextAction {
    pub action_id: String,
    pub goal: String,
    pub kind: String,
    pub inputs: Map<String, Value>,
    pub allowed_targets: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub side_effect: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StationRequest {
    pub protocol: String,
    pub observation: MetroObservation,
    pub observed_epoch_seconds: i64,
    pub decided_at: String,
    pub intended_orientation: OrientationInput,
    #[serde(default)]
    pub observed_orientation: Option<OrientationInput>,
    pub correction: CorrectionConfig,
    pub safety: SafetyConfig,
    pub authority_context: AuthorityContextInput,
    pub next_action: NextAction,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StationDecision {
    pub protocol: String,
    pub decision_id: String,
    pub source_observation_id: String,
    pub source_receipt_ref: String,
    pub caused_by_action_id: String,
    pub source_bead_ref: String,
    pub verdict: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority_proof_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<NextAction>,
    pub decided_at: String,
}

pub fn evaluate_request(request: StationRequest) -> Result<StationDecision, String> {
    validate_request(&request)?;

    let observation = &request.observation;
    let bead_id = BeadId::new(format!("bead:metro:{}", observation.observation_id));
    let source_bead_ref = format!("lifetra-bead://{}", bead_id.as_str());
    let timestamp = Timestamp::new(request.observed_epoch_seconds);

    let mut bead = TrajectoryBead::new(bead_id.clone(), BeadScale::Event, timestamp, timestamp);
    let evidence_status = if observation.receipt_status == "UNKNOWN" {
        EvidenceStatus::Unknown
    } else {
        EvidenceStatus::Supported
    };

    for proof_ref in &observation.proof_refs {
        bead = bead.with_evidence(EvidenceRef::new(
            proof_ref,
            evidence_status.clone(),
            format!("Metro receipt status: {}", observation.receipt_status),
        ));
    }

    if observation.receipt_status == "UNKNOWN" {
        bead = bead.with_unknown("Metro external effect remains UNKNOWN");
        if bead
            .prove_transition("metro-unknown", "effect completion is unresolved")
            .is_ok()
        {
            return Err("UNKNOWN observation unexpectedly produced a trajectory commit".into());
        }
        return Ok(block_decision(&request, &source_bead_ref));
    }

    if observation.receipt_status == "REJECTED" {
        return Ok(block_decision(&request, &source_bead_ref));
    }

    let observed = request
        .observed_orientation
        .ok_or_else(|| "observed_orientation is required for confirmed outcomes".to_string())?;

    let transition_label = match observation.receipt_status.as_str() {
        "SUCCEEDED" => "metro-succeeded",
        "FAILED" => "metro-failed",
        other => return Err(format!("unsupported confirmed receipt status {other:?}")),
    };

    let commit = bead
        .prove_transition(
            transition_label,
            format!("confirmed Metro outcome from {}", observation.receipt_id),
        )
        .map_err(|block| format!("trajectory bead did not commit: {block:?}"))?;

    let proven = ProvenOrientation::from_commit(observed.into_vector(), &commit)
        .map_err(|block| format!("proven orientation blocked: {block:?}"))?;
    let delta =
        lifetra::OrientationDelta::between(request.intended_orientation.into_vector(), proven);

    let correction_policy = CorrectionPolicy::new(
        request.correction.gain,
        request.correction.engage_threshold,
        request.correction.release_threshold,
        request.correction.max_step,
    )
    .map_err(|block| format!("invalid correction policy: {block:?}"))?;
    let correction = correction_policy
        .propose(&delta, CorrectionMemory::default())
        .map_err(|block| format!("correction blocked: {block:?}"))?;

    let envelope = SafetyEnvelope::new(
        parse_autonomy(&request.safety.autonomy)?,
        request.safety.min_proof_refs,
        request.safety.quorum_required,
        request.safety.require_human_approval,
        request.safety.max_autonomous_adjustment,
        request.safety.hard_max_adjustment,
    )
    .map_err(|block| format!("invalid safety envelope: {block:?}"))?;
    let context = AuthorityContext {
        human_approval: parse_approval(&request.authority_context.human_approval)?,
        quorum_approvals: request.authority_context.quorum_approvals,
    };
    let authority = DecisionAuthority::new(envelope).evaluate(&correction, context);

    let decision_id = format!("decision-{}", observation.observation_id);
    let (verdict, execution_mode) = match authority.verdict {
        AuthorityVerdict::Allow(ExecutionMode::Automatic) => ("ALLOW", Some("automatic")),
        AuthorityVerdict::Allow(ExecutionMode::ApprovedManual) => {
            ("ALLOW", Some("approved_manual"))
        }
        AuthorityVerdict::RequireApproval => ("REQUIRE_APPROVAL", None),
        AuthorityVerdict::Block => ("BLOCK", None),
    };

    let (authority_proof_ref, next_action) = if verdict == "ALLOW" {
        let mut next = request.next_action.clone();
        inject_lifetra_context(
            &mut next,
            &source_bead_ref,
            &delta,
            &correction,
            execution_mode.expect("ALLOW always has an execution mode"),
        );
        (
            Some(format!("lifetra-authority://{decision_id}")),
            Some(next),
        )
    } else {
        (None, None)
    };

    Ok(StationDecision {
        protocol: DECISION_PROTOCOL.into(),
        decision_id,
        source_observation_id: observation.observation_id.clone(),
        source_receipt_ref: format!("metro-receipt://{}", observation.receipt_id),
        caused_by_action_id: observation.action_id.clone(),
        source_bead_ref,
        verdict: verdict.into(),
        authority_proof_ref,
        next_action,
        decided_at: request.decided_at,
    })
}

fn validate_request(request: &StationRequest) -> Result<(), String> {
    if request.protocol != STATION_REQUEST_PROTOCOL {
        return Err(format!(
            "unsupported station request protocol {:?}",
            request.protocol
        ));
    }
    let observation = &request.observation;
    if observation.protocol != OBSERVATION_PROTOCOL {
        return Err(format!(
            "unsupported observation protocol {:?}",
            observation.protocol
        ));
    }
    if observation.observation_id.is_empty()
        || observation.action_id.is_empty()
        || observation.receipt_id.is_empty()
    {
        return Err("observation_id, action_id, and receipt_id are required".into());
    }
    if observation.hash_algorithm != "sha256" {
        return Err("only sha256 Metro receipt hashes are supported in v0.1".into());
    }
    if observation.receipt_hash.len() != 64
        || !observation
            .receipt_hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("receipt_hash must be 64 lowercase hexadecimal characters".into());
    }
    match observation.receipt_status.as_str() {
        "SUCCEEDED" | "FAILED" | "UNKNOWN" | "REJECTED" => {}
        other => return Err(format!("unsupported receipt status {other:?}")),
    }
    if observation.proof_refs.is_empty() || !unique_non_empty(&observation.proof_refs) {
        return Err("proof_refs must be non-empty and unique".into());
    }
    if matches!(observation.previous_bead_ref.as_deref(), Some("")) {
        return Err("previous_bead_ref must be non-empty when present".into());
    }
    if observation.observed_at.is_empty() || request.decided_at.is_empty() {
        return Err("observed_at and decided_at are required".into());
    }

    request
        .intended_orientation
        .validate("intended_orientation")?;
    if let Some(observed) = request.observed_orientation {
        observed.validate("observed_orientation")?;
    }

    let next = &request.next_action;
    if next.action_id.is_empty() || next.goal.is_empty() || next.kind.is_empty() {
        return Err("next_action action_id, goal, and kind are required".into());
    }
    if next.action_id == observation.action_id {
        return Err("next_action must not silently reuse the prior Metro action_id".into());
    }
    if next.allowed_targets.is_empty() || !unique_non_empty(&next.allowed_targets) {
        return Err("next_action allowed_targets must be non-empty and unique".into());
    }
    if next.timeout_ms == Some(0) {
        return Err("next_action timeout_ms must be greater than zero when present".into());
    }

    Ok(())
}

fn block_decision(request: &StationRequest, source_bead_ref: &str) -> StationDecision {
    StationDecision {
        protocol: DECISION_PROTOCOL.into(),
        decision_id: format!("decision-{}", request.observation.observation_id),
        source_observation_id: request.observation.observation_id.clone(),
        source_receipt_ref: format!("metro-receipt://{}", request.observation.receipt_id),
        caused_by_action_id: request.observation.action_id.clone(),
        source_bead_ref: source_bead_ref.into(),
        verdict: "BLOCK".into(),
        authority_proof_ref: None,
        next_action: None,
        decided_at: request.decided_at.clone(),
    }
}

fn inject_lifetra_context(
    next: &mut NextAction,
    source_bead_ref: &str,
    delta: &lifetra::OrientationDelta,
    correction: &lifetra::CorrectionDecision,
    execution_mode: &str,
) {
    let (dominant_axis, dominant_gap) = delta.dominant_gap();
    next.inputs.insert(
        "lifetra".into(),
        json!({
            "source_bead_ref": source_bead_ref,
            "alignment_score": delta.alignment_score(),
            "dominant_gap": {
                "axis": dominant_axis.to_string(),
                "value": dominant_gap,
            },
            "correction": {
                "growth": correction.adjustment.growth,
                "stability": correction.adjustment.stability,
                "truth": correction.adjustment.truth,
                "connection": correction.adjustment.connection,
            },
            "next_orientation": {
                "growth": correction.next_orientation.toward_growth,
                "stability": correction.next_orientation.toward_stability,
                "truth": correction.next_orientation.toward_truth,
                "connection": correction.next_orientation.toward_connection,
            },
            "execution_mode": execution_mode,
        }),
    );
}

fn parse_autonomy(value: &str) -> Result<AutonomyLevel, String> {
    match value {
        "observe_only" => Ok(AutonomyLevel::ObserveOnly),
        "recommend_only" => Ok(AutonomyLevel::RecommendOnly),
        "bounded_automatic" => Ok(AutonomyLevel::BoundedAutomatic),
        other => Err(format!("unsupported autonomy level {other:?}")),
    }
}

fn parse_approval(value: &str) -> Result<ApprovalState, String> {
    match value {
        "unknown" => Ok(ApprovalState::Unknown),
        "approved" => Ok(ApprovalState::Approved),
        "denied" => Ok(ApprovalState::Denied),
        other => Err(format!("unsupported human approval state {other:?}")),
    }
}

fn unique_non_empty(values: &[String]) -> bool {
    let mut seen = HashSet::with_capacity(values.len());
    values
        .iter()
        .all(|value| !value.is_empty() && seen.insert(value.as_str()))
}
