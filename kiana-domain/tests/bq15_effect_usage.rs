//! BQ-15 domain fixtures: tool/effect/resource usage and its admitted envelope.
//!
//! Denials come first: a rejected or not-started request can never claim effect, a started
//! observation can never exist without the server-admitted envelope, and measurements above that
//! envelope (output/artifact/log/storage bytes, wall time, started count) are refused per
//! dimension instead of being counted.

use kiana_domain::{
    AttemptId, EffectResourceKind, EffectUsageEnvelope, EffectUsageObservation, EffectUsageReceipt,
    EffectUsageState, EventId, ExecutionId, InvocationId, RunId, UsageId, UsageLedgerLayer,
    MAX_EFFECT_USAGE_ARTIFACT_BYTES, MAX_EFFECT_USAGE_INVOCATIONS, MAX_EFFECT_USAGE_LOG_BYTES,
    MAX_EFFECT_USAGE_OUTPUT_BYTES, MAX_EFFECT_USAGE_STORAGE_BYTES, MAX_EFFECT_USAGE_WALL_TIME_MS,
};

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

/// The widest envelope the hard limits allow: it can never widen the absolute bounds.
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

/// Bind the observation to an envelope that was narrowed in exactly one dimension.
fn narrow(
    usage: &EffectUsageObservation,
    apply: impl FnOnce(&mut EffectUsageEnvelope),
) -> Result<EffectUsageObservation, String> {
    let mut envelope = envelope_for(usage);
    apply(&mut envelope);
    envelope.envelope_digest = envelope.digest();
    envelope.validate()?;
    usage.clone().bind_envelope(envelope)
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
    let usage = EffectUsageObservation::new(
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
        if state.is_not_started() { 0 } else { 4 },
        state,
        EventId::new(),
        1,
    )
    .expect("usage");
    if usage.state.is_not_started() {
        usage
    } else {
        // The envelope borrows the observation, so it must be built before the move.
        let envelope = envelope_for(&usage);
        usage.bind_envelope(envelope).expect("bound usage")
    }
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
    // A rejected request has zero effect and must not claim an admitted envelope either.
    assert_eq!(
        denied
            .clone()
            .bind_envelope(envelope_for(&denied))
            .unwrap_err(),
        "effect_usage_envelope_not_applicable"
    );
    let receipt = EffectUsageReceipt::from_observations(run, [denied]).expect("receipt");
    let tool = receipt.layer(UsageLedgerLayer::Tool).expect("tool layer");
    assert_eq!(tool.started_invocations, 0);
    assert_eq!(tool.successful_invocations, 0);
    assert_eq!(tool.rejected_invocations, 1);
    assert!(receipt.envelope_digests.is_empty());

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
fn admitted_envelope_can_only_narrow_hard_limits() {
    let run = RunId::new();
    let usage = observation(
        run,
        UsageLedgerLayer::Effect,
        EffectResourceKind::Storage,
        EffectUsageState::Succeeded,
        1,
        1,
        0,
    );

    // An envelope wider than the absolute hard limit is not an admission.
    assert_eq!(
        EffectUsageEnvelope::new(
            usage.run_id,
            usage.invocation_id,
            usage.attempt_id,
            usage.attempt,
            usage.lease_digest.clone(),
            usage.resource_digest.clone(),
            MAX_EFFECT_USAGE_INVOCATIONS,
            MAX_EFFECT_USAGE_OUTPUT_BYTES + 1,
            MAX_EFFECT_USAGE_ARTIFACT_BYTES,
            MAX_EFFECT_USAGE_LOG_BYTES,
            MAX_EFFECT_USAGE_STORAGE_BYTES,
            MAX_EFFECT_USAGE_WALL_TIME_MS,
        )
        .unwrap_err(),
        "effect_usage_envelope_invalid"
    );
    // Zero started invocations can never admit a started effect.
    assert_eq!(
        EffectUsageEnvelope::new(
            usage.run_id,
            usage.invocation_id,
            usage.attempt_id,
            usage.attempt,
            usage.lease_digest.clone(),
            usage.resource_digest.clone(),
            0,
            MAX_EFFECT_USAGE_OUTPUT_BYTES,
            MAX_EFFECT_USAGE_ARTIFACT_BYTES,
            MAX_EFFECT_USAGE_LOG_BYTES,
            MAX_EFFECT_USAGE_STORAGE_BYTES,
            MAX_EFFECT_USAGE_WALL_TIME_MS,
        )
        .unwrap_err(),
        "effect_usage_envelope_invalid"
    );
    // A tampered envelope digest is refused before it can bound anything.
    let mut tampered = envelope_for(&usage);
    tampered.max_storage_bytes = MAX_EFFECT_USAGE_STORAGE_BYTES - 1;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "effect_usage_envelope_digest_mismatch"
    );
}

