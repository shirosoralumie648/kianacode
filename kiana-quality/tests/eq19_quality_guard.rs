#[test]
fn volatile_normalization_requires_declared_rules_and_has_no_io_path() {
    let source = include_str!("../src/volatile.rs");
    assert!(source.contains("VolatilePolicy"));
    assert!(source.contains("UndeclaredVolatile"));
    assert!(source.contains("replacement_count"));
    assert!(source.contains("<TS>"));
    assert!(source.contains("<UUID>"));
    assert!(source.contains("<TEMP_PATH>"));
    assert!(source.contains("<ACTOR>"));
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("tokio::"));
    assert!(!source.contains("reqwest::"));
}
