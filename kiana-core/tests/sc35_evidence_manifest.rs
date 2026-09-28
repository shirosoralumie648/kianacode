//! SC-35 evidence manifest: deny-first.
//!
//! The order matters. Every refusal below is reached through the public constructors and the two
//! public checks, so a test that passes is a statement about the shape a caller actually sees. The
//! two success paths are last on purpose: until every way of making an empty or flattering claim
//! has been refused, a passing manifest proves very little.

use kiana_core::{
    CommandInvocation, EnvironmentFact, EvidenceFeatureStatus, EvidenceManifest, EvidenceProofLevel,
    FixtureKind, FixtureRef, FixtureRegistry, EVIDENCE_MANIFEST_SCHEMA, FIXTURE_REGISTRY_SCHEMA,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn fixture(path: &str) -> FixtureRef {
    FixtureRef::new(path, FixtureKind::Fixture, DIGEST_A)
}

fn registry() -> FixtureRegistry {
    FixtureRegistry::new(vec![
        fixture("kiana-core/tests/fixtures/one.json"),
        FixtureRef::new(
            "kiana-core/tests/fixtures/two.json",
            FixtureKind::Cassette,
            DIGEST_B,
        ),
    ])
    .expect("registry")
}

/// A manifest that passes every check. Each deny test below breaks exactly one thing about it.
fn manifest() -> EvidenceManifest {
    EvidenceManifest::new(
        "sc35-fixture",
        "0be643aa",
        "recorder",
        "reviewer",
        CommandInvocation::new(
            vec!["cargo".to_owned(), "test".to_owned(), "-p".to_owned(), "kiana-core".to_owned()],
            "/repo",
            Some(0),
        ),
        vec![EnvironmentFact::new("CI", "github-actions")],
        vec![fixture("kiana-core/tests/fixtures/one.json")],
        "",
        "row moved to in-progress",
        EvidenceFeatureStatus::Partial,
        EvidenceProofLevel::Source,
        vec!["no runtime behaviour was observed".to_owned()],
        None,
    )
    .expect("baseline manifest")
}

// ---------------------------------------------------------------------------
// The five things the card says go missing, each refused on its own.
// ---------------------------------------------------------------------------

#[test]
fn a_manifest_without_a_command_is_refused() {
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "reviewer",
            CommandInvocation::new(Vec::new(), "/repo", Some(0)),
            vec![EnvironmentFact::new("CI", "github-actions")],
            vec![fixture("kiana-core/tests/fixtures/one.json")],
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            vec!["limitation".to_owned()],
            None,
        )
        .unwrap_err(),
        "evidence_manifest_command_required"
    );
}

#[test]
fn a_manifest_whose_command_never_produced_an_exit_code_is_refused() {
    // `None` is the honest representation of "the run was cancelled or never started". A
    // sentinel like -1 would be indistinguishable from a real process exit.
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "reviewer",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", None),
            vec![EnvironmentFact::new("CI", "github-actions")],
            vec![fixture("kiana-core/tests/fixtures/one.json")],
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            vec!["limitation".to_owned()],
            None,
        )
        .unwrap_err(),
        "evidence_manifest_exit_code_required"
    );
}

#[test]
fn a_manifest_without_an_environment_is_refused() {
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "reviewer",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
            Vec::new(),
            vec![fixture("kiana-core/tests/fixtures/one.json")],
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            vec!["limitation".to_owned()],
            None,
        )
        .unwrap_err(),
        "evidence_manifest_environment_required"
    );
}

#[test]
fn a_manifest_without_a_source_snapshot_is_refused() {
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "",
            "recorder",
            "reviewer",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
            vec![EnvironmentFact::new("CI", "github-actions")],
            vec![fixture("kiana-core/tests/fixtures/one.json")],
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            vec!["limitation".to_owned()],
            None,
        )
        .unwrap_err(),
        "evidence_manifest_source_snapshot_invalid"
    );
}

#[test]
fn a_manifest_with_no_fixture_and_no_reason_is_refused() {
    // "We checked nothing" must not be spelled the same way as "we checked and it was fine".
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "reviewer",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
            vec![EnvironmentFact::new("CI", "github-actions")],
            Vec::new(),
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            vec!["limitation".to_owned()],
            None,
        )
        .unwrap_err(),
        "evidence_manifest_fixture_required"
    );
}

#[test]
fn a_manifest_with_no_limitations_is_refused() {
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "reviewer",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
            vec![EnvironmentFact::new("CI", "github-actions")],
            vec![fixture("kiana-core/tests/fixtures/one.json")],
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            Vec::new(),
            None,
        )
        .unwrap_err(),
        "evidence_manifest_limitations_required"
    );
}