#[test]
fn measurements_above_the_admitted_envelope_are_refused_per_dimension() {
    let run = RunId::new();
    let usage = EffectUsageObservation::new(
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
        4096,
        2048,
        1024,
        512,
        4,
        EffectUsageState::Succeeded,
        EventId::new(),
        1,
    )
    .expect("usage shape");

    // The same measurements fit the absolute hard limit but are refused by the admitted quota.
    assert_eq!(
        narrow(&usage, |envelope| envelope.max_output_bytes = 1024).unwrap_err(),
        "effect_usage_envelope_exceeded:output_bytes"
    );
    assert_eq!(
        narrow(&usage, |envelope| envelope.max_artifact_bytes = 1024).unwrap_err(),
        "effect_usage_envelope_exceeded:artifact_bytes"
    );
    assert_eq!(
        narrow(&usage, |envelope| envelope.max_log_bytes = 512).unwrap_err(),
        "effect_usage_envelope_exceeded:log_bytes"
    );
    assert_eq!(
        narrow(&usage, |envelope| envelope.max_storage_bytes = 256).unwrap_err(),
        "effect_usage_envelope_exceeded:storage_bytes"
    );
    assert_eq!(
        narrow(&usage, |envelope| envelope.max_wall_time_ms = 3).unwrap_err(),
        "effect_usage_envelope_exceeded:wall_time_ms"
    );
    let bound = narrow(&usage, |envelope| envelope.max_output_bytes = 4096)
        .expect("admitted measurements bind");
    assert_eq!(bound.output_bytes, 4096);
}

#[test]
fn started_usage_requires_the_server_admitted_envelope() {
    let run = RunId::new();
    let usage = EffectUsageObservation::new(
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
        8,
        0,
        0,
        0,
        4,
        EffectUsageState::Succeeded,
        EventId::new(),
        1,
    )
    .expect("usage shape");
    // A started fact without the admitted envelope cannot be validated or projected.
    assert_eq!(
        usage.validate().unwrap_err(),
        "effect_usage_envelope_missing"
    );
    assert_eq!(
        usage
            .validate_against_envelope(&envelope_for(&usage))
            .unwrap_err(),
        "effect_usage_envelope_missing"
    );

    let bound = usage
        .clone()
        .bind_envelope(envelope_for(&usage))
        .expect("bound");
    // A widened or swapped envelope no longer matches the committed binding.
    let mut widened = envelope_for(&usage);
    widened.max_output_bytes = MAX_EFFECT_USAGE_OUTPUT_BYTES - 1;
    widened.envelope_digest = widened.digest();
    widened.validate().expect("widened envelope");
    assert_eq!(
        bound.validate_against_envelope(&widened).unwrap_err(),
        "effect_usage_envelope_digest_mismatch"
    );

    // Lease/resource drift is a binding mismatch, not a quiet re-admission.
    let mut drifted = envelope_for(&usage);
    drifted.lease_digest = digest('d');
    drifted.envelope_digest = drifted.digest();
    drifted.validate().expect("drifted envelope");
    assert_eq!(
        bound.validate_against_envelope(&drifted).unwrap_err(),
        "effect_usage_envelope_binding_mismatch"
    );
}

#[test]
fn per_fact_started_count_is_bounded_and_duplicate_attempts_are_rejected() {
    let run = RunId::new();
    // One observation is one invocation attempt: a batch count of 2 is an unbounded claim.
    assert_eq!(
        EffectUsageObservation::new(
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
            2,
            1,
            8,
            0,
            0,
            0,
            4,
            EffectUsageState::Succeeded,
            EventId::new(),
            1,
        )
        .unwrap_err(),
        "effect_usage_started_count_unbounded"
    );

    // Two facts for the same attempt/layer/resource would double count the same started work.
    let first = observation(
        run,
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        EffectUsageState::Succeeded,
        1,
        1,
        8,
    );
    let second = EffectUsageObservation::new(
        UsageId::new(),
        run,
        first.invocation_id,
        first.execution_id,
        first.attempt_id,
        first.attempt,
        first.layer,
        first.resource,
        first.owner_digest.clone(),
        first.lease_digest.clone(),
        first.resource_digest.clone(),
        1,
        1,
        8,
        0,
        0,
        0,
        4,
        EffectUsageState::Succeeded,
        EventId::new(),
        2,
    )
    .expect("second usage shape");
    let second_envelope = envelope_for(&second);
    let second = second.bind_envelope(second_envelope).expect("bound");
    assert_eq!(
        EffectUsageReceipt::from_observations(run, [first, second]).unwrap_err(),
        "effect_usage_duplicate_attempt_usage"
    );
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
    // The replay kept exactly one envelope for the started layer, and the started count stays
    // aligned with the committed facts (one per attempt).
    assert_eq!(
        receipt
            .layer(UsageLedgerLayer::Tool)
            .unwrap()
            .started_invocations,
        1
    );
    assert_eq!(receipt.envelope_digests.len(), 3);
    receipt.validate().expect("receipt stays valid");
}

#[test]
fn receipt_with_started_layers_must_keep_its_envelope_digests() {
    let run = RunId::new();
    let usage = observation(
        run,
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        EffectUsageState::Succeeded,
        1,
        1,
        8,
    );
    let receipt = EffectUsageReceipt::from_observations(run, [usage]).expect("receipt");
    let mut value = serde_json::to_value(&receipt).expect("receipt json");
    value
        .as_object_mut()
        .expect("receipt object")
        .remove("envelope_digests");
    let stripped: EffectUsageReceipt =
        serde_json::from_value(value).expect("receipt decodes without the optional field");
    assert_eq!(
        stripped.validate().unwrap_err(),
        "effect_usage_receipt_envelope_bounds_invalid"
    );
}
