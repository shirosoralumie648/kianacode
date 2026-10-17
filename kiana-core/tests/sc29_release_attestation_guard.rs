//! SC-29 source guard for release artifact, provenance and signature binding.

#[test]
fn release_attestation_keeps_subject_builder_and_transparency_bindings() {
    let domain = include_str!("../../kiana-domain/src/release_attestation.rs");
    let verifier = include_str!("../../scripts/verify-release-provenance.py");
    let workflow = include_str!("../../.github/workflows/sc29-release-provenance.yml");
    let status = include_str!("../../CURRENT_STATUS.md");
    let evidence = status
        .split("### 2026-10-17 SC-29")
        .nth(1)
        .expect("SC-29 evidence section")
        .split("\n### ")
        .next()
        .unwrap();
    let baseline = include_str!("../../docs/roadmap/sc29-release-provenance-baseline.md");
    for marker in [
        "ReleaseManifest",
        "ReleaseProvenance",
        "ReleaseSignatureAttestation",
        "subject_manifest_digest",
        "transparency_log_entry_digest",
        "release_verification_builder_mismatch",
        "release_verification_toolchain_mismatch",
        "release_verification_signature_unverified",
        "release_verification_tag_mismatch",
        "ReleaseVerificationStatus::Unknown",
        "verify_bytes",
        "release_artifact_digest_mismatch",
        "release_artifact_size_mismatch",
        "release_manifest_artifact_tag_mismatch",
        "release_verification_source_tree_digest_mismatch",
        "release_verification_cargo_lock_digest_mismatch",
        "release_provenance_build_type_invalid",
        "first_mismatched_version_token",
    ] {
        assert!(
            domain.contains(marker),
            "SC-29 domain marker missing: {marker}"
        );
    }
    for marker in [
        "manifest_digest",
        "source_revision",
        "builder_id",
        "toolchain_digest",
        "transparency_log_entry_digest",
        "release_verification_artifact_digest_mismatch",
        "release_verification_artifact_size_mismatch",
        "unknown_field",
        "self_test_forged_tag_accepted",
    ] {
        assert!(
            verifier.contains(marker),
            "SC-29 verifier marker missing: {marker}"
        );
    }
    for marker in [
        "contents: read",
        "verify-release-provenance.py --self-test",
        "cargo test -p kiana-domain --test sc29_release_attestation",
        "cargo test -p kiana-core --test sc29_release_attestation_guard",
        "cargo fmt --all --check",
        "cargo fetch --locked",
        "python3 -B scripts/tests/test_sc29_release_provenance.py",
        "bash scripts/validate-sc29-release-provenance.sh",
        "--no-run",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-29 workflow marker missing: {marker}"
        );
    }
    for marker in [
        "ReleaseManifest",
        "SLSA",
        "transparency",
        "builder",
        "toolchain",
        "subject digest",
        "source",
        "limitations",
        "reviewer",
    ] {
        assert!(
            if marker == "reviewer" || marker == "limitations" {
                evidence.contains(&format!("{marker}:"))
            } else {
                baseline.contains(marker)
            },
            "SC-29 baseline marker missing: {marker}"
        );
    }
    assert!(!verifier.contains("requests."));
    assert!(!verifier.contains("urllib."));
    assert!(!verifier.contains("subprocess"));
    assert!(!verifier.contains("socket"));
    // The evidence boundary stays pure: the domain contract never signs, publishes,
    // reads the filesystem, spawns a process or reaches the network.
    assert!(!domain.contains("reqwest"));
    assert!(!domain.contains("std::fs"));
    assert!(!domain.contains("std::process"));
    assert!(!domain.contains("tokio"));
}

