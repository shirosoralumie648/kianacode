//! INT-17 Broker behaviour fixture: connector quota envelope at the effect boundary.
//!
//! The Broker is the last server-owned checkpoint before a connector handler runs, so these
//! tests drive the real sealed `CapabilityBroker` (registered `connector.invoke` handler and
//! `ExecutionPermitVerifierPort`) with constructed server-owned quota envelopes.  Deny-first: a
//! missing claim or policy, an orphan claim/policy without the reservation, tampered quota
//! digests, a stale claim revision, an expired window, an over-limit reservation (concurrency
//! above the policy ceiling) and a foreign policy fence all leave the recording handler at zero
//! calls.  The success case proves the exact admitted quota envelope reaches the handler once
//! while legacy requests without the additive envelope keep their compatibility path.
//!
//! The INT-16 invocation reservation and INT-18 effect permit are constructed here only because
//! the same effect boundary already requires them at HEAD; the quota rules below are INT-17's
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
const IDEMPOTENCY_KEY: &str = "broker-quota-key";
const WORKER_ID: &str = "quota-worker";
const BINDING_REVISION: u64 = 7;
const AUTHORITY_REVISION: u64 = 4;
const CONFIGURATION_EPOCH: u64 = 3;
const POLICY_EPOCH: u64 = 5;
const DATA_REVISION: u64 = 9;
const QUOTA_AUTHORITY_EPOCH: u64 = 11;
const QUOTA_CONFIGURATION_REVISION: &str = "quota.config.3";

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
                "quota_reservation_id": request.request.arguments["connector_quota_reservation"]["reservation_id"],
                "quota_worker": request.request.arguments["connector_quota_claim"]["worker_id"],
                "quota_command_digest": request.request.arguments["connector_quota_reservation"]["command_digest"],
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
            connector_id: "connector-int17-broker".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-int17-broker".to_owned(),
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
            binding_id: "binding-int17-broker".to_owned(),
            connector_id: "connector-int17-broker".to_owned(),
            account_id: "account-int17-broker".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/int17-broker.json".to_owned(),
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
    ConnectorReservationLease::new("lease-int17", "fence-int17", now - 1_000, now + 3_600_000)
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
        ConnectorInvocationReservation::new("reservation-int17", command(binding, attempt), lease)
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

fn quota_limit(
    max_requests: u64,
    max_tokens: u64,
    max_budget_micros: u64,
    max_concurrency: u32,
) -> ConnectorQuotaLimit {
    ConnectorQuotaLimit {
        max_requests,
        max_tokens,
        max_budget_micros,
        max_concurrency,
    }
}

/// Server-owned quota policy for the same connector/account/project identity the command uses.
/// The UTC window covers the test clock; `rate_card_id` and the capacity lease digest are
/// server-derived in production and fixed here.
fn quota_policy(
    binding: &ConnectorBindingSnapshot,
    authority_epoch: u64,
    max_concurrency: u32,
) -> ConnectorQuotaPolicy {
    let now = now_ms() as u128;
    let clock = ClockObservation::observe("int17-broker", now, now, None, 1).expect("clock");
    let window = QuotaWindow::from_clock(&clock, 3_600_000).expect("window");
    ConnectorQuotaPolicy::new(
        ConnectorQuotaKey::new(
            binding.definition.connector_id.as_str(),
            binding.definition.version.as_str(),
            binding.binding.account_id.as_str(),
            binding.project_root.as_str(),
            "main",
            1,
        )
        .expect("quota key"),
        window,
        quota_limit(10, 1_000, 10_000, max_concurrency),
        quota_limit(10, 1_000, 10_000, max_concurrency),
        quota_limit(10, 1_000, 10_000, max_concurrency),
        authority_epoch,
        QUOTA_CONFIGURATION_REVISION,
        Some(RateCardId::new()),
        Some(digest('c')),
    )
    .expect("quota policy")
}

fn quota_dimensions(concurrency: u32) -> ConnectorQuotaDimensions {
    ConnectorQuotaDimensions {
        requests: 1,
        tokens: 20,
        budget_micros: 100,
        concurrency,
    }
}

