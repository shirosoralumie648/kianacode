//! CAP-30 dynamic tool discovery at the broker boundary — deny-first.
//!
//! Every test here is a decision over supplied values. Nothing in this file starts a process,
//! loads a package, verifies a signature, opens a socket or executes a capability. The success
//! paths are last on purpose: a fixture that proves admission before it proves refusal proves
//! nothing about the refusal.

use kiana_capability_broker::{
    capability_key, evidence_key, intersect_extension_scope, operation_namespace,
    AdmittedDiscoveryCandidate, DiscoveredToolCandidate, DiscoveryDenial, DiscoveryProvenance,
    DiscoverySearchOutcome, DiscoveryStatus, DiscoverySurface, DynamicDiscoveryReport,
    DynamicDiscoveryRequest, ExtensionAdmissionEvidence, ProjectTrustVerdict,
    RegisteredDiscoveryTool, DISCOVERY_REQUEST_SCHEMA,
};
use kiana_domain::{
    json_digest, CapabilityKind, ExtensionAdapterStatus, ExtensionEffect, ScopeDimension,
    ScopeLimit, ScopeSet,
};
use std::collections::{BTreeMap, BTreeSet};

const NOW: u64 = 1_800_000_000_000;
const PRINCIPAL: &str = "principal:cap30";
const PROJECT: &str = "project:cap30";
const EXTENSION: &str = "ext:acme-index";
const COMPONENT: &str = "component:search";

/// A well-formed `sha256:` digest. Derived from a label rather than hard-coded so two different
/// labels can never collide by copy-paste.
fn digest(label: &str) -> String {
    json_digest(&serde_json::json!({"cap30": label}))
}

fn scope(operations: &[&str], namespaces: &[&str]) -> ScopeSet {
    ScopeSet::new(
        ScopeDimension::Restricted(operations.iter().map(|o| (*o).to_owned()).collect()),
        ScopeDimension::NotApplicable,
        ScopeDimension::Restricted(namespaces.iter().map(|n| (*n).to_owned()).collect()),
        ScopeDimension::NotApplicable,
        ScopeLimit::NotApplicable,
        ScopeLimit::NotApplicable,
    )
    .expect("scope is canonical")
}

fn restricted_catalog(operation: &str) -> BTreeMap<String, RegisteredDiscoveryTool> {
    BTreeMap::from([(
        operation.to_owned(),
        registered(operation, ExtensionEffect::ReadOnly),
    )])
}

fn registered(operation: &str, effect: ExtensionEffect) -> RegisteredDiscoveryTool {
    let namespace = operation_namespace(operation)
        .unwrap_or("context")
        .to_owned();
    RegisteredDiscoveryTool {
        tool_name: format!("acme.index.{namespace}"),
        namespace,
        schema_digest: digest(&format!("schema-{operation}")),
        capability: CapabilityKind::Query,
        effect,
        side_effecting: effect == ExtensionEffect::ReadWrite,
    }
}

fn honest_candidate() -> DiscoveredToolCandidate {
    DiscoveredToolCandidate {
        tool_name: "acme.index.context".to_owned(),
        namespace: "context".to_owned(),
        operation: "context.search".to_owned(),
        capability: CapabilityKind::Query,
        extension_id: EXTENSION.to_owned(),
        component_id: COMPONENT.to_owned(),
        observed_manifest_digest: digest("manifest"),
        claimed_capabilities: BTreeSet::from(["query".to_owned()]),
        schema_digest: digest("schema-context.search"),
        effect: ExtensionEffect::ReadOnly,
        side_effecting: false,
    }
}

fn honest_evidence() -> ExtensionAdmissionEvidence {
    ExtensionAdmissionEvidence {
        extension_id: EXTENSION.to_owned(),
        component_id: COMPONENT.to_owned(),
        project_id: PROJECT.to_owned(),
        registered_manifest_digest: digest("manifest"),
        registered_package_sha256: "ab".repeat(32),
        provenance: DiscoveryProvenance::PublisherSigned,
        project_local: false,
        project_trust: ProjectTrustVerdict::Unknown,
        status: ExtensionAdapterStatus::Available,
        effect: ExtensionEffect::ReadOnly,
        required_capabilities: BTreeSet::from(["query".to_owned()]),
        registry_generation: 7,
    }
}

fn evidence_map(
    record: ExtensionAdmissionEvidence,
) -> BTreeMap<String, ExtensionAdmissionEvidence> {
    BTreeMap::from([(
        evidence_key(&record.extension_id, &record.component_id),
        record,
    )])
}

fn surface() -> DiscoverySurface {
    DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        scope(&["context.search"], &["context"]),
        ExtensionEffect::ReadWrite,
        restricted_catalog("context.search"),
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid")
}

fn request_for(surface: &DiscoverySurface) -> DynamicDiscoveryRequest {
    DynamicDiscoveryRequest {
        schema: DISCOVERY_REQUEST_SCHEMA.to_owned(),
        request_id: "discovery:req-1".to_owned(),
        query: "index search".to_owned(),
        surface_digest: surface.digest.clone(),
        issued_at_unix_ms: NOW - 1_000,
        max_results: 16,
    }
}

fn completed(candidates: Vec<DiscoveredToolCandidate>) -> DiscoverySearchOutcome {
    DiscoverySearchOutcome::Completed(candidates)
}

fn evaluate(
    request: &DynamicDiscoveryRequest,
    surface: &DiscoverySurface,
    evidence: &BTreeMap<String, ExtensionAdmissionEvidence>,
    outcome: &DiscoverySearchOutcome,
) -> DynamicDiscoveryReport {
    DynamicDiscoveryReport::evaluate(request, surface, evidence, outcome, NOW)
}

