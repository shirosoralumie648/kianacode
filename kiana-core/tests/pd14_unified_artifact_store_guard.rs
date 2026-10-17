//! PD-14 source guard and lane wiring for the unified content-addressed artifact store.
//!
//! The behaviour fixtures prove what the store does; this file proves the tree still contains the
//! surface those fixtures drive, that both artifact port surfaces are served by one implementation,
//! and that the fixtures, the focused lane and the shard registry are actually wired to each other.

/// Fixtures in `pd14_unified_artifact_store.rs`. Each one must be defined there, listed in the
/// baseline document and invoked by the focused lane, which is what keeps the three in sync.
const FIXTURES: [&str; 11] = [
    "open_refuses_relative_roots_traversal_symlinks_scope_and_limits",
    "stage_refuses_size_digest_bound_scope_and_version_drift_before_writing",
    "staged_content_is_unreadable_until_a_matching_commit",
    "identical_bytes_are_deduplicated_but_scopes_never_merge",
    "symlinked_hardlinked_and_non_regular_files_are_refused",
    "a_replaced_root_path_cannot_redirect_an_open_store",
    "pre_existing_blob_drift_and_conflicting_writers_fail_closed",
    "file_sync_failure_denies_the_stage_and_publishes_nothing",
    "directory_sync_failure_is_reported_as_unknown_and_the_retry_is_idempotent",
    "legacy_artifact_port_and_manifest_surface_agree",
    "reopen_preserves_the_published_manifest",
];

#[test]
fn unified_artifact_store_keeps_the_deny_boundaries_and_one_implementation() {
    let store = include_str!("../src/artifact_store.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");
    let helpers = include_str!("../src/artifacts.rs");
    let exports = include_str!("../src/lib.rs");

    // One implementation serves both artifact port surfaces. Descriptor-pinned opens, no-replace
    // renames and directory syncs are what make the publish atomic, so the primitives must stay.
    for marker in [
        "impl ContentAddressedArtifactStorePort for UnifiedArtifactStore",
        "impl ArtifactStorePort for UnifiedArtifactStore",
        "spawn_blocking",
        "O_NOFOLLOW",
        "LINUX_RENAME_NOREPLACE",
        "linux_artifact_sync_directory",
        "rename_noreplace",
        "read_regular",
        "ArtifactManifest",
    ] {
        assert!(
            store.contains(marker),
            "PD-14 store marker missing: {marker}"
        );
    }

    // Every denial the card names is a stable reason the store itself produces.
    for reason in [
        "artifact_root_must_be_absolute",
        "artifact_root_component_invalid",
        "artifact_owner_scope_invalid",
        "artifact_store_limits_invalid",
        "artifact_scope_not_owned",
        "artifact_content_too_large",
        "artifact_content_size_mismatch",
        "artifact_content_hash_mismatch",
        "artifact_content_hash_invalid",
        "artifact_version_conflict",
        "artifact_version_already_staged",
        "artifact_manifest_revision_conflict",
        "artifact_reference_manifest_mismatch",
        "artifact_manifest_digest_mismatch",
        "artifact_manifest_missing",
        "artifact_version_not_committed",
        "artifact_version_not_staged",
        "artifact_file_symlinked",
        "artifact_file_hardlinked",
        "artifact_file_not_regular",
        "artifact_sync_failed",
        "result_unknown:artifact_directory_sync_failed",
        "artifact_store_platform_unsupported",
    ] {
        assert!(
            store.contains(reason),
            "PD-14 denial reason missing: {reason}"
        );
    }

    // No path re-resolution, no shelling out, no reuse of the in-memory adapter and no pretend
    // erasure: each would be a second write or authority surface for the same bytes.
    for forbidden in [
        "fn invalidate_artifact",
        "tokio::fs",
        "std::fs::rename(",
        "canonicalize",
        "Command::new",
        "MemoryArtifactStore",
        "kiana_eventlog",
    ] {
        assert!(
            !store.contains(forbidden),
            "PD-14 forbidden marker present: {forbidden}"
        );
    }

    // The syscall helpers are shared with the daemon-side atomic writer rather than duplicated.
    for helper in [
        "pub(crate) const LINUX_RENAME_NOREPLACE",
        "pub(crate) fn linux_create_artifact_temp",
        "pub(crate) fn linux_artifact_renameat2",
        "pub(crate) fn linux_artifact_unlinkat",
        "pub(crate) fn linux_artifact_sync_directory",
    ] {
        assert!(
            helpers.contains(helper),
            "PD-14 shared helper missing: {helper}"
        );
    }

    // The port contract and manifest schema live in kiana-ports, and the store is exported from
    // the core crate root.
    for marker in [
        "pub const ARTIFACT_MANIFEST_SCHEMA",
        "pub struct ArtifactManifest",
        "pub trait ContentAddressedArtifactStorePort",
        "content_addressed_artifact_store_unsupported",
        "blob_names_version_content",
        "manifest_digest",
    ] {
        assert!(
            ports.contains(marker),
            "PD-14 port marker missing: {marker}"
        );
    }
    assert!(exports.contains("mod artifact_store;"));
    assert!(exports.contains("pub use artifact_store::"));
}

