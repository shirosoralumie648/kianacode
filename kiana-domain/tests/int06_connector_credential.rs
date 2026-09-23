use kiana_domain::*;
use std::collections::{BTreeMap, BTreeSet};

const NOW: u64 = 10_000;
const IDEMPOTENCY_KEY: &str = "connector-invoke-1";

fn binding() -> ConnectorBindingSnapshot {
    let definition = ConnectorDefinition {
        schema: "kiana.connector-definition.v1".to_owned(),
        connector_id: "connector-demo".to_owned(),
        version: "v1".to_owned(),
        provider_id: "provider-demo".to_owned(),
        adapter: "local_fixture".to_owned(),
        operations: BTreeMap::from([(
            "read".to_owned(),
            ConnectorOperation {
                effect: ConnectorEffect::ReadOnly,
                required_scope: "read".to_owned(),
                data_classes: BTreeSet::from(["internal".to_owned()]),
            },
        )]),
        rate_limit_per_minute: 10,
        idempotency_required: true,
        reconciliation_required: true,
        data_processing: "local_only".to_owned(),
    };
    let account = AccountBinding {
        schema: "kiana.account-binding.v1".to_owned(),
        binding_id: "binding-demo".to_owned(),
        connector_id: "connector-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        read_scopes: BTreeSet::from(["read".to_owned()]),
        write_scopes: BTreeSet::new(),
        fixture_path: "fixtures/connector.json".to_owned(),
        fixture_sha256: format!("sha256:{}", "a".repeat(64)),
        credential_ref: Some(
            SecretRef::new(
                "env",
                "INT06_CONNECTOR_SECRET",
                CONNECTOR_CREDENTIAL_PURPOSE,
                "connector:connector-demo:v1",
                4,
            )
            .unwrap(),
        ),
    };
    ConnectorBindingSnapshot {
        definition,
        binding: account,
        project_root: "/repo".to_owned(),
        revision: 7,
        status: "active".to_owned(),
    }
}

fn secret_ref(binding: &ConnectorBindingSnapshot) -> SecretRef {
    binding
        .binding
        .credential_ref
        .clone()
        .expect("opaque reference")
}

fn invocation(binding: &ConnectorBindingSnapshot) -> ConnectorCredentialInvocation {
    ConnectorCredentialInvocation::issue(
        binding,
        "read",
        RequestId::new(),
        IDEMPOTENCY_KEY,
        &secret_ref(binding),
        NOW,
        500,
    )
    .expect("bound connector credential invocation")
}

#[test]
fn invocation_consumes_exact_binding_once_and_projects_redacted_evidence() {
    assert!(schema_contract(CONNECTOR_CREDENTIAL_INVOCATION_SCHEMA).is_some());
    let binding = binding();
    let request_id = RequestId::new();
    let reference = secret_ref(&binding);
    let mut invocation = ConnectorCredentialInvocation::issue(
        &binding,
        "read",
        request_id,
        IDEMPOTENCY_KEY,
        &reference,
        NOW,
        500,
    )
    .unwrap();

    let evidence = invocation
        .consume_for(&binding, "read", request_id, IDEMPOTENCY_KEY, NOW + 1)
        .unwrap();
    assert_eq!(evidence.binding_revision, 7);
    assert_eq!(evidence.binding_digest, binding_digest(&binding));
    assert_eq!(evidence.credential_generation, 4);
    assert_eq!(evidence.secret_ref_digest, reference.reference_digest);
    assert_eq!(evidence.lease_digest, invocation.lease.lease_digest);
    let encoded = serde_json::to_string(&evidence).unwrap();
    assert!(!encoded.contains("INT06_CONNECTOR_SECRET"));
    assert!(!encoded.contains("secret_value"));
    assert_eq!(
        invocation
            .consume_for(&binding, "read", request_id, IDEMPOTENCY_KEY, NOW + 2)
            .unwrap_err(),
        "credential_lease_replayed"
    );
}

#[test]
fn invocation_rejects_wrong_binding_operation_key_and_expiry() {
    let active = binding();
    let mut wrong_account = active.clone();
    wrong_account.binding.account_id = "other-account".to_owned();
    let mut lease = invocation(&active);
    let invocation_id = lease.invocation_id;
    assert_eq!(
        lease
            .consume_for(
                &wrong_account,
                "read",
                invocation_id,
                IDEMPOTENCY_KEY,
                NOW + 1,
            )
            .unwrap_err(),
        "connector_credential_invocation_binding_mismatch"
    );

    let mut lease = invocation(&active);
    let invocation_id = lease.invocation_id;
    assert_eq!(
        lease
            .consume_for(&active, "write", invocation_id, IDEMPOTENCY_KEY, NOW + 1,)
            .unwrap_err(),
        "connector_operation_unregistered"
    );

    let mut lease = invocation(&active);
    let invocation_id = lease.invocation_id;
    assert_eq!(
        lease
            .consume_for(&active, "read", invocation_id, "different-key", NOW + 1,)
            .unwrap_err(),
        "connector_credential_invocation_binding_mismatch"
    );

    let mut lease = invocation(&active);
    let invocation_id = lease.invocation_id;
    assert_eq!(
        lease
            .consume_for(&active, "read", invocation_id, IDEMPOTENCY_KEY, NOW + 500,)
            .unwrap_err(),
        "credential_lease_expired"
    );
}

