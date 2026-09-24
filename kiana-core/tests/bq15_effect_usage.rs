use kiana_core::project_effect_usage;
use kiana_domain::{
    AttemptId, EffectResourceKind, EffectUsageObservation, EffectUsageState, ExecutionId,
    InvocationId, RequestId, RunId, UsageId, UsageLedgerLayer,
};
use serde_json::json;

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn terminal_event(
    run_id: RunId,
    kind: &str,
    layer: UsageLedgerLayer,
    resource: EffectResourceKind,
    state: EffectUsageState,
    started: u64,
    success: u64,
) -> kiana_domain::RuntimeEvent {
    let request_id = RequestId::new();
    let mut event = kiana_domain::RuntimeEvent::new(
        request_id,
        1,
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
        16,
        0,
        0,
        0,
        3,
        state,
        event.event_id,
        event.sequence,
    )
    .expect("usage");
    event.data["invocation_id"] = json!(usage.invocation_id);
    event.data["attempt"] = json!(usage.attempt);
    event.data["effect_usage"] = serde_json::to_value(usage).expect("usage json");
    event
}

#[test]
fn projection_counts_only_started_usage_and_keeps_layers() {
    let run = RunId::new();
    let events = vec![
        terminal_event(
            run,
            "capability.completed",
            UsageLedgerLayer::Tool,
            EffectResourceKind::Shell,
            EffectUsageState::Succeeded,
            1,
            1,
        ),
        terminal_event(
            run,
            "capability.blocked",
            UsageLedgerLayer::Tool,
            EffectResourceKind::Mcp,
            EffectUsageState::Rejected,
            0,
            0,
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
}

#[test]
fn run_drift_is_rejected_before_usage_can_reach_receipt() {
    let run = RunId::new();
    let mut event = terminal_event(
        run,
        "capability.completed",
        UsageLedgerLayer::Effect,
        EffectResourceKind::Artifact,
        EffectUsageState::Succeeded,
        1,
        1,
    );
    event.data["run_id"] = json!(RunId::new());
    assert!(project_effect_usage(run, &[event]).is_err());
}
