#[test]
fn ui07_typed_client_keeps_control_plane_as_effect_authority() {
    let client = include_str!("../../kiana-client/src/typed.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "QueryClient",
        "FeedClient",
        "ActionClient",
        "ArtifactClient",
        "ClientRequestOptions",
        "deadline_unix_ms",
        "CancellationToken",
        "UI_ACTION_STATUS_OPERATION",
        "CommandRetryForbidden",
        "LateResponse",
        "ui_history_instance_mismatch",
        "ui_history_epoch_mismatch",
        "Accepted is intentionally returned as Accepted",
        "RequestEnvelope::command",
    ] {
        assert!(
            client.contains(marker) || protocol.contains(marker),
            "UI-07 typed-client marker missing: {marker}"
        );
    }
    assert!(!client.contains("CapabilityBroker"));
    assert!(!client.contains("ModelClient"));
    assert!(!client.contains("tokio::spawn"));
}

/// UI-07 completion slice: the request lifecycle, the deny matrix and the manual lane must stay
/// wired to the fixtures that prove them, and the typed client must never grow an execution path.
#[test]
fn ui07_typed_client_lifecycle_stays_wired_to_the_lane_and_the_auto_shards() {
    let typed = include_str!("../../kiana-client/src/typed.rs");
    let client_lib = include_str!("../../kiana-client/src/lib.rs");
    let fixtures = include_str!("../../kiana-client/tests/ui07_typed_clients.rs");
    let plan_raw = include_str!("../../kiana-client/tests/fixtures/ui07-typed-clients.json");
    let baseline = include_str!("../../docs/roadmap/ui07-typed-client-baseline.md");
    let lane = include_str!("../../.github/workflows/ui07-typed-client.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");

    // 1. deny-first classification and the listener fence stay in the client source.
    for marker in [
        "ClientError::NotInitialized",
        "ClientError::WorkspaceMismatch",
        "ClientError::UnknownSchema",
        "ClientError::DuplicateListener",
        "ClientError::ListenerInactive",
        "ClientError::CommandRetryForbidden",
        "ClientError::Cancelled",
        "ClientError::DeadlineExceeded",
        "ClientError::LateResponse",
        "record.cancellation.is_cancelled()",
        "client_request_id_mismatch",
        "ui_history_instance_mismatch",
        "ui_history_epoch_mismatch",
        "ui_artifact_page_invalid",
        "Accepted is intentionally returned as Accepted",
    ] {
        assert!(
            typed.contains(marker) || client_lib.contains(marker),
            "UI-07 typed-client fence missing: {marker}"
        );
    }
    assert!(typed.contains("cancellation: CancellationToken"));
    assert!(typed.contains("let cancellation = options.cancellation.clone();"));

    // 2. the declared plan is the source plan: every operation the facade can send is listed,
    //    and every listed operation has a stable string constant in the client source.
    let plan: serde_json::Value =
        serde_json::from_str(plan_raw).expect("UI-07 fixture plan must stay valid JSON");
    assert_eq!(plan["schema"], "kiana.ui07-typed-clients-fixture.v1");
    let operations: Vec<&str> = plan["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .map(|value| value.as_str().expect("operation name"))
        .collect();
    assert_eq!(
        operations,
        vec![
            "ui.initialize",
            "ui.query.snapshot",
            "ui.query.history",
            "ui.query.command_status",
            "ui.feed.subscribe",
            "ui.feed.resume",
            "ui.artifact.page",
            "ui.action.submit",
            "ui.action.cancel",
            "ui.action.continue",
            "ui.action.status",
        ]
    );
    for operation in &operations {
        assert!(
            typed.contains(&format!("\"{operation}\"")),
            "UI-07 operation is not declared in the typed client source: {operation}"
        );
    }
    let facades: Vec<&str> = plan["facades"]
        .as_array()
        .expect("facades")
        .iter()
        .map(|value| value.as_str().expect("facade"))
        .collect();
    assert_eq!(
        facades,
        vec![
            "QueryClient",
            "FeedClient",
            "ActionClient",
            "ArtifactClient"
        ]
    );
    for facade in &facades {
        assert!(typed.contains(facade) && client_lib.contains(facade));
    }

    // 3. the declared budgets equal the client-side literals, and the fixture assertions that
    //    check them are the same numbers (formatting-insensitive whitespace comparison).
    let typed_flat: String = typed.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(typed_flat.contains("fn default_page_size() -> usize { 64 }"));
    assert!(typed_flat.contains("fn default_history_limit() -> usize { 128 }"));
    assert!(typed.contains("self.page_size == 0 || self.page_size > 256"));
    assert!(typed.contains("self.limit == 0 || self.limit > 256"));
    assert!(typed.contains("self.limitations.len() > 32"));
    assert_eq!(plan["budgets"]["default_page_size"], 64);
    assert_eq!(plan["budgets"]["max_page_size"], 256);
    assert_eq!(plan["budgets"]["default_history_limit"], 128);
    assert_eq!(plan["budgets"]["max_history_limit"], 256);
    assert_eq!(plan["budgets"]["max_limitations"], 32);
    for assertion in [
        "assert_eq!(plan.budgets.max_page_size, 256);",
        "assert_eq!(plan.budgets.default_page_size, 64);",
        "assert_eq!(plan.budgets.default_history_limit, 128);",
        "assert_eq!(plan.budgets.max_history_limit, 256);",
        "assert_eq!(plan.budgets.max_limitations, 32);",
    ] {
        assert!(
            fixtures.contains(assertion),
            "UI-07 client fixture must compare the declared budget: {assertion}"
        );
    }

    // 4. every declared fixture exists in the client fixture file and is named in the baseline,
    //    and each required deny marker resolves to client source.
    let fixture_names: Vec<&str> = plan["fixtures"]
        .as_array()
        .expect("fixtures")
        .iter()
        .map(|value| value.as_str().expect("fixture name"))
        .collect();
    assert_eq!(fixture_names.len(), 17);
    for fixture in &fixture_names {
        assert!(
            fixtures.contains(&format!("fn {fixture}(")),
            "UI-07 client fixture missing: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "UI-07 baseline must name the client fixture: {fixture}"
        );
    }
    for marker in plan["required"].as_array().expect("required") {
        let marker = marker.as_str().expect("required marker");
        assert!(
            typed.contains(marker) || client_lib.contains(marker) || fixtures.contains(marker),
            "UI-07 required marker is not backed by client source: {marker}"
        );
    }
    for marker in plan["denied"].as_array().expect("denied") {
        assert!(!marker.as_str().expect("denied marker").trim().is_empty());
    }
    assert!(plan["limitations"]
        .as_array()
        .is_some_and(|limits| !limits.is_empty()));

    // 5. the typed client never opens a second execution path, and the fixtures stay honest.
    for source in [typed, fixtures] {
        for forbidden in [
            "CapabilityBroker",
            "KianaHarness",
            "ControlPlane::new",
            "DaemonHost",
            "EventStorePort",
            "tokio::spawn",
            ".append(",
        ] {
            assert!(
                !source.contains(forbidden),
                "UI-07 typed client must not open a second path: {forbidden}"
            );
        }
        assert!(!source.contains("#[ignore"));
        assert!(!source.contains("--ignored"));
    }
    assert!(!plan_raw.contains("#[ignore"));

    // 6. the manual lane stays dispatch-only, read-only and runs the three UI-07 targets.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(!lane.contains("  push:"));
    assert!(!lane.contains("  pull_request:"));
    assert!(lane.contains("permissions:\n  contents: read"));
    assert!(lane.contains("cargo fetch --locked"));
    assert!(lane.contains("cargo fmt --all --check"));
    for target in [
        "-p kiana-client --test ui07_typed_clients",
        "-p kiana-core --test ui07_typed_client_guard",
        "-p kiana-daemon --test ui07_typed_client_guard",
        "-p kiana-client -p kiana-core -p kiana-daemon --no-run --locked",
    ] {
        assert!(lane.contains(target), "UI-07 lane target missing: {target}");
    }

    // 7. the automatic shard ledger still runs the client/daemon crates and the core guard.
    let shards: serde_json::Value =
        serde_json::from_str(shards_raw).expect("test shard ledger must stay valid JSON");
    assert_eq!(shards["kiana-client"]["targets"], serde_json::Value::Null);
    assert_eq!(shards["kiana-daemon"]["targets"], serde_json::Value::Null);
    let mut core_targets: Vec<String> = Vec::new();
    for entry in shards.as_object().expect("shards").values() {
        if entry["crate"] == "kiana-core" {
            if let Some(targets) = entry["targets"].as_array() {
                core_targets.extend(
                    targets
                        .iter()
                        .filter_map(|value| value.as_str().map(std::borrow::ToOwned::to_owned)),
                );
            }
        }
    }
    assert!(
        core_targets
            .iter()
            .any(|target| target == "ui07_typed_client_guard"),
        "the auto shard ledger must keep running the UI-07 core guard"
    );
    assert!(baseline.contains("ui07_typed-client.yml"));
    assert!(baseline.contains("ui07-typed-clients.json"));
    assert!(baseline.contains("kiana-core-s6/6"));
}
