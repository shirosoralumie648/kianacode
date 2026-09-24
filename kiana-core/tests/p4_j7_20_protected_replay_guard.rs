use std::fs;

#[test]
fn protected_replay_source_never_serializes_private_bytes_or_bypasses_scope() {
    let source = fs::read_to_string("../kiana-domain/src/protected_replay.rs")
        .expect("protected replay source");
    assert!(source.contains("missing_reasoning_material_blocks_resume"));
    assert!(source.contains("replay_material_cannot_cross_connection_or_model_scope"));
    assert!(source.contains("Private bytes are intentionally not serializable"));
    assert!(!source.contains("derive(Serialize"));

    let provider = fs::read_to_string("../kiana-provider/src/request.rs").expect("request source");
    assert!(provider.contains("model_replay_storage_unavailable"));
    assert!(provider.contains("validate_for_call"));
}