fn reissued(
    mut request: DynamicDiscoveryRequest,
    surface: &DiscoverySurface,
) -> DynamicDiscoveryRequest {
    request.surface_digest = surface.digest.clone();
    request
}

// ---------------------------------------------------------------------------
// request level: a stale or withdrawn grant is answered before any tool question
// ---------------------------------------------------------------------------

#[test]
fn cached_result_cannot_outlive_the_grant_it_was_issued_under() {
    let original = surface();
    let issued = request_for(&original);

    // The operator revokes and re-seals. The digest moves, so the cached result no longer names
    // the surface it was decided under.
    let revoked = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        4,
        BTreeSet::from(["query".to_owned()]),
        original.scope.clone(),
        ExtensionEffect::ReadWrite,
        original.catalog.clone(),
        original.catalog_digest.clone(),
        true,
        original.expires_at_unix_ms,
    )
    .expect("re-sealed surface is valid");
    assert_ne!(revoked.digest, original.digest);

    let report = evaluate(
        &issued,
        &revoked,
        &evidence_map(honest_evidence()),
        &completed(vec![honest_candidate()]),
    );
    assert_eq!(report.status, DiscoveryStatus::Denied);
    assert_eq!(report.denial_codes(), vec!["discovery_authorization_stale"]);
    assert!(report.promotable().is_empty());
    assert!(
        !report.retry_required,
        "a stale result is final, not retryable"
    );
    assert_eq!(
        report.denials[0].candidate_ref, "",
        "a request-level refusal names no candidate"
    );

    // Narrowing the grant without revoking it is equally invisible to the old result.
    let narrowed = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::new(),
        original.scope.clone(),
        ExtensionEffect::ReadWrite,
        original.catalog.clone(),
        original.catalog_digest.clone(),
        false,
        original.expires_at_unix_ms,
    )
    .expect("narrowed surface is valid");
    let report = evaluate(
        &issued,
        &narrowed,
        &evidence_map(honest_evidence()),
        &completed(vec![honest_candidate()]),
    );
    assert_eq!(report.denial_codes(), vec!["discovery_authorization_stale"]);
    assert!(report.promotable().is_empty());
}

#[test]
fn revoked_grant_denies_even_when_the_caller_resealed_the_digest() {
    let live = surface();
    let revoked = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        live.granted_capabilities.clone(),
        live.scope.clone(),
        ExtensionEffect::ReadWrite,
        live.catalog.clone(),
        live.catalog_digest.clone(),
        true,
        live.expires_at_unix_ms,
    )
    .expect("surface is valid");

    // The request is bound to the *revoked* surface, so staleness cannot be the answer here and the
    // explicit revocation flag has to carry the refusal on its own.
    let bound = request_for(&revoked);
    let report = evaluate(
        &bound,
        &revoked,
        &evidence_map(honest_evidence()),
        &completed(vec![honest_candidate()]),
    );
    assert_eq!(
        report.denial_codes(),
        vec!["discovery_authorization_revoked"]
    );
    assert!(report.promotable().is_empty());
}

#[test]
fn expired_grant_is_refused_at_the_expiry_instant() {
    let surface = surface();
    let bound = request_for(&surface);
    let evidence = evidence_map(honest_evidence());
    let outcome = completed(vec![honest_candidate()]);

    // One millisecond before expiry the same inputs admit.
    assert_eq!(
        DynamicDiscoveryReport::evaluate(
            &bound,
            &surface,
            &evidence,
            &outcome,
            surface.expires_at_unix_ms - 1
        )
        .status,
        DiscoveryStatus::Admitted
    );
    // At the expiry instant they do not. `>=` rather than `>` is the boundary that matters here.
    let report = DynamicDiscoveryReport::evaluate(
        &bound,
        &surface,
        &evidence,
        &outcome,
        surface.expires_at_unix_ms,
    );
    assert_eq!(
        report.denial_codes(),
        vec!["discovery_authorization_expired"]
    );
    assert!(report.promotable().is_empty());
}

// ---------------------------------------------------------------------------
// the search itself failing is Unknown, never allow-all and never deny-all
// ---------------------------------------------------------------------------

#[test]
fn failed_search_is_unknown_and_promotes_nothing() {
    let surface = surface();
    let bound = request_for(&surface);
    let evidence = evidence_map(honest_evidence());

    let failed = DiscoverySearchOutcome::Failed {
        reason: "index backend timed out".to_owned(),
    };
    let report = evaluate(&bound, &surface, &evidence, &failed);
    assert_eq!(report.status, DiscoveryStatus::Unknown);
    assert_eq!(report.denial_codes(), vec!["discovery_search_failed"]);
    assert!(
        report.retry_required,
        "an Unknown report must demand a re-run"
    );

    // Neither degradation is representable: the report carries no candidates, so a caller that
    // reads a non-admitted report as "everything" or as "nothing, continue" has the same empty
    // set either way, and `promotable` refuses both readings explicitly.
    assert!(report.admitted.is_empty());
    assert!(report.promotable().is_empty());
    assert_ne!(report.status, DiscoveryStatus::Admitted);
    assert_ne!(report.status, DiscoveryStatus::Denied);
    DynamicDiscoveryReport::validate_against(&report, &bound, &surface, &evidence, &failed, NOW)
        .unwrap();

    // An empty *successful* search is a different, final answer: Denied, and no re-run demanded.
    let empty = evaluate(&bound, &surface, &evidence, &completed(Vec::new()));
    assert_eq!(empty.status, DiscoveryStatus::Denied);
    assert!(!empty.retry_required);
    assert!(empty.denial_codes().is_empty());
    assert!(empty.promotable().is_empty());
    assert_ne!(empty.status, DiscoveryStatus::Unknown);
}

