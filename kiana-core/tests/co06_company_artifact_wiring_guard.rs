//! CI-only source guards for the CO-06 Company persistence boundary.
//!
//! These guards intentionally do not claim cross-store atomicity. They pin the ordering that
//! prevents a denied state transition from publishing a blob, that a Company fact may only cite a
//! manifest correlated with the staged bytes and readable back after commit, and that EventLog
//! stays the only fact source while an explicitly injected ArtifactStorePort holds immutable bytes.

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

#[test]
fn artifact_publication_verifies_staged_manifest_and_readback_before_fact() {
    let source = include_str!("../src/company.rs");
    let persist_start = source
        .find("async fn persist_company_artifact")
        .expect("artifact persistence boundary");
    let persist = &source[persist_start..];
    let stage = persist
        .find("store.stage_artifact_version(version, content).await?")
        .expect("artifact stage boundary");
    let manifest_fence = persist
        .find("company_artifact_staged_manifest_mismatch")
        .expect("staged manifest correlation fence");
    let commit = persist
        .find(".commit_artifact(persisted.as_ref(), Some(persisted.version))")
        .expect("artifact commit boundary");
    let readback = persist
        .find("company_artifact_committed_blob_unverified")
        .expect("committed blob verification fence");
    assert!(
        stage < manifest_fence && manifest_fence < commit && commit < readback,
        "the admitted manifest must correlate with the staged bytes before commit, and the committed blob must be readable back, before a Company fact may cite it"
    );
    assert!(persist.contains(".read_artifact(&persisted.as_ref())"));
}

#[test]
fn artifact_publication_window_is_recorded_before_the_fact_and_resolved_after() {
    let source = include_str!("../src/company.rs");
    let record = source
        .find(".record_artifact_publication_intent(&command_id")
        .expect("publication intent boundary");
    let persist = source
        .find("self.persist_company_artifact(&mut proof).await")
        .expect("artifact persistence boundary");
    let commit = source
        .find("self.commit_company(&context, event).await")
        .expect("Company EventLog commit boundary");
    let resolve = source
        .find("self.resolve_artifact_publication_intent(")
        .expect("publication resolution boundary");
    assert!(
        record < persist && persist < commit && commit < resolve,
        "the publication intent must be durable before the blob is staged and before the fact append, and resolved after it"
    );
}

#[test]
fn artifact_publication_reconciliation_classifies_without_appending_a_fact() {
    let source = include_str!("../src/company.rs");
    let start = source
        .find("pub async fn reconcile_artifact_publications")
        .expect("reconciliation boundary");
    let rest = &source[start..];
    let end = rest
        .find("pub async fn artifact_workspace_delta")
        .expect("workspace delta boundary");
    let reconciliation = &rest[..end];
    assert!(reconciliation.contains("pending_publication_intents().await"));
    assert!(reconciliation.contains("terminal_state()"));
    assert!(reconciliation.contains("resolve_publication_intent("));
    assert!(
        !reconciliation.contains("commit_company")
            && !reconciliation.contains("append_event")
            && !reconciliation.contains("commit_protected_event"),
        "reconciliation must never append a Company fact"
    );
}

#[test]
fn artifact_orphan_retention_is_policy_gated_and_plan_only_by_default() {
    let source = include_str!("../src/company.rs");
    let plan = source
        .find("pub async fn plan_artifact_publication_retention")
        .expect("retention plan boundary");
    let apply = source
        .find("pub async fn apply_artifact_publication_retention")
        .expect("retention apply boundary");
    let helper = source
        .find("async fn artifact_publication_retention")
        .expect("retention helper boundary");
    assert!(plan < apply && apply < helper);
    // The read-only plan entry must not be able to remove bytes, and it must not require a write
    // profile even to look.
    let plan_body = &source[plan..apply];
    assert!(plan_body.contains("company_context(context, false)"));
    assert!(
        !plan_body.contains("remove_orphan_blob") && !plan_body.contains("allow_blob_removal"),
        "the plan entry must stay read-only and policy-agnostic"
    );
    let apply_body = &source[apply..helper];
    assert!(apply_body.contains("company_context(context, true)"));
    // Removal is only reachable after the explicit policy permission, the retention window and the
    // per-run cap, and the helper never appends a Company fact.
    let helper_body = &source[helper..];
    let window = helper_body
        .find("orphan_age_below_retention_window")
        .expect("retention window fence");
    let capacity = helper_body
        .find("orphan_retention_capacity_exceeded")
        .expect("retention capacity fence");
    let permission = helper_body
        .find("orphan_blob_removal_not_permitted")
        .expect("explicit-removal permission fence");
    let removal = helper_body
        .find("remove_orphan_blob(&intent.artifact)")
        .expect("orphan removal boundary");
    assert!(
        window < capacity && capacity < permission && permission < removal,
        "retention must check the window, the cap and the explicit removal permission before touching bytes"
    );
    assert!(
        helper_body.contains("artifact_orphan_removal_unsupported")
            && helper_body.contains("artifact_orphan_removal_failed"),
        "an unsupported or failed removal must be denied instead of being reported as reclaimed"
    );
    let helper_end = helper_body
        .find("fn company_context(")
        .expect("next boundary");
    let helper_body = &helper_body[..helper_end];
    assert!(
        !helper_body.contains("commit_company")
            && !helper_body.contains("append_event")
            && !helper_body.contains("commit_protected_event"),
        "retention must never append a Company fact"
    );
}

#[test]
fn artifact_workspace_delta_is_a_confined_read_only_projection() {
    let source = include_str!("../src/company.rs");
    let start = source
        .find("pub async fn artifact_workspace_delta")
        .expect("workspace delta boundary");
    let rest = &source[start..];
    let end = rest.find("fn company_context(").expect("next boundary");
    let delta = &rest[..end];
    assert!(delta.contains("read_company_artifact(context, &relative)"));
    assert!(delta.contains("project_artifact_exists("));
    assert!(delta.contains("validate_artifact_reference_content"));
    assert!(
        !delta.contains("write_project_artifact")
            && !delta.contains("spawn_from_packet")
            && !delta.contains("handle_command"),
        "the workspace delta must stay a read-only projection"
    );
}

#[test]
fn artifact_original_readback_prefers_the_store_and_verifies_the_fact_fallback() {
    let source = include_str!("../src/company.rs");
    let start = source
        .find("pub async fn artifact_workspace_delta")
        .expect("workspace delta boundary");
    let rest = &source[start..];
    let end = rest.find("fn company_context(").expect("next boundary");
    let delta = &rest[..end];
    let store_read = delta
        .find("store.read_artifact(&reference).await")
        .expect("immutable store read");
    let marker = delta
        .find("historical_source_committed_fact")
        .expect("fact-backed original marker");
    let fallback = delta
        .find("Self::fact_backed_original(&artifact, &reference)")
        .expect("fact-backed read-back");
    assert!(
        store_read < marker && marker < fallback,
        "the immutable store copy is preferred and the committed fact is only a disclosed fallback"
    );
    let helper_start = delta
        .find("fn fact_backed_original(")
        .expect("fact-backed helper boundary");
    let helper = &delta[helper_start..];
    assert!(
        helper.contains("validate_artifact_reference_content"),
        "a fact-backed original must be re-verified against the typed reference before it is served"
    );
}
