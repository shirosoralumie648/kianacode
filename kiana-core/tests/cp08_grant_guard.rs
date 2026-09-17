#[test]
fn cp08_grant_authority_is_domain_reducer_and_does_not_bypass_eventlog_control() {
    let domain = include_str!("../../kiana-domain/src/grant_authority.rs");
    let authority = include_str!("../src/authority.rs");
    let registry = include_str!("../src/cell_registry.rs");
    for marker in [
        "GrantAuthorityEnvelope",
        "GrantLedger",
        "root_run",
        "parent_grant_id",
        "GrantAuthorityStatus::Revoked",
        "grant_ledger_child_scope_widened",
        "active_grant",
        "snapshot",
        "restore",
    ] {
        assert!(
            domain.contains(marker),
            "grant authority marker missing: {marker}"
        );
    }
    assert!(authority.contains("AuthorityLedger::rebuild"));
    assert!(authority.contains("authority_epoch"));
    assert!(registry.contains("CellRegistryPort"));
    for forbidden in [
        "CapabilityBroker",
        "DaemonHost",
        "authorize_and_execute",
        "second authority",
    ] {
        assert!(
            !domain.contains(forbidden),
            "grant reducer must not {forbidden}"
        );
    }
}
