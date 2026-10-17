//! UI-08 boundary guard: entrypoints project the shared entity store, they never own a second one.
//!
//! The reducer itself lives in `kiana-client`; entrypoints (CLI, Workbench, Web, Desktop) may read
//! its namespace-scoped views, but a presenter that keeps its own entity map would become a second
//! fact source.  These assertions are source-only: they run in CI, not in the product path.

#[test]
fn entrypoints_do_not_define_a_second_entity_store() {
    let client = include_str!("../../kiana-client/src/ui_store.rs");
    let entrypoints = include_str!("../src/lib.rs");
    assert!(client.contains("UiEntityStore"));
    assert!(!entrypoints.contains("struct UiEntityStore"));
    assert!(!client.contains("ModelClient"));
    assert!(!client.contains("CapabilityBroker"));
    for forbidden in [
        "UiEntityStoreSnapshot",
        "enum UiStoreEvent",
        "struct UiOptimisticUpdate",
        "fn ensure_capacity",
    ] {
        assert!(
            !entrypoints.contains(forbidden),
            "entrypoints must not own the shared store: {forbidden}"
        );
    }
}

/// The completion-slice fences must stay in the shared store, and a presenter must read each
/// namespace from the store instead of keeping its own copy of run/connection/submission/draft/
/// inbox/artifact state.
#[test]
fn entrypoints_project_the_shared_store_and_never_guess_an_entity() {
    let client = include_str!("../../kiana-client/src/ui_store.rs");
    let entrypoints = include_str!("../src/lib.rs");
    for marker in [
        "entities_of_kind",
        "count_of_kind",
        "UiEntityKind::Draft",
        "ui_feed_entity_kind_missing",
        "ui_feed_draft_kind_forbidden",
        "ui_feed_entity_revision_invalid",
        "ui_store_snapshot_draft_entity",
        "PendingConflict",
        "ui_store_stale_revision",
        "ui_store_scope_mismatch",
    ] {
        assert!(
            client.contains(marker),
            "the shared store is missing the UI-08 fence: {marker}"
        );
    }
    // Drafts belong to the input layer, and the store must never write a fact: no broker, no
    // control plane, no event store, no filesystem and no model loop inside the projection.
    for forbidden in [
        "CapabilityBroker",
        "ControlPlane::new",
        "DaemonHost",
        "EventStorePort",
        "KianaHarness",
        "tokio::spawn",
        "ModelClient",
        "std::fs::",
    ] {
        assert!(
            !client.contains(forbidden),
            "the shared store must not contain {forbidden}"
        );
    }
    assert!(!client.contains("#[ignore"));
}
