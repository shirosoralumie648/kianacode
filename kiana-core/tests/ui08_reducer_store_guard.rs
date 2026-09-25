#[test]
fn ui08_store_is_a_pure_bounded_projection() {
    let store = include_str!("../../kiana-client/src/ui_store.rs");
    let client = include_str!("../../kiana-client/src/lib.rs");
    for marker in [
        "UiEntityStore",
        "UiEntityKind",
        "UiEntityLifecycle",
        "UiStoreScope",
        "UiStoreEvent",
        "UiStoreGapReason",
        "UiOptimisticUpdate",
        "UiStoreChange",
        "reduce(&self",
        "dehydrate",
        "hydrate",
        "CacheFullProtected",
        "StaleRevision",
        "ScopeMismatch",
        "needs_snapshot",
        "unknown_commands",
        "optimistic",
        "ensure_capacity",
        "ui_store_optimistic_entity_missing",
        "ui_store_optimistic_entity_mismatch",
        "previous.workspace",
    ] {
        assert!(
            store.contains(marker) || client.contains(marker),
            "UI-08 reducer marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker",
        "ControlPlane::new",
        "DaemonHost",
        "tokio::spawn",
        "ModelClient",
        "std::fs::",
    ] {
        assert!(
            !store.contains(forbidden),
            "UI-08 reducer must not contain {forbidden}"
        );
    }
}
