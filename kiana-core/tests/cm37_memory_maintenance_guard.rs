use std::fs;

#[test]
fn maintenance_contract_covers_gc_ttl_orphan_quota_and_retention_fences() {
    let maintenance = fs::read_to_string("../kiana-domain/src/memory_maintenance.rs")
        .expect("memory maintenance source");
    for marker in [
        "MEMORY_MAINTENANCE_SCHEMA",
        "MaintenanceObjectKind",
        "IndexGeneration",
        "SummaryArtifact",
        "ResultArtifact",
        "CacheEntry",
        "Tombstone",
        "MaintenanceDecision",
        "DeleteEligible",
        "ReportOrphan",
        "memory_maintenance_delete_reference_or_retention_gate",
        "memory_maintenance_delete_expiry_or_generation_gate",
        "memory_maintenance_orphan_report_invalid",
        "quota_satisfied_after_plan",
        "canonical_bytes",
    ] {
        assert!(
            maintenance.contains(marker),
            "CM-37 marker missing: {marker}"
        );
    }
    for forbidden in [
        "std::fs",
        "remove_file",
        "delete_file",
        "rename",
        "tokio::spawn",
        "EventStorePort",
        "CapabilityBrokerPort",
        "ProviderGateway",
    ] {
        assert!(
            !maintenance.contains(forbidden),
            "maintenance contract gained a side-effect path: {forbidden}"
        );
    }
}

#[test]
fn maintenance_reuses_existing_retention_index_and_cache_boundaries() {
    let retention =
        fs::read_to_string("../kiana-domain/src/retention.rs").expect("retention source");
    let watermark = fs::read_to_string("../kiana-domain/src/retention_watermark.rs")
        .expect("retention watermark source");
    let index = fs::read_to_string("../kiana-domain/src/index_generation.rs")
        .expect("index generation source");
    let cache = fs::read_to_string("../kiana-query/src/cache_policy.rs").expect("cache source");
    for (name, source, markers) in [
        (
            "retention",
            retention,
            vec!["RetentionScan", "LegalHoldReceipt", "RetentionDisposition"],
        ),
        (
            "watermark",
            watermark,
            vec!["RetentionWatermark", "tombstone_committed", "Blocked"],
        ),
        (
            "index",
            index,
            vec![
                "IndexGenerationState",
                "IndexManifest",
                "IndexGenerationStatus",
            ],
        ),
        (
            "cache",
            cache,
            vec![
                "ContextCacheDecision",
                "business_result_from_cache",
                "Stale",
            ],
        ),
    ] {
        for marker in markers {
            assert!(
                source.contains(marker),
                "{name} boundary marker missing: {marker}"
            );
        }
    }
}

#[test]
fn maintenance_fixture_cannot_masquerade_as_physical_cleanup() {
    let fixture = fs::read_to_string("../kiana-domain/tests/fixtures/cm37-memory-maintenance.json")
        .expect("maintenance fixture");
    assert!(fixture.contains("\"effect_authority\": \"none\""));
    assert!(fixture.contains("\"physical_cleanup_proven\": false"));
    assert!(!fixture.contains("remove_file") && !fixture.contains("delete_file"));
}
