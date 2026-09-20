#[test]
fn hook_replay_is_receipt_only_and_fences_unknown_cleanup() {
    let receipt = include_str!("../../kiana-query/src/hook_receipt.rs");
    for marker in [
        "HookReceipt",
        "snapshot_digest",
        "patch_before_digest",
        "cleanup",
        "requires_control_plane_decision",
        "executed: false",
        "hook_receipt_invalid_or_not_replayable",
    ] {
        assert!(receipt.contains(marker), "missing EXT-18 marker: {marker}");
    }
    assert!(!receipt.contains("Command::new"));
    assert!(!receipt.contains("tokio::process"));
}