#[test]
fn invocation_rejects_stale_target_wrong_secret_scope_and_unknown_fields() {
    let active = binding();
    let mut changed_target = active.clone();
    changed_target.binding.fixture_sha256 = format!("sha256:{}", "b".repeat(64));
    let mut lease = invocation(&active);
    let lease_id = lease.invocation_id;
    assert_eq!(
        lease
            .consume_for(&changed_target, "read", lease_id, IDEMPOTENCY_KEY, NOW + 1,)
            .unwrap_err(),
        "connector_credential_invocation_binding_mismatch"
    );

    let mut widened_scope = active.clone();
    widened_scope.binding.read_scopes.insert("admin".to_owned());
    let mut lease = invocation(&active);
    let invocation_id = lease.invocation_id;
    assert_eq!(
        lease
            .consume_for(
                &widened_scope,
                "read",
                invocation_id,
                IDEMPOTENCY_KEY,
                NOW + 1,
            )
            .unwrap_err(),
        "connector_credential_invocation_binding_mismatch"
    );

    let wrong_purpose = SecretRef::new(
        "env",
        "INT06_WRONG_PURPOSE",
        "provider.request",
        "connector:connector-demo:v1",
        1,
    )
    .unwrap();
    assert_eq!(
        ConnectorCredentialInvocation::issue(
            &active,
            "read",
            RequestId::new(),
            IDEMPOTENCY_KEY,
            &wrong_purpose,
            NOW,
            100,
        )
        .unwrap_err(),
        "connector_credential_secret_ref_binding_mismatch"
    );

    let unbound_reference = SecretRef::new(
        "env",
        "INT06_UNBOUND_SECRET",
        CONNECTOR_CREDENTIAL_PURPOSE,
        "connector:connector-demo:v1",
        4,
    )
    .unwrap();
    assert_eq!(
        ConnectorCredentialInvocation::issue(
            &active,
            "read",
            RequestId::new(),
            IDEMPOTENCY_KEY,
            &unbound_reference,
            NOW,
            100,
        )
        .unwrap_err(),
        "connector_credential_reference_mismatch"
    );

    let mut value = serde_json::to_value(invocation(&active)).unwrap();
    value["secret_value"] = serde_json::json!("INT06_RAW_SECRET_SENTINEL");
    assert!(serde_json::from_value::<ConnectorCredentialInvocation>(value).is_err());
}

#[test]
fn binding_rejects_secret_values_instead_of_environment_reference_names() {
    let mut active = binding();
    let value = active.binding.credential_ref.as_ref().unwrap();
    active.binding.credential_ref = Some(
        SecretRef::new(
            "env",
            "sk-INT06_RAW_SENTINEL_VALUE",
            value.purpose.clone(),
            value.audience.clone(),
            value.generation,
        )
        .unwrap(),
    );
    assert_eq!(
        active.validate().unwrap_err(),
        "connector_credential_reference_invalid"
    );
}

#[test]
fn lease_rejects_wrong_purpose_audience_and_effect_target() {
    let active = binding();

    let mut wrong_purpose = invocation(&active);
    wrong_purpose.lease.purpose = "provider.request".to_owned();
    wrong_purpose.lease.lease_digest = wrong_purpose.lease.digest();
    let wrong_purpose_id = wrong_purpose.invocation_id;
    assert_eq!(
        wrong_purpose
            .consume_for(&active, "read", wrong_purpose_id, IDEMPOTENCY_KEY, NOW + 1,)
            .unwrap_err(),
        "credential_lease_binding_mismatch"
    );

    let mut wrong_audience = invocation(&active);
    wrong_audience.lease.audience = "connector:other:v1".to_owned();
    wrong_audience.lease.lease_digest = wrong_audience.lease.digest();
    let wrong_audience_id = wrong_audience.invocation_id;
    assert_eq!(
        wrong_audience
            .consume_for(&active, "read", wrong_audience_id, IDEMPOTENCY_KEY, NOW + 1,)
            .unwrap_err(),
        "credential_lease_binding_mismatch"
    );

    let mut wrong_target = invocation(&active);
    wrong_target.lease.endpoint_digest = json_digest(&serde_json::json!("other-target"));
    wrong_target.lease.lease_digest = wrong_target.lease.digest();
    let wrong_target_id = wrong_target.invocation_id;
    assert_eq!(
        wrong_target
            .consume_for(&active, "read", wrong_target_id, IDEMPOTENCY_KEY, NOW + 1,)
            .unwrap_err(),
        "credential_lease_endpoint_mismatch"
    );
}

#[test]
fn consume_rejects_mutable_non_one_shot_and_overlong_leases() {
    let active = binding();

    let mut reusable = invocation(&active);
    reusable.lease.one_shot = false;
    reusable.lease.lease_digest = reusable.lease.digest();
    let reusable_id = reusable.invocation_id;
    assert_eq!(
        reusable
            .consume_for(&active, "read", reusable_id, IDEMPOTENCY_KEY, NOW + 1,)
            .unwrap_err(),
        "connector_credential_one_shot_required"
    );

    let mut overlong = invocation(&active);
    overlong.lease.expires_at_unix_ms = NOW + CONNECTOR_CREDENTIAL_MAX_TTL_MS + 1;
    overlong.lease.lease_digest = overlong.lease.digest();
    let overlong_id = overlong.invocation_id;
    assert_eq!(
        overlong
            .consume_for(&active, "read", overlong_id, IDEMPOTENCY_KEY, NOW + 1,)
            .unwrap_err(),
        "connector_credential_lease_ttl_exceeded"
    );
}
