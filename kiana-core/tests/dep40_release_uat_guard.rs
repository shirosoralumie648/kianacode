//! DEP-40 source guard for cross-entrypoint release/upgrade/recovery UAT.

#[test]
fn uat_matrix_and_entrypoints_keep_one_authoritative_spine() {
    let matrix = include_str!("../../kiana-domain/src/release_uat.rs");
    let evidence = include_str!("../../kiana-domain/src/release_uat_evidence.rs");
    let cli = include_str!("../../kiana-entrypoints/src/cli.rs");
    let web = include_str!("../../kiana-entrypoints/src/web.rs");
    let workbench = include_str!("../../kiana-entrypoints/src/workbench_chat.rs");
    let desktop = include_str!("../../contrib/desktop/main.js");
    let daemon = include_str!("../../kiana-daemon/src/lib.rs");
    let control = include_str!("../src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/dep40-release-uat-baseline.md");

    for marker in [
        "UatEntrypoint",
        "UatScenario",
        "Release",
        "Upgrade",
        "Rollback",
        "Backup",
        "Restore",
        "Migration",
        "Health",
        "Denied",
        "Succeeded",
        "RestartRecovered",
        "Replayed",
        "ResultUnknown",
        "uat_unknown_retry_forbidden",
        "uat_denied_coverage_missing",
        "uat_success_coverage_missing",
        "daemon_spine_digest",
        "control_plane_digest",
        "harness_digest",
        "Fake",
        "LiveOptIn",
        "ReleaseUatEvidence",
        "release_uat_fake_cannot_claim_live",
        "release_uat_verified_evidence_incomplete",
        "unknown_reconciled",
        "receipt_digests",
    ] {
        assert!(
            matrix.contains(marker) || evidence.contains(marker),
            "DEP-40 matrix marker missing: {marker}"
        );
    }
    for (name, source, markers) in [
        (
            "CLI",
            cli,
            &["DaemonHost", "ControlPlaneRequest", "ControlPlaneResponse"][..],
        ),
        (
            "Web",
            web,
            &["DaemonHost", "/api/health", "harness_run"][..],
        ),
        (
            "Workbench",
            workbench,
            &["DaemonHost", "harness_run", "subscribe_run"][..],
        ),
        (
            "Desktop",
            desktop,
            &["waitForUrl", "startHarness", "stopWorker"][..],
        ),
        (
            "DaemonHost",
            daemon,
            &["pub struct DaemonHost", "ControlPlane", "KianaHarness"][..],
        ),
        (
            "ControlPlane",
            control,
            &["pub struct ControlPlane", "CapabilityBroker"][..],
        ),
    ] {
        for marker in markers {
            assert!(
                source.contains(marker),
                "DEP-40 {name} marker missing: {marker}"
            );
        }
    }
    assert!(!cli.contains("CapabilityBroker::new"));
    assert!(!web.contains("CapabilityBroker::new"));
    assert!(!workbench.contains("CapabilityBroker::new"));
    for marker in [
        "release/upgrade/rollback/backup/restore/migration/health",
        "CLI",
        "Web",
        "Workbench",
        "Desktop",
        "deny",
        "success",
        "restart",
        "replay",
        "result_unknown",
        "fake provider",
        "live opt-in",
        "partial",
        "not a live deployment",
    ] {
        assert!(
            baseline.contains(marker),
            "DEP-40 baseline marker missing: {marker}"
        );
    }
}
