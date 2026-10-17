//! INT-16 Broker behaviour fixture: invocation reservation, command digest and permit CAS.
//!
//! The Broker is the effect boundary between an authorized request and a connector handler, so
//! these tests drive the real `CapabilityBroker` (sealed catalog, registered `connector.invoke`
//! handler and `ExecutionPermitVerifierPort`) with constructed server-owned reservation
//! envelopes.  Deny-first: an uncommitted reservation, a stale or cross-attempt permit, a
//! tampered reservation/permit, a permit without a reservation and a reservation without a
//! permit all leave the recording handler at zero calls.  The success case proves that the exact
//! committed reservation reaches the handler exactly once and that a legacy request without the
//! additive envelope still follows the compatibility path.
//!
//! The existing INT-18 effect permit is composed here only because the same effect boundary
//! already requires it at HEAD; the reservation/command-digest rules below are INT-16's
//! contribution and no adapter, network transport or second execution loop is involved.

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
const IDEMPOTENCY_KEY: &str = "broker-key";
const BINDING_REVISION: u64 = 7;
const AUTHORITY_REVISION: u64 = 4;
const CONFIGURATION_EPOCH: u64 = 3;
const POLICY_EPOCH: u64 = 5;
const DATA_REVISION: u64 = 9;

struct RecordingHandler {
    calls: Arc<AtomicUsize>,
    binding_version: &'static str,
}

struct NoopHandler {
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
                "command_digest": request.request.arguments["connector_reservation"]["command"]["command_digest"],
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

fn binding() -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: ConnectorDefinition {
            schema: "kiana.connector-definition.v1".to_owned(),
            connector_id: "connector-int16-broker".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-int16-broker".to_owned(),
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
        },
        binding: AccountBinding {
            schema: "kiana.account-binding.v1".to_owned(),
            binding_id: "binding-int16-broker".to_owned(),
            connector_id: "connector-int16-broker".to_owned(),
            account_id: "account-int16-broker".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/int16-broker.json".to_owned(),
            fixture_sha256: digest('f'),
            credential_ref: None,
        },
        project_root: "/repo".to_owned(),
        revision: BINDING_REVISION,
        status: "active".to_owned(),
    }
}

fn live_lease() -> ConnectorReservationLease {
    let now = now_ms();
    ConnectorReservationLease::new("lease-int16", "fence-int16", now - 1_000, now + 3_600_000)
        .expect("lease")
}

fn stale_lease() -> ConnectorReservationLease {
    let now = now_ms();
    ConnectorReservationLease::new("lease-int16", "fence-int16", now - 120_000, now - 60_000)
        .expect("lease")
}

fn command(binding: &ConnectorBindingSnapshot, attempt: u32) -> ConnectorInvocationCommand {
    ConnectorInvocationCommand::from_payload(
        InvocationId::new(),
        attempt,
        binding,
        OPERATION,
        digest('o'),
        AUTHORITY_REVISION,
        "policy.v7",
        "config.v1",
        0,
        DATA_REVISION,
        &json!({"value": 1}),
        IDEMPOTENCY_KEY,
    )
    .expect("command")
}

/// A reservation exactly as ControlPlane would commit it: the command identity is untouched and
/// only the CAS state/state revision advance.
fn committed_reservation(
    binding: &ConnectorBindingSnapshot,
    lease: ConnectorReservationLease,
    attempt: u32,
) -> ConnectorInvocationReservation {
    let mut reservation =
        ConnectorInvocationReservation::new("reservation-int16", command(binding, attempt), lease)
            .expect("reservation");
    reservation.state = ConnectorReservationState::Committed;
    reservation.state_revision = 2;
    reservation
}

fn effect_permit(
    reservation: &ConnectorInvocationReservation,
    binding: &ConnectorBindingSnapshot,
) -> ConnectorEffectPermit {
    let fence = ConnectorEffectFence::new(
        connector_effect_scope_digest(binding, OPERATION).expect("scope digest"),
        AUTHORITY_REVISION,
        CONFIGURATION_EPOCH,
        POLICY_EPOCH,
        0,
        DATA_REVISION,
    )
    .expect("fence");
    let now = now_ms();
    ConnectorEffectPermit::issue(reservation, binding, &fence, now, now + 60_000)
        .expect("effect permit")
}

