//! INT-19 source and wiring guard: the connector dispatch lifecycle stays a pure domain contract,
//! the daemon journal is the only connector object that may write the EventStore, and the
//! deny-first fixtures stay wired to the manual lane and the automatic CI shards.  `include_str!`
//! assertions are source-level evidence only; they are not runtime receipts.

const DOMAIN_SOURCE: &str = include_str!("../../kiana-domain/src/connector_dispatch.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/int19_connector_dispatch.rs");
const DAEMON_SOURCE: &str = include_str!("../../kiana-daemon/src/connectors.rs");
const DAEMON_FIXTURES: &str = include_str!("../../kiana-daemon/tests/int19_connector_dispatch.rs");
const DAEMON_GUARD: &str =
    include_str!("../../kiana-daemon/tests/int19_connector_dispatch_guard.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/int19-connector-dispatch.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");
const BASELINE: &str = include_str!("../../docs/roadmap/int19-connector-dispatch-baseline.md");

#[test]
fn dispatch_and_observation_stay_separate_facts_on_the_single_spine() {
    for marker in [
        "ConnectorDispatchLifecycle",
        "ConnectorDispatchStage",
        "connector_dispatch_terminal_resurrection",
        "connector_dispatch_transition_invalid",
        "connector_dispatch_pre_effect_artifacts_forbidden",
        "connector_dispatch_observation_incomplete",
        "connector_dispatch_result_commit_incomplete",
        "connector_dispatch_unknown_evidence_invalid",
        "connector_dispatch_lifecycle_digest_mismatch",
        "connector_dispatch_lifecycle_header_invalid",
        // The evidence digests are emitted as `format!("{field}_invalid")`; the base field
        // literals below are what the source contains.
        "connector_dispatch_receipt_digest",
        "connector_dispatch_observation_digest",
        "connector_dispatch_result_digest",
        "connector_dispatch_reason_invalid",
        "connector_dispatch_observation_changed",
        "CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND",
        "CONNECTOR_DISPATCH_LIFECYCLE_SCHEMA",
        "pub fn prepared",
        "pub fn advance",
        "pub fn validate",
        "pub fn digest",
        "is_terminal",
    ] {
        assert!(
            DOMAIN_SOURCE.contains(marker),
            "INT-19 domain marker missing: {marker}"
        );
    }

    // The lifecycle is a pure value model: it cannot authorize, persist or dispatch anything.
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "ControlPlane",
        "KianaHarness",
        "tokio::spawn",
        "std::process",
        "reqwest",
    ] {
        assert!(
            !DOMAIN_SOURCE.contains(forbidden),
            "the dispatch lifecycle must not contain {forbidden}"
        );
    }

    // The daemon handler never holds the EventStore itself: the narrow journal owns connector
    // fact persistence, and the adapter/observer own neither the journal nor authorization.
    for marker in [
        "struct ConnectorEventJournal",
        "struct ConnectorRegistry {\n    journal:",
        "LocalFixtureAdapter",
        "LocalFixtureEffectObserver",
        "append_lifecycle",
        "adapter.dispatch()",
        "observer.observe",
        "result_unknown:connector_dispatch_incomplete",
        "result_unknown:connector_result_commit_missing",
        "result_unknown:connector_dispatch_terminal",
        "connector_dispatch_terminal_resurrection",
    ] {
        assert!(
            DAEMON_SOURCE.contains(marker) || DOMAIN_SOURCE.contains(marker),
            "INT-19 daemon marker missing: {marker}"
        );
    }
    let registry_start = DAEMON_SOURCE.find("struct ConnectorRegistry").unwrap();
    let journal_start = DAEMON_SOURCE.find("struct ConnectorEventJournal").unwrap();
    let journal_end = DAEMON_SOURCE[journal_start..]
        .find("impl ConnectorRegistry")
        .map(|offset| journal_start + offset)
        .expect("journal section end");
    let registry_section = &DAEMON_SOURCE[registry_start..journal_start];
    assert!(!registry_section.contains("EventStorePort"));
    assert!(
        DAEMON_SOURCE[registry_start..journal_end].contains("journal: Arc<ConnectorEventJournal>")
    );
    assert_eq!(
        DAEMON_SOURCE.matches("self.events").count(),
        2,
        "only the journal methods may touch the EventStore directly"
    );
    let adapter_start = DAEMON_SOURCE
        .find("struct LocalFixtureAdapter")
        .expect("adapter section");
    let observer_start = DAEMON_SOURCE
        .find("struct LocalFixtureEffectObserver")
        .expect("observer section");
    let observer_end = DAEMON_SOURCE[observer_start..]
        .find("fn receipt_output")
        .map(|offset| observer_start + offset)
        .expect("observer section end");
    // The section boundaries include the next item's doc comment; prose that *mentions* the
    // journal or authorization must not count as holding them.
    let strip_doc_comments = |section: &str| -> String {
        section
            .lines()
            .filter(|line| !line.trim_start().starts_with("///"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    for (label, section) in [
        ("adapter", &DAEMON_SOURCE[adapter_start..observer_start]),
        ("observer", &DAEMON_SOURCE[observer_start..observer_end]),
    ] {
        let section = strip_doc_comments(section);
        for forbidden in [
            "EventStorePort",
            "ConnectorEventJournal",
            "journal",
            "authorize",
        ] {
            assert!(
                !section.contains(forbidden),
                "the connector {label} must not hold {forbidden}"
            );
        }
    }

    // The phase facts are committed before the adapter is entered and the connector fact is
    // appended only after the terminal lifecycle was committed.
    let dispatching_commit = DAEMON_SOURCE
        .find("&dispatching, starting_version + 1")
        .expect("dispatching commit");
    let adapter = DAEMON_SOURCE
        .find("adapter.dispatch()")
        .expect("adapter dispatch");
    let observation = DAEMON_SOURCE.find("observer.observe").expect("observation");
    let observed_commit = DAEMON_SOURCE
        .find("&observed, starting_version + 2")
        .expect("observed commit");
    let terminal_commit = DAEMON_SOURCE
        .find("&lifecycle, starting_version + 3")
        .expect("terminal commit");
    let dispatch_call = DAEMON_SOURCE
        .find(".dispatch_local_fixture(")
        .expect("dispatch call");
    let fact_kind = DAEMON_SOURCE
        .find("\"connector.invoked\",")
        .expect("connector fact");
    assert!(dispatching_commit < adapter);
    assert!(adapter < observation);
    assert!(observation < observed_commit);
    assert!(observed_commit < terminal_commit);
    assert!(terminal_commit < dispatch_call);
    assert!(dispatch_call < fact_kind);
}

#[test]
fn dispatch_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards() {
    for fixture in [
        // Domain lifecycle fixtures: deny-first, digest coverage last.
        "skipped_phases_and_premature_artifacts_are_refused_before_any_dispatch",
        "late_results_cannot_resurrect_a_terminal_attempt",
        "forged_lifecycle_headers_digests_and_reasons_fail_closed",
        "lifecycle_digest_covers_every_identity_phase_and_evidence_field",
    ] {
        assert!(
            DOMAIN_FIXTURES.contains(fixture),
            "INT-19 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        // Daemon spine: ordered chain + effect-free replay, wire refusal, crash-window recovery.
        "dispatch_and_observation_facts_precede_the_connector_fact_and_replay_is_effect_free",
        "wire_supplied_lifecycle_or_observation_material_is_refused_before_any_effect",
        "an_incomplete_or_terminal_attempt_without_a_fact_is_reported_unknown_and_never_reexecutes",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "INT-19 daemon fixture missing: {fixture}"
        );
    }
    assert!(
        DAEMON_GUARD.contains("connector_dispatch_observation_and_journal_boundaries_are_explicit")
    );
    assert!(DAEMON_GUARD.contains("dispatch_refusal_codes_and_behaviour_fixtures_stay_pinned"));
    for fixtures in [DOMAIN_FIXTURES, DAEMON_FIXTURES, DAEMON_GUARD] {
        assert!(!fixtures.contains("#[ignore"));
        assert!(!fixtures.contains("ignore ="));
    }

    for step in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test int19_connector_dispatch --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test int19_connector_dispatch --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test int19_connector_dispatch_guard --locked -- --test-threads=1",
        "cargo test -p kiana-core --test int19_connector_dispatch_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(step),
            "missing INT-19 lane step: {step}"
        );
    }
    assert_eq!(MANUAL_LANE.matches("--no-run").count(), 4);
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:\n  contents: read"));
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "--ignored",
        "|| true",
        "cargo test --workspace",
    ] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "INT-19 lane must not contain {forbidden}"
        );
    }

    // Automatic coverage: the domain fixture stays in its registered chunked shard, the new core
    // guard joins the same core shard, and the daemon fixtures are picked up by the whole-crate
    // daemon shard.
    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("test shards parse");
    let shard_map = shards.as_object().expect("shard map");
    let owner = |target: &str| -> Option<String> {
        shard_map.iter().find_map(|(name, entry)| {
            let targets = entry["targets"].as_array()?;
            targets
                .iter()
                .any(|value| value.as_str() == Some(target))
                .then(|| name.clone())
        })
    };
    assert_eq!(
        owner("int19_connector_dispatch").as_deref(),
        Some("kiana-domain-s3/4"),
        "the domain fixture must stay in its registered shard"
    );
    assert_eq!(
        owner("int19_connector_dispatch_guard").as_deref(),
        Some("kiana-core-s4/6"),
        "the core guard must stay in its registered shard"
    );
    assert!(shards["kiana-daemon"]["targets"].is_null());

    assert!(BASELINE.contains("proof_level: source"));
    assert!(BASELINE.contains("connector_dispatch_terminal_resurrection"));
    assert!(BASELINE.contains("2026-10-17 completion slice (CI-only)"));
}
