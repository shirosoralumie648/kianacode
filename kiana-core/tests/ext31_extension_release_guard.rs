use std::fs;

#[test]
fn extension_release_gate_binds_fixed_metrics_supply_chain_and_limitations() {
    let release = fs::read_to_string("../kiana-domain/src/extension_release.rs")
        .expect("extension release source");
    for marker in [
        "EXTENSION_BENCHMARK_SCHEMA",
        "CatalogCold",
        "CatalogWarm",
        "ResourceRead",
        "HookLatency",
        "PackageVerify",
        "SnapshotRebuild",
        "ExtensionReleaseStatus",
        "extension_benchmark_unobserved_metrics_claimed",
        "extension_release_ready_evidence_incomplete",
        "extension_release_metric_coverage_missing",
        "blockers",
    ] {
        assert!(release.contains(marker), "EXT-31 marker missing: {marker}");
    }
    for forbidden in [
        "std::process::Command",
        "tokio::spawn",
        "reqwest::Client",
        "EventStorePort",
        "CapabilityBroker::new",
        "remove_file",
        "publish_release",
    ] {
        assert!(
            !release.contains(forbidden),
            "extension release contract gained an effect path: {forbidden}"
        );
    }
}

#[test]
fn extension_release_gate_points_to_existing_ci_supply_chain_and_smoke_boundaries() {
    let release = fs::read_to_string("../../scripts/release-smoke.sh").expect("release smoke");
    let harness =
        fs::read_to_string("../../scripts/harness-golden-smoke.sh").expect("harness smoke");
    let supply = fs::read_to_string("../../scripts/supply-chain-scan.sh").expect("supply scan");
    let module_map = fs::read_to_string("../../docs/module-map.md").expect("module map");
    for (name, source, markers) in [
        ("release", release, vec!["release", "smoke"]),
        ("harness", harness, vec!["golden", "smoke"]),
        ("supply", supply, vec!["cargo", "audit"]),
        (
            "module_map",
            module_map,
            vec!["Skills / Plugins / Hooks", "extension"],
        ),
    ] {
        for marker in markers {
            assert!(
                source
                    .to_ascii_lowercase()
                    .contains(&marker.to_ascii_lowercase()),
                "{name} marker missing: {marker}"
            );
        }
    }
}

#[test]
fn extension_release_fixture_keeps_publish_and_benchmark_unknown() {
    let fixture = fs::read_to_string("../kiana-domain/tests/fixtures/ext31-extension-release.json")
        .expect("release fixture");
    assert!(fixture.contains("\"status\": \"partial\""));
    assert!(fixture.contains("\"release_published\": false"));
    assert!(fixture.contains("github_ci_pending"));
}