#[test]
fn malformed_request_and_corrupt_inputs_are_refused_not_answered() {
    let surface = surface();
    let evidence = evidence_map(honest_evidence());
    let outcome = completed(vec![honest_candidate()]);

    let mut bad_digest = request_for(&surface);
    bad_digest.surface_digest = "sha256:not-a-digest".to_owned();
    assert_eq!(
        evaluate(&bad_digest, &surface, &evidence, &outcome).denial_codes(),
        vec!["discovery_request_invalid"]
    );

    let mut bad_query = request_for(&surface);
    bad_query.query = "   ".to_owned();
    assert_eq!(
        evaluate(&bad_query, &surface, &evidence, &outcome).denial_codes(),
        vec!["discovery_request_invalid"]
    );

    let mut zero_results = request_for(&surface);
    zero_results.max_results = 0;
    assert_eq!(
        evaluate(&zero_results, &surface, &evidence, &outcome).denial_codes(),
        vec!["discovery_request_invalid"]
    );

    // A surface whose own seal no longer covers its contents is refused rather than trusted.
    let mut tampered_surface = surface.clone();
    tampered_surface.authority_epoch = 99;
    let report = evaluate(
        &request_for(&surface),
        &tampered_surface,
        &evidence,
        &outcome,
    );
    assert_eq!(report.denial_codes(), vec!["discovery_surface_invalid"]);

    let mut bad_evidence = honest_evidence();
    bad_evidence.registered_manifest_digest = "sha256:zz".to_owned();
    let report = evaluate(
        &request_for(&surface),
        &surface,
        &evidence_map(bad_evidence),
        &outcome,
    );
    assert_eq!(report.denial_codes(), vec!["discovery_surface_invalid"]);
    assert!(report.promotable().is_empty());
}

// ---------------------------------------------------------------------------
// candidate level: identity, then supply chain, then authorization
// ---------------------------------------------------------------------------

#[test]
fn unknown_tool_namespace_and_schema_all_fail_closed() {
    let surface = surface();
    let evidence = evidence_map(honest_evidence());
    let bound = request_for(&surface);

    let mut unknown_tool = honest_candidate();
    unknown_tool.operation = "context.definitely_not_registered".to_owned();
    assert_eq!(
        evaluate(&bound, &surface, &evidence, &completed(vec![unknown_tool])).denial_codes(),
        vec!["discovery_unknown_tool"]
    );

    let mut unknown_namespace = honest_candidate();
    unknown_namespace.namespace = "totally_other".to_owned();
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence,
            &completed(vec![unknown_namespace])
        )
        .denial_codes(),
        vec!["discovery_unknown_namespace"]
    );

    // A namespace the surface lists but the registry binds elsewhere is still refused: the registry
    // entry for an operation is the only authority on which namespace that operation lives in.
    let mut mislabelled = honest_candidate();
    mislabelled.namespace = "memory".to_owned();
    let with_two = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        scope(&["context.search"], &["context", "memory"]),
        ExtensionEffect::ReadWrite,
        restricted_catalog("context.search"),
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid");
    let report = evaluate(
        &reissued(bound.clone(), &with_two),
        &with_two,
        &evidence,
        &completed(vec![mislabelled]),
    );
    assert_eq!(report.denial_codes(), vec!["discovery_unknown_namespace"]);

    let mut unregistered_schema = honest_candidate();
    unregistered_schema.schema_digest = digest("schema-someone-elses-tool").to_owned();
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence,
            &completed(vec![unregistered_schema])
        )
        .denial_codes(),
        vec!["discovery_schema_unregistered"]
    );

    // A surface that names no namespaces admits no namespace at all, rather than all of them.
    let open = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        ScopeSet::new(
            ScopeDimension::Restricted(vec!["context.search".to_owned()]),
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeLimit::NotApplicable,
            ScopeLimit::NotApplicable,
        )
        .expect("scope is canonical"),
        ExtensionEffect::ReadWrite,
        restricted_catalog("context.search"),
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid");
    let report = evaluate(
        &reissued(bound, &open),
        &open,
        &evidence,
        &completed(vec![honest_candidate()]),
    );
    assert_eq!(report.denial_codes(), vec!["discovery_unknown_namespace"]);
    assert!(report.promotable().is_empty());
}

#[test]
fn a_candidate_outside_the_authorization_surface_is_refused() {
    // The tool is real and the extension is genuine; the *grant* simply does not offer it. This is
    // the dynamic-search-bypasses-static-authorization case: the search found it, the grant did not.
    let narrow = surface();
    let evidence = evidence_map(honest_evidence());

    let wide = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        scope(&["context.search", "context.repo_map"], &["context"]),
        ExtensionEffect::ReadWrite,
        BTreeMap::from([
            (
                "context.search".to_owned(),
                registered("context.search", ExtensionEffect::ReadOnly),
            ),
            (
                "context.repo_map".to_owned(),
                registered("context.repo_map", ExtensionEffect::ReadOnly),
            ),
        ]),
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid");

    let mut extra = honest_candidate();
    extra.operation = "context.repo_map".to_owned();
    extra.tool_name = "acme.index.context".to_owned();
    extra.schema_digest = digest("schema-context.repo_map");

    // On the wide surface it is admitted, so the refusal below is about the grant, not the tool.
    let wide_report = evaluate(
        &reissued(request_for(&narrow), &wide),
        &wide,
        &evidence,
        &completed(vec![extra.clone()]),
    );
    assert_eq!(wide_report.status, DiscoveryStatus::Admitted);

    // The narrow surface never offered `context.repo_map`, so the same result is not in scope.
    let report = evaluate(
        &request_for(&narrow),
        &narrow,
        &evidence,
        &completed(vec![extra]),
    );
    assert_eq!(report.denial_codes(), vec!["discovery_unknown_tool"]);
    assert!(report.promotable().is_empty());
    assert_eq!(report.denials[0].candidate_ref, "context.repo_map");
}

