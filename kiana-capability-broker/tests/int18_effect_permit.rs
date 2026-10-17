//! INT-18 Broker behaviour fixture: effect-time permit and epoch fencing.
//!
//! The Broker is the last server-owned gate between an authorized connector request and its
//! handler.  These tests drive the real sealed `CapabilityBroker` with a constructed envelope and
//! prove deny-first: a missing, tampered, expired, stale-epoch or scope-drifted effect permit — and
//! a revoked/reconfigured/foreign binding snapshot — leaves both the recording handler and the
//! one-shot `ExecutionPermitVerifierPort` at zero calls.  Only the success case dispatches once.
//! No adapter, network transport or second execution loop is involved.

use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::*;
use kiana_ports::{CapabilityBrokerPort, ExecutionPermitVerifierPort, PortError};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};

const OPERATION: &str = "read";
const IDEMPOTENCY_KEY: &str = "int18-broker-key";
const BINDING_REVISION: u64 = 7;
const AUTHORITY_EPOCH: u64 = 4;
const CONFIGURATION_EPOCH: u64 = 3;
const POLICY_EPOCH: u64 = 5;
const DATA_EPOCH: u64 = 9;

struct RecordingHandler {
    calls: Arc<AtomicUsize>,
    binding_version: &'static str,
}

struct NoopHandler {
    binding_version: &'static str,
}

struct RecordingVerifier {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ExecutionPermitVerifierPort for RecordingVerifier {
    async fn verify_and_consume(
        &self,
        _request: &AuthorizedCapabilityRequest,
    ) -> Result<(), PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
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

#[async_trait]
impl CapabilityHandler for RecordingHandler {
    fn binding_version(&self) -> &'static str {
        self.binding_version
    }

    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CapabilityResult::success(
            request.request.request_id,
            json!({
                "operation": request.request.operation,
                "reservation_id": request.request.arguments["connector_reservation"]["reservation_id"],
                "scope_digest": request.request.arguments["connector_effect_permit"]["scope_digest"],
                "permit_digest": request.request.arguments["connector_effect_permit"]["permit_digest"],
            }),
        ))
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64
}

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

fn definition(connector_id: &str) -> ConnectorDefinition {
    ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: connector_id.to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-int18-broker".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            OPERATION.to_owned(),
            ConnectorOperation {
                effect: ConnectorEffect::ReadOnly,
                required_scope: "read".to_owned(),
                data_classes: BTreeSet::new(),
            },
        )]),
        rate_limit_per_minute: 10,
        idempotency_required: true,
        reconciliation_required: true,
        data_processing: "local_only".to_owned(),
    }
}

fn binding_for(connector_id: &str) -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: definition(connector_id),
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: format!("binding-{connector_id}"),
            connector_id: connector_id.to_owned(),
            account_id: format!("account-{connector_id}"),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/int18-broker.json".to_owned(),
            fixture_sha256: digest('f'),
            credential_ref: None,
        },
        project_root: "/repo".to_owned(),
        revision: BINDING_REVISION,
        status: "active".to_owned(),
    }
}

fn binding() -> ConnectorBindingSnapshot {
    binding_for("connector-int18-broker")
}

fn live_lease() -> ConnectorReservationLease {
    let now = now_ms();
    ConnectorReservationLease::new("lease-int18", "fence-int18", now - 1_000, now + 3_600_000)
        .expect("lease")
}

fn command(binding: &ConnectorBindingSnapshot, attempt: u32) -> ConnectorInvocationCommand {
    ConnectorInvocationCommand::from_payload(
        InvocationId::new(),
        attempt,
        binding,
        OPERATION,
        digest('o'),
        AUTHORITY_EPOCH,
        "policy.v7",
        "config.v1",
        0,
        DATA_EPOCH,
        &json!({"value": 1}),
        IDEMPOTENCY_KEY,
    )
    .expect("command")
}

