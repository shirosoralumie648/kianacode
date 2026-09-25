use kiana_domain::{
    ExtensionGoldenEffect, ExtensionGoldenHookOutcome, ExtensionGoldenMatrix,
    ExtensionGoldenOutcome, ExtensionGoldenScenario, ExtensionGoldenSurface, ExtensionGoldenTrace,
};
use std::collections::BTreeMap;
use std::fs;

const INPUT: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SNAPSHOT: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const POLICY: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const GATE: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const FINAL_INPUT: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const REVALIDATION: &str =
    "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn surfaces() -> BTreeMap<ExtensionGoldenSurface, String> {
    ExtensionGoldenSurface::ALL
        .into_iter()
        .map(|surface| (surface, SNAPSHOT.to_owned()))
        .collect()
}

fn trace(scenario: ExtensionGoldenScenario) -> ExtensionGoldenTrace {
    let (outcome, effect, hook, final_input, revalidation, capability, broker, receipt, package) =
        match scenario {
            ExtensionGoldenScenario::HookUpdate => (
                ExtensionGoldenOutcome::Allowed,
                ExtensionGoldenEffect::Capability,
                ExtensionGoldenHookOutcome::Update,
                Some(FINAL_INPUT.to_owned()),
                Some(REVALIDATION.to_owned()),
                Some(digest('1')),
                Some(digest('2')),
                Some(digest('3')),
                None,
            ),
            ExtensionGoldenScenario::SignedInstall => (
                ExtensionGoldenOutcome::Allowed,
                ExtensionGoldenEffect::RegistryMutation,
                ExtensionGoldenHookOutcome::None,
                None,
                None,
                None,
                None,
                Some(digest('3')),
                Some(digest('4')),
            ),
            ExtensionGoldenScenario::AllowedCapability => (
                ExtensionGoldenOutcome::Allowed,
                ExtensionGoldenEffect::Capability,
                ExtensionGoldenHookOutcome::Allow,
                None,
                None,
                Some(digest('1')),
                Some(digest('2')),
                Some(digest('3')),
                None,
            ),
            ExtensionGoldenScenario::HookTimeout => (
                ExtensionGoldenOutcome::Unknown,
                ExtensionGoldenEffect::None,
                ExtensionGoldenHookOutcome::Timeout,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            ExtensionGoldenScenario::HookBlock => (
                ExtensionGoldenOutcome::Denied,
                ExtensionGoldenEffect::None,
                ExtensionGoldenHookOutcome::Block,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            ExtensionGoldenScenario::HookAsk => (
                ExtensionGoldenOutcome::Denied,
                ExtensionGoldenEffect::None,
                ExtensionGoldenHookOutcome::Ask,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            ExtensionGoldenScenario::HookCancelled => (
                ExtensionGoldenOutcome::Denied,
                ExtensionGoldenEffect::None,
                ExtensionGoldenHookOutcome::Cancelled,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            _ => (
                ExtensionGoldenOutcome::Denied,
                ExtensionGoldenEffect::None,
                ExtensionGoldenHookOutcome::None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
        };
    ExtensionGoldenTrace::new(
        format!("ext29-{scenario:?}"),
        scenario,
        outcome,
        effect,
        hook,
        INPUT,
        SNAPSHOT,
        POLICY,
        GATE,
        final_input,
        revalidation,
        capability,
        broker,
        receipt,
        package,
        surfaces(),
        if outcome == ExtensionGoldenOutcome::Allowed {
            1
        } else {
            0
        },
        0,
        0,
    )
    .expect("extension golden trace")
}

#[test]
fn extension_golden_matrix_covers_deny_allow_unknown_and_five_surfaces() {
    let matrix = ExtensionGoldenMatrix::new(
        SNAPSHOT,
        ExtensionGoldenScenario::ALL
            .into_iter()
            .map(trace)
            .collect(),
    )
    .expect("extension golden matrix");
    matrix.validate().expect("valid extension matrix");
    assert_eq!(matrix.cases.len(), 13);
    assert_eq!(matrix.denied_cases().expect("denied cases").len(), 9);
    assert!(matrix.canonical_bytes().expect("canonical matrix").len() > 512);
}

#[test]
fn deny_first_trace_cannot_contain_capability_or_broker_effect() {
    let mut denied = trace(ExtensionGoldenScenario::PathEscape);
    denied.capability_request_digest = Some(digest('1'));
    assert_eq!(
        denied.validate().unwrap_err(),
        "extension_golden_denied_effect_leak"
    );
}

#[test]
fn hook_update_requires_final_input_and_controlplane_revalidation() {
    let mut updated = trace(ExtensionGoldenScenario::HookUpdate);
    updated.revalidation_digest = None;
    assert_eq!(
        updated.validate().unwrap_err(),
        "extension_golden_hook_update_revalidation_missing"
    );

    let mut drift = trace(ExtensionGoldenScenario::AllowedCapability);
    drift
        .surface_snapshot_digests
        .insert(ExtensionGoldenSurface::Web, digest('9'));
    assert_eq!(
        drift.validate().unwrap_err(),
        "extension_golden_surface_snapshot_drift"
    );
}

#[test]
fn fixture_is_offline_metadata_and_does_not_execute_extension_code() {
    let raw = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ext29-extension-golden.json"
    ))
    .expect("extension fixture");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("fixture json");
    assert_eq!(value["schema"], "kiana.extension-golden-fixture.v1");
    assert_eq!(value["mode"], "offline_no_effects");
    assert_eq!(value["surface_count"], 5);
    assert_eq!(value["broker_invocations"], 0);
    assert!(!raw.contains("shell_command") && !raw.contains("http://"));
}
