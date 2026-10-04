//! CI-only source guards for the CO-06 Company persistence boundary.
//!
//! These guards intentionally do not claim cross-store atomicity. They pin the ordering that
//! prevents a denied state transition from publishing a blob and keeps EventLog as the only fact
//! source while an explicitly injected ArtifactStorePort holds immutable bytes.

#[test]
fn artifact_publication_is_after_transition_and_before_company_fact_commit() {
    let source = include_str!("../src/company.rs");
    let transition = source
        .find("let next = match state.transition(&request.command, &authority, &proof)")
        .expect("Company transition boundary");
    let persist = source
        .find("self.persist_company_artifact(&mut proof).await")
        .expect("artifact persistence boundary");
    let commit = source
        .find("self.commit_company(&context, event).await")
        .expect("Company EventLog commit boundary");
    assert!(
        transition < persist,
        "denied transitions must not publish blobs"
    );
    assert!(
        persist < commit,
        "Company facts must reference staged bytes"
    );
}

#[test]
fn artifact_publication_requires_atomic_event_store_before_stage() {
    let source = include_str!("../src/company.rs");
    let transition = source
        .find("let next = match state.transition(&request.command, &authority, &proof)")
        .expect("Company transition boundary");
    let atomicity_gate = source
        .find("company_artifact_event_atomicity_unsupported")
        .expect("artifact/EventLog atomicity deny boundary");
    let persist = source
        .find("self.persist_company_artifact(&mut proof).await")
        .expect("artifact persistence boundary");
    assert!(
        transition < atomicity_gate && atomicity_gate < persist,
        "artifact publication must be denied before staging when EventLog atomic transitions are unavailable"
    );
}

#[test]
fn artifact_event_commit_failure_is_fenced_as_result_unknown() {
    let source = include_str!("../src/company.rs");
    assert!(source.contains("let artifact_persisted = self.artifact_store.is_some()"));
    assert!(source.contains("Err(_) if artifact_persisted"));
    assert!(source.contains("result_unknown:company_artifact_event_commit_unconfirmed"));
}

#[test]
fn artifact_store_is_explicitly_injected_and_historical_reads_use_the_port() {
    let control_plane = include_str!("../src/lib.rs");
    let constructors = include_str!("../src/commands.rs");
    let company = include_str!("../src/company.rs");
    assert!(control_plane.contains("artifact_store: Option<Arc<dyn ArtifactStorePort>>"));
    assert!(constructors.contains("pub fn with_artifact_store"));
    assert!(company.contains("store.read_artifact(&reference).await"));
    assert!(company.contains("stage_artifact_version(version, content).await"));
}
