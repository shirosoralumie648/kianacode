use kiana_domain::{
    ExtensionCrashPoint, ExtensionRecoveryAction, ExtensionRecoveryCase, ExtensionRecoveryMatrix,
    ExtensionRecoveryTerminal,
};
use std::fs;

const SNAPSHOT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn case(point: ExtensionCrashPoint) -> ExtensionRecoveryCase {
    let (action, pending, started, known, terminal, receipt, quarantine) = match point {
        ExtensionCrashPoint::PackageBeforeRename => (
            ExtensionRecoveryAction::QuarantinePackage,
            false,
            false,
            false,
            ExtensionRecoveryTerminal::Blocked,
            None,
            Some(digest('1')),
        ),
        ExtensionCrashPoint::RenameBeforeEvent => (
            ExtensionRecoveryAction::RetryIdempotentCommand,
            false,
            false,
            false,
            ExtensionRecoveryTerminal::Recovered,
            Some(digest('2')),
            None,
        ),
        ExtensionCrashPoint::EventBeforeProjection | ExtensionCrashPoint::UpgradeSwitch => (
            ExtensionRecoveryAction::RebuildRegistryAndSnapshot,
            false,
            true,
            true,
            ExtensionRecoveryTerminal::Recovered,
            Some(digest('2')),
            None,
        ),
        ExtensionCrashPoint::HookRunning | ExtensionCrashPoint::BrokerResultUnknown => (
            ExtensionRecoveryAction::ReconcileUnknown,
            false,
            true,
            false,
            ExtensionRecoveryTerminal::Unknown,
            None,
            None,
        ),
        ExtensionCrashPoint::ApprovalExpiring => (
            ExtensionRecoveryAction::AwaitApproval,
            true,
            false,
            false,
            ExtensionRecoveryTerminal::Blocked,
            None,
            None,
        ),
    };
    ExtensionRecoveryCase::new(
        format!("ext30-{point:?}"),
        point,
        20,
        4,
        digest('3'),
        SNAPSHOT,
        digest('4'),
        digest('5'),
        pending,
        started,
        known,
        false,
        action,
        terminal,
        receipt,
        quarantine,
    )
    .expect("recovery case")
}

#[test]
fn recovery_matrix_covers_all_crash_points_without_duplicate_effects() {
    let matrix = ExtensionRecoveryMatrix::new(
        SNAPSHOT,
        ExtensionCrashPoint::ALL.into_iter().map(case).collect(),
    )
    .expect("recovery matrix");
    matrix.validate().expect("valid recovery matrix");
    assert_eq!(matrix.cases.len(), 7);
    assert_eq!(matrix.unknown_cases().expect("unknown cases").len(), 2);
    assert!(matrix.canonical_bytes().expect("canonical matrix").len() > 512);
}

#[test]
fn unknown_effect_never_uses_automatic_retry() {
    let mut unknown = case(ExtensionCrashPoint::BrokerResultUnknown);
    unknown.action = ExtensionRecoveryAction::RetryIdempotentCommand;
    assert_eq!(
        unknown.validate().unwrap_err(),
        "extension_recovery_retry_action_invalid"
    );

    let mut duplicate = case(ExtensionCrashPoint::EventBeforeProjection);
    duplicate.duplicate_effect = true;
    assert_eq!(
        duplicate.validate().unwrap_err(),
        "extension_recovery_case_header_invalid"
    );
}

#[test]
fn approval_expiry_waits_for_new_authority_and_package_failure_is_quarantined() {
    let approval = case(ExtensionCrashPoint::ApprovalExpiring);
    assert_eq!(approval.action, ExtensionRecoveryAction::AwaitApproval);
    assert_eq!(approval.terminal, ExtensionRecoveryTerminal::Blocked);
    let package = case(ExtensionCrashPoint::PackageBeforeRename);
    assert_eq!(package.action, ExtensionRecoveryAction::QuarantinePackage);
    assert!(package.quarantine_package_digest.is_some());
}

#[test]
fn fixture_is_recovery_metadata_and_does_not_claim_durable_reopen() {
    let raw = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ext30-extension-recovery.json"
    ))
    .expect("recovery fixture");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("fixture json");
    assert_eq!(value["schema"], "kiana.extension-recovery-fixture.v1");
    assert_eq!(value["new_process_reopen_proven"], false);
    assert_eq!(value["automatic_retry_unknown"], false);
    assert!(!raw.contains("exec") && !raw.contains("http://"));
}