/// A reservation exactly as ControlPlane would commit it, with a still-valid lease window.
fn committed_reservation(
    binding: &ConnectorBindingSnapshot,
    attempt: u32,
) -> ConnectorInvocationReservation {
    let mut reservation = ConnectorInvocationReservation::new(
        format!("reservation-{}", binding.binding.binding_id),
        command(binding, attempt),
        live_lease(),
    )
    .expect("reservation");
    reservation.state = ConnectorReservationState::Committed;
    reservation.state_revision = 2;
    reservation
}

fn fence(
    binding: &ConnectorBindingSnapshot,
    authority_epoch: u64,
    data_epoch: u64,
    credential_epoch: u64,
) -> ConnectorEffectFence {
    ConnectorEffectFence::new(
        connector_effect_scope_digest(binding, OPERATION).expect("scope digest"),
        authority_epoch,
        CONFIGURATION_EPOCH,
        POLICY_EPOCH,
        credential_epoch,
        data_epoch,
    )
    .expect("fence")
}

fn effect_permit(
    reservation: &ConnectorInvocationReservation,
    binding: &ConnectorBindingSnapshot,
) -> ConnectorEffectPermit {
    let now = now_ms();
    ConnectorEffectPermit::issue(
        reservation,
        binding,
        &fence(binding, AUTHORITY_EPOCH, DATA_EPOCH, 0),
        now,
        now + 60_000,
    )
    .expect("effect permit")
}

/// The complete server-owned envelope: binding snapshot, committed reservation, one-shot permit
/// and the INT-18 effect permit plus the fence epochs read at the effect boundary.
fn envelope(binding: &ConnectorBindingSnapshot) -> Value {
    let reservation = committed_reservation(binding, 1);
    let permit = ConnectorInvocationPermit::issue(&reservation).expect("permit");
    json!({
        "operation": OPERATION,
        "payload": {"value": 1},
        "idempotency_key": IDEMPOTENCY_KEY,
        "binding_snapshot": binding,
        "connector_reservation": reservation,
        "connector_permit": permit,
        "connector_effect_permit": effect_permit(&reservation, binding),
        "connector_configuration_epoch": CONFIGURATION_EPOCH,
        "connector_policy_epoch": POLICY_EPOCH,
        "connector_credential_epoch": 0,
    })
}

fn sealed_broker_for(
    handler: Arc<dyn CapabilityHandler>,
    target_operation: &str,
    verifier_calls: &Arc<AtomicUsize>,
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
    broker.set_permit_verifier(Arc::new(RecordingVerifier {
        calls: verifier_calls.clone(),
    }));
    broker
}

