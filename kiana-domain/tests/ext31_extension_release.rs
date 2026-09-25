use kiana_domain::{
    ExtensionBenchmarkMetric, ExtensionBenchmarkObservation, ExtensionBenchmarkStatus,
    ExtensionReleaseGate, ExtensionReleaseStatus,
};
use std::fs;

const ENV: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TOOLCHAIN: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SOURCE: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const LOCK: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const MANIFEST: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const SUPPLY: &str = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

fn observation(metric: ExtensionBenchmarkMetric) -> ExtensionBenchmarkObservation {
    ExtensionBenchmarkObservation::new(
        metric,
        ExtensionBenchmarkStatus::Skipped,
        None,
        None,
        0,
        4096,
        0,
        0,
        1,
        ENV,
        TOOLCHAIN,
        Some("local benchmark deliberately not run; CI is the authority".to_owned()),
    )
    .expect("benchmark observation")
}

fn gate() -> ExtensionReleaseGate {
    ExtensionReleaseGate::new(
        SOURCE,
        LOCK,
        MANIFEST,
        SUPPLY,
        SUPPLY,
        ExtensionReleaseStatus::Partial,
        ExtensionBenchmarkMetric::ALL
            .into_iter()
            .map(observation)
            .collect(),
        vec!["CI benchmark and release smoke pending".to_owned()],
    )
    .expect("release gate")
}

#[test]
fn release_gate_requires_fixed_metrics_and_preserves_unobserved_limitations() {
    let gate = gate();
    gate.validate().expect("valid partial gate");
    assert_eq!(gate.blockers().expect("blockers").len(), 7);
    assert!(gate.canonical_bytes().expect("canonical gate").len() > 512);
}

#[test]
fn ready_gate_cannot_hide_skipped_benchmarks() {
    let result = ExtensionReleaseGate::new(
        SOURCE,
        LOCK,
        MANIFEST,
        SUPPLY,
        SUPPLY,
        ExtensionReleaseStatus::Ready,
        ExtensionBenchmarkMetric::ALL
            .into_iter()
            .map(observation)
            .collect(),
        Vec::new(),
    );
    assert_eq!(
        result.unwrap_err(),
        "extension_release_ready_evidence_incomplete"
    );
}

#[test]
fn observed_metric_requires_ordered_percentiles_and_sample_count() {
    let result = ExtensionBenchmarkObservation::new(
        ExtensionBenchmarkMetric::HookLatency,
        ExtensionBenchmarkStatus::Observed,
        Some(20),
        Some(10),
        0,
        4096,
        100,
        100,
        1,
        ENV,
        TOOLCHAIN,
        None,
    );
    assert_eq!(
        result.unwrap_err(),
        "extension_benchmark_observation_invalid"
    );
}

#[test]
fn fixture_is_ci_metadata_and_does_not_claim_benchmark_or_release_success() {
    let raw = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/ext31-extension-release.json"
    ))
    .expect("release fixture");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("fixture json");
    assert_eq!(value["schema"], "kiana.extension-release-fixture.v1");
    assert_eq!(value["status"], "partial");
    assert_eq!(value["benchmark_execution"], "github_ci_pending");
    assert_eq!(value["release_published"], false);
    assert!(!raw.contains("secret") && !raw.contains("cargo run"));
}
