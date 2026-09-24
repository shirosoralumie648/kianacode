#[test]
fn entrypoints_do_not_define_a_second_entity_store() {
    let client = include_str!("../../kiana-client/src/ui_store.rs");
    let entrypoints = include_str!("../src/lib.rs");
    assert!(client.contains("UiEntityStore"));
    assert!(!entrypoints.contains("struct UiEntityStore"));
    assert!(!client.contains("ModelClient"));
    assert!(!client.contains("CapabilityBroker"));
}
