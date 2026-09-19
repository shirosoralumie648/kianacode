use kiana_domain::{
    ExtensionCallbackPermit, ExtensionEffect, ExtensionLifecyclePhase, ExtensionLifecycleRecord,
    ExtensionSandboxProfile,
};
use std::collections::BTreeSet;

fn package() -> String {
    "a".repeat(64)
}

fn manifest_digest() -> String {
    format!("sha256:{}", "b".repeat(64))
}

#[test]
fn enabled_callback_permit_is_bounded_to_package_manifest_and_read_only_sandbox() {
    let permit = ExtensionCallbackPermit::new(
        "review-skill",
        package(),
        manifest_digest(),
        "context.query",
        ExtensionLifecyclePhase::Enabled,
        ExtensionEffect::ReadOnly,
        ExtensionSandboxProfile::ReadOnly,
        BTreeSet::from(["memory.search".to_owned()]),
        100,
    )
    .unwrap();
    permit.validate().unwrap();
    assert_eq!(permit.phase, ExtensionLifecyclePhase::Enabled);
    assert!(!permit.sandbox.allows_network());
}

#[test]
fn revoked_or_read_only_write_callback_is_denied() {
    assert_eq!(
        ExtensionCallbackPermit::new(
            "review-skill",
            package(),
            manifest_digest(),
            "memory.write",
            ExtensionLifecyclePhase::Enabled,
            ExtensionEffect::ReadOnly,
            ExtensionSandboxProfile::ReadOnly,
            BTreeSet::from(["memory.write".to_owned()]),
            100,
        )
        .unwrap_err(),
        "extension_callback_read_only_write_denied"
    );
    assert_eq!(
        ExtensionCallbackPermit::new(
            "review-skill",
            package(),
            manifest_digest(),
            "context.query",
            ExtensionLifecyclePhase::Revoked,
            ExtensionEffect::ReadOnly,
            ExtensionSandboxProfile::ReadOnly,
            BTreeSet::from(["memory.search".to_owned()]),
            100,
        )
        .unwrap_err(),
        "extension_callback_permit_phase_invalid"
    );
}

#[test]
fn lifecycle_record_is_append_only_and_rejects_invalid_transition() {
    let record = ExtensionLifecycleRecord::transition(
        "review-skill",
        package(),
        Some(ExtensionLifecyclePhase::Enabled),
        ExtensionLifecyclePhase::Uninstalled,
        "principal:operator",
        4,
        "operator requested uninstall",
        None,
    )
    .unwrap();
    record.validate().unwrap();
    assert_eq!(
        ExtensionLifecycleRecord::transition(
            "review-skill",
            package(),
            Some(ExtensionLifecyclePhase::Uninstalled),
            ExtensionLifecyclePhase::Enabled,
            "principal:operator",
            5,
            "revive",
            None,
        )
        .unwrap_err(),
        "extension_lifecycle_transition_invalid"
    );
}
