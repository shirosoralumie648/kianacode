//! NM-19 source guard: the external notification contract stays default-off, source-only and
//! caller-independent, and every fixture stays wired to the automatic CI shard plus the manual lane.
//!
//! Unlike the original two-file scan, this guard walks the whole notification module set in
//! `kiana-core` and `kiana-domain`, so a new transport/signer module cannot evade it by simply not
//! being listed in an `include_str!` pair.

use std::fs;
use std::path::{Path, PathBuf};

fn module_files(crate_dir: &str, prefix: &str) -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join(crate_dir)
        .join("src");
    let mut files = Vec::new();
    collect_dir(&root, prefix, &mut files);
    files.sort_by(|left, right| left.0.cmp(&right.0));
    files
}

fn collect_dir(dir: &Path, prefix: &str, files: &mut Vec<(String, String)>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("cannot read module directory {}: {error}", dir.display()));
    for entry in entries {
        let path = entry.expect("readable directory entry").path();
        if path.is_dir() {
            collect_dir(&path, prefix, files);
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(prefix) || !name.ends_with(".rs") {
            continue;
        }
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let relative: PathBuf = path
            .strip_prefix(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap())
            .expect("path under workspace root")
            .to_path_buf();
        files.push((relative.display().to_string(), content));
    }
}

fn scan(prefix: &str) -> Vec<(String, String)> {
    let mut files = module_files("kiana-core", prefix);
    files.extend(module_files("kiana-domain", prefix));
    assert!(
        files.len() >= 20,
        "NM-19 guard must scan the whole notification module set, found {}",
        files.len()
    );
    files
}

#[test]
fn external_notification_contract_has_no_network_or_direct_effect_path() {
    let files = scan("notification");
    let mut core = String::new();
    let mut domain = String::new();
    for (path, content) in &files {
        if path.starts_with("kiana-core") {
            core.push_str(content);
        } else {
            domain.push_str(content);
        }
    }
    for marker in [
        "ExternalNotificationPolicy",
        "allowed_origins",
        "ExternalNotificationReplayStore",
        "ReadyForExplicitConnector",
        "ReconcileRequired",
        "direct_effect: false",
        "external_notification_disabled",
        "provider_receipt_ref",
        "ExternalNotificationPolicyRegistry",
        "ExternalNotificationPolicySnapshot",
        "ExternalNotificationAdmissionContext",
        "external_notification_policy_not_pinned",
        "external_notification_policy_revision_rollback",
        "external_notification_policy_replay",
        "external_notification_nonce_replayed",
        "external_notification_delivery_replayed",
        "external_notification_replay_capacity_exceeded",
        "external_notification_signature_binding_mismatch",
        "ExternalNotificationSignatureVerification",
        "NotSupported",
        "is_forbidden_address",
    ] {
        assert!(
            core.contains(marker) || domain.contains(marker),
            "NM-19 marker missing: {marker}"
        );
    }
    for forbidden in [
        "reqwest::Client",
        "tokio::net",
        "tokio::spawn",
        "std::net::TcpStream",
        "std::net::TcpListener",
        "std::net::UdpSocket",
        "std::process::Command",
        "send(",
        "connect(",
        "mcp_http",
        "A2aClient",
        "async fn",
    ] {
        for (path, content) in &files {
            assert!(
                !content.contains(forbidden),
                "NM-19 network path widened in {path}: {forbidden}"
            );
        }
    }
    // The gate must not gain a second execution loop, a broker call or a direct fact write.
    for (path, content) in &files {
        for forbidden in [
            "CapabilityBroker",
            "append_event",
            "EventLog::",
            "std::fs::write",
        ] {
            assert!(
                !content.contains(forbidden),
                "NM-19 authority widened in {path}: {forbidden}"
            );
        }
    }
}

