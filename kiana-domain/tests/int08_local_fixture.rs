use kiana_domain::*;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

fn binding() -> ConnectorBindingSnapshot {
    ConnectorBindingSnapshot {
        definition: ConnectorDefinition {
            schema: "kiana.connector-definition.v1".to_owned(),
            connector_id: "connector-demo".to_owned(),
            version: "v1".to_owned(),
            provider_id: "provider-demo".to_owned(),
            adapter: "local_fixture".to_owned(),
            operations: BTreeMap::from([(
                "lookup".to_owned(),
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
            binding_id: "binding-demo".to_owned(),
            connector_id: "connector-demo".to_owned(),
            account_id: "account-demo".to_owned(),
            read_scopes: BTreeSet::from(["read".to_owned()]),
            write_scopes: BTreeSet::new(),
            fixture_path: "fixtures/connector.json".to_owned(),
            fixture_sha256: format!("sha256:{}", "a".repeat(64)),
            credential_ref: None,
        },
        project_root: "/repo".to_owned(),
        revision: 1,
        status: "active".to_owned(),
    }
}

fn fixture() -> ConnectorFixture {
    ConnectorFixture {
        schema: CONNECTOR_FIXTURE_SCHEMA.to_owned(),
        connector_id: "connector-demo".to_owned(),
        account_id: "account-demo".to_owned(),
        operations: BTreeMap::from([(
            "lookup".to_owned(),
            vec![ConnectorFixtureCase {
                payload: json!({"id": "one"}),
                outcome: ProviderOutcome::Succeeded,
                receipt_id: "receipt-one".to_owned(),
                result: json!({"value": "fixture"}),
                external_effect: false,
            }],
        )]),
        external_effect: false,
    }
}

#[test]
fn fixture_schema_bounds_hash_and_payload_matching_fail_closed() {
    let fixture = fixture();
    fixture.validate_for_binding(&binding()).unwrap();
    let bytes = serde_json::to_vec(&fixture).unwrap();
    let decoded = ConnectorFixture::from_bytes(&bytes).unwrap();
    assert_eq!(
        decoded
            .find_case("lookup", &json!({"id": "one"}))
            .unwrap()
            .receipt_id,
        "receipt-one"
    );
    assert!(decoded.find_case("unknown", &json!({})).is_err());
    assert!(decoded.find_case("lookup", &json!({"id": "two"})).is_err());

    let hash = connector_fixture_sha256(&bytes);
    assert!(connector_fixture_hash_matches(&hash, &bytes));
    assert!(connector_fixture_hash_matches(
        &format!("sha256:{hash}"),
        &bytes
    ));
    assert!(!connector_fixture_hash_matches(
        &hash,
        b"fixture replacement"
    ));
    assert_eq!(
        connector_payload_sha256(&json!({"b": 2, "a": 1})),
        connector_payload_sha256(&json!({"a": 1, "b": 2}))
    );

    let oversized = vec![b' '; CONNECTOR_FIXTURE_MAX_BYTES + 1];
    assert_eq!(
        ConnectorFixture::from_bytes(&oversized).unwrap_err(),
        "connector_fixture_size_exceeded"
    );
}

#[test]
fn duplicate_payload_and_external_effect_are_rejected() {
    let mut duplicate = fixture();
    duplicate
        .operations
        .get_mut("lookup")
        .unwrap()
        .push(ConnectorFixtureCase {
            payload: json!({"id": "one"}),
            outcome: ProviderOutcome::Failed,
            receipt_id: "receipt-two".to_owned(),
            result: json!({"error": "duplicate"}),
            external_effect: false,
        });
    let duplicate_bytes = serde_json::to_vec(&duplicate).unwrap();
    assert_eq!(
        ConnectorFixture::from_bytes(&duplicate_bytes).unwrap_err(),
        "connector_fixture_payload_duplicate"
    );

    let mut effect = fixture();
    effect.external_effect = true;
    let effect_bytes = serde_json::to_vec(&effect).unwrap();
    assert_eq!(
        ConnectorFixture::from_bytes(&effect_bytes).unwrap_err(),
        "connector_fixture_external_effect_denied"
    );
    let mut case_effect = fixture();
    case_effect.operations.get_mut("lookup").unwrap()[0].external_effect = true;
    let case_effect_bytes = serde_json::to_vec(&case_effect).unwrap();
    assert_eq!(
        ConnectorFixture::from_bytes(&case_effect_bytes).unwrap_err(),
        "connector_fixture_external_effect_denied"
    );
}

#[test]
fn deterministic_provider_receipt_is_canonical_and_redacted() {
    let fixture = fixture();
    let binding = binding();
    let first = fixture
        .provider_receipt(&binding, "lookup", "lookup-one", &json!({"id": "one"}))
        .unwrap();
    let second = fixture
        .provider_receipt(&binding, "lookup", "lookup-one", &json!({"id": "one"}))
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.source, CONNECTOR_FIXTURE_SOURCE);
    assert_eq!(
        first.final_payload_sha256,
        connector_payload_sha256(&json!({"id": "one"}))
    );
    assert_eq!(
        fixture
            .provider_receipt(&binding, "missing", "lookup-one", &json!({}))
            .unwrap_err(),
        "connector_fixture_operation_missing"
    );
}
