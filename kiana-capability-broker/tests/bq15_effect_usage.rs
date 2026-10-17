//! BQ-15 Broker fixtures: usage evidence is sealed and re-validated after the handler ran.
//!
//! Denials come first: an adapter cannot return usage without the server-admitted envelope, it
//! cannot widen that envelope, it cannot report measurements above it, and an over-time effect
//! fails closed after the Broker replaces the claimed wall time with its own measurement.

use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AttemptId, AuthenticatedPrincipalRef, AuthorizedCapabilityRequest, CapabilityKind,
    CapabilityRequest, CapabilityResult, EffectResourceKind, EffectUsageEnvelope,
    EffectUsageObservation, EffectUsageState, ExecutionId, ExecutionScope, InvocationId,
    ProjectIdentity, RequestId, RiskLevel, RunId, ScopeDimension, ScopeLimit, ScopeSet, SessionId,
    UsageId, UsageLedgerLayer, MAX_EFFECT_USAGE_ARTIFACT_BYTES, MAX_EFFECT_USAGE_INVOCATIONS,
    MAX_EFFECT_USAGE_LOG_BYTES, MAX_EFFECT_USAGE_OUTPUT_BYTES, MAX_EFFECT_USAGE_STORAGE_BYTES,
    MAX_EFFECT_USAGE_WALL_TIME_MS,
};
use kiana_ports::{CapabilityBrokerPort, ExecutionPermitVerifierPort, PortError};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UsageCase {
    /// Well-formed usage: the adapter claims 1 ms, the Broker measures the real elapsed time.
    Bound,
    /// The adapter binds usage to its own wider envelope instead of the admitted one.
    InflatedEnvelope,
    /// The adapter forges a self-consistent fact whose bytes exceed the admitted envelope.
    FloodedOutput,
    /// The adapter returns success without any usage evidence.
    NoUsage,
    /// The request admitted no envelope, yet the adapter reports usage.
    NoAdmittedEnvelope,
    /// The adapter marks the effect as not executed but reports a started usage.
    NotExecutedStartedUsage,
    /// The usage lease digest does not match the authorized request.
    LeaseDrift,
    /// The measured wall time exceeds the admitted wall-time bound.
    SlowBeyondWallEnvelope,
}

struct UsageHandler {
    case: UsageCase,
    binding_version: &'static str,
}

struct TestPermitVerifier;

#[async_trait]
impl ExecutionPermitVerifierPort for TestPermitVerifier {
    async fn verify_and_consume(
        &self,
        _request: &AuthorizedCapabilityRequest,
    ) -> Result<(), PortError> {
        Ok(())
    }
}

struct NoopHandler {
    binding_version: &'static str,
}

#[async_trait]
impl CapabilityHandler for NoopHandler {
    fn binding_version(&self) -> &'static str {
        self.binding_version
    }

    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        Ok(CapabilityResult::success(
            request.request.request_id,
            Value::Null,
        ))
    }
}

struct UsageInputs {
    run_id: RunId,
    invocation_id: InvocationId,
    owner_digest: String,
    lease_digest: String,
    resource_digest: String,
    attempt_id: AttemptId,
    attempt: u32,
    admitted: Option<EffectUsageEnvelope>,
}

fn digest(ch: char) -> String {
    format!("sha256:{}", ch.to_string().repeat(64))
}

fn usage_inputs(request: &AuthorizedCapabilityRequest) -> UsageInputs {
    let scope = request
        .request
        .execution_scope
        .as_ref()
        .expect("execution scope");
    let run_id = scope.run_id.expect("run id");
    let arguments = request
        .request
        .arguments
        .as_object()
        .expect("arguments object");
    let lease_digest = arguments
        .get("lease_digest")
        .and_then(Value::as_str)
        .expect("lease digest")
        .to_owned();
    let resource_digest = arguments
        .get("resource_digest")
        .and_then(Value::as_str)
        .expect("resource digest")
        .to_owned();
    let attempt = arguments
        .get("attempt")
        .and_then(Value::as_u64)
        .expect("attempt") as u32;
    let admitted = arguments
        .get("effect_usage_envelope")
        .map(|value| EffectUsageEnvelope::from_json(value).expect("admitted envelope"));
    let attempt_id = admitted
        .as_ref()
        .map(|envelope| envelope.attempt_id)
        .unwrap_or_else(AttemptId::new);
    UsageInputs {
        run_id,
        invocation_id: InvocationId::from_uuid(request.request.request_id.as_uuid()),
        owner_digest: scope.scope_digest.clone(),
        lease_digest,
        resource_digest,
        attempt_id,
        attempt,
        admitted,
    }
}