#[test]
fn a_capability_the_grant_does_not_carry_is_refused() {
    let surface = surface();
    let evidence = evidence_map(honest_evidence());
    let bound = request_for(&surface);

    // A descriptor that renames the capability of a registered operation is a lie about identity.
    let mut lying = honest_candidate();
    lying.capability = CapabilityKind::Network;
    assert_eq!(
        evaluate(&bound, &surface, &evidence, &completed(vec![lying])).denial_codes(),
        vec!["discovery_capability_claim_mismatch"]
    );

    // With the registry agreeing on `network`, the refusal becomes the one that matters: this
    // grant does not carry `network` at all.
    let network_catalog = BTreeMap::from([(
        "context.search".to_owned(),
        RegisteredDiscoveryTool {
            capability: CapabilityKind::Network,
            ..registered("context.search", ExtensionEffect::ReadOnly)
        },
    )]);
    let network_surface = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        scope(&["context.search"], &["context"]),
        ExtensionEffect::ReadWrite,
        network_catalog,
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid");
    let mut network_candidate = honest_candidate();
    network_candidate.capability = CapabilityKind::Network;
    let report = evaluate(
        &reissued(bound, &network_surface),
        &network_surface,
        &evidence,
        &completed(vec![network_candidate]),
    );
    assert_eq!(
        report.denial_codes(),
        vec!["discovery_capability_not_granted"]
    );
    assert!(report.promotable().is_empty());
}

#[test]
fn an_operation_outside_the_grant_scope_is_refused() {
    // Registry and granted capability agree; the scope dimension simply does not list it.
    let narrow = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        scope(&["context.repo_map"], &["context"]),
        ExtensionEffect::ReadWrite,
        restricted_catalog("context.search"),
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid");
    let report = evaluate(
        &reissued(request_for(&surface()), &narrow),
        &narrow,
        &evidence_map(honest_evidence()),
        &completed(vec![honest_candidate()]),
    );
    assert_eq!(
        report.denial_codes(),
        vec!["discovery_operation_out_of_scope"]
    );
    assert!(report.promotable().is_empty());
}

#[test]
fn untrusted_provenance_and_untrusted_project_are_refused_before_the_manifest_is_compared() {
    let surface = surface();
    let bound = request_for(&surface);

    let mut unknown_trust = honest_evidence();
    unknown_trust.project_local = true;
    unknown_trust.project_trust = ProjectTrustVerdict::Unknown;
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(unknown_trust),
            &completed(vec![honest_candidate()])
        )
        .denial_codes(),
        vec!["discovery_project_trust_unverified"]
    );

    let mut untrusted = honest_evidence();
    untrusted.project_local = true;
    untrusted.project_trust = ProjectTrustVerdict::Untrusted;
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(untrusted),
            &completed(vec![honest_candidate()])
        )
        .denial_codes(),
        vec!["discovery_project_trust_unverified"]
    );

    // A project-local component that *is* trusted still needs a verifiable provenance root: trust
    // says "this project is trusted", provenance says "these bytes came from somewhere checkable".
    let mut self_asserted = honest_evidence();
    self_asserted.project_local = true;
    self_asserted.project_trust = ProjectTrustVerdict::Trusted;
    self_asserted.provenance = DiscoveryProvenance::SelfAsserted;
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(self_asserted),
            &completed(vec![honest_candidate()])
        )
        .denial_codes(),
        vec!["discovery_provenance_unverified"]
    );

    let mut no_provenance = honest_evidence();
    no_provenance.provenance = DiscoveryProvenance::Unknown;
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(no_provenance),
            &completed(vec![honest_candidate()])
        )
        .denial_codes(),
        vec!["discovery_provenance_unverified"]
    );

    // With both facts in order the same candidate is admitted, which is what makes the refusals
    // above refusals rather than a blanket ban on project-local extensions.
    let mut trusted = honest_evidence();
    trusted.project_local = true;
    trusted.project_trust = ProjectTrustVerdict::Trusted;
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(trusted),
            &completed(vec![honest_candidate()])
        )
        .status,
        DiscoveryStatus::Admitted
    );
}

#[test]
fn manifest_digest_drift_is_supply_chain_tampering_not_a_cache_miss() {
    let surface = surface();
    let bound = request_for(&surface);

    let mut drifted = honest_candidate();
    drifted.observed_manifest_digest = digest("manifest-after-upgrade").to_owned();
    let report = evaluate(
        &bound,
        &surface,
        &evidence_map(honest_evidence()),
        &completed(vec![drifted]),
    );
    assert_eq!(
        report.denial_codes(),
        vec!["extension_manifest_digest_mismatch"]
    );
    assert_eq!(report.denials[0].candidate_ref, "context.search");
    assert!(report.promotable().is_empty());

    // The registry moving instead of the descriptor produces the same refusal, because the two
    // sides have to agree rather than one of them winning.
    let mut upgraded = honest_evidence();
    upgraded.registered_manifest_digest = digest("manifest-after-upgrade").to_owned();
    let report = evaluate(
        &bound,
        &surface,
        &evidence_map(upgraded),
        &completed(vec![honest_candidate()]),
    );
    assert_eq!(
        report.denial_codes(),
        vec!["extension_manifest_digest_mismatch"]
    );
    assert!(report.promotable().is_empty());
}

