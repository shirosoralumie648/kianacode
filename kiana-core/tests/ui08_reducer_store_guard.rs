#[test]
fn ui08_store_is_a_pure_bounded_projection() {
    let store = include_str!("../../kiana-client/src/ui_store.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    for marker in [
        "UiEntityStore",
        "UiEntityKind",
        "UiEntityLifecycle",
        "UiStoreScope",
        "UiStoreEvent",
        "UiStoreGapReason",
        "UiOptimisticUpdate",
        "UiStoreChange",
        "reduce(&self",
        "dehydrate",
        "hydrate",
        "CacheFullProtected",
        "StaleRevision",
        "ScopeMismatch",
        "needs_snapshot",
        "unknown_commands",
        "optimistic",
        "ensure_capacity",
        "ui_store_optimistic_entity_missing",
        "ui_store_optimistic_entity_mismatch",
        "previous.workspace",
    ] {
        assert!(
            store.contains(marker) || client.contains(marker),
            "UI-08 reducer marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ControlPlane::new",
        "DaemonHost",
        "tokio::spawn",
        "ModelClient",
        "std::fs::",
    ] {
        assert!(
            !store.contains(forbidden),
            "UI-08 reducer must not contain {forbidden}"
        );
    }
}

/// UI-08 completion slice: the declarative plan, the replayed reducer trace, the runtime fixtures
/// and the manual lane must stay wired together, and the store must never grow an execution path.
#[test]
fn ui08_reducer_plan_and_trace_stay_wired_to_the_lane_and_the_auto_shards() {
    let store = include_str!("../../kiana-client/src/ui_store.rs");
    let client_lib = include_str!("../../kiana-client/src/lib.rs");
    let fixtures = include_str!("../../kiana-client/tests/ui08_reducer_store.rs");
    let plan_raw = include_str!("../../kiana-client/tests/fixtures/ui08-reducer-store.json");
    let entrypoints_guard =
        include_str!("../../kiana-entrypoints/tests/ui08_store_boundary_guard.rs");
    let entrypoints_lib = include_str!("../../kiana-entrypoints/src/lib.rs");
    let daemon_fixture = include_str!("../../kiana-daemon/tests/ui08_entity_store.rs");
    let baseline = include_str!("../../docs/roadmap/ui08-reducer-store-baseline.md");
    let lane = include_str!("../../.github/workflows/ui08-reducer-store.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");

    // 1. the fences of the completion slice stay in the reducer source, and the store keeps no
    //    authority of its own.
    for marker in [
        "ui_feed_entity_kind_missing",
        "ui_feed_draft_kind_forbidden",
        "ui_feed_entity_revision_invalid",
        "ui_store_snapshot_draft_entity",
        "PendingConflict",
        "adopt_authority_epoch",
        "refresh_rollback_baselines",
        "gap_transition",
        "entities_of_kind",
        "count_of_kind",
        "initial_boundary",
        "replay_at_position",
        "ui_store_optimistic_entity_missing",
        "ui_store_optimistic_entity_mismatch",
        "UI_ENTITY_STORE_MAX_ENTITIES: usize = 4_096;",
        "UI_ENTITY_STORE_MAX_EVENTS: usize = 8_192;",
        "UI_ENTITY_STORE_MAX_UNKNOWN: usize = 512;",
    ] {
        assert!(
            store.contains(marker) || client_lib.contains(marker),
            "UI-08 fence marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ControlPlane::new",
        "DaemonHost",
        "tokio::spawn",
        "ModelClient",
        "std::fs::",
        ".append(",
        "EventStorePort",
        "KianaHarness",
    ] {
        assert!(
            !store.contains(forbidden),
            "UI-08 reducer must not contain {forbidden}"
        );
    }

    // 2. the declared plan is the source plan: kinds, lifecycle names, gap reasons and budgets.
    let plan: serde_json::Value =
        serde_json::from_str(plan_raw).expect("UI-08 plan must stay valid JSON");
    assert_eq!(plan["schema"], "kiana.ui08-reducer-store-fixture.v1");
    assert_eq!(plan["step"], "UI-08");
    let kinds: Vec<&str> = plan["entity_kinds"]
        .as_array()
        .expect("entity_kinds")
        .iter()
        .map(|value| value.as_str().expect("entity kind"))
        .collect();
    assert_eq!(
        kinds,
        vec![
            "run",
            "connection",
            "submission",
            "draft",
            "inbox",
            "artifact"
        ]
    );
    let lifecycles: Vec<&str> = plan["lifecycles"]
        .as_array()
        .expect("lifecycles")
        .iter()
        .map(|value| value.as_str().expect("lifecycle"))
        .collect();
    assert_eq!(
        lifecycles,
        vec!["authoritative", "optimistic_pending", "unknown"]
    );
    let reasons: Vec<&str> = plan["gap_reasons"]
        .as_array()
        .expect("gap_reasons")
        .iter()
        .map(|value| value.as_str().expect("gap reason"))
        .collect();
    assert_eq!(
        reasons,
        vec![
            "sequence_gap",
            "feed_gap",
            "epoch_reset",
            "snapshot_required",
            "pending_conflict"
        ]
    );
    assert_eq!(plan["budgets"]["max_entities"].as_u64(), Some(4_096));
    assert_eq!(plan["budgets"]["max_events"].as_u64(), Some(8_192));
    assert_eq!(plan["budgets"]["max_unknown"].as_u64(), Some(512));
    assert!(!plan["denied"].as_array().expect("denied").is_empty());
    assert!(!plan["limitations"]
        .as_array()
        .expect("limitations")
        .is_empty());

    // 3. every declared fixture is a real test function in the client fixture file, and every
    //    declared trace operation is handled there.
    let declared: Vec<&str> = plan["fixtures"]
        .as_array()
        .expect("fixtures")
        .iter()
        .map(|value| value.as_str().expect("fixture name"))
        .collect();
    assert!(
        declared.len() >= 17,
        "the completion slice must keep every named fixture"
    );
    for fixture in &declared {
        assert!(
            fixtures.contains(&format!("fn {fixture}(")),
            "UI-08 fixture is not defined in kiana-client/tests/ui08_reducer_store.rs: {fixture}"
        );
    }
    for fixture in [
        "live_feed_starts_at_the_initial_boundary_and_markers_never_regress",
        "replayed_terminal_after_the_boundary_is_not_a_second_item",
        "epoch_reset_drops_authoritative_state_and_keeps_local_pending",
        "generic_feed_requires_an_explicit_kind_and_refuses_the_draft_namespace",
        "pending_conflict_never_lets_an_optimistic_value_hide_a_newer_revision",
        "snapshot_merge_refreshes_the_rollback_baseline_instead_of_a_stale_value",
        "hydrate_refuses_a_forged_draft_entity",
        "a_stale_rejection_never_deletes_an_authoritative_entity",
        "kind_scoped_views_keep_the_six_namespaces_separate",
        "ui08_reducer_plan_and_trace_replay_deterministically",
    ] {
        assert!(
            declared.contains(&fixture),
            "the plan must keep the completion-slice fixture: {fixture}"
        );
    }
    let steps = plan["trace"]["steps"].as_array().expect("trace steps");
    assert!(
        steps.len() >= 30,
        "the reducer trace must keep the deny-first sequence"
    );
    for step in steps {
        let op = step["op"].as_str().expect("trace op");
        assert!(
            fixtures.contains(&format!("\"{op}\"")),
            "the trace uses an operation the fixture does not handle: {op}"
        );
    }
    let trace_raw = serde_json::to_string(&plan["trace"]).expect("trace encodes");
    assert!(trace_raw.contains("pending_conflict"));
    assert!(trace_raw.contains("ui_store_duplicate_event"));
    assert!(trace_raw.contains("ui_feed_draft_kind_forbidden"));
    assert!(trace_raw.contains("ui_store_gap_required"));
    assert!(trace_raw.contains("ui_store_stale_revision"));

    // 4. the reducer stays a projection: no local test is disabled and no second store exists.
    assert!(!store.contains("#[ignore"));
    assert!(!fixtures.contains("#[ignore"));
    assert!(!entrypoints_lib.contains("UiEntityStore"));
    assert!(!entrypoints_lib.contains("UiStoreEvent"));
    assert!(!entrypoints_lib.contains("fn ensure_capacity"));
    assert!(entrypoints_guard.contains("UiEntityStore"));
    assert!(entrypoints_guard.contains("ui_store_snapshot_draft_entity"));
    assert!(entrypoints_guard.contains("ui_feed_entity_kind_missing"));
    assert!(!daemon_fixture.contains("EventStorePort"));
    assert!(!daemon_fixture.contains("CapabilityBroker"));
    assert!(daemon_fixture.contains("ui_feed_entity_kind_missing"));
    assert!(daemon_fixture.contains("persisted_events"));

    // 5. the baseline document keeps the state graph and the fixture matrix.
    for marker in [
        "UiEntityStore",
        "PendingConflict",
        "ui_store_snapshot_draft_entity",
        "live_feed_starts_at_the_initial_boundary_and_markers_never_regress",
        "ui08_reducer_plan_and_trace_replay_deterministically",
        "ui08_entity_store.rs",
        "presenter",
        "proof_level",
    ] {
        assert!(
            baseline.contains(marker),
            "UI-08 baseline is missing: {marker}"
        );
    }

    // 6. the manual lane stays dispatch-only, read-only and bound to the same targets.
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
            "UI-08 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-client --test ui08_reducer_store --locked -- --test-threads=1",
        "cargo test -p kiana-core --test ui08_reducer_store_guard --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui08_store_boundary_guard --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test ui08_entity_store --locked -- --test-threads=1",
        "cargo test -p kiana-client -p kiana-core -p kiana-entrypoints -p kiana-daemon --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-08 lane is missing: {command}");
    }

    // 7. the automatic shard book still covers the core guard and the whole-crate shards.
    let shards: serde_json::Value =
        serde_json::from_str(shards_raw).expect("test shards must stay valid JSON");
    let core_shard = shards["kiana-core-s6/6"]["targets"]
        .as_array()
        .expect("kiana-core-s6/6 targets");
    assert!(
        core_shard
            .iter()
            .any(|value| value.as_str() == Some("ui08_reducer_store_guard")),
        "the automatic lane must keep ui08_reducer_store_guard"
    );
    for crate_name in ["kiana-client", "kiana-daemon", "kiana-entrypoints"] {
        assert!(
            shards[crate_name]["targets"].is_null(),
            "{crate_name} must stay a whole-crate shard so new UI-08 targets are discovered"
        );
    }
}