/// An envelope the adapter minted for itself; it is never the admitted one unless it matches.
fn handler_envelope(
    inputs: &UsageInputs,
    lease_digest: &str,
    max_output_bytes: u64,
    max_wall_time_ms: u64,
) -> EffectUsageEnvelope {
    EffectUsageEnvelope::new(
        inputs.run_id,
        inputs.invocation_id,
        inputs.attempt_id,
        inputs.attempt,
        lease_digest,
        inputs.resource_digest.clone(),
        MAX_EFFECT_USAGE_INVOCATIONS,
        max_output_bytes,
        MAX_EFFECT_USAGE_ARTIFACT_BYTES,
        MAX_EFFECT_USAGE_LOG_BYTES,
        MAX_EFFECT_USAGE_STORAGE_BYTES,
        max_wall_time_ms,
    )
    .expect("handler envelope")
}

fn handler_usage(
    inputs: &UsageInputs,
    envelope: &EffectUsageEnvelope,
    output_bytes: u64,
    claimed_wall_time_ms: u64,
) -> EffectUsageObservation {
    let usage = EffectUsageObservation::new(
        UsageId::new(),
        inputs.run_id,
        inputs.invocation_id,
        ExecutionId::new(),
        envelope.attempt_id,
        envelope.attempt,
        UsageLedgerLayer::Tool,
        EffectResourceKind::Shell,
        inputs.owner_digest.clone(),
        envelope.lease_digest.clone(),
        envelope.resource_digest.clone(),
        1,
        1,
        output_bytes,
        0,
        0,
        0,
        claimed_wall_time_ms,
        EffectUsageState::Succeeded,
        kiana_domain::EventId::new(),
        1,
    )
    .expect("usage shape");
    usage.bind_envelope(envelope.clone()).expect("bound usage")
}

/// Forge a self-consistent fact whose measurements exceed the envelope it carries.  This is what
/// an untrusted adapter could do outside the domain constructors; the Broker must still refuse it.
fn forge_output_bytes(usage: EffectUsageObservation, output_bytes: u64) -> EffectUsageObservation {
    let value = serde_json::to_value(&usage).expect("usage json");
    let mut forged: EffectUsageObservation =
        serde_json::from_value(value).expect("decode without validation");
    forged.output_bytes = output_bytes;
    forged.usage_digest = forged.digest();
    forged
}

#[async_trait]
impl CapabilityHandler for UsageHandler {
    fn binding_version(&self) -> &'static str {
        self.binding_version
    }

    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        let request_id = request.request.request_id;
        if self.case == UsageCase::NoUsage {
            return Ok(CapabilityResult::success(
                request_id,
                json!({"operation": "shell.exec"}),
            ));
        }
        let inputs = usage_inputs(&request);
        let lease_digest = if self.case == UsageCase::LeaseDrift {
            digest('d')
        } else {
            inputs.lease_digest.clone()
        };
        let envelope = match self.case {
            UsageCase::InflatedEnvelope | UsageCase::NoAdmittedEnvelope => handler_envelope(
                &inputs,
                &lease_digest,
                MAX_EFFECT_USAGE_OUTPUT_BYTES,
                MAX_EFFECT_USAGE_WALL_TIME_MS,
            ),
            _ => inputs.admitted.clone().unwrap_or_else(|| {
                handler_envelope(
                    &inputs,
                    &lease_digest,
                    MAX_EFFECT_USAGE_OUTPUT_BYTES,
                    MAX_EFFECT_USAGE_WALL_TIME_MS,
                )
            }),
        };
        let claimed_wall_time_ms = if self.case == UsageCase::Bound { 1 } else { 0 };
        let mut usage = handler_usage(&inputs, &envelope, 8, claimed_wall_time_ms);
        if self.case == UsageCase::FloodedOutput {
            usage = forge_output_bytes(usage, 4_096);
        }
        if matches!(
            self.case,
            UsageCase::Bound | UsageCase::SlowBeyondWallEnvelope
        ) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        let mut output = json!({
            "operation": "shell.exec",
            "effect_usage": serde_json::to_value(&usage).expect("usage json"),
        });
        if self.case == UsageCase::NotExecutedStartedUsage {
            output["not_executed"] = json!(true);
        }
        Ok(CapabilityResult::success(request_id, output))
    }
}