#[test]
fn external_notification_fixtures_are_deny_first_and_wired_to_lane_and_shard() {
    let fixtures = include_str!("nm19_external_notification.rs");
    let fixture_position = |name: &str| {
        fixtures
            .find(name)
            .unwrap_or_else(|| panic!("NM-19 fixture missing: {name}"))
    };
    let accept =
        fixture_position("fn enabled_policy_still_returns_explicit_connector_handoff_only");
    for denial in [
        "fn a_caller_cannot_fabricate_an_enabled_policy",
        "fn a_caller_cannot_publish_a_policy_revision_out_of_order",
        "fn a_disabled_server_returns_not_supported_and_never_direct_effect",
        "fn nonce_replay_delivery_replay_and_expiry_fail_closed",
        "fn origin_allowlist_and_signature_binding_fail_closed",
        "fn signature_check_reports_not_supported_instead_of_faking_verification",
        "fn receipt_binding_drift_is_refused",
    ] {
        assert!(
            fixture_position(denial) < accept,
            "NM-19 refusal must precede the success fixture: {denial}"
        );
    }
    for forbidden in ["#[ignore", "--ignored", "assert!(true"] {
        assert!(
            !fixtures.contains(forbidden),
            "NM-19 fixtures must not be weakened: {forbidden}"
        );
    }
    // Every refusal the fixture matrix names must be asserted by its exact stable code.
    let fixture_json = include_str!("fixtures/nm19-external-notification.json");
    for code in [
        "external_notification_policy_not_pinned",
        "external_notification_policy_revision_rollback",
        "external_notification_policy_replay",
        "external_notification_origin_ssrf_denied",
        "external_notification_nonce_replayed",
        "external_notification_delivery_replayed",
        "external_notification_replay_capacity_exceeded",
        "external_notification_epoch_mismatch",
        "external_notification_expired",
        "external_notification_origin_not_allowlisted",
        "external_notification_signature_binding_mismatch",
        "external_notification_signature_key_missing",
        "external_notification_receipt_binding_mismatch",
    ] {
        assert!(
            fixtures.contains(code)
                || include_str!("../../kiana-domain/tests/nm19_external_notification.rs")
                    .contains(code),
            "NM-19 refusal code is not asserted: {code}"
        );
    }
    for marker in [
        "\"policy_source\": \"server_owned_snapshot\"",
        "\"direct_effect\": false",
        "\"proof_ceiling\": \"source\"",
        "caller_fabricated_enabled_policy",
        "policy_revision_rollback_or_replay",
        "ssrf_or_metadata_origin",
        "delivery_replay",
        "signature_verification_not_supported",
        "server_owned_policy_snapshot",
    ] {
        assert!(
            fixture_json.contains(marker),
            "NM-19 fixture marker missing: {marker}"
        );
    }

    let shards = include_str!("../../scripts/ci/test-shards.json");
    for target in [
        "\"nm19_external_notification\"",
        "\"nm19_external_notification_guard\"",
    ] {
        assert!(shards.contains(target), "shard target missing: {target}");
    }

    let baseline = include_str!("../../docs/roadmap/nm19-external-notification-baseline.md");
    for marker in [
        "proof_level=source",
        "Known limits",
        "server-owned",
        "ExternalNotificationPolicyRegistry",
        "ExternalNotificationReplayStore",
        "is_forbidden_address",
        "kiana-core-s4/6",
        "kiana-domain-s3/4",
        "not_supported",
    ] {
        assert!(
            baseline.contains(marker),
            "baseline marker missing: {marker}"
        );
    }

    let lane = include_str!("../../.github/workflows/nm19-external-notification.yml");
    for marker in [
        "workflow_dispatch:",
        "permissions:",
        "contents: read",
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-core --test nm19_external_notification --locked",
        "cargo test -p kiana-core --test nm19_external_notification_guard --locked",
        "cargo test -p kiana-domain --test nm19_external_notification --locked",
        "--no-run",
    ] {
        assert!(lane.contains(marker), "NM-19 lane marker missing: {marker}");
    }
    for forbidden in [
        "push:",
        "pull_request:",
        "continue-on-error",
        "--ignored",
        "|| true",
    ] {
        assert!(
            !lane.contains(forbidden),
            "NM-19 lane must stay manual and fail-closed: {forbidden}"
        );
    }
}
