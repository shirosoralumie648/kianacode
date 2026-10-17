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
fn fixture_schema_bounds_and_unknown_fields_fail_closed() {
    let mut too_many_operations = fixture();
    let template = too_many_operations.operations["lookup"][0].clone();
    too_many_operations.operations.clear();
    for index in 0..=CONNECTOR_FIXTURE_MAX_OPERATIONS {
        too_many_operations
            .operations
            .insert(format!("operation-{index}"), vec![template.clone()]);
    }
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&too_many_operations).unwrap())
            .unwrap_err(),
        "connector_fixture_operation_limit"
    );

    let mut too_many_cases = fixture();
    {
        let cases = too_many_cases.operations.get_mut("lookup").unwrap();
        let template = cases[0].clone();
        for index in 0..=CONNECTOR_FIXTURE_MAX_CASES_PER_OPERATION {
            let mut case = template.clone();
            case.payload = json!({"id": format!("case-{index}")});
            case.receipt_id = format!("receipt-{index}");
            cases.push(case);
        }
    }
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&too_many_cases).unwrap()).unwrap_err(),
        "connector_fixture_case_limit"
    );

    let mut empty_operations = fixture();
    empty_operations.operations.clear();
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&empty_operations).unwrap()).unwrap_err(),
        "connector_fixture_operation_limit"
    );

    let mut empty_cases = fixture();
    empty_cases.operations.get_mut("lookup").unwrap().clear();
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&empty_cases).unwrap()).unwrap_err(),
        "connector_fixture_case_limit"
    );

    // Unknown fields are rejected at both levels instead of widening the accepted schema.
    let mut unknown_field = serde_json::to_value(fixture()).unwrap();
    unknown_field["extra"] = json!(true);
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&unknown_field).unwrap()).unwrap_err(),
        "connector_fixture_invalid"
    );
    let mut unknown_case_field = serde_json::to_value(fixture()).unwrap();
    unknown_case_field["operations"]["lookup"][0]["extra"] = json!(true);
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&unknown_case_field).unwrap())
            .unwrap_err(),
        "connector_fixture_invalid"
    );

    let mut unsupported_schema = fixture();
    unsupported_schema.schema = "kiana.connector-fixture.v2".to_owned();
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&unsupported_schema).unwrap())
            .unwrap_err(),
        "connector_fixture_schema_unsupported"
    );

    let mut malformed_identity = fixture();
    malformed_identity.connector_id = "bad connector id".to_owned();
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&malformed_identity).unwrap())
            .unwrap_err(),
        "connector_fixture_identity_mismatch"
    );

    let mut malformed_receipt = fixture();
    malformed_receipt.operations.get_mut("lookup").unwrap()[0].receipt_id =
        "bad/receipt".to_owned();
    assert_eq!(
        ConnectorFixture::from_bytes(&serde_json::to_vec(&malformed_receipt).unwrap()).unwrap_err(),
        "connector_fixture_invalid"
    );

    // An operation that the definition does not register cannot be smuggled in by the fixture.
    let mut unregistered = fixture();
    let lookup_cases = unregistered.operations["lookup"].clone();
    unregistered
        .operations
        .insert("extra".to_owned(), lookup_cases);
    assert_eq!(
        unregistered.validate_for_binding(&binding()).unwrap_err(),
        "connector_fixture_operation_unregistered"
    );
}