fn sealed_broker_for(
    handler: Arc<dyn CapabilityHandler>,
    target_operation: &str,
) -> CapabilityBroker {
    let mut broker = CapabilityBroker::new();
    for operation in kiana_domain::ACTION_OPERATIONS {
        let descriptor = kiana_domain::capability_action_descriptor(operation)
            .expect("catalog operation has a descriptor");
        let handler: Arc<dyn CapabilityHandler> = if *operation == target_operation {
            handler.clone()
        } else {
            Arc::new(NoopHandler {
                binding_version: descriptor.binding_version,
            })
        };
        broker
            .register_static(descriptor.capability, *operation, handler)
            .unwrap();
    }
    broker.validate_catalog_bindings().unwrap();
    broker.set_permit_verifier(Arc::new(TestPermitVerifier));
    broker
}

fn action_scope(request: &CapabilityRequest, run_id: RunId) -> ExecutionScope {
    let permission_scope = ScopeSet::new(
        ScopeDimension::Restricted(vec![request.operation.clone()]),
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeDimension::NotApplicable,
        ScopeLimit::NotApplicable,
        ScopeLimit::NotApplicable,
    )
    .unwrap();
    let principal = AuthenticatedPrincipalRef::local();
    let project = ProjectIdentity::new(
        "/repo",
        "/repo",
        None,
        None,
        kiana_domain::json_digest(&json!("trusted")),
    )
    .unwrap();
    let mut scope = ExecutionScope {
        schema: kiana_domain::EXECUTION_SCOPE_SCHEMA.to_owned(),
        version: kiana_domain::EXECUTION_SCOPE_SCHEMA_VERSION,
        principal,
        project,
        session_id: SessionId::new("bq15-effect-usage"),
        run_id: Some(run_id),
        turn_id: None,
        cell_id: None,
        grant_refs: Vec::new(),
        budget_lease_id: None,
        work_packet_id: None,
        environment_id: "kiana-local".to_owned(),
        workspace_revision: None,
        permission_scope: permission_scope.clone(),
        read_roots: vec!["/repo".to_owned()],
        write_roots: Vec::new(),
        read_denies: Vec::new(),
        write_denies: Vec::new(),
        memory_scopes: Vec::new(),
        server_scopes: Vec::new(),
        network_policy: Vec::new(),
        authority_epoch: 1,
        trust_revision: kiana_domain::json_digest(&json!("trusted")),
        data_epoch: 1,
        cancellation_epoch: 1,
        deadline_unix_ms: u64::MAX,
        fencing_token: 1,
        catalog_digest: kiana_domain::capability_action_catalog_digest(),
        action_digest: kiana_domain::capability_action_digest(request),
        permission_scope_digest: permission_scope.digest(),
        scope_digest: String::new(),
    };
    scope.scope_digest = scope.digest();
    scope
}

/// Build a normalized `shell.exec` request carrying the server-owned binding digests and, when
/// requested, the admitted effect envelope.
fn shell_request(
    run_id: RunId,
    attempt_id: AttemptId,
    admitted: Option<(u64, u64)>,
) -> CapabilityRequest {
    let mut request = CapabilityRequest::new(
        RequestId::new(),
        CapabilityKind::Process,
        "shell.exec",
        json!({ "command": "true" }),
    )
    .with_risk(RiskLevel::ReadOnly);
    let request_id = request.request_id;
    let lease_digest = digest('b');
    let resource_digest = digest('c');
    let arguments = request.arguments.as_object_mut().expect("arguments object");
    arguments.insert("lease_digest".to_owned(), json!(lease_digest.clone()));
    arguments.insert("resource_digest".to_owned(), json!(resource_digest.clone()));
    arguments.insert("attempt".to_owned(), json!(1));
    if let Some((max_output_bytes, max_wall_time_ms)) = admitted {
        let envelope = EffectUsageEnvelope::new(
            run_id,
            InvocationId::from_uuid(request_id.as_uuid()),
            attempt_id,
            1,
            lease_digest,
            resource_digest,
            MAX_EFFECT_USAGE_INVOCATIONS,
            max_output_bytes,
            MAX_EFFECT_USAGE_ARTIFACT_BYTES,
            MAX_EFFECT_USAGE_LOG_BYTES,
            MAX_EFFECT_USAGE_STORAGE_BYTES,
            max_wall_time_ms,
        )
        .expect("admitted envelope");
        arguments.insert(
            "effect_usage_envelope".to_owned(),
            envelope.to_json().expect("envelope json"),
        );
    }
    kiana_domain::normalize_capability_action(&mut request).expect("normalize");
    request.execution_scope = Some(action_scope(&request, run_id));
    request
}