#[test]
fn pd14_fixtures_are_wired_into_the_artifact_store_lane() {
    let fixtures_source = include_str!("pd14_unified_artifact_store.rs");
    let workflow = include_str!("../../.github/workflows/pd14-artifact-store.yml");
    let shards = include_str!("../../scripts/ci/test-shards.json");
    let baseline = include_str!("../../docs/roadmap/pd14-unified-artifact-store-baseline.md");
    let ports_fixtures = include_str!("../../kiana-ports/tests/pd14_artifact_manifest_port.rs");

    for fixture in FIXTURES {
        assert!(
            fixtures_source.contains(format!("fn {fixture}").as_str()),
            "PD-14 fixture is not defined in the behaviour target: {fixture}"
        );
        assert!(
            baseline.contains(fixture),
            "PD-14 baseline does not name the fixture: {fixture}"
        );
        assert!(
            workflow.contains(fixture),
            "PD-14 focused lane does not run the fixture: {fixture}"
        );
    }

    for command in [
        "cargo test -p kiana-core --test pd14_unified_artifact_store --locked -- --test-threads=1",
        "cargo test -p kiana-core --test pd14_unified_artifact_store_guard --locked -- --test-threads=1",
        "cargo test -p kiana-ports --test pd14_artifact_manifest_port --locked -- --test-threads=1",
        "cargo test -p kiana-core --test pd14_unified_artifact_store --no-run --locked",
        "cargo test -p kiana-core --test pd14_unified_artifact_store_guard --no-run --locked",
        "cargo test -p kiana-ports --test pd14_artifact_manifest_port --no-run --locked",
        "--exact",
        "--test-threads=1",
        "cargo fmt --all --check",
    ] {
        assert!(
            workflow.contains(command),
            "PD-14 focused lane command missing: {command}"
        );
    }
    for forbidden in ["#[ignore]", "continue-on-error"] {
        assert!(
            !workflow.contains(forbidden),
            "PD-14 focused lane must not contain: {forbidden}"
        );
    }

    for target in [
        "pd14_unified_artifact_store",
        "pd14_unified_artifact_store_guard",
    ] {
        assert!(
            shards.contains(format!("\"{target}\"").as_str()),
            "PD-14 core target is not registered in a test shard: {target}"
        );
    }
    assert!(
        ports_fixtures.contains("fn manifest_rejects_blob_digest_schema_version_and_seal_drift"),
        "PD-14 port contract fixture is missing from kiana-ports"
    );
    assert!(
        ports_fixtures
            .contains("fn manifest_accepts_both_digest_spellings_and_round_trips_through_json"),
        "PD-14 port contract fixture is missing from kiana-ports"
    );
    assert!(
        ports_fixtures.contains("fn port_defaults_deny_with_one_stable_reason"),
        "PD-14 port contract fixture is missing from kiana-ports"
    );
}