#[test]
fn missing_evidence_and_a_non_available_adapter_are_refused() {
    let surface = surface();
    let bound = request_for(&surface);

    // The search names a component the registry has never admitted.
    let report = evaluate(
        &bound,
        &surface,
        &BTreeMap::new(),
        &completed(vec![honest_candidate()]),
    );
    assert_eq!(report.denial_codes(), vec!["discovery_evidence_missing"]);
    assert!(report.promotable().is_empty());

    for status in [
        ExtensionAdapterStatus::RequiresApproval,
        ExtensionAdapterStatus::NotSupported,
        ExtensionAdapterStatus::Denied,
    ] {
        let mut record = honest_evidence();
        record.status = status;
        let report = evaluate(
            &bound,
            &surface,
            &evidence_map(record),
            &completed(vec![honest_candidate()]),
        );
        assert_eq!(
            report.denial_codes(),
            vec!["discovery_adapter_not_available"]
        );
        assert!(report.promotable().is_empty());
    }

    let mut foreign = honest_evidence();
    foreign.project_id = "project:someone-else".to_owned();
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(foreign),
            &completed(vec![honest_candidate()])
        )
        .denial_codes(),
        vec!["discovery_project_mismatch"]
    );
}

// ---------------------------------------------------------------------------
// permission union: an extension may only narrow, never widen
// ---------------------------------------------------------------------------

#[test]
fn an_extension_asking_for_more_than_the_parent_grant_is_refused_not_narrowed() {
    let surface = surface(); // grants exactly {query}
    let bound = request_for(&surface);

    // The manifest itself needs a capability the grant does not carry.
    let mut greedy_manifest = honest_evidence();
    greedy_manifest.required_capabilities =
        BTreeSet::from(["query".to_owned(), "network".to_owned()]);
    let report = evaluate(
        &bound,
        &surface,
        &evidence_map(greedy_manifest),
        &completed(vec![honest_candidate()]),
    );
    assert_eq!(
        report.denial_codes(),
        vec!["extension_scope_superset_denied"]
    );
    assert!(
        report.promotable().is_empty(),
        "a superset is denied, not granted the difference"
    );

    // The descriptor claims more than even its own manifest.
    let mut greedy_descriptor = honest_candidate();
    greedy_descriptor.claimed_capabilities =
        BTreeSet::from(["query".to_owned(), "secret".to_owned()]);
    let report = evaluate(
        &bound,
        &surface,
        &evidence_map(honest_evidence()),
        &completed(vec![greedy_descriptor]),
    );
    assert_eq!(
        report.denial_codes(),
        vec!["extension_scope_superset_denied"]
    );
    assert!(report.promotable().is_empty());

    // The intersection helper is the non-amplifying primitive the decision rests on.
    let requested = BTreeSet::from([
        "query".to_owned(),
        "network".to_owned(),
        "secret".to_owned(),
    ]);
    let granted = BTreeSet::from(["query".to_owned()]);
    let intersection = intersect_extension_scope(&requested, &granted);
    assert_eq!(intersection, BTreeSet::from(["query".to_owned()]));
    assert!(intersection.is_subset(&granted));
    assert!(!intersection.contains("network"));
    assert!(!intersection.contains("secret"));

    // An empty request is a subset of everything, so it never trips the superset rule.
    assert!(intersect_extension_scope(&BTreeSet::new(), &granted).is_empty());
}

// ---------------------------------------------------------------------------
// effect is a property of the manifest and the registry, never of the descriptor
// ---------------------------------------------------------------------------

#[test]
fn a_descriptor_cannot_escalate_its_own_effect() {
    let surface = surface(); // ceiling ReadWrite, registry ReadOnly
    let bound = request_for(&surface);

    let mut escalating = honest_candidate();
    escalating.effect = ExtensionEffect::ReadWrite;
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(honest_evidence()),
            &completed(vec![escalating])
        )
        .denial_codes(),
        vec!["discovery_effect_claim_escalated"]
    );

    // A manifest that claims read-write while the registry bound read-only cannot lift the
    // descriptor either: the weaker of the two server-owned values wins.
    let mut loud_manifest = honest_evidence();
    loud_manifest.effect = ExtensionEffect::ReadWrite;
    let mut still_read_only = honest_candidate();
    still_read_only.effect = ExtensionEffect::ReadWrite;
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(loud_manifest),
            &completed(vec![still_read_only])
        )
        .denial_codes(),
        vec!["discovery_effect_claim_escalated"]
    );

    // A genuinely read-write extension still cannot pass a read-only ceiling.
    let read_only_ceiling = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        scope(&["context.search"], &["context"]),
        ExtensionEffect::ReadOnly,
        BTreeMap::from([(
            "context.search".to_owned(),
            registered("context.search", ExtensionEffect::ReadWrite),
        )]),
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid");
    let mut rw_evidence = honest_evidence();
    rw_evidence.effect = ExtensionEffect::ReadWrite;
    let mut rw_candidate = honest_candidate();
    rw_candidate.effect = ExtensionEffect::ReadWrite;
    let report = evaluate(
        &reissued(bound.clone(), &read_only_ceiling),
        &read_only_ceiling,
        &evidence_map(rw_evidence.clone()),
        &completed(vec![rw_candidate.clone()]),
    );
    assert_eq!(
        report.denial_codes(),
        vec!["discovery_effect_ceiling_exceeded"]
    );
    assert!(report.promotable().is_empty());

    // The same read-write extension under a read-write ceiling is admitted, so the refusal is the
    // ceiling and not a blanket ban on side-effecting tools.
    let rw_ceiling = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        scope(&["context.search"], &["context"]),
        ExtensionEffect::ReadWrite,
        BTreeMap::from([(
            "context.search".to_owned(),
            registered("context.search", ExtensionEffect::ReadWrite),
        )]),
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid");
    let report = evaluate(
        &reissued(bound, &rw_ceiling),
        &rw_ceiling,
        &evidence_map(rw_evidence),
        &completed(vec![rw_candidate]),
    );
    assert_eq!(report.status, DiscoveryStatus::Admitted);
    assert!(
        report.promotable()[0].side_effecting,
        "side_effecting comes from the registry, not from the descriptor"
    );
    assert_eq!(report.promotable()[0].effect, ExtensionEffect::ReadWrite);
}

