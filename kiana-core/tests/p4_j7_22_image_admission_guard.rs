//! P4-J7-22 图片准入的跨 crate 静态源码守卫。
//!
//! `include_str!` 读取 Domain admission、model content、Provider request/gateway、Provider
//! 端到端夹具、baseline 和 workflow 字符串，确认拒绝码、编码入口、按消息落点注入以及
//! 限额标记仍存在；它不调用 ArtifactStore、ProviderGateway 或真实网络请求。

#[test]
/// 固定 Artifact/ProcessingGrant 绑定、Provider 编码后预算与不读文件/不抓 URL 的源码边界。
fn p4_j7_22_image_admission_stays_artifact_bound_and_encodes_after_admission() {
    let domain = include_str!("../../kiana-domain/src/image_input.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let provider = include_str!("../../kiana-provider/src/request.rs");
    let fixtures = include_str!("../../kiana-provider/tests/p4_j7_22_image_admission.rs");
    let gateway = include_str!("../../kiana-provider/src/lib.rs");
    // 旧 daemon provider 边界没有用户附件 DTO：AttachmentRef 必须在发送前显式失败，不能把原始
    // 路径/URL 交给任何 provider 编码。
    let daemon = include_str!("../../kiana-daemon/src/model_client.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-22-image-admission-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-22-image-admission.yml");

    for marker in [
        "ImageInputAdmission",
        "ProcessingGrant",
        "ArtifactRef",
        "revoked_artifact_grant_blocks_send",
        "untrusted_image_path_never_reaches_provider",
        "remote_image_url_is_not_fetched_implicitly",
        "image_processing_grant_expired",
        "unsupported_document_upload",
        "unsupported_audio_video_input",
        "image_media_type_mismatch",
        "validate_for_dispatch",
        "allowed_data_classes",
        "policy_digest",
        "data_epoch",
    ] {
        assert!(
            domain.contains(marker),
            "domain image marker missing: {marker}"
        );
    }
    for marker in [
        "compile_with_images",
        "validate_image_admissions",
        "image_placements",
        "inject_image_placements",
        "image_payload_limit_applies_after_encoding",
        "image_attachment_role_unsupported",
        "image_tool_result_protocol_unsupported",
        "image_admission_required",
        "image_admission_unreferenced",
        "image_admission_count_limit",
        "transport::unix_ms",
        "STANDARD.encode",
        "data:image/",
    ] {
        assert!(
            provider.contains(marker),
            "provider image admission marker missing: {marker}"
        );
    }
    for marker in [
        "untrusted_attachment_without_admission_never_reaches_provider",
        "untrusted_image_path_and_remote_url_never_reach_provider",
        "authorized_image_input_reaches_vision_capable_provider",
        "image_hash_matches_admitted_payload_after_encoding",
        "revoked_artifact_grant_blocks_send",
        "retention_expiry_is_checked_at_dispatch_time",
        "admission_bound_to_another_route_cannot_be_replayed",
        "non_vision_route_rejects_attachment_before_send",
        "document_audio_and_video_inputs_are_explicitly_unsupported",
        "tool_result_image_reaches_anthropic_tool_result",
        "tool_result_image_is_rejected_without_a_protocol_tool_image_shape",
        "prepare_call_with_images",
    ] {
        assert!(
            fixtures.contains(marker),
            "provider fixture missing: {marker}"
        );
    }
    assert!(gateway.contains("prepare_call_with_images"));
    assert!(daemon.contains("unsupported_content_block_fails_before_request"));
    assert!(model.contains("validate_image_attachment_ref"));
    assert!(!provider.contains("std::fs"));
    assert!(!provider.contains("read_to_end"));
    assert!(!fixtures.contains("std::fs"));
    assert!(!fixtures.contains("reqwest::"));
    for marker in [
        "untrusted_image_path_never_reaches_provider",
        "revoked_artifact_grant_blocks_send",
        "image_payload_limit_applies_after_encoding",
        "remote_image_url_is_not_fetched_implicitly",
        "authorized_image_input_reaches_vision_capable_provider",
        "image_hash_matches_admitted_payload",
        "cargo fmt --all --check",
        "cargo test -p kiana-domain --test p4_j7_22_image_admission",
        "cargo test -p kiana-provider --lib",
        "cargo test -p kiana-provider --test p4_j7_22_image_admission",
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "P4-J7-22 evidence marker missing: {marker}"
        );
    }
}