fn usage_broker(case: UsageCase) -> CapabilityBroker {
    let descriptor = kiana_domain::capability_action_descriptor("shell.exec").unwrap();
    sealed_broker_for(
        Arc::new(UsageHandler {
            case,
            binding_version: descriptor.binding_version,
        }),
        "shell.exec",
    )
}

async fn execute_case(
    case: UsageCase,
    admitted: Option<(u64, u64)>,
) -> Result<(CapabilityResult, String), PortError> {
    let run_id = RunId::new();
    let request = shell_request(run_id, AttemptId::new(), admitted);
    let admitted_digest = request
        .arguments
        .get("effect_usage_envelope")
        .and_then(|value| value.get("envelope_digest"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let broker = usage_broker(case);
    let authorized = AuthorizedCapabilityRequest::new("policy:bq15", request).unwrap();
    broker
        .execute(authorized)
        .await
        .map(|result| (result, admitted_digest))
}

#[tokio::test]
async fn broker_seals_measured_wall_time_and_keeps_the_admitted_envelope() {
    let (result, admitted_digest) = execute_case(UsageCase::Bound, Some((1_024, 60_000)))
        .await
        .expect("bounded usage is accepted");
    let usage = EffectUsageObservation::from_json(&result.output["effect_usage"])
        .expect("sealed usage stays self-consistent");
    // The adapter claimed 1 ms; the Broker's own measurement replaced it.
    assert_ne!(usage.wall_time_ms, 1);
    assert!(
        usage.wall_time_ms >= 5,
        "measured wall time: {}",
        usage.wall_time_ms
    );
    assert_eq!(usage.output_bytes, 8);
    assert_eq!(usage.started_count, 1);
    assert_eq!(usage.successful_count, 1);
    let envelope = usage.envelope.as_ref().expect("admitted envelope");
    assert_eq!(envelope.envelope_digest, admitted_digest);
    assert_eq!(envelope.max_output_bytes, 1_024);
}

#[tokio::test]
async fn adapter_cannot_widen_or_bypass_the_admitted_envelope() {
    // The adapter binds usage to its own wider envelope instead of the admitted one.
    assert_eq!(
        execute_case(UsageCase::InflatedEnvelope, Some((1_024, 60_000)))
            .await
            .unwrap_err(),
        PortError::Failed("effect_usage_envelope_digest_mismatch".to_owned())
    );
    // A forged fact above the admitted output quota is refused before it is counted.
    assert_eq!(
        execute_case(UsageCase::FloodedOutput, Some((1_024, 60_000)))
            .await
            .unwrap_err(),
        PortError::Failed("effect_usage_envelope_exceeded:output_bytes".to_owned())
    );
    // Usage without any admitted envelope is not metered work.
    assert_eq!(
        execute_case(UsageCase::NoAdmittedEnvelope, None)
            .await
            .unwrap_err(),
        PortError::Failed("effect_usage_envelope_required".to_owned())
    );
    // Once an envelope was admitted, success without usage evidence fails closed.
    assert_eq!(
        execute_case(UsageCase::NoUsage, Some((1_024, 60_000)))
            .await
            .unwrap_err(),
        PortError::Failed("effect_usage_observation_required".to_owned())
    );
    // A lease drift cannot be reported as a successful started observation.
    assert_eq!(
        execute_case(UsageCase::LeaseDrift, Some((1_024, 60_000)))
            .await
            .unwrap_err(),
        PortError::Failed("effect_usage_lease_mismatch".to_owned())
    );
    // A not-executed result can never carry a started usage.
    assert_eq!(
        execute_case(UsageCase::NotExecutedStartedUsage, Some((1_024, 60_000)))
            .await
            .unwrap_err(),
        PortError::Failed("effect_usage_result_state_mismatch".to_owned())
    );
    // The Broker's own wall-time measurement over the admitted bound fails closed.
    assert_eq!(
        execute_case(UsageCase::SlowBeyondWallEnvelope, Some((1_024, 1)))
            .await
            .unwrap_err(),
        PortError::Failed("effect_usage_envelope_exceeded:wall_time_ms".to_owned())
    );
}
