//! BQ-15 core projection fixtures: terminal facts are the only usage source.
//!
//! Every started fact must carry the server-admitted envelope; terminal kinds must agree with
//! the usage state; the rebuilt receipt counts exactly the started facts with their bytes and
//! Broker-measured wall time.

use kiana_core::project_effect_usage;
use kiana_domain::{
    AttemptId, EffectResourceKind, EffectUsageEnvelope, EffectUsageObservation, EffectUsageState,
    ExecutionId, InvocationId, RequestId, RunId, UsageId, UsageLedgerLayer,
    MAX_EFFECT_USAGE_ARTIFACT_BYTES, MAX_EFFECT_USAGE_INVOCATIONS, MAX_EFFECT_USAGE_LOG_BYTES,
    MAX_EFFECT_USAGE_OUTPUT_BYTES, MAX_EFFECT_USAGE_STORAGE_BYTES, MAX_EFFECT_USAGE_WALL_TIME_MS,
};
use serde_json::json;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn envelope_for(usage: &EffectUsageObservation) -> EffectUsageEnvelope {
    EffectUsageEnvelope::new(
        usage.run_id,
        usage.invocation_id,
        usage.attempt_id,
        usage.attempt,
        usage.lease_digest.clone(),
        usage.resource_digest.clone(),
        MAX_EFFECT_USAGE_INVOCATIONS,
        MAX_EFFECT_USAGE_OUTPUT_BYTES,
        MAX_EFFECT_USAGE_ARTIFACT_BYTES,
        MAX_EFFECT_USAGE_LOG_BYTES,
        MAX_EFFECT_USAGE_STORAGE_BYTES,
        MAX_EFFECT_USAGE_WALL_TIME_MS,
    )
    .expect("envelope")
}

#[allow(clippy::too_many_arguments)]
fn terminal_event(
    run_id: RunId,
    sequence: u64,
    kind: &str,
    layer: UsageLedgerLayer,
    resource: EffectResourceKind,
    state: EffectUsageState,
    started: u64,
    success: u64,
    output_bytes: u64,
    bind_envelope: bool,
) -> kiana_domain::RuntimeEvent {
    let request_id = RequestId::new();
    let mut event = kiana_domain::RuntimeEvent::new(
        request_id,
        sequence,
        kind,
        json!({
            "run_id": run_id,
            "capability_request_id": request_id,
            "effect_started": !state.is_not_started(),
        }),
    )
    .expect("event");
    let usage = EffectUsageObservation::new(
        UsageId::new(),
        run_id,
        InvocationId::from_uuid(request_id.as_uuid()),
        ExecutionId::new(),
        AttemptId::new(),
        1,
        layer,
        resource,
        digest('a'),
        digest('b'),
        digest('c'),
        started,
        success,
        output_bytes,
        0,
        0,
        0,
        if state.is_not_started() { 0 } else { 3 },
        state,
        event.event_id,
        event.sequence,
    )
    .expect("usage");
    let usage = if bind_envelope && !usage.state.is_not_started() {
        // envelope 只依赖 bound 之前的 observation，先在移动 `usage` 前导出。
        let envelope = envelope_for(&usage);
        usage.bind_envelope(envelope).expect("bound usage")
    } else {
        usage
    };
    event.data["invocation_id"] = json!(usage.invocation_id);
    event.data["attempt"] = json!(usage.attempt);
    event.data["effect_usage"] = serde_json::to_value(&usage).expect("usage json");
    event
}

#[test]
fn projection_counts_only_started_usage_and_keeps_layers() {
    let run = RunId::new();
    let events = vec![
        terminal_event(
            run,
            1,
            "capability.completed",
            UsageLedgerLayer::Tool,
            EffectResourceKind::Shell,
            EffectUsageState::Succeeded,
            1,
            1,
            16,
            true,
        ),
        terminal_event(
            run,
            2,
            "capability.failed",
            UsageLedgerLayer::Tool,
            EffectResourceKind::Mcp,
            EffectUsageState::Rejected,
            0,
            0,
            0,
            true,
        ),
    ];
    let receipt = project_effect_usage(run, &events)
        .expect("projection")
        .expect("receipt");
    let tool = receipt.layer(UsageLedgerLayer::Tool).expect("tool");
    assert_eq!(tool.started_invocations, 1);
    assert_eq!(tool.successful_invocations, 1);
    assert_eq!(tool.rejected_invocations, 1);
    assert_eq!(tool.output_bytes, 16);
    assert_eq!(receipt.envelope_digests.len(), 1);
    assert_eq!(receipt.source_cursor, 2);
}