#[test]
fn duplicate_operations_are_refused_so_one_result_cannot_smuggle_two_descriptors() {
    let surface = surface();
    let bound = request_for(&surface);

    // The same operation twice: the first is admitted, the second is refused as a duplicate, and
    // exactly one descriptor survives. The point is that a result cannot present two different
    // claims for one operation and let the caller pick.
    let mut shadow = honest_candidate();
    shadow.tool_name = "acme.index.shadow".to_owned();
    let report = evaluate(
        &bound,
        &surface,
        &evidence_map(honest_evidence()),
        &completed(vec![honest_candidate(), shadow]),
    );
    assert_eq!(report.status, DiscoveryStatus::Admitted);
    assert_eq!(report.denial_codes(), vec!["discovery_duplicate_candidate"]);
    assert_eq!(report.denials[0].candidate_ref, "context.search");
    assert_eq!(report.promotable().len(), 1);
    assert_eq!(report.promotable()[0].tool_name, "acme.index.context");

    // When the first claim is itself refused, nothing is admitted and the report is a flat Denied.
    let mut first_bad = honest_candidate();
    first_bad.observed_manifest_digest = digest("manifest-after-upgrade").to_owned();
    let mut second_bad = honest_candidate();
    second_bad.tool_name = "acme.index.second".to_owned();
    let report = evaluate(
        &bound,
        &surface,
        &evidence_map(honest_evidence()),
        &completed(vec![first_bad, second_bad]),
    );
    assert_eq!(report.status, DiscoveryStatus::Denied);
    assert_eq!(
        report.denial_codes(),
        vec![
            "extension_manifest_digest_mismatch",
            "discovery_duplicate_candidate"
        ]
    );
    assert!(report.promotable().is_empty());
}

#[test]
fn an_oversized_result_is_refused_rather_than_truncated() {
    let surface = surface();
    let bound = request_for(&surface);
    let flood = (0..200)
        .map(|index| DiscoveredToolCandidate {
            operation: format!("context.op{index}"),
            tool_name: format!("acme.op{index}"),
            ..honest_candidate()
        })
        .collect::<Vec<_>>();
    let report = evaluate(
        &bound,
        &surface,
        &evidence_map(honest_evidence()),
        &completed(flood),
    );
    assert_eq!(report.denial_codes(), vec!["discovery_request_invalid"]);
    assert!(report.promotable().is_empty());
}

// ---------------------------------------------------------------------------
// decision order and the report's own integrity
// ---------------------------------------------------------------------------

