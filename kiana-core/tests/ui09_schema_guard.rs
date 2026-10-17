#[test]
fn ui09_schema_gate_is_data_only_and_generated_catalog_is_bound() {
    let validator = include_str!("../../scripts/validate-ui09-schemas.py");
    let lock = include_str!("../../docs/schemas/ui/schema-lock.v1.json");
    let matrix = include_str!("../../docs/schemas/ui/compatibility-matrix.v1.json");
    let catalog = include_str!("../../kiana-client/src/ui_schema_generated.rs");
    for marker in [
        "schema-lock.v1",
        "generated_rust",
        "schema_sha256",
        "unknown_field_example",
        "deprecated_fields",
        "max_bytes",
        "frozen_shape",
        "same_version_policy",
        "unknown_major_policy",
        "reject_before_decode",
        "UI_SCHEMA_CONTRACTS",
    ] {
        assert!(
            validator.contains(marker)
                || lock.contains(marker)
                || matrix.contains(marker)
                || catalog.contains(marker),
            "UI-09 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ControlPlane::new",
        "DaemonHost",
        "tokio::spawn",
        "ModelClient",
        "std::fs::",
        "reqwest",
    ] {
        assert!(
            !validator.contains(forbidden),
            "schema gate must not execute {forbidden}"
        );
        assert!(
            !catalog.contains(forbidden),
            "generated catalog must not execute {forbidden}"
        );
    }
}

fn ui09_schema_files() -> [(&'static str, &'static str); 7] {
    [
        (
            "kiana.ui-action-result.v1.schema.json",
            include_str!("../../docs/schemas/ui/kiana.ui-action-result.v1.schema.json"),
        ),
        (
            "kiana.ui-action.v1.schema.json",
            include_str!("../../docs/schemas/ui/kiana.ui-action.v1.schema.json"),
        ),
        (
            "kiana.ui-entity-store-snapshot.v1.schema.json",
            include_str!("../../docs/schemas/ui/kiana.ui-entity-store-snapshot.v1.schema.json"),
        ),
        (
            "kiana.ui-feed-frame.v1.schema.json",
            include_str!("../../docs/schemas/ui/kiana.ui-feed-frame.v1.schema.json"),
        ),
        (
            "kiana.ui-handshake-request.v1.schema.json",
            include_str!("../../docs/schemas/ui/kiana.ui-handshake-request.v1.schema.json"),
        ),
        (
            "kiana.ui-handshake-response.v1.schema.json",
            include_str!("../../docs/schemas/ui/kiana.ui-handshake-response.v1.schema.json"),
        ),
        (
            "kiana.ui-snapshot.v1.schema.json",
            include_str!("../../docs/schemas/ui/kiana.ui-snapshot.v1.schema.json"),
        ),
    ]
}

/// UI-09 completion slice: the frozen shape, the Rust drift gate, the negative fixtures, the
/// round-trip target and the manual lane must stay wired together, and the schema assets must never
/// grow an execution path of their own.
#[test]
fn ui09_shape_rust_drift_and_negative_fixtures_stay_wired_to_the_lane_and_auto_shards() {
    let validator = include_str!("../../scripts/validate-ui09-schemas.py");
    let lock_raw = include_str!("../../docs/schemas/ui/schema-lock.v1.json");
    let matrix_raw = include_str!("../../docs/schemas/ui/compatibility-matrix.v1.json");
    let round_trip = include_str!("../../kiana-client/tests/ui09_schema_round_trip.rs");
    let registry = include_str!("../../kiana-domain/src/contracts.rs");
    let ui_contracts = include_str!("../../kiana-protocol/src/ui_contracts.rs");
    let ui_store = include_str!("../../kiana-client/src/ui_store.rs");
    let baseline = include_str!("../../docs/roadmap/ui09-schema-baseline.md");
    let lane = include_str!("../../.github/workflows/ui09-schema-contract.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");

    // 1. the same-version shape gate, the Rust drift gate and the negative-case bindings stay in
    //    the read-only validator.
    for marker in [
        "schema_shape_drift",
        "rust_field_drift",
        "rust_unknown_fields_not_denied",
        "schema_optional_field_rust_required",
        "rust_required_field_schema_optional",
        "negative_case_not_rejected",
        "negative_case_rejected_for_other_reason",
        "negative_case_enforced_by_test_missing",
        "frozen_shape",
        "property_descriptor",
        "shape_inventory",
        "--write",
    ] {
        assert!(
            validator.contains(marker),
            "UI-09 validator marker missing: {marker}"
        );
    }
    // `--write` refreshes hashes and the generated catalog only: a same-version shape change keeps
    // failing until a human updates the recorded shape.
    assert!(validator.contains("``frozen_shape`` is"));
    assert!(!validator.contains("contract[\"frozen_shape\"] = "));

    // 2. every contract records a frozen shape, the schema's own inventory and its canonical hash;
    //    this file recomputes them instead of trusting the lock.
    let lock: serde_json::Value =
        serde_json::from_str(lock_raw).expect("schame lock must stay valid JSON");
    let contracts = lock["contracts"]
        .as_array()
        .expect("lock contracts must be an array");
    assert_eq!(
        contracts.len(),
        7,
        "the lock covers seven UI DTO boundaries"
    );
    let schemas = ui09_schema_files();
    let mut locked_ids = Vec::new();
    for contract in contracts {
        let id = contract["id"].as_str().expect("contract id");
        locked_ids.push(id.to_owned());
        let file = contract["schema_file"].as_str().expect("schema file");
        let schema_raw = schemas
            .iter()
            .find(|(name, _)| *name == file)
            .map(|(_, raw)| *raw)
            .expect("every locked schema file must exist");
        let schema: serde_json::Value =
            serde_json::from_str(schema_raw).expect("schema must stay valid JSON");
        assert_eq!(schema["$id"].as_str(), Some(id), "{id} $id drifted");
        assert_eq!(
            contract["schema_sha256"].as_str(),
            Some(kiana_domain::json_digest(&schema).as_str()),
            "{id} recorded hash must be the canonical digest of the checked-in schema"
        );
        let frozen = &contract["frozen_shape"];
        assert_eq!(
            frozen["deny_unknown_fields"],
            serde_json::json!(true),
            "{id} must stay a strict boundary"
        );
        let mut frozen_required = frozen["required"]
            .as_array()
            .expect("frozen required")
            .iter()
            .map(|value| value.as_str().expect("required name").to_owned())
            .collect::<Vec<_>>();
        frozen_required.sort();
        let mut schema_required = schema["required"]
            .as_array()
            .expect("schema required")
            .iter()
            .map(|value| value.as_str().expect("required name").to_owned())
            .collect::<Vec<_>>();
        schema_required.sort();
        assert_eq!(
            frozen_required, schema_required,
            "{id} frozen required fields drifted"
        );
        let frozen_properties = frozen["properties"]
            .as_object()
            .expect("frozen properties")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let mut schema_properties = schema["properties"]
            .as_object()
            .expect("schema properties")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        schema_properties.sort();
        let mut frozen_properties_sorted = frozen_properties;
        frozen_properties_sorted.sort();
        assert_eq!(
            frozen_properties_sorted, schema_properties,
            "{id} frozen property inventory drifted"
        );
        // The Rust owner is hand-written and must keep denying unknown fields.
        let rust_source = match contract["rust_source"].as_str().expect("rust source") {
            "kiana-protocol/src/ui_contracts.rs" => ui_contracts,
            "kiana-client/src/ui_store.rs" => ui_store,
            other => panic!("unexpected UI-09 rust source: {other}"),
        };
        let rust_type = contract["rust_type"].as_str().expect("rust type");
        assert!(
            rust_source.contains(&format!("pub struct {rust_type} {{")),
            "{id} Rust owner {rust_type} is missing"
        );
        assert!(
            rust_source.contains("#[serde(deny_unknown_fields)]"),
            "{id} Rust owner must deny unknown fields"
        );
    }
    assert_eq!(
        locked_ids,
        vec![
            "kiana.ui-action-result.v1",
            "kiana.ui-action.v1",
            "kiana.ui-entity-store-snapshot.v1",
            "kiana.ui-feed-frame.v1",
            "kiana.ui-handshake-request.v1",
            "kiana.ui-handshake-response.v1",
            "kiana.ui-snapshot.v1",
        ],
        "the locked UI DTO set must not silently shrink or reorder"
    );

    // 3. the canonical registry holds the locked ids, so the version gate can refuse an unknown
    //    major before any payload is decoded.
    for id in &locked_ids {
        assert!(
            registry.contains(&format!("name: \"{id}\"")),
            "{id} must stay registered in kiana-domain's canonical schema registry"
        );
        assert!(
            kiana_domain::schema_contract(id).is_some(),
            "{id} must resolve through the canonical schema registry"
        );
        assert!(
            kiana_domain::check_schema_compatibility(id, &kiana_domain::SchemaVersion::new(1, 0))
                .is_ok(),
            "{id} locked version must stay compatible"
        );
        assert!(
            kiana_domain::check_schema_compatibility(id, &kiana_domain::SchemaVersion::new(2, 0))
                .is_err(),
            "{id} unknown major must fail closed"
        );
    }

    // 4. the compatibility matrix keeps the four negative cases bound to a fixture or to the
    //    runtime test that proves them.
    let matrix: serde_json::Value =
        serde_json::from_str(matrix_raw).expect("compatibility matrix must stay valid JSON");
    assert_eq!(matrix["same_version_policy"].as_str(), Some("frozen_shape"));
    assert_eq!(
        matrix["unknown_major_policy"].as_str(),
        Some("reject_before_decode")
    );
    assert_eq!(
        matrix["unknown_command_policy"].as_str(),
        Some("reject_with_schema_unsupported")
    );
    let cases = matrix["negative_cases"]
        .as_array()
        .expect("negative cases must be an array");
    let case_ids = cases
        .iter()
        .map(|case| case["case_id"].as_str().expect("case id").to_owned())
        .collect::<Vec<_>>();
    for case_id in [
        "remove_required_field",
        "change_field_type",
        "unknown_major",
        "unknown_command",
    ] {
        assert!(
            case_ids.contains(&case_id.to_owned()),
            "the matrix must keep the negative case: {case_id}"
        );
    }
    for case in cases {
        let case_id = case["case_id"].as_str().expect("case id");
        if let Some(fixture) = case["fixture"].as_str() {
            assert!(
                fixture.starts_with("negative/"),
                "{case_id} must point at a negative fixture"
            );
            assert!(
                !case["expected_error"].as_str().unwrap_or("").is_empty(),
                "{case_id} must record the reason it is refused"
            );
        } else {
            assert_eq!(
                case["enforced_by"].as_str(),
                Some("kiana-client/tests/ui09_schema_round_trip.rs"),
                "{case_id} must be enforced by the round-trip target"
            );
            assert_eq!(
                case["enforced_by_test"].as_str(),
                Some("ui09_version_and_command_gates_fail_closed_before_decode"),
                "{case_id} must name the runtime test that proves it"
            );
        }
    }

    // 5. the round-trip target stays a pure typed value test bound to the locked fixtures.
    for marker in [
        "ui09_locked_examples_round_trip_through_the_rust_wire_types",
        "ui09_unknown_field_examples_are_rejected_by_strict_rust_decode",
        "ui09_version_and_command_gates_fail_closed_before_decode",
        "ui09_lock_contracts_are_registered_and_negative_fixtures_stay_refused",
        "docs/schemas/ui/examples/kiana.ui-action.v1.json",
        "docs/schemas/ui/examples/kiana.ui-snapshot.v1.json",
        "docs/schemas/ui/negative/kiana.ui-action.v1.remove-required-field.json",
        "docs/schemas/ui/negative/kiana.ui-action.v1.change-field-type.json",
        "docs/schemas/ui/negative/kiana.ui-action.v1.unknown-major.json",
        "ui_feed_cursor_digest_mismatch",
        "ui_store_snapshot_digest_mismatch",
        "ui_action_schema_invalid",
    ] {
        assert!(
            round_trip.contains(marker),
            "UI-09 round-trip target is missing: {marker}"
        );
    }
    for forbidden in [
        "DaemonHost",
        "CapabilityBroker",
        "ControlPlane",
        "std::fs::",
        "tokio::spawn",
        "reqwest",
        "#[ignore",
    ] {
        assert!(
            !round_trip.contains(forbidden),
            "the round-trip target must not use {forbidden}"
        );
    }

    // 6. the baseline document keeps the completion slice and its limits.
    for marker in [
        "frozen_shape",
        "rust_field_drift",
        "negative_case_not_rejected",
        "ui09_schema_round_trip.rs",
        "proof_level",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-09 baseline is missing: {marker}"
        );
    }

    // 7. the manual lane stays dispatch-only, read-only and bound to the same targets.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:"));
    assert!(lane.contains("contents: read"));
    for forbidden in [
        "pull_request",
        "  push:",
        "schedule:",
        "continue-on-error",
        "|| true",
        "--ignored",
    ] {
        assert!(
            !lane.contains(forbidden),
            "UI-09 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "python3 scripts/validate-ui09-schemas.py",
        "cargo test -p kiana-client --test ui09_schema_catalog --locked -- --test-threads=1",
        "cargo test -p kiana-client --test ui09_schema_round_trip --locked -- --test-threads=1",
        "cargo test -p kiana-core --test ui09_schema_guard --locked -- --test-threads=1",
        "cargo test -p kiana-client -p kiana-core --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-09 lane is missing: {command}");
    }

    // 8. the automatic shard book stays the authority for the crate lanes.
    let shards: serde_json::Value =
        serde_json::from_str(shards_raw).expect("test shards must stay valid JSON");
    let core_shard = shards["kiana-core-s6/6"]["targets"]
        .as_array()
        .expect("kiana-core-s6/6 targets");
    assert!(
        core_shard
            .iter()
            .any(|value| value.as_str() == Some("ui09_schema_guard")),
        "the automatic lane must keep ui09_schema_guard"
    );
    for crate_name in ["kiana-client", "kiana-protocol", "kiana-domain"] {
        assert!(
            shards[crate_name]["targets"].is_null(),
            "{crate_name} must stay a whole-crate shard so new UI-09 targets are discovered"
        );
    }
}