#[test]
fn fixture_replacement_and_hash_spelling_are_pinned() {
    let bytes = serde_json::to_vec(&fixture()).unwrap();
    let pinned = connector_fixture_sha256(&bytes);
    assert!(connector_fixture_hash_valid(&pinned));
    assert!(connector_fixture_hash_valid(&format!("sha256:{pinned}")));
    assert!(!connector_fixture_hash_valid("sha256:not-a-digest"));
    assert!(!connector_fixture_hash_valid(
        pinned.to_uppercase().as_str()
    ));

    let mut replaced = fixture();
    replaced.operations.get_mut("lookup").unwrap()[0].result = json!({"value": "replaced"});
    let replaced_bytes = serde_json::to_vec(&replaced).unwrap();
    assert_ne!(bytes, replaced_bytes);
    assert!(!connector_fixture_hash_matches(&pinned, &replaced_bytes));
    assert!(!connector_fixture_hash_matches(
        &format!("sha256:{pinned}"),
        &replaced_bytes
    ));
    assert!(connector_fixture_hash_matches(&pinned, &bytes));
}

#[test]
fn fixture_receipts_are_byte_stable_for_every_outcome() {
    let mut fixture = fixture();
    fixture.operations.get_mut("lookup").unwrap().extend([
        ConnectorFixtureCase {
            payload: json!({"id": "two"}),
            outcome: ProviderOutcome::Failed,
            receipt_id: "receipt-two".to_owned(),
            result: json!({"detail": "fixture failure"}),
            external_effect: false,
        },
        ConnectorFixtureCase {
            payload: json!({"id": "three"}),
            outcome: ProviderOutcome::Unknown,
            receipt_id: "receipt-three".to_owned(),
            result: json!({"detail": "fixture unknown"}),
            external_effect: false,
        },
        ConnectorFixtureCase {
            payload: json!({"a": 1, "b": 2}),
            outcome: ProviderOutcome::Succeeded,
            receipt_id: "receipt-canonical".to_owned(),
            result: json!({"value": "canonical"}),
            external_effect: false,
        },
    ]);
    let binding = binding();
    fixture.validate_for_binding(&binding).unwrap();

    // Object-key order is canonicalized before matching, so both spellings select the same case
    // and produce byte-identical receipts.
    let first = fixture
        .provider_receipt(&binding, "lookup", "lookup-one", &json!({"b": 2, "a": 1}))
        .unwrap();
    let second = fixture
        .provider_receipt(&binding, "lookup", "lookup-one", &json!({"a": 1, "b": 2}))
        .unwrap();
    assert_eq!(first.provider_receipt_id, "receipt-canonical");
    assert_eq!(first, second);
    assert_eq!(
        serde_json::to_vec(&first).unwrap(),
        serde_json::to_vec(&second).unwrap()
    );
    assert_eq!(first.outcome, ProviderOutcome::Succeeded);

    for (payload, receipt_id, outcome) in [
        (json!({"id": "two"}), "receipt-two", ProviderOutcome::Failed),
        (
            json!({"id": "three"}),
            "receipt-three",
            ProviderOutcome::Unknown,
        ),
    ] {
        let receipt = fixture
            .provider_receipt(&binding, "lookup", "lookup-key", &payload)
            .unwrap();
        let repeated = fixture
            .provider_receipt(&binding, "lookup", "lookup-key", &payload)
            .unwrap();
        assert_eq!(receipt, repeated);
        assert_eq!(
            serde_json::to_vec(&receipt).unwrap(),
            serde_json::to_vec(&repeated).unwrap()
        );
        assert_eq!(receipt.outcome, outcome);
        assert_eq!(receipt.provider_receipt_id, receipt_id);
        assert_eq!(receipt.source, CONNECTOR_FIXTURE_SOURCE);
        assert_eq!(receipt.idempotency_key, "lookup-key");
        assert_eq!(
            receipt.final_payload_sha256,
            connector_payload_sha256(&payload)
        );
    }
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
    let mut missing_fixture_binding = binding.clone();
    missing_fixture_binding.definition.operations.insert(
        "missing".to_owned(),
        binding.definition.operations["lookup"].clone(),
    );
    assert_eq!(
        fixture
            .provider_receipt(
                &missing_fixture_binding,
                "missing",
                "lookup-one",
                &json!({})
            )
            .unwrap_err(),
        "connector_fixture_operation_missing"
    );
}
