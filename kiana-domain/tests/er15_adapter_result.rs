use kiana_domain::{
    attach_adapter_result, AdapterCommitState, AdapterResult, AdapterResultKind,
    CapabilityEffectState, CapabilityProcessState, CapabilityResult, CapabilityStopState,
    RequestId,
};
use serde_json::json;

#[test]
fn adapter_result_is_common_and_digest_only() {
    let request_id = RequestId::new();
    let result = CapabilityResult::success(
        request_id,
        json!({"stdout":"ok","stderr":"","effect_started":true,"effect_known":true}),
    );
    let envelope = AdapterResult::from_result(
        &result,
        AdapterResultKind::Shell,
        AdapterCommitState::Committed,
    )
    .unwrap();
    assert_eq!(envelope.adapter, AdapterResultKind::Shell);
    assert_eq!(envelope.effect, CapabilityEffectState::Succeeded);
    assert_eq!(envelope.process, CapabilityProcessState::Exited);
    assert_eq!(envelope.stop, CapabilityStopState::NotRequested);
    assert!(envelope.output_digest.starts_with("sha256:"));
    assert!(envelope.validate().is_ok());

    let attached = attach_adapter_result(
        result,
        AdapterResultKind::Shell,
        AdapterCommitState::Committed,
    )
    .unwrap();
    let encoded = attached.output["adapter_result"].clone();
    assert_eq!(AdapterResult::from_json(&encoded).unwrap(), envelope);
    assert!(attached.output["adapter_result"].get("stdout").is_none());
}

#[test]
fn adapter_unknown_and_cancelled_boundaries_stay_fenced() {
    let request_id = RequestId::new();
    let unknown = CapabilityResult::failure_with_code(
        request_id,
        kiana_domain::CapabilityErrorCode::ResultUnknown,
        Some("mcp_disconnect_after_send_is_unknown"),
    );
    let unknown_envelope = AdapterResult::from_result(
        &unknown,
        AdapterResultKind::Mcp,
        AdapterCommitState::Unknown,
    )
    .unwrap();
    assert_eq!(unknown_envelope.effect, CapabilityEffectState::Unknown);
    assert!(unknown_envelope.reconciliation_required);

    let not_started = CapabilityResult {
        request_id,
        success: false,
        output: json!({
            "cancelled": true,
            "not_executed": true,
            "stop_confirmed": true,
            "effect_started": false,
            "effect_known": true,
        }),
        evidence_refs: Vec::new(),
    };
    let cancelled_envelope = AdapterResult::from_result(
        &not_started,
        AdapterResultKind::Memory,
        AdapterCommitState::NotStarted,
    )
    .unwrap();
    assert_eq!(
        cancelled_envelope.process,
        CapabilityProcessState::NotStarted
    );
    assert_eq!(cancelled_envelope.effect, CapabilityEffectState::NotStarted);
    assert!(cancelled_envelope.validate().is_ok());

    let mut tampered = unknown_envelope.to_json().unwrap();
    tampered["adapter_digest"] = json!("sha256:bad");
    assert_eq!(
        AdapterResult::from_json(&tampered).unwrap_err(),
        "adapter_result_header_invalid"
    );
}

#[test]
fn hook_observation_rejects_unfenced_unknown_and_unbounded_output() {
    let request_id = RequestId::new();
    let observed = AdapterResult::from_observation(
        request_id,
        AdapterResultKind::Hook,
        AdapterCommitState::Committed,
        CapabilityProcessState::Exited,
        CapabilityStopState::NotRequested,
        CapabilityEffectState::NotStarted,
        &json!({"stdout":"{}","stderr":""}),
        &["hook-output-ref".to_owned()],
        false,
    )
    .unwrap();
    assert_eq!(observed.evidence_ref_digests.len(), 1);

    let error = AdapterResult::from_observation(
        request_id,
        AdapterResultKind::Mcp,
        AdapterCommitState::Unknown,
        CapabilityProcessState::Unknown,
        CapabilityStopState::Unknown,
        CapabilityEffectState::Unknown,
        &json!(null),
        &[],
        false,
    )
    .unwrap_err();
    assert_eq!(error, "adapter_result_unknown_not_fenced");

    let huge = json!({"output": "x".repeat(600 * 1024)});
    assert_eq!(
        AdapterResult::from_observation(
            request_id,
            AdapterResultKind::Patch,
            AdapterCommitState::Committed,
            CapabilityProcessState::Exited,
            CapabilityStopState::NotRequested,
            CapabilityEffectState::Succeeded,
            &huge,
            &[],
            false,
        )
        .unwrap_err(),
        "adapter_result_output_limit"
    );
}
