#[path = "../examples/metro_station/support.rs"]
mod support;

use serde_json::Map;
use support::{
    evaluate_request, AuthorityContextInput, CorrectionConfig, MetroObservation, NextAction,
    OrientationInput, SafetyConfig, StationRequest, OBSERVATION_PROTOCOL, STATION_REQUEST_PROTOCOL,
};

fn request(status: &str, observed_growth: Option<f32>) -> StationRequest {
    StationRequest {
        protocol: STATION_REQUEST_PROTOCOL.into(),
        observation: MetroObservation {
            protocol: OBSERVATION_PROTOCOL.into(),
            observation_id: "observation-action-001".into(),
            action_id: "action-001".into(),
            receipt_id: "receipt-action-001".into(),
            receipt_status: status.into(),
            receipt_hash: "a".repeat(64),
            hash_algorithm: "sha256".into(),
            proof_refs: vec!["metro-receipt://receipt-action-001".into()],
            previous_bead_ref: None,
            observed_at: "2026-09-16T13:30:00Z".into(),
        },
        observed_epoch_seconds: 1_789_565_400,
        decided_at: "2026-09-16T13:30:01Z".into(),
        intended_orientation: OrientationInput {
            growth: 0.8,
            stability: 0.5,
            truth: 0.8,
            connection: 0.5,
        },
        observed_orientation: observed_growth.map(|growth| OrientationInput {
            growth,
            stability: 0.5,
            truth: 0.8,
            connection: 0.5,
        }),
        correction: CorrectionConfig {
            gain: 0.5,
            engage_threshold: 0.1,
            release_threshold: 0.05,
            max_step: 0.1,
        },
        safety: SafetyConfig {
            autonomy: "bounded_automatic".into(),
            min_proof_refs: 1,
            quorum_required: 0,
            require_human_approval: false,
            max_autonomous_adjustment: 0.1,
            hard_max_adjustment: 0.25,
        },
        authority_context: AuthorityContextInput {
            human_approval: "unknown".into(),
            quorum_approvals: 0,
        },
        next_action: NextAction {
            action_id: "action-002".into(),
            goal: "Verify the corrected implementation".into(),
            kind: "qa.verify".into(),
            inputs: Map::new(),
            allowed_targets: vec!["qa-agent".into()],
            timeout_ms: Some(5_000),
            side_effect: false,
        },
    }
}

#[test]
fn confirmed_small_delta_can_emit_an_allowed_next_action() {
    let decision = evaluate_request(request("SUCCEEDED", Some(0.6)))
        .expect("proof-backed bounded correction should evaluate");

    assert_eq!(decision.verdict, "ALLOW");
    assert!(decision.authority_proof_ref.is_some());
    let next = decision
        .next_action
        .expect("allowed decision needs next action");
    assert_eq!(next.action_id, "action-002");
    assert!(next.inputs.contains_key("lifetra"));
}

#[test]
fn unknown_effect_blocks_without_collapsing_to_failure_or_success() {
    let decision =
        evaluate_request(request("UNKNOWN", None)).expect("UNKNOWN is a valid observation state");

    assert_eq!(decision.verdict, "BLOCK");
    assert!(decision.authority_proof_ref.is_none());
    assert!(decision.next_action.is_none());
}

#[test]
fn correction_outside_automatic_envelope_requires_approval() {
    let mut request = request("SUCCEEDED", Some(0.2));
    request.intended_orientation.growth = 0.9;
    request.correction.gain = 1.0;
    request.correction.max_step = 0.2;

    let decision = evaluate_request(request).expect("soft envelope should evaluate");

    assert_eq!(decision.verdict, "REQUIRE_APPROVAL");
    assert!(decision.next_action.is_none());
}

#[test]
fn new_control_action_cannot_reuse_the_prior_action_identity() {
    let mut request = request("SUCCEEDED", Some(0.6));
    request.next_action.action_id = request.observation.action_id.clone();

    let error = evaluate_request(request).expect_err("action identity reuse must fail closed");

    assert!(error.contains("must not silently reuse"));
}