#[test]
fn decision_order_is_fixed_and_a_tampered_report_is_refused() {
    let surface = surface();
    let evidence = evidence_map(honest_evidence());
    let outcome = completed(vec![honest_candidate()]);
    let bound = request_for(&surface);

    // A descriptor that violates several rules reports the earliest one, not a softened version.
    let mut many_violations = honest_candidate();
    many_violations.effect = ExtensionEffect::ReadWrite; // would fail the effect checks
    many_violations.claimed_capabilities = BTreeSet::from(["secret".to_owned()]); // and the union
    many_violations.schema_digest = digest("schema-drifted"); // and the registry
    let report = evaluate(
        &bound,
        &surface,
        &evidence,
        &completed(vec![many_violations]),
    );
    assert_eq!(report.denial_codes(), vec!["discovery_schema_unregistered"]);

    // Trust outranks authorization: an untrusted descriptor that is also out of scope reports the
    // trust failure, because an unverified source is not a permission question.
    let mut untrusted_and_out_of_scope = honest_candidate();
    untrusted_and_out_of_scope.claimed_capabilities = BTreeSet::from(["secret".to_owned()]);
    let mut self_asserted = honest_evidence();
    self_asserted.provenance = DiscoveryProvenance::SelfAsserted;
    assert_eq!(
        evaluate(
            &bound,
            &surface,
            &evidence_map(self_asserted),
            &completed(vec![untrusted_and_out_of_scope])
        )
        .denial_codes(),
        vec!["discovery_provenance_unverified"]
    );

    // A stale request is answered as stale even though the candidate is genuine.
    let mut stale = request_for(&surface);
    stale.surface_digest = digest("some-other-surface");
    assert_eq!(
        evaluate(&stale, &surface, &evidence, &outcome).denial_codes(),
        vec!["discovery_authorization_stale"]
    );

    // An honest report re-derives.
    let good = evaluate(&bound, &surface, &evidence, &outcome);
    assert_eq!(good.status, DiscoveryStatus::Admitted);
    DynamicDiscoveryReport::validate_against(&good, &bound, &surface, &evidence, &outcome, NOW)
        .unwrap();

    // An unsealed edit is caught by the report's own digest.
    let mut flipped = good.clone();
    flipped.status = DiscoveryStatus::Denied;
    assert_eq!(
        DynamicDiscoveryReport::validate_against(
            &flipped, &bound, &surface, &evidence, &outcome, NOW
        ),
        Err("discovery_report_digest_mismatch".to_owned())
    );

    // A correctly sealed forgery is caught by the structural invariants, not by the digest.
    let mut appended = good.clone();
    appended.admitted.push(AdmittedDiscoveryCandidate {
        tool_name: "acme.smuggled".to_owned(),
        namespace: "context".to_owned(),
        operation: "context.smuggled".to_owned(),
        capability_key: "query".to_owned(),
        effective_capabilities: BTreeSet::from(["query".to_owned()]),
        effect: ExtensionEffect::ReadOnly,
        side_effecting: false,
    });
    appended.digest = appended.sealed_digest();
    assert_eq!(
        DynamicDiscoveryReport::validate_against(
            &appended, &bound, &surface, &evidence, &outcome, NOW
        ),
        Err("discovery_report_binding_invalid".to_owned())
    );

    let mut reflipped = good.clone();
    reflipped.retry_required = true;
    reflipped.digest = reflipped.sealed_digest();
    assert_eq!(
        DynamicDiscoveryReport::validate_against(
            &reflipped, &bound, &surface, &evidence, &outcome, NOW
        ),
        Err("discovery_report_retry_without_unknown".to_owned())
    );

    // A report cannot smuggle candidates in behind a non-admitted status.
    let failed = DiscoverySearchOutcome::Failed {
        reason: "boom".to_owned(),
    };
    let unknown = evaluate(&bound, &surface, &evidence, &failed);
    let mut smuggled = unknown.clone();
    smuggled.admitted.push(AdmittedDiscoveryCandidate {
        tool_name: "acme.smuggled".to_owned(),
        namespace: "context".to_owned(),
        operation: "context.search".to_owned(),
        capability_key: "query".to_owned(),
        effective_capabilities: BTreeSet::from(["query".to_owned()]),
        effect: ExtensionEffect::ReadOnly,
        side_effecting: false,
    });
    smuggled.digest = smuggled.sealed_digest();
    assert_eq!(
        DynamicDiscoveryReport::validate_against(
            &smuggled, &bound, &surface, &evidence, &failed, NOW
        ),
        Err("discovery_report_admission_without_status".to_owned())
    );

    // The Unknown reason is pinned, so a failed search cannot be relabelled as a verdict.
    let mut relabelled = unknown.clone();
    relabelled.denials[0].code = "discovery_unknown_tool";
    relabelled.digest = relabelled.sealed_digest();
    assert_eq!(
        DynamicDiscoveryReport::validate_against(
            &relabelled,
            &bound,
            &surface,
            &evidence,
            &failed,
            NOW
        ),
        Err("discovery_report_unknown_reason_invalid".to_owned())
    );
}

#[test]
fn every_denial_code_is_stable_snake_case_and_precedence_is_a_total_order() {
    let all = [
        DiscoveryDenial::RequestInvalid,
        DiscoveryDenial::SurfaceInvalid,
        DiscoveryDenial::AuthorizationStale,
        DiscoveryDenial::AuthorizationRevoked,
        DiscoveryDenial::AuthorizationExpired,
        DiscoveryDenial::SearchFailed,
        DiscoveryDenial::CandidateInvalid,
        DiscoveryDenial::DuplicateCandidate,
        DiscoveryDenial::UnknownNamespace,
        DiscoveryDenial::UnknownTool,
        DiscoveryDenial::SchemaUnregistered,
        DiscoveryDenial::EvidenceMissing,
        DiscoveryDenial::ProjectTrustUnverified,
        DiscoveryDenial::ProvenanceUnverified,
        DiscoveryDenial::ManifestDigestMismatch,
        DiscoveryDenial::AdapterNotAvailable,
        DiscoveryDenial::ProjectMismatch,
        DiscoveryDenial::CapabilityClaimMismatch,
        DiscoveryDenial::CapabilityNotGranted,
        DiscoveryDenial::OperationOutOfScope,
        DiscoveryDenial::ExtensionScopeSuperset,
        DiscoveryDenial::EffectClaimEscalated,
        DiscoveryDenial::EffectCeilingExceeded,
    ];
    let mut codes = BTreeSet::new();
    let mut precedences = BTreeSet::new();
    for denial in all {
        let code = denial.as_str();
        assert!(!code.is_empty());
        assert!(
            code.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "{code} is not stable snake_case"
        );
        assert!(codes.insert(code), "{code} is duplicated");
        assert!(
            precedences.insert(denial.precedence()),
            "{code} reuses a precedence"
        );
        // Only a failed search is an Unknown; every other refusal is a definite answer.
        assert_eq!(denial.is_unknown(), code == "discovery_search_failed");
    }
    assert_eq!(codes.len(), 23);
    assert_eq!(precedences.len(), 23);
    assert_eq!(*precedences.iter().next().unwrap(), 0);
    assert_eq!(*precedences.iter().next_back().unwrap(), 22);
}

// ---------------------------------------------------------------------------
// helpers that shape the search itself
// ---------------------------------------------------------------------------