#[test]
fn a_manifest_whose_reviewer_is_missing_or_is_the_recorder_is_refused() {
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
            vec![EnvironmentFact::new("CI", "github-actions")],
            vec![fixture("kiana-core/tests/fixtures/one.json")],
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            vec!["limitation".to_owned()],
            None,
        )
        .unwrap_err(),
        "evidence_manifest_reviewer_required"
    );
    // Self-review is refused as its own code, not as a malformed reviewer: "nobody checked this"
    // and "the author checked their own work" are different problems.
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "recorder",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
            vec![EnvironmentFact::new("CI", "github-actions")],
            vec![fixture("kiana-core/tests/fixtures/one.json")],
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            vec!["limitation".to_owned()],
            None,
        )
        .unwrap_err(),
        "evidence_manifest_reviewer_self"
    );
}

// ---------------------------------------------------------------------------
// The sixth thing the card names: evidence that was edited by hand.
// ---------------------------------------------------------------------------

#[test]
fn a_hand_edited_manifest_cannot_republish_itself() {
    let mut edited = manifest();
    // The most flattering possible edit: claim a stronger proof level after the fact.
    edited.proof_level = EvidenceProofLevel::Durable;
    // The seal was computed over the original content, so it no longer matches.
    assert_eq!(
        edited.validate().unwrap_err(),
        "evidence_manifest_digest_mismatch"
    );

    let mut relabelled = manifest();
    relabelled.status_change = "proved the gate end to end".to_owned();
    assert_eq!(
        relabelled.validate().unwrap_err(),
        "evidence_manifest_digest_mismatch"
    );
}

#[test]
fn a_fixture_whose_bytes_moved_after_the_claim_is_refused() {
    // The cassette still exists and is still a valid file. It is simply no longer evidence for
    // anything, which is exactly what a digest binding is supposed to notice.
    let claim = manifest();
    let stale = FixtureRegistry::new(vec![FixtureRef::new(
        "kiana-core/tests/fixtures/one.json",
        FixtureKind::Fixture,
        DIGEST_B,
    )])
    .expect("stale registry");
    assert_eq!(
        claim.validate_against(&stale).unwrap_err(),
        "evidence_manifest_fixture_digest_mismatch"
    );
}

#[test]
fn a_manifest_cannot_cite_a_file_the_registry_has_never_seen() {
    let claim = EvidenceManifest::new(
        "id",
        "0be643aa",
        "recorder",
        "reviewer",
        CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
        vec![EnvironmentFact::new("CI", "github-actions")],
        vec![fixture("kiana-core/tests/fixtures/never-registered.json")],
        "",
        "status",
        EvidenceFeatureStatus::Partial,
        EvidenceProofLevel::Source,
        vec!["limitation".to_owned()],
        None,
    )
    .expect("shape is valid on its own");
    assert_eq!(
        claim.validate_against(&registry()).unwrap_err(),
        "evidence_manifest_fixture_unknown"
    );
}

#[test]
fn a_fixture_relabelled_from_cassette_to_fixture_is_refused() {
    // A cassette is a recording of a real run; a fixture is something a human wrote. Swapping the
    // label makes a stale recording look like a designed input.
    let claim = EvidenceManifest::new(
        "id",
        "0be643aa",
        "recorder",
        "reviewer",
        CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
        vec![EnvironmentFact::new("CI", "github-actions")],
        vec![FixtureRef::new(
            "kiana-core/tests/fixtures/two.json",
            FixtureKind::Fixture,
            DIGEST_B,
        )],
        "",
        "status",
        EvidenceFeatureStatus::Partial,
        EvidenceProofLevel::Source,
        vec!["limitation".to_owned()],
        None,
    )
    .expect("shape is valid on its own");
    assert_eq!(
        claim.validate_against(&registry()).unwrap_err(),
        "evidence_manifest_fixture_kind_conflict"
    );
}

// ---------------------------------------------------------------------------
// A claim may not be stronger than what it is about.
// ---------------------------------------------------------------------------

#[test]
fn a_deferred_thing_cannot_carry_a_durable_claim() {
    assert_eq!(
        EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "reviewer",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
            vec![EnvironmentFact::new("CI", "github-actions")],
            vec![fixture("kiana-core/tests/fixtures/one.json")],
            "",
            "status",
            EvidenceFeatureStatus::Deferred,
            EvidenceProofLevel::Durable,
            vec!["limitation".to_owned()],
            None,
        )
        .unwrap_err(),
        "evidence_manifest_proof_above_feature"
    );
}

