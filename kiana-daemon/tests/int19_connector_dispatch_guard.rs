//! INT-19 source guard: the connector capability handler never authorizes an effect or writes the
//! EventLog itself, and the dispatch chain commits `prepared → dispatching → observed → terminal`
//! before the single `connector.invoked` fact.  `include_str!` assertions are source-level
//! evidence only; they are not runtime receipts.

const DAEMON: &str = include_str!("../src/connectors.rs");
const DOMAIN: &str = include_str!("../../kiana-domain/src/connector_dispatch.rs");
const DOMAIN_FIXTURES: &str = include_str!("../../kiana-domain/tests/int19_connector_dispatch.rs");
const DAEMON_FIXTURES: &str = include_str!("int19_connector_dispatch.rs");

#[test]
fn connector_dispatch_observation_and_journal_boundaries_are_explicit() {
    for marker in [
        "ConnectorEventJournal",
        "ConnectorDispatchLifecycle",
        "ConnectorDispatchStage",
        "LocalFixtureAdapter",
        "LocalFixtureEffectObserver",
        "ConnectorDispatchStage::Prepared",
        "ConnectorDispatchStage::Dispatching",
        "ConnectorDispatchStage::Observed",
        "ConnectorDispatchStage::ResultCommitted",
        "connector_dispatch_terminal_resurrection",
        "result_unknown:connector_dispatch_incomplete",
        "result_unknown:connector_result_commit_missing",
        "result_unknown:connector_dispatch_terminal",
        "append_lifecycle",
        "adapter.dispatch()",
        "observer.observe",
    ] {
        assert!(
            DAEMON.contains(marker) || DOMAIN.contains(marker),
            "INT-19 marker missing: {marker}"
        );
    }

    for marker in [
        "connector_dispatch_transition_invalid",
        "connector_dispatch_pre_effect_artifacts_forbidden",
        "connector_dispatch_observation_incomplete",
        "connector_dispatch_result_commit_incomplete",
        "connector_dispatch_unknown_evidence_invalid",
        "connector_dispatch_lifecycle_digest_mismatch",
        "connector_dispatch_lifecycle_header_invalid",
        "connector_dispatch_reason_invalid",
        "connector_dispatch_observation_changed",
    ] {
        assert!(
            DOMAIN.contains(marker),
            "INT-19 domain refusal missing: {marker}"
        );
    }

    assert!(DAEMON.contains("struct ConnectorRegistry {\n    journal:"));
    assert!(DAEMON.contains("struct ConnectorEventJournal {\n    events: Arc<dyn EventStorePort>"));
    assert!(DOMAIN.contains("pub const CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND"));
    assert!(DOMAIN.contains("connector_dispatch_pre_effect_artifacts_forbidden"));

    // The journal is the only object in the connector module that holds the EventStore, and the
    // adapter/observer hold neither the journal nor any authorization authority.
    let adapter_start = DAEMON
        .find("struct LocalFixtureAdapter")
        .expect("local fixture adapter");
    let observer_start = DAEMON
        .find("struct LocalFixtureEffectObserver")
        .expect("local fixture observer");
    let observer_end = DAEMON[observer_start..]
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
        ("adapter", &DAEMON[adapter_start..observer_start]),
        ("observer", &DAEMON[observer_start..observer_end]),
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
                "the {label} must not hold {forbidden}"
            );
        }
    }
    let registry_start = DAEMON.find("struct ConnectorRegistry").unwrap();
    let journal_start = DAEMON.find("struct ConnectorEventJournal").unwrap();
    assert!(registry_start < journal_start);
    let registry_section = &DAEMON[registry_start..journal_start];
    assert!(!registry_section.contains("EventStorePort"));
    assert_eq!(
        DAEMON.matches("self.events").count(),
        2,
        "only the journal methods may touch the EventStore directly"
    );
    assert!(!DOMAIN.contains("EventStorePort"));
    assert!(!DOMAIN.contains("CapabilityBroker"));

    // The phase facts are committed before the adapter is entered, the observation fact is
    // committed before the terminal fact, and the connector fact is appended only after the
    // dispatch outcome (which already carries the terminal lifecycle) is returned.
    let prepared = DAEMON
        .find("ConnectorDispatchLifecycle::prepared")
        .expect("prepared lifecycle");
    let dispatching_commit = DAEMON
        .find("&dispatching, starting_version + 1")
        .expect("dispatching commit");
    let adapter = DAEMON.find("adapter.dispatch()").expect("adapter dispatch");
    let observation = DAEMON.find("observer.observe").expect("observation");
    let observed_commit = DAEMON
        .find("&observed, starting_version + 2")
        .expect("observed commit");
    let terminal_commit = DAEMON
        .find("&lifecycle, starting_version + 3")
        .expect("terminal commit");
    let dispatch_call = DAEMON
        .find(".dispatch_local_fixture(")
        .expect("dispatch call");
    let fact_kind = DAEMON
        .find("\"connector.invoked\",")
        .expect("connector fact");
    assert!(prepared < dispatching_commit);
    assert!(dispatching_commit < adapter);
    assert!(adapter < observation);
    assert!(observation < observed_commit);
    assert!(observed_commit < terminal_commit);
    assert!(terminal_commit < dispatch_call);
    assert!(dispatch_call < fact_kind);
}

#[test]
fn dispatch_refusal_codes_and_behaviour_fixtures_stay_pinned() {
    for fixture in [
        "dispatch_and_observation_facts_precede_the_connector_fact_and_replay_is_effect_free",
        "wire_supplied_lifecycle_or_observation_material_is_refused_before_any_effect",
        "an_incomplete_or_terminal_attempt_without_a_fact_is_reported_unknown_and_never_reexecutes",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(fixture),
            "INT-19 daemon fixture missing: {fixture}"
        );
    }
    for fixture in [
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
    for fixtures in [DAEMON_FIXTURES, DOMAIN_FIXTURES] {
        assert!(!fixtures.contains(concat!("#[", "ignore")));
        assert!(!fixtures.contains(concat!("ignore", " =")));
    }
    assert!(DAEMON_FIXTURES.contains("result_unknown:connector_dispatch_incomplete"));
    assert!(DAEMON_FIXTURES.contains("result_unknown:connector_result_commit_missing"));
    assert!(DAEMON_FIXTURES.contains("result_unknown:connector_dispatch_terminal"));
    assert!(DAEMON_FIXTURES.contains("CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND"));
    assert!(DAEMON_FIXTURES.contains("EventStorePort"));
}
