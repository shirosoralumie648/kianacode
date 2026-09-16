#[test]
fn company_load_path_uses_deterministic_reducer_and_explicit_migration() {
    let reducer = include_str!("../../kiana-domain/src/company_replay.rs");
    let core = include_str!("../src/company.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");

    for marker in [
        "CompanyReplayReducer",
        "company_replay_gap",
        "company_replay_duplicate_command",
        "company_event_schema_unsupported",
        "LEGACY_COMPANY_EVENT_SCHEMA",
        "migrate_company_event",
    ] {
        assert!(reducer.contains(marker), "replay marker missing: {marker}");
    }
    for marker in ["CompanyReplayReducer::new", ".apply(event)", "into_parts()"] {
        assert!(
            core.contains(marker),
            "core reducer wiring missing: {marker}"
        );
    }
    for marker in ["COMPANY_EVENT_SCHEMA", "CompanyState", "transition"] {
        assert!(
            company.contains(marker),
            "domain reducer marker missing: {marker}"
        );
    }
}