#[test]
fn an_absolute_or_escaping_fixture_path_is_refused() {
    for path in ["/etc/passwd", "../outside.json", "https://example.com/x.json"] {
        let claim = EvidenceManifest::new(
            "id",
            "0be643aa",
            "recorder",
            "reviewer",
            CommandInvocation::new(vec!["cargo".to_owned()], "/repo", Some(0)),
            vec![EnvironmentFact::new("CI", "github-actions")],
            vec![FixtureRef::new(path, FixtureKind::Fixture, DIGEST_A)],
            "",
            "status",
            EvidenceFeatureStatus::Partial,
            EvidenceProofLevel::Source,
            vec!["limitation".to_owned()],
            None,
        );
        assert_eq!(
            claim.unwrap_err(),
            "evidence_manifest_fixture_path_invalid",
            "path {path} should be refused"
        );
    }
}

// ---------------------------------------------------------------------------
// The chain: a sequence of claims, not a pile of them.
// ---------------------------------------------------------------------------

#[test]
fn a_manifest_that_claims_a_predecessor_it_does_not_supply_is_refused() {
    let mut orphan = manifest();
    orphan.previous_manifest_digest = Some(DIGEST_A.to_owned());
    orphan.manifest_digest = orphan.digest();
    // The link is asserted and nothing it points at was supplied. That is what a forged chain
    // looks like, so it is refused rather than treated as "the first manifest".
    assert_eq!(
        orphan.verify_chain(None).unwrap_err(),
        "evidence_manifest_chain_orphan"
    );
}

#[test]
fn a_manifest_whose_predecessor_digest_does_not_match_is_refused() {
    let first = manifest();
    let mut second = manifest();
    second.manifest_id = "sc35-second".to_owned();
    second.previous_manifest_digest = Some(DIGEST_A.to_owned());
    second.manifest_digest = second.digest();
    assert_eq!(
        second.verify_chain(Some(&first)).unwrap_err(),
        "evidence_manifest_chain_broken"
    );
}

// ---------------------------------------------------------------------------
// Success paths, last.
// ---------------------------------------------------------------------------

#[test]
fn a_complete_manifest_validates_and_binds_to_its_registry() {
    let claim = manifest();
    claim.validate().expect("shape");
    claim.validate_against(&registry()).expect("registry binding");
    assert_eq!(claim.schema, EVIDENCE_MANIFEST_SCHEMA);
    assert_eq!(claim.manifest_digest, claim.digest());
    assert_eq!(registry().schema, FIXTURE_REGISTRY_SCHEMA);
    // A documentation-only claim is allowed to have no fixture, but only if it says so.
    let docs_only = EvidenceManifest::new(
        "sc35-docs",
        "0be643aa",
        "recorder",
        "reviewer",
        CommandInvocation::new(vec!["git".to_owned(), "diff".to_owned()], "/repo", Some(0)),
        vec![EnvironmentFact::new("CI", "github-actions")],
        Vec::new(),
        "documentation only, no fixture is cited",
        "docs updated",
        EvidenceFeatureStatus::Partial,
        EvidenceProofLevel::Source,
        vec!["nothing was executed".to_owned()],
        None,
    )
    .expect("docs-only manifest");
    docs_only.validate().expect("docs-only shape");
}

#[test]
fn a_chain_of_two_manifests_verifies() {
    let first = manifest();
    let second = EvidenceManifest::new(
        "sc35-second",
        "0be643aa",
        "recorder",
        "reviewer",
        CommandInvocation::new(vec!["cargo".to_owned(), "test".to_owned()], "/repo", Some(0)),
        vec![EnvironmentFact::new("CI", "github-actions")],
        vec![fixture("kiana-core/tests/fixtures/one.json")],
        "",
        "second status",
        EvidenceFeatureStatus::Partial,
        EvidenceProofLevel::Source,
        vec!["still source only".to_owned()],
        Some(first.manifest_digest.clone()),
    )
    .expect("second manifest");
    second.verify_chain(Some(&first)).expect("chain");
    // The identity is part of the link: a manifest that follows itself is not a chain.
    let mut self_following = manifest();
    self_following.previous_manifest_digest = Some(self_following.manifest_digest.clone());
    self_following.manifest_digest = self_following.digest();
    let error = self_following
        .verify_chain(Some(&self_following.clone()))
        .unwrap_err();
    assert_eq!(error, "evidence_manifest_chain_identity");
}
