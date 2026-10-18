//! DEP-03 source guards: the compatibility matrix stays a pure, deny-only contract on the single
//! ControlPlane path.
//!
//! The first guard pins the contract surface: all twelve axes, every stable reason code that the
//! fixtures assert (including the exact reason codes added by the uncovered-reason slice and the
//! store-format bound), and the effect boundaries the matrix must never cross. The second guard
//! pins the wiring: schema registration, the deny-first fixture order and the explicit app_build
//! compatibility fixture, the manual lane, and the retired doc-reference exemptions.

#[path = "support/source_guard.rs"]
mod source_guard;

#[test]
fn dep03_compatibility_matrix_stays_pure_and_explicit() {
    let domain = include_str!("../../kiana-domain/src/deployment_compatibility.rs");
    let core = include_str!("../src/deployment_compatibility.rs");
    for marker in [
        "DeploymentVersionAxes",
        "DeploymentCompatibilityMatrix",
        "DEPLOYMENT_COMPATIBILITY_SCHEMA",
        "app_build",
        "protocol_version",
        "domain_schema",
        "store_format",
        "projection_version",
        "workflow_definition_digest",
        "provider_route_digest",
        "extension_digest",
        "config_revision",
        "authority_epoch",
        "data_epoch",
        "generation",
        // Stable reason codes: the twelve axes produce exactly these denials.
        "protocol_major_mismatch",
        "domain_schema_major_mismatch",
        "projection_major_mismatch",
        "projection_version_downgrade",
        "store_format_downgrade",
        "store_format_unsupported",
        "authority_epoch_rollback",
        "data_epoch_rollback",
        "generation_rollback",
        "workflow_definition_drift",
        "provider_route_drift",
        "extension_digest_drift",
        "config_revision_drift",
        "MAX_SUPPORTED_STORE_FORMAT",
        "validate_deployment_compatibility",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker),
            "DEP-03 marker missing: {marker}"
        );
    }
    // The unsupported-format bound is derived from the layout the workspace writes, not invented
    // by the caller.
    assert!(
        domain.contains("MAX_SUPPORTED_STORE_FORMAT: u32 = crate::JOURNAL_WRITER_VERSION"),
        "the store-format bound must be tied to JOURNAL_WRITER_VERSION"
    );
    // app_build is digest-bound. It is compared only through the axes digest, so the reason
    // function must never push an app_build-specific reason.
    let reasons = source_guard::item_source(domain, "fn compatibility_reasons(");
    assert!(
        !reasons.contains("app_build"),
        "app_build must stay non-blocking: compatibility_reasons must not read it"
    );
    assert!(
        source_guard::item_source(domain, "pub fn digest(&self) -> String").contains("app_build"),
        "app_build must stay digest-bound through the axes digest"
    );
    for forbidden in [
        "std::fs",
        "std::process::Command",
        "tokio::",
        "CapabilityBroker",
        "EventStore::append",
        "apply_migration",
        "start_revision",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "DEP-03 compatibility path crossed an effect boundary: {forbidden}"
        );
    }
}

/// DEP-03 completion slice: the schema is registered in the canonical registry, the fixtures are
/// deny-first with the card-explicit app_build success fixture last, the manual lane compiles and
/// fetches locked dependencies, and neither resolved doc-reference exemption is still claimed.
#[test]
fn dep03_compatibility_is_registered_deny_first_and_lane_covered() {
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let fixture = include_str!("../../kiana-domain/tests/dep03_compatibility.rs");
    let workflow = include_str!("../../.github/workflows/dep03-compatibility.yml");
    let exemptions = include_str!("../../scripts/ci/doc-reference-exemptions.txt");

    // The canonical registry owns the schema name; the module constant must match it verbatim.
    let domain = include_str!("../../kiana-domain/src/deployment_compatibility.rs");
    assert!(domain.contains(
        r#"DEPLOYMENT_COMPATIBILITY_SCHEMA: &str = "kiana.deployment-compatibility.v1""#
    ));
    assert!(
        contracts.contains(r#"name: "kiana.deployment-compatibility.v1","#),
        "kiana.deployment-compatibility.v1 must be registered in SCHEMA_CONTRACTS"
    );
    let registry_entry = contracts
        .find(r#"name: "kiana.deployment-compatibility.v1","#)
        .expect("registry entry");
    let tail = &contracts[registry_entry..];
    let entry_end = tail.find("},").expect("registry entry terminator");
    let entry = &tail[..entry_end];
    assert!(entry.contains("SchemaLayer::Domain"));
    assert!(entry.contains(r#"owner_crate: "kiana-domain""#));
    assert!(
        entry.contains("allow_unknown_fields: false"),
        "the matrix denies unknown fields, so the registry entry must too"
    );

    // Deny-first order: the store/app_build decisions and every refusal fixture precede the two
    // success fixtures, and the card-explicit app_build fixture is the final test.
    for fixture_name in [
        "deployment_compatibility_schema_is_registered_and_unknown_major_fails_closed",
        "unsupported_store_format_is_named_and_blocked",
        "unknown_major_downgrade_and_digest_drift_are_blocked",
        "config_revision_drift_is_blocked",
        "projection_minor_lag_is_blocked",
        "epoch_generation_and_schema_major_rollbacks_are_blocked",
        "status_and_reasons_cannot_be_forged",
        "identical_axes_are_compatible_and_round_trip",
        "differing_app_build_stays_compatible_with_empty_reasons",
    ] {
        assert!(
            fixture.contains(fixture_name),
            "DEP-03 domain fixture missing: {fixture_name}"
        );
    }
    let deny_first = fixture.find("unsupported_store_format_is_named_and_blocked");
    let app_build_success = fixture.find("differing_app_build_stays_compatible_with_empty_reasons");
    assert!(
        deny_first.is_some() && app_build_success.is_some() && deny_first < app_build_success,
        "the store-format refusal fixture must precede the app_build success fixture"
    );
    assert!(
        app_build_success.is_some_and(|position| { !fixture[position..].contains("#[test]") }),
        "the card-explicit app_build fixture must stay the final test"
    );
    assert!(!fixture.contains("#[ignore]"));
    // The explicit store decision is a supported-format bound, not downgrade-only acceptance.
    assert!(fixture.contains("MAX_SUPPORTED_STORE_FORMAT + 1"));
    assert!(
        fixture.contains("Compatible") && fixture.contains("matrix.reasons.is_empty()"),
        "the app_build fixture must assert Compatible with empty reasons"
    );

    // The manual lane stays dispatch-only, fetches locked dependencies, runs serially and compiles.
    for marker in [
        "workflow_dispatch:",
        "permissions:\n  contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test dep03_compatibility_guard",
        "cargo test -p kiana-domain --test dep03_compatibility",
        "--test-threads=1",
        "--no-run",
    ] {
        assert!(
            workflow.contains(marker),
            "DEP-03 lane step missing: {marker}"
        );
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "DEP-03 lane must not contain: {forbidden}"
        );
    }

    // Both workflow paths now exist in the tree, so their historical exemption lines must be gone.
    assert!(
        !exemptions.contains("docs/roadmap.md :: .github/workflows/dep03-compatibility.yml"),
        "the resolved roadmap.md exemption must be pruned"
    );
    assert!(
        !exemptions
            .contains("docs/roadmap/dep03-compatibility-baseline.md :: .github/workflows/dep03-compatibility.yml"),
        "the resolved baseline exemption must be pruned"
    );
}