/// The claimed reservation the ControlPlane CAS would leave behind, with every digest
/// recomputed so only the stated field can explain a later denial.
fn claimed_quota_reservation(
    policy: &ConnectorQuotaPolicy,
    invocation_id: InvocationId,
    attempt: u32,
    command_digest: &str,
    requested: ConnectorQuotaDimensions,
    issued_at_unix_ms: u64,
    expires_at_unix_ms: u64,
) -> ConnectorQuotaReservation {
    let mut reservation = ConnectorQuotaReservation::new(
        QuotaReservationId::new(),
        invocation_id,
        attempt,
        RunId::new(),
        AttemptId::new(),
        digest('d'),
        command_digest,
        policy,
        requested,
        issued_at_unix_ms,
        expires_at_unix_ms,
    )
    .expect("quota reservation");
    reservation.state = ConnectorQuotaReservationState::Claimed;
    reservation.claimed_by = Some(WORKER_ID.to_owned());
    reservation.revision = 2;
    reservation.reservation_digest = reservation.digest();
    reservation
}

/// The complete server-owned envelope: binding snapshot, committed INT-16 reservation, one-shot
/// permit, INT-18 effect permit and the INT-17 quota reservation/claim/policy triple.
fn envelope(binding: &ConnectorBindingSnapshot) -> Value {
    let reservation = committed_reservation(binding, live_lease(), 1);
    let permit = ConnectorInvocationPermit::issue(&reservation).expect("permit");
    let policy = quota_policy(binding, QUOTA_AUTHORITY_EPOCH, 1);
    let now = now_ms();
    let quota_reservation = claimed_quota_reservation(
        &policy,
        reservation.command.invocation_id,
        reservation.command.attempt,
        &reservation.command.command_digest,
        quota_dimensions(1),
        now - 1_000,
        now + 3_600_000,
    );
    let quota_claim =
        ConnectorQuotaClaim::new(&quota_reservation, WORKER_ID, now).expect("quota claim");
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
        "connector_quota_reservation": quota_reservation,
        "connector_quota_claim": quota_claim,
        "connector_quota_policy": policy,
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
        session_id: SessionId::new("int17-broker-test"),
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
    AuthorizedCapabilityRequest::new("policy:int17-quota", request).expect("authorized")
}

fn recording_broker(calls: &Arc<AtomicUsize>) -> CapabilityBroker {
    sealed_broker_for(
        Arc::new(RecordingHandler {
            calls: calls.clone(),
            binding_version: kiana_domain::capability_action_descriptor("connector.invoke")
                .expect("descriptor")
                .binding_version,
        }),
        "connector.invoke",
    )
}

