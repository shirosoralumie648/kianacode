#[test]
fn company_surface_parity_reuses_shared_snapshot_and_never_owns_business_facts() {
    let parity = include_str!("../src/company_surface_parity.rs");
    let web = include_str!("../src/web.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let protocol = include_str!("../../kiana-protocol/src/lib.rs");
    for marker in [
        "COMPANY_SURFACE_PARITY_SCHEMA",
        "CompanySurface",
        "CompanySurfaceFrame",
        "CompanySurfaceParity",
        "StaleSurfaceAction",
        "snapshot_digest",
        "source_cursor",
        "authority_epoch",
        "company_surface_action_stale",
        "EntryPointKind",
        "company_governance",
        "DaemonHost",
    ] {
        assert!(
            parity.contains(marker)
                || web.contains(marker)
                || workbench.contains(marker)
                || protocol.contains(marker),
            "CO-41 marker missing: {marker}"
        );
    }
    for forbidden in [
        "run_assistant_turn",
        "ModelClient::new",
        "CompanyState::default()",
    ] {
        assert!(
            !parity.contains(forbidden),
            "CO-41 second-state marker present: {forbidden}"
        );
    }
}
