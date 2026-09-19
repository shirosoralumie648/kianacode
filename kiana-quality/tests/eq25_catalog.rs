use kiana_quality::{
    build_catalog, CatalogError, CatalogOptions, CatalogScenario, CatalogStatus, CatalogTier,
};

const DIGEST: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn scenario(
    id: &str,
    tier: CatalogTier,
    order: &str,
    key: &str,
    golden: Option<&str>,
) -> CatalogScenario {
    CatalogScenario {
        scenario_id: id.to_owned(),
        tier,
        stable_order: order.to_owned(),
        dedupe_key: key.to_owned(),
        golden_trace_digest: golden.map(str::to_owned),
        tags: vec!["regression".to_owned()],
    }
}

#[test]
fn curated_deep_no_golden_and_dedupe_are_visible() {
    let catalog = build_catalog(
        vec![
            scenario("deep-1", CatalogTier::Deep, "02", "same", Some(DIGEST)),
            scenario(
                "curated-no-golden",
                CatalogTier::Curated,
                "01",
                "no-golden",
                None,
            ),
            scenario(
                "curated-duplicate",
                CatalogTier::Curated,
                "03",
                "same",
                Some(DIGEST),
            ),
        ],
        CatalogOptions::default(),
    )
    .unwrap();
    assert_eq!(catalog.entries.len(), 2);
    assert_eq!(catalog.entries[0].status, CatalogStatus::NoGolden);
    assert_eq!(
        catalog.entries[0].skip_reason.as_deref(),
        Some("no_golden_trace")
    );
    assert_eq!(catalog.entries[1].status, CatalogStatus::Skipped);
    assert_eq!(
        catalog.entries[1].skip_reason.as_deref(),
        Some("deep_opt_in_required")
    );
    assert_eq!(catalog.entries[1].duplicate_count, 2);
}

#[test]
fn deep_opt_in_and_input_permutation_produce_stable_order_and_digest() {
    let first = vec![
        scenario("b", CatalogTier::Curated, "02", "b", Some(DIGEST)),
        scenario("a", CatalogTier::Deep, "01", "a", Some(DIGEST)),
    ];
    let second = vec![first[1].clone(), first[0].clone()];
    let options = CatalogOptions {
        include_deep: true,
        max_entries: 10,
    };
    let left = build_catalog(first, options.clone()).unwrap();
    let right = build_catalog(second, options).unwrap();
    assert_eq!(left, right);
    assert_eq!(left.entries[0].scenario_id, "a");
    assert!(left
        .entries
        .iter()
        .all(|entry| entry.status == CatalogStatus::Ready));
}

#[test]
fn invalid_catalog_inputs_fail_closed() {
    assert_eq!(
        build_catalog(Vec::new(), CatalogOptions::default()).unwrap_err(),
        CatalogError::CatalogEmpty
    );
    assert_eq!(
        build_catalog(
            vec![scenario(
                "bad",
                CatalogTier::Curated,
                "01",
                "bad",
                Some("bad")
            )],
            CatalogOptions::default(),
        )
        .unwrap_err(),
        CatalogError::GoldenDigestInvalid
    );
    assert_eq!(
        build_catalog(
            vec![scenario(
                "ok",
                CatalogTier::Curated,
                "01",
                "ok",
                Some(DIGEST)
            )],
            CatalogOptions {
                include_deep: false,
                max_entries: 0,
            },
        )
        .unwrap_err(),
        CatalogError::OptionsInvalid
    );
}