/// The complete server-owned envelope: binding snapshot, committed reservation, one-shot permit
/// and the INT-18 effect permit required by the same effect boundary.
fn envelope(binding: &ConnectorBindingSnapshot) -> Value {
    let reservation = committed_reservation(binding, live_lease(), 1);
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

fn action_scope(request: &CapabilityRequest) -> ExecutionScope {
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
        session_id: SessionId::new("int16-broker-test"),
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
        authority_epoch: AUTHORITY_REVISION,
        trust_revision: kiana_domain::json_digest(&json!("trusted")),
        data_epoch: DATA_REVISION,
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

fn authorized(arguments: Value) -> AuthorizedCapabilityRequest {
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
    request.execution_scope = Some(action_scope(&request));
    AuthorizedCapabilityRequest::new("policy:int16-reservation", request).expect("authorized")
}

#[tokio::test]
async fn committed_reservation_reaches_the_handler_exactly_once_with_its_command_digest() {
    let binding = binding();
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = sealed_broker_for(
        Arc::new(RecordingHandler {
            calls: calls.clone(),
            binding_version: kiana_domain::capability_action_descriptor("connector.invoke")
                .expect("descriptor")
                .binding_version,
        }),
        "connector.invoke",
    );
    let complete = envelope(&binding);
    let expected_digest = complete["connector_reservation"]["command"]["command_digest"].clone();
    let expected_reservation_id = complete["connector_reservation"]["reservation_id"].clone();

    let result = broker
        .execute(authorized(complete))
        .await
        .expect("admitted");
    assert_eq!(result.output["operation"], json!("connector.invoke"));
    assert_eq!(result.output["reservation_id"], expected_reservation_id);
    assert_eq!(result.output["command_digest"], expected_digest);
    assert_eq!(calls.load(Ordering::SeqCst), 1, "one permit, one dispatch");

    // The additive envelope stays optional: a legacy request without it still follows the
    // compatibility path until ControlPlane emits committed reservation facts for every caller.
    let legacy = json!({
        "operation": OPERATION,
        "payload": {"value": 1},
        "idempotency_key": IDEMPOTENCY_KEY,
        "binding_snapshot": binding,
    });
    broker
        .execute(authorized(legacy))
        .await
        .expect("legacy path");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn uncommitted_stale_and_tampered_reservations_never_reach_the_handler() {
    let binding = binding();
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = sealed_broker_for(
        Arc::new(RecordingHandler {
            calls: calls.clone(),
            binding_version: kiana_domain::capability_action_descriptor("connector.invoke")
                .expect("descriptor")
                .binding_version,
        }),
        "connector.invoke",
    );

    let complete = envelope(&binding);
    let reservation: ConnectorInvocationReservation =
        serde_json::from_value(complete["connector_reservation"].clone()).expect("reservation");
    let permit: ConnectorInvocationPermit =
        serde_json::from_value(complete["connector_permit"].clone()).expect("permit");

    let mut cases: Vec<(&str, Value)> = Vec::new();

    // 1. A reservation without its one-shot permit cannot proceed.
    let mut without_permit = complete.clone();
    without_permit
        .as_object_mut()
        .expect("envelope object")
        .remove("connector_permit");
    cases.push(("connector_permit_required", without_permit));

    // 2. An orphan permit is not silently ignored: without the server-owned reservation the
    // envelope is refused instead of falling back to the compatibility path.
    let orphan_permit = json!({
        "operation": OPERATION,
        "payload": {"value": 1},
        "idempotency_key": IDEMPOTENCY_KEY,
        "binding_snapshot": binding,
        "connector_permit": permit,
    });
    cases.push(("connector_reservation_required", orphan_permit));

    // 3. An uncommitted reservation can never be consumed, even with an otherwise valid permit.
    let mut uncommitted = reservation.clone();
    uncommitted.state = ConnectorReservationState::Reserved;
    uncommitted.state_revision = 1;
    let mut uncommitted_case = complete.clone();
    uncommitted_case["connector_reservation"] = json!(uncommitted);
    cases.push(("connector_reservation_not_committed", uncommitted_case));

    // 4. A permit from an expired reservation window is refused before the handler.
    let stale = committed_reservation(&binding, stale_lease(), 1);
    let stale_permit = ConnectorInvocationPermit::issue(&stale).expect("stale permit");
    let mut stale_permit_case = complete.clone();
    stale_permit_case["connector_reservation"] = json!(stale);
    stale_permit_case["connector_permit"] = json!(stale_permit);
    cases.push(("connector_permit_expired", stale_permit_case));

    // 5. A permit issued for another attempt of the same reservation cannot be substituted.
    let attempt_two = committed_reservation(&binding, live_lease(), 2);
    let mut cross_attempt = complete.clone();
    cross_attempt["connector_permit"] =
        json!(ConnectorInvocationPermit::issue(&attempt_two).expect("attempt two permit"));
    cases.push(("connector_permit_reservation_mismatch", cross_attempt));

    // 6. A reservation whose recorded digest no longer matches its content is refused.
    let mut tampered_reservation = complete.clone();
    tampered_reservation["connector_reservation"]["reservation_digest"] = json!(digest('z'));
    cases.push((
        "connector_invocation_reservation_invalid",
        tampered_reservation,
    ));

    // 7. A permit whose recorded digest no longer matches its content is refused.
    let mut tampered_permit = complete.clone();
    tampered_permit["connector_permit"]["permit_digest"] = json!(digest('z'));
    cases.push(("connector_invocation_permit_invalid", tampered_permit));

    // 8. A structurally invalid envelope is refused before any deserialization defaults apply.
    let mut malformed = complete.clone();
    malformed["connector_permit"] = json!({"schema": "kiana.connector-invocation-permit.v1"});
    cases.push(("connector_permit_invalid", malformed));

    for (reason, arguments) in cases {
        let error = broker.execute(authorized(arguments)).await.unwrap_err();
        assert!(
            matches!(&error, PortError::Conflict(value) if value.as_str() == reason),
            "expected {reason}, got {error:?}"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "denied connector request must not reach the handler ({reason})"
        );
    }
}