/// The SC-29 fixtures, refusal codes and lane wiring are bound to one another. `include_str!`
/// hits are text checks, not runtime proof.
#[test]
fn sc29_deny_first_fixtures_codes_and_lane_are_wired() {
    let domain = include_str!("../../kiana-domain/src/release_attestation.rs");
    let domain_fixture = include_str!("../../kiana-domain/tests/sc29_release_attestation.rs");
    let verifier_fixture = include_str!("../../scripts/tests/test_sc29_release_provenance.py");
    let validator = include_str!("../../scripts/validate-sc29-release-provenance.sh");
    let workflow = include_str!("../../.github/workflows/sc29-release-provenance.yml");
    let ci = include_str!("../../.github/workflows/ci.yml");
    let baseline = include_str!("../../docs/roadmap/sc29-release-provenance-baseline.md");
    let card = include_str!("../../docs/roadmap/security-compliance.md");
    let roadmap = include_str!("../../docs/roadmap.md");
    let current_status = include_str!("../../CURRENT_STATUS.md");

    // Fixture names, deny-first order: the single success fixture is last on both surfaces.
    for fixture in [
        "forged_artifact_filenames_cannot_claim_a_different_version_than_the_tag",
        "builder_source_and_toolchain_drift_are_blocked_before_subject_binding",
        "subject_digest_mismatch_is_blocked_for_provenance_signature_and_artifact_bytes",
        "unverified_or_missing_transparency_stays_unknown_and_unknown_fields_fail",
        "artifact_names_cannot_smuggle_paths_and_digest_drift_is_rejected",
        "release_contracts_stay_bounded_strict_and_digest_bound",
        "exact_manifest_provenance_signature_and_transparency_binding_verifies",
    ] {
        assert!(
            domain_fixture.contains(fixture),
            "SC-29 domain fixture missing: {fixture}"
        );
    }
    for fixture in [
        "test_missing_or_malformed_evidence_is_refused",
        "test_unknown_fields_are_refused_before_any_binding_check",
        "test_forged_tag_and_filename_are_refused",
        "test_builder_source_and_toolchain_drift_are_refused",
        "test_subject_and_recomputed_digest_mismatch_are_refused",
        "test_artifact_bytes_must_match_name_size_and_digest",
        "test_unverified_or_unlogged_signatures_are_refused",
        "test_version_and_bounds_are_enforced",
        "test_verifier_stays_offline_and_does_not_mutate_evidence",
        "test_valid_fixture_verifies_last_and_output_is_deterministic",
    ] {
        assert!(
            verifier_fixture.contains(fixture),
            "SC-29 verifier fixture missing: {fixture}"
        );
    }
    let success = domain_fixture
        .find("fn exact_manifest_provenance_signature_and_transparency_binding_verifies")
        .expect("domain success fixture");
    for denial in [
        "fn forged_artifact_filenames_cannot_claim_a_different_version_than_the_tag",
        "fn builder_source_and_toolchain_drift_are_blocked_before_subject_binding",
        "fn subject_digest_mismatch_is_blocked_for_provenance_signature_and_artifact_bytes",
        "fn release_contracts_stay_bounded_strict_and_digest_bound",
    ] {
        assert!(
            domain_fixture.find(denial).unwrap() < success,
            "SC-29 deny-first ordering broken: {denial}"
        );
    }
    let verifier_success = verifier_fixture
        .find("def test_valid_fixture_verifies_last_and_output_is_deterministic")
        .expect("verifier success fixture");
    for denial in [
        "def test_missing_or_malformed_evidence_is_refused",
        "def test_unknown_fields_are_refused_before_any_binding_check",
        "def test_forged_tag_and_filename_are_refused",
        "def test_builder_source_and_toolchain_drift_are_refused",
        "def test_subject_and_recomputed_digest_mismatch_are_refused",
        "def test_artifact_bytes_must_match_name_size_and_digest",
        "def test_unverified_or_unlogged_signatures_are_refused",
        "def test_version_and_bounds_are_enforced",
        "def test_verifier_stays_offline_and_does_not_mutate_evidence",
    ] {
        assert!(
            verifier_fixture.find(denial).unwrap() < verifier_success,
            "SC-29 deny-first ordering broken: {denial}"
        );
    }

    // Stable refusal codes stay pinned by name in the fixtures that prove them.
    for code in [
        "release_manifest_tag_invalid",
        "release_manifest_artifact_tag_mismatch",
        "release_manifest_artifact_duplicate",
        "release_manifest_artifact_count_invalid",
        "release_manifest_digest_mismatch",
        "release_manifest_header_invalid",
        "release_manifest_source_tree_digest_invalid",
        "release_artifact_name_invalid",
        "release_artifact_digest_mismatch",
        "release_artifact_size_mismatch",
        "release_artifact_observed_digest_invalid",
        "release_provenance_build_type_invalid",
        "release_provenance_material_duplicate",
        "release_provenance_digest_mismatch",
        "release_verification_tag_mismatch",
        "release_verification_source_revision_mismatch",
        "release_verification_builder_mismatch",
        "release_verification_toolchain_mismatch",
        "release_verification_source_tree_digest_mismatch",
        "release_verification_cargo_lock_digest_mismatch",
        "release_verification_provenance_subject_mismatch",
        "release_verification_signature_subject_mismatch",
        "release_verification_signature_unverified",
        "release_verification_transparency_missing",
        "release_verification_artifact_name_mismatch",
        "release_verification_artifact_digest_mismatch",
        "release_verification_artifact_size_mismatch",
        "release_verification_artifact_missing",
        "release_verification_artifact_argument_invalid",
        "release_manifest_unknown_field",
        "release_artifact_unknown_field",
        "release_provenance_unknown_field",
        "release_material_unknown_field",
        "release_signature_unknown_field",
        "release_manifest_read_failed",
        "release_signature_attestation_digest_mismatch",
        "release_signature_transparency_digest_invalid",
        "release_signature_algorithm_invalid",
        "release_signature_verified_invalid",
        "release_artifact_size_invalid",
        "release_provenance_material_count_invalid",
        "release_manifest_builder_id_invalid",
        "release_provenance_header_invalid",
        "release_signature_header_invalid",
        "release_verification_toolchain_digest_mismatch",
    ] {
        assert!(
            domain_fixture.contains(code) || verifier_fixture.contains(code),
            "SC-29 fixture no longer pins refusal code: {code}"
        );
    }
    assert!(
        verifier_fixture.contains("release_manifest_artifact_tag_mismatch")
            || domain_fixture.contains("release_manifest_artifact_tag_mismatch"),
        "the filename/tag forgery path must stay pinned"
    );
    // Single authority: the domain contract must not grow a signer, a publisher, a
    // filesystem reader or a second verification loop.
    assert!(!domain.contains("fn sign"));
    assert!(!domain.contains("fn publish"));
    assert!(!domain.contains("fn upload"));
    assert!(!domain_fixture.contains("#[ignore]"));
    assert!(!verifier_fixture.contains("unittest.skip"));

    // Manual lane stays dispatch-only, locked, serial and fail-closed.
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "python3 -B scripts/tests/test_sc29_release_provenance.py",
        "python3 scripts/verify-release-provenance.py --self-test",
        "bash scripts/validate-sc29-release-provenance.sh",
        "cargo test -p kiana-domain --test sc29_release_attestation --locked -- --test-threads=1",
        "cargo test -p kiana-core --test sc29_release_attestation_guard --locked -- --test-threads=1",
        "--no-run",
    ] {
        assert!(
            workflow.contains(marker),
            "SC-29 lane marker missing: {marker}"
        );
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "schedule:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !workflow.contains(forbidden),
            "SC-29 lane must stay manual and fail-closed: {forbidden}"
        );
    }
    assert!(validator.contains("test_sc29_release_provenance.py"));
    assert!(validator.contains("verify-release-provenance.py --self-test"));
    assert!(ci.contains("bash scripts/validate-sc29-release-provenance.sh"));

    // Auto shards still claim the integration targets.
    let shards: serde_json::Value =
        serde_json::from_str(include_str!("../../scripts/ci/test-shards.json"))
            .expect("shard registry json");
    let domain_targets = shards["kiana-domain-s4/4"]["targets"]
        .as_array()
        .expect("domain shard targets");
    assert!(domain_targets
        .iter()
        .any(|target| target.as_str() == Some("sc29_release_attestation")));
    let core_targets = shards["kiana-core-s6/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("sc29_release_attestation_guard")));

    // Bookkeeping anchors stay discoverable.
    assert!(baseline.contains("2026-10-17 completion slice (CI-only)"));
    assert!(baseline.contains("release_manifest_artifact_tag_mismatch"));
    assert!(card.contains("<a id=\"step-sc-29\"></a>SC-29"));
    assert!(roadmap.contains("[`SC-29`](roadmap/security-compliance.md#step-sc-29)"));
    assert!(current_status.contains("### 2026-10-17 SC-29"));
}