fn action_scope(
    request: &CapabilityRequest,
    authority_epoch: u64,
    data_epoch: u64,
) -> ExecutionScope {
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
        session_id: SessionId::new("int18-broker-test"),
        run_id: None,
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
        authority_epoch,
        trust_revision: kiana_domain::json_digest(&json!("trusted")),
        data_epoch,
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

fn authorized_with_epochs(
    arguments: Value,
    authority_epoch: u64,
    data_epoch: u64,
) -> AuthorizedCapabilityRequest {
    let mut request = CapabilityRequest::new(
        RequestId::new(),
        kiana_domain::capability_action_descriptor("connector.invoke")
            .expect("descriptor")
            .capability,
        "connector.invoke",
        arguments,
    )
    .with_risk(RiskLevel::ReadOnly);
    kiana_domain::normalize_capability_action(&mut request).expect("normalized action");
    request.execution_scope = Some(action_scope(&request, authority_epoch, data_epoch));
    AuthorizedCapabilityRequest::new("policy:int18-effect-permit", request).expect("authorized")
}

fn authorized(arguments: Value) -> AuthorizedCapabilityRequest {
    authorized_with_epochs(arguments, AUTHORITY_EPOCH, DATA_EPOCH)
}

fn assert_conflict(error: PortError, reason: &str) {
    assert!(
        matches!(&error, PortError::Conflict(value) if value.as_str() == reason),
        "expected {reason}, got {error:?}"
    );
}

#[tokio::test]
async fn current_effect_permit_reaches_the_handler_exactly_once() {
    let binding = binding();
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let verifier_calls = Arc::new(AtomicUsize::new(0));
    let broker = sealed_broker_for(
        Arc::new(RecordingHandler {
            calls: handler_calls.clone(),
            binding_version: kiana_domain::capability_action_descriptor("connector.invoke")
                .expect("descriptor")
                .binding_version,
        }),
        "connector.invoke",
        &verifier_calls,
    );
    let complete = envelope(&binding);
    let expected_scope = complete["connector_effect_permit"]["scope_digest"].clone();
    let expected_digest = complete["connector_effect_permit"]["permit_digest"].clone();
    let expected_reservation = complete["connector_reservation"]["reservation_id"].clone();

    let result = broker
        .execute(authorized(complete))
        .await
        .expect("admitted");
    assert_eq!(result.output["operation"], json!("connector.invoke"));
    assert_eq!(result.output["reservation_id"], expected_reservation);
    assert_eq!(result.output["scope_digest"], expected_scope);
    assert_eq!(result.output["permit_digest"], expected_digest);
    assert_eq!(
        handler_calls.load(Ordering::SeqCst),
        1,
        "one permit, one dispatch"
    );
    assert_eq!(
        verifier_calls.load(Ordering::SeqCst),
        1,
        "the effect fence must clear before the one-shot execution permit"
    );
}

#[tokio::test]
async fn stale_revoked_rotated_and_expired_effect_material_never_reaches_the_handler() {
    let binding = binding();
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let verifier_calls = Arc::new(AtomicUsize::new(0));
    let broker = sealed_broker_for(
        Arc::new(RecordingHandler {
            calls: handler_calls.clone(),
            binding_version: kiana_domain::capability_action_descriptor("connector.invoke")
                .expect("descriptor")
                .binding_version,
        }),
        "connector.invoke",
        &verifier_calls,
    );

    let complete = envelope(&binding);
    let mut cases: Vec<(&str, u64, u64, Value)> = Vec::new();

    // 1. The effect permit is mandatory whenever a reservation is present.
    let mut without_effect_permit = complete.clone();
    without_effect_permit
        .as_object_mut()
        .expect("envelope object")
        .remove("connector_effect_permit");
    cases.push((
        "connector_effect_permit_required",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        without_effect_permit,
    ));

    // 2. A permit whose recorded digest no longer matches its content is refused.
    let mut tampered = complete.clone();
    tampered["connector_effect_permit"]["payload_digest"] = json!(digest('x'));
    cases.push((
        "connector_effect_permit_invalid",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        tampered,
    ));

    // 3. Authority/data epochs come from the execution scope read at the effect boundary.
    cases.push((
        "connector_effect_authority_epoch_stale",
        AUTHORITY_EPOCH + 1,
        DATA_EPOCH,
        complete.clone(),
    ));
    cases.push((
        "connector_effect_data_epoch_stale",
        AUTHORITY_EPOCH,
        DATA_EPOCH + 1,
        complete.clone(),
    ));

    // 4. Configuration/policy/credential epochs are re-read server-side as well.
    let mut configuration_drift = complete.clone();
    configuration_drift["connector_configuration_epoch"] = json!(CONFIGURATION_EPOCH + 1);
    cases.push((
        "connector_effect_configuration_epoch_stale",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        configuration_drift,
    ));
    let mut policy_drift = complete.clone();
    policy_drift["connector_policy_epoch"] = json!(POLICY_EPOCH + 1);
    cases.push((
        "connector_effect_policy_epoch_stale",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        policy_drift,
    ));
    let mut credential_drift = complete.clone();
    credential_drift["connector_credential_epoch"] = json!(7);
    cases.push((
        "connector_effect_credential_epoch_stale",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        credential_drift,
    ));

    // 5. Missing fence epochs are a shape error, not a default.
    let mut without_configuration_epoch = complete.clone();
    without_configuration_epoch
        .as_object_mut()
        .expect("envelope object")
        .remove("connector_configuration_epoch");
    cases.push((
        "connector_configuration_epoch_required",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        without_configuration_epoch,
    ));
    let mut without_policy_epoch = complete.clone();
    without_policy_epoch
        .as_object_mut()
        .expect("envelope object")
        .remove("connector_policy_epoch");
    cases.push((
        "connector_policy_epoch_required",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        without_policy_epoch,
    ));

    // 6. The binding snapshot is mandatory and must be the exact admitted revision.
    let mut without_binding = complete.clone();
    without_binding
        .as_object_mut()
        .expect("envelope object")
        .remove("binding_snapshot");
    cases.push((
        "connector_binding_snapshot_required",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        without_binding,
    ));
    let mut revoked = binding.clone();
    revoked.status = "revoked".to_owned();
    let mut revoked_case = complete.clone();
    revoked_case["binding_snapshot"] = json!(revoked);
    cases.push((
        "connector_binding_revoked",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        revoked_case,
    ));
    let mut reconfigured = binding.clone();
    reconfigured.revision += 1;
    let mut reconfigured_case = complete.clone();
    reconfigured_case["binding_snapshot"] = json!(reconfigured);
    cases.push((
        "connector_effect_binding_revision_stale",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        reconfigured_case,
    ));
    let foreign = binding_for("connector-int18-broker-other");
    let mut foreign_case = complete.clone();
    foreign_case["binding_snapshot"] = json!(foreign);
    cases.push((
        "connector_effect_binding_snapshot_stale",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        foreign_case,
    ));

    // 7. Scope widening at the same revision changes the scope digest.
    let mut widened = binding.clone();
    widened.binding.read_scopes.insert("read-extra".to_owned());
    let mut widened_case = complete.clone();
    widened_case["binding_snapshot"] = json!(widened);
    cases.push((
        "connector_effect_scope_digest_mismatch",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        widened_case,
    ));

    // 8. An expired effect permit is not refreshed from a still-valid lease.
    let now = now_ms();
    let reservation: ConnectorInvocationReservation =
        serde_json::from_value(complete["connector_reservation"].clone()).expect("reservation");
    let expired = ConnectorEffectPermit::issue(
        &reservation,
        &binding,
        &fence(&binding, AUTHORITY_EPOCH, DATA_EPOCH, 0),
        now - 60_000,
        now - 30_000,
    )
    .expect("expired permit is well-formed");
    let mut expired_case = complete.clone();
    expired_case["connector_effect_permit"] = json!(expired);
    cases.push((
        "connector_effect_permit_expired",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        expired_case,
    ));

    // 9. A permit from another attempt of the same command cannot be substituted.
    let attempt_two = committed_reservation(&binding, 2);
    let attempt_two_permit = effect_permit(&attempt_two, &binding);
    let mut cross_attempt = complete.clone();
    cross_attempt["connector_effect_permit"] = json!(attempt_two_permit);
    cases.push((
        "connector_effect_permit_reservation_mismatch",
        AUTHORITY_EPOCH,
        DATA_EPOCH,
        cross_attempt,
    ));

    for (reason, authority_epoch, data_epoch, arguments) in cases {
        let error = broker
            .execute(authorized_with_epochs(
                arguments,
                authority_epoch,
                data_epoch,
            ))
            .await
            .unwrap_err();
        assert_conflict(error, reason);
        assert_eq!(
            handler_calls.load(Ordering::SeqCst),
            0,
            "denied connector request must not reach the handler ({reason})"
        );
        assert_eq!(
            verifier_calls.load(Ordering::SeqCst),
            0,
            "the effect fence must deny before the one-shot permit is consumed ({reason})"
        );
    }

    // The exact envelope still dispatches once after every refusal: the boundary is a fence, not a
    // permanent lockout.
    broker
        .execute(authorized(complete))
        .await
        .expect("admitted after denials");
    assert_eq!(handler_calls.load(Ordering::SeqCst), 1);
    assert_eq!(verifier_calls.load(Ordering::SeqCst), 1);
}
