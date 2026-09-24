use kiana_domain::{
    AttemptId, EffectResourceKind, EffectUsageObservation, EffectUsageReceipt, EffectUsageState,
    EventId, ExecutionId, InvocationId, RunId, UsageId, UsageLedgerLayer,
    MAX_EFFECT_USAGE_OUTPUT_BYTES,
};

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn observation(
    run_id: RunId,
    layer: UsageLedgerLayer,
    resource: EffectResourceKind,
    state: EffectUsageState,
    started: u64,
    success: u64,
    output_bytes: u64,
) -> EffectUsageObservation {
    EffectUsageObservation::new(
        UsageId::new(),
        run_id,
        InvocationId::new(),
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
        4,
        state,
        EventId::new(),
        1,
    )
    .expect("usage")
}

#[test]
fn denied_and_not_started_observations_cannot_claim_effect() {
    let run = RunId::new();
    let denied = observation(
        run,
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        EffectUsageState::Rejected,
        0,
        0,
        0,
    );
    let receipt = EffectUsageReceipt::from_observations(run, [denied]).expect("receipt");
    let tool = receipt.layer(UsageLedgerLayer::Tool).expect("tool layer");
    assert_eq!(tool.started_invocations, 0);
    assert_eq!(tool.successful_invocations, 0);
    assert_eq!(tool.rejected_invocations, 1);

    assert!(EffectUsageObservation::new(
        UsageId::new(),
        run,
        InvocationId::new(),
        ExecutionId::new(),
        AttemptId::new(),
        1,
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        digest('a'),
        digest('b'),
        digest('c'),
        1,
        0,
        0,
        0,
        0,
        0,
        0,
        EffectUsageState::Rejected,
        EventId::new(),
        1,
    )
    .is_err());
}

#[test]
fn binding_drift_and_output_flood_fail_closed() {
    let run = RunId::new();
    let usage = observation(
        run,
        UsageLedgerLayer::Tool,
        EffectResourceKind::Mcp,
        EffectUsageState::Succeeded,
        1,
        1,
        8,
    );
    assert!(usage
        .validate_binding(
            RunId::new(),
            usage.invocation_id,
            usage.attempt_id,
            usage.attempt,
            &usage.owner_digest,
            &usage.lease_digest,
            &usage.resource_digest,
        )
        .is_err());
    assert!(EffectUsageObservation::new(
        UsageId::new(),
        run,
        InvocationId::new(),
        ExecutionId::new(),
        AttemptId::new(),
        1,
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        digest('a'),
        digest('b'),
        digest('c'),
        1,
        1,
        MAX_EFFECT_USAGE_OUTPUT_BYTES + 1,
        0,
        0,
        0,
        1,
        EffectUsageState::Succeeded,
        EventId::new(),
        1,
    )
    .is_err());
}

#[test]
fn model_tool_and_effect_layers_remain_separate_and_replay_is_idempotent() {
    let run = RunId::new();
    let model = observation(
        run,
        UsageLedgerLayer::Model,
        EffectResourceKind::Model,
        EffectUsageState::Succeeded,
        1,
        1,
        10,
    );
    let tool = observation(
        run,
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        EffectUsageState::Succeeded,
        1,
        1,
        20,
    );
    let effect = observation(
        run,
        UsageLedgerLayer::Effect,
        EffectResourceKind::Artifact,
        EffectUsageState::Succeeded,
        1,
        1,
        0,
    );
    let replay = tool.clone();
    let receipt =
        EffectUsageReceipt::from_observations(run, [model, tool, replay, effect]).expect("receipt");
    assert_eq!(receipt.layers.len(), 3);
    assert_eq!(
        receipt.layer(UsageLedgerLayer::Model).unwrap().output_bytes,
        10
    );
    assert_eq!(
        receipt.layer(UsageLedgerLayer::Tool).unwrap().output_bytes,
        20
    );
    assert_eq!(
        receipt
            .layer(UsageLedgerLayer::Effect)
            .unwrap()
            .artifact_bytes,
        0
    );
}