#[test]
fn namespaces_are_derived_and_capability_keys_are_canonical() {
    assert_eq!(operation_namespace("context.search"), Some("context"));
    assert_eq!(operation_namespace("apply_patch"), None);
    assert_eq!(operation_namespace(".leading"), None);
    assert_eq!(operation_namespace("trailing."), None);
    assert_eq!(operation_namespace(""), None);

    assert_eq!(capability_key(&CapabilityKind::Query), "query");
    assert_eq!(capability_key(&CapabilityKind::Process), "process");
    assert_eq!(
        capability_key(&CapabilityKind::Other("widget.render".to_owned())),
        "other.widget.render"
    );
    // An unreviewed category can never collide with a built-in one.
    assert_ne!(
        capability_key(&CapabilityKind::Other("query".to_owned())),
        capability_key(&CapabilityKind::Query)
    );
    assert_eq!(
        evidence_key("ext:acme", "component:search"),
        "ext:acme#component:search"
    );
}

// ---------------------------------------------------------------------------
// success paths, last
// ---------------------------------------------------------------------------

#[test]
fn an_honest_read_only_extension_within_the_grant_is_admitted_as_a_candidate_only() {
    let surface = surface();
    let evidence = evidence_map(honest_evidence());
    let outcome = completed(vec![honest_candidate()]);
    let bound = request_for(&surface);

    let report = evaluate(&bound, &surface, &evidence, &outcome);
    assert_eq!(report.status, DiscoveryStatus::Admitted);
    assert!(report.denial_codes().is_empty());
    assert!(!report.retry_required);

    let promotable = report.promotable();
    assert_eq!(promotable.len(), 1);
    let admitted = &promotable[0];
    // Identity comes from the registry, not from the descriptor.
    assert_eq!(admitted.operation, "context.search");
    assert_eq!(admitted.namespace, "context");
    assert_eq!(admitted.tool_name, "acme.index.context");
    assert_eq!(admitted.capability_key, "query");
    assert!(!admitted.side_effecting);
    // The carried scope is the intersection, so it can never exceed the grant.
    assert_eq!(
        admitted.effective_capabilities,
        BTreeSet::from(["query".to_owned()])
    );
    assert!(admitted
        .effective_capabilities
        .is_subset(&surface.granted_capabilities));

    // The report binds the surface, the catalog and the epoch it was decided against.
    assert_eq!(report.surface_digest, surface.digest);
    assert_eq!(report.catalog_digest, surface.catalog_digest);
    assert_eq!(report.authority_epoch, surface.authority_epoch);
    assert_eq!(report.request_id, bound.request_id);

    DynamicDiscoveryReport::validate_against(&report, &bound, &surface, &evidence, &outcome, NOW)
        .unwrap();
}

#[test]
fn several_honest_candidates_are_admitted_together_and_a_bad_one_does_not_poison_them() {
    // Three distinct operations, so each candidate is refused for its own reason rather than
    // colliding on the duplicate rule first.
    let operations = [
        "context.search",
        "context.repo_map",
        "context.artifact_graph.read",
    ];
    let wide = DiscoverySurface::new(
        PRINCIPAL,
        PROJECT,
        3,
        BTreeSet::from(["query".to_owned()]),
        scope(&operations, &["context"]),
        ExtensionEffect::ReadWrite,
        operations
            .iter()
            .map(|operation| {
                (
                    (*operation).to_owned(),
                    registered(operation, ExtensionEffect::ReadOnly),
                )
            })
            .collect::<BTreeMap<_, _>>(),
        digest("catalog"),
        false,
        NOW + 60_000,
    )
    .expect("surface is valid");

    let at = |operation: &str| {
        let mut candidate = honest_candidate();
        candidate.operation = operation.to_owned();
        candidate.schema_digest = digest(&format!("schema-{operation}"));
        candidate
    };

    // Middle entry: a genuine supply-chain drift, refused while its two honest neighbours survive.
    let mut drifted = at("context.repo_map");
    drifted.observed_manifest_digest = digest("manifest-after-upgrade").to_owned();

    let bound = reissued(request_for(&surface()), &wide);
    let evidence = evidence_map(honest_evidence());
    let outcome = completed(vec![
        at("context.search"),
        drifted,
        at("context.artifact_graph.read"),
    ]);
    let report = evaluate(&bound, &wide, &evidence, &outcome);

    assert_eq!(report.status, DiscoveryStatus::Admitted);
    assert_eq!(
        report.denial_codes(),
        vec!["extension_manifest_digest_mismatch"]
    );
    assert_eq!(report.denials[0].candidate_ref, "context.repo_map");
    let admitted_operations = report
        .promotable()
        .iter()
        .map(|candidate| candidate.operation.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        admitted_operations,
        vec![
            "context.search".to_owned(),
            "context.artifact_graph.read".to_owned()
        ]
    );

    DynamicDiscoveryReport::validate_against(&report, &bound, &wide, &evidence, &outcome, NOW)
        .unwrap();
}

#[test]
fn the_five_tool_model_surface_is_untouched_by_any_of_this() {
    // The admitted operations here are `context.*`, reached through the existing brokered
    // capability path rather than as new model-visible tool schemas. This assertion exists so a
    // future change that starts *registering* discovered tools as model-visible schemas has to
    // delete a test rather than slip in unnoticed.
    let surface = surface();
    let report = evaluate(
        &request_for(&surface),
        &surface,
        &evidence_map(honest_evidence()),
        &completed(vec![honest_candidate()]),
    );
    let admitted_names = report
        .promotable()
        .iter()
        .map(|candidate| candidate.tool_name.clone())
        .collect::<Vec<_>>();
    for frozen in [
        "shell",
        "apply_patch",
        "mcp",
        "memory.search",
        "memory.write",
    ] {
        assert!(
            !admitted_names.contains(&frozen.to_owned()),
            "this slice must not mint a model-visible tool named {frozen}"
        );
    }
    assert_eq!(admitted_names, vec!["acme.index.context".to_owned()]);
}
