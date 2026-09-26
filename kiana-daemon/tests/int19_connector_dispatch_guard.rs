#[test]
fn connector_dispatch_observation_and_journal_boundaries_are_explicit() {
    let daemon = include_str!("../src/connectors.rs");
    let domain = include_str!("../../kiana-domain/src/connector_dispatch.rs");

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
        "append_lifecycle",
        "adapter.dispatch()",
        "observer.observe",
    ] {
        assert!(
            daemon.contains(marker) || domain.contains(marker),
            "INT-19 marker missing: {marker}"
        );
    }

    assert!(daemon.contains("struct ConnectorRegistry {\n    journal:"));
    assert!(daemon.contains("struct ConnectorEventJournal {\n    events: Arc<dyn EventStorePort>"));
    assert!(domain.contains("pub const CONNECTOR_DISPATCH_LIFECYCLE_EVENT_KIND"));
    assert!(domain.contains("connector_dispatch_pre_effect_artifacts_forbidden"));

    let prepared = daemon
        .find("ConnectorDispatchLifecycle::prepared")
        .expect("prepared lifecycle");
    let dispatching_commit = daemon
        .find("&dispatching, starting_version + 1")
        .expect("dispatching commit");
    let adapter = daemon.find("adapter.dispatch()").expect("adapter dispatch");
    let observation = daemon.find("observer.observe").expect("observation");
    let observed_commit = daemon
        .find("&observed, starting_version + 2")
        .expect("observed commit");
    let terminal_commit = daemon
        .find("&lifecycle, starting_version + 3")
        .expect("terminal commit");
    assert!(prepared < dispatching_commit);
    assert!(dispatching_commit < adapter);
    assert!(adapter < observation);
    assert!(observation < observed_commit);
    assert!(observed_commit < terminal_commit);

    let registry_start = daemon.find("struct ConnectorRegistry").unwrap();
    let journal_start = daemon.find("struct ConnectorEventJournal").unwrap();
    assert!(registry_start < journal_start);
    let registry_section = &daemon[registry_start..journal_start];
    assert!(!registry_section.contains("EventStorePort"));
    assert!(!domain.contains("EventStorePort"));
    assert!(!domain.contains("CapabilityBroker"));
}