#[test]
fn run_drift_is_rejected_before_usage_can_reach_receipt() {
    let run = RunId::new();
    let mut event = terminal_event(
        run,
        1,
        "capability.completed",
        UsageLedgerLayer::Effect,
        EffectResourceKind::Artifact,
        EffectUsageState::Succeeded,
        1,
        1,
        0,
        true,
    );
    event.data["run_id"] = json!(RunId::new());
    assert!(project_effect_usage(run, &[event]).is_err());
}

#[test]
fn started_usage_without_the_admitted_envelope_is_rejected() {
    let run = RunId::new();
    let event = terminal_event(
        run,
        1,
        "capability.completed",
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        EffectUsageState::Succeeded,
        1,
        1,
        8,
        false,
    );
    let error = project_effect_usage(run, &[event]).expect_err("unmetered started fact");
    assert_eq!(
        error,
        kiana_core::EffectUsageProjectionError::Invalid("effect_usage_envelope_missing".to_owned())
    );
}

#[test]
fn terminal_kind_must_agree_with_the_usage_state() {
    let run = RunId::new();
    let completed_with_failure = terminal_event(
        run,
        1,
        "capability.completed",
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        EffectUsageState::Failed,
        1,
        0,
        8,
        true,
    );
    assert_eq!(
        project_effect_usage(run, &[completed_with_failure]).unwrap_err(),
        kiana_core::EffectUsageProjectionError::Invalid("completed_usage_not_success".to_owned())
    );

    let failed_with_success = terminal_event(
        run,
        1,
        "capability.failed",
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        EffectUsageState::Succeeded,
        1,
        1,
        8,
        true,
    );
    assert_eq!(
        project_effect_usage(run, &[failed_with_success]).unwrap_err(),
        kiana_core::EffectUsageProjectionError::Invalid("failed_usage_success".to_owned())
    );

    // Unknown effects must stay unknown in the terminal fact.
    let unknown_with_success = terminal_event(
        run,
        1,
        "capability.result_unknown",
        UsageLedgerLayer::Tool,
        EffectResourceKind::Mcp,
        EffectUsageState::Succeeded,
        1,
        1,
        8,
        true,
    );
    assert_eq!(
        project_effect_usage(run, &[unknown_with_success]).unwrap_err(),
        kiana_core::EffectUsageProjectionError::Invalid(
            "unknown_terminal_usage_required".to_owned()
        )
    );
}

#[test]
fn receipt_alignment_follows_started_facts_bytes_and_wall_time() {
    let run = RunId::new();
    let events = vec![
        terminal_event(
            run,
            1,
            "capability.completed",
            UsageLedgerLayer::Model,
            EffectResourceKind::Model,
            EffectUsageState::Succeeded,
            1,
            1,
            10,
            true,
        ),
        terminal_event(
            run,
            2,
            "capability.completed",
            UsageLedgerLayer::Tool,
            EffectResourceKind::Shell,
            EffectUsageState::Succeeded,
            1,
            1,
            20,
            true,
        ),
        terminal_event(
            run,
            3,
            "capability.failed",
            UsageLedgerLayer::Effect,
            EffectResourceKind::Log,
            EffectUsageState::Failed,
            1,
            0,
            30,
            true,
        ),
    ];
    let receipt = project_effect_usage(run, &events)
        .expect("projection")
        .expect("receipt");
    assert_eq!(receipt.layers.len(), 3);
    let started: u64 = receipt
        .layers
        .iter()
        .map(|layer| layer.started_invocations)
        .sum();
    let successful: u64 = receipt
        .layers
        .iter()
        .map(|layer| layer.successful_invocations)
        .sum();
    let output_bytes: u64 = receipt.layers.iter().map(|layer| layer.output_bytes).sum();
    let wall_time_ms: u64 = receipt.layers.iter().map(|layer| layer.wall_time_ms).sum();
    // Three committed started facts, three successful, 60 output bytes, 3 x 3 ms measured.
    assert_eq!(started, 3);
    assert_eq!(successful, 2);
    assert_eq!(output_bytes, 60);
    assert_eq!(wall_time_ms, 9);
    assert_eq!(receipt.source_cursor, 3);
    assert_eq!(receipt.source_event_ids.len(), 3);
    assert_eq!(receipt.envelope_digests.len(), 3);
    receipt.validate().expect("receipt stays valid");
}