#[tokio::test]
async fn admitted_quota_envelope_reaches_the_handler_exactly_once() {
    let binding = binding();
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = recording_broker(&calls);
    let complete = envelope(&binding);
    let expected_reservation_id = complete["connector_quota_reservation"]["reservation_id"].clone();
    let expected_command_digest = complete["connector_quota_reservation"]["command_digest"].clone();

    let result = broker
        .execute(authorized(complete))
        .await
        .expect("admitted");
    assert_eq!(result.output["operation"], json!("connector.invoke"));
    assert_eq!(
        result.output["quota_reservation_id"],
        expected_reservation_id
    );
    assert_eq!(
        result.output["quota_command_digest"],
        expected_command_digest
    );
    assert_eq!(result.output["quota_worker"], json!(WORKER_ID));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "one admitted envelope, one dispatch"
    );

    // The additive envelope stays optional: a legacy request without it still follows the
    // compatibility path until ControlPlane emits quota facts for every caller.
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
async fn over_limit_stale_and_tampered_quota_envelopes_never_reach_the_handler() {
    let binding = binding();
    let calls = Arc::new(AtomicUsize::new(0));
    let broker = recording_broker(&calls);

    let complete = envelope(&binding);
    let quota_reservation: ConnectorQuotaReservation =
        serde_json::from_value(complete["connector_quota_reservation"].clone())
            .expect("reservation");
    let quota_claim: ConnectorQuotaClaim =
        serde_json::from_value(complete["connector_quota_claim"].clone()).expect("claim");
    let current_policy: ConnectorQuotaPolicy =
        serde_json::from_value(complete["connector_quota_policy"].clone()).expect("policy");

    let mut cases: Vec<(&str, Value)> = Vec::new();

    // 1. A quota reservation without its claim cannot proceed.
    let mut without_claim = complete.clone();
    without_claim
        .as_object_mut()
        .expect("envelope object")
        .remove("connector_quota_claim");
    cases.push(("connector_quota_claim_required", without_claim));

    // 2. A quota reservation without its policy cannot proceed.
    let mut without_policy = complete.clone();
    without_policy
        .as_object_mut()
        .expect("envelope object")
        .remove("connector_quota_policy");
    cases.push(("connector_quota_policy_required", without_policy));

    // 3. An orphan claim/policy without the server-owned reservation is refused rather than
    //    silently falling back to the compatibility path.
    let mut orphan = complete.clone();
    let orphan_object = orphan.as_object_mut().expect("envelope object");
    orphan_object.remove("connector_quota_reservation");
    cases.push(("connector_quota_reservation_required", orphan));

    // 4. A reservation whose recorded digest no longer matches its content is refused.
    let mut tampered_reservation = complete.clone();
    tampered_reservation["connector_quota_reservation"]["reservation_digest"] = json!(digest('z'));
    cases.push(("connector_quota_reservation_invalid", tampered_reservation));

    // 5. A claim whose recorded digest no longer matches its content is refused.
    let mut tampered_claim = complete.clone();
    tampered_claim["connector_quota_claim"]["claim_digest"] = json!(digest('z'));
    cases.push(("connector_quota_claim_invalid", tampered_claim));

    // 6. A policy whose recorded digest no longer matches its content is refused.
    let mut tampered_policy = complete.clone();
    tampered_policy["connector_quota_policy"]["policy_digest"] = json!(digest('z'));
    cases.push(("connector_quota_policy_invalid", tampered_policy));

    // 7. A claim from before the CAS claim (expected revision 1) cannot fence the claimed
    //    reservation (revision 2).
    let mut stale_claim = quota_claim.clone();
    stale_claim.expected_revision = 1;
    stale_claim.claim_digest = stale_claim.digest();
    let mut stale_claim_case = complete.clone();
    stale_claim_case["connector_quota_claim"] = json!(stale_claim);
    cases.push(("connector_quota_claim_fence_mismatch", stale_claim_case));

    // 8. An expired quota reservation is refused at the effect boundary.
    let now = now_ms();
    let expired_policy = quota_policy(&binding, QUOTA_AUTHORITY_EPOCH, 1);
    let expired = claimed_quota_reservation(
        &expired_policy,
        quota_reservation.invocation_id,
        quota_reservation.attempt,
        &quota_reservation.command_digest,
        quota_dimensions(1),
        now - 120_000,
        now - 60_000,
    );
    let expired_claim =
        ConnectorQuotaClaim::new(&expired, WORKER_ID, now - 121_000).expect("expired claim");
    let mut expired_case = complete.clone();
    expired_case["connector_quota_reservation"] = json!(expired);
    expired_case["connector_quota_claim"] = json!(expired_claim);
    expired_case["connector_quota_policy"] = json!(expired_policy);
    cases.push(("connector_quota_reservation_expired", expired_case));

    // 9. A self-consistent reservation requesting more concurrency than the policy ceiling can
    //    never dispatch, even though every digest matches its content.
    let over_policy = quota_policy(&binding, QUOTA_AUTHORITY_EPOCH, 1);
    let over_limit = claimed_quota_reservation(
        &over_policy,
        quota_reservation.invocation_id,
        quota_reservation.attempt,
        &quota_reservation.command_digest,
        quota_dimensions(2),
        now - 1_000,
        now + 3_600_000,
    );
    let over_claim =
        ConnectorQuotaClaim::new(&over_limit, WORKER_ID, now).expect("over-limit claim");
    let mut over_case = complete.clone();
    over_case["connector_quota_reservation"] = json!(over_limit);
    over_case["connector_quota_claim"] = json!(over_claim);
    over_case["connector_quota_policy"] = json!(over_policy);
    cases.push(("connector_quota_connector_limit_exceeded", over_case));

    // 10. A policy fence drift (different authority epoch / policy digest than the reservation)
    //     is refused before the adapter.
    let drifted_policy = quota_policy(&binding, QUOTA_AUTHORITY_EPOCH + 1, 1);
    let mut drifted_case = complete.clone();
    drifted_case["connector_quota_policy"] = json!(drifted_policy);
    cases.push(("connector_quota_policy_fence_stale", drifted_case));

    // Sanity: the unmodified envelope still carries the claim/policy the fixture mutations use.
    assert_eq!(current_policy.authority_epoch, QUOTA_AUTHORITY_EPOCH);
    assert_eq!(quota_claim.worker_id, WORKER_ID);

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
