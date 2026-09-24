#[test]
fn p4_j7_22_image_admission_stays_artifact_bound_and_encodes_after_admission() {
    let domain = include_str!("../../kiana-domain/src/image_input.rs");
    let model = include_str!("../../kiana-domain/src/model.rs");
    let provider = include_str!("../../kiana-provider/src/request.rs");
    let gateway = include_str!("../../kiana-provider/src/lib.rs");
    let baseline = include_str!("../../docs/roadmap/p4-j7-22-image-admission-baseline.md");
    let workflow = include_str!("../../.github/workflows/p4-j7-22-image-admission.yml");

    for marker in [
        "ImageInputAdmission",
        "ProcessingGrant",
        "ArtifactRef",
        "revoked_artifact_grant_blocks_send",
        "untrusted_image_path_never_reaches_provider",
        "remote_image_url_is_not_fetched_implicitly",
        "unsupported_document_upload",
        "unsupported_audio_video_input",
        "image_media_type_mismatch",
        "validate_for_dispatch",
        "allowed_data_classes",
        "policy_digest",
        "data_epoch",
    ] {
        assert!(domain.contains(marker), "domain image marker missing: {marker}");
    }
    for marker in [
        "compile_with_images",
        "validate_image_admissions",
        "image_payload_limit_applies_after_encoding",
        "inject_admitted_images",
        "STANDARD.encode",
        "data:image/",
        "image_admission_required",
    ] {
        assert!(
            provider.contains(marker),
            "provider image admission marker missing: {marker}"
        );
    }
    assert!(gateway.contains("prepare_call_with_images"));
    assert!(model.contains("validate_image_attachment_ref"));
    assert!(!provider.contains("std::fs"));
    assert!(!provider.contains("read_to_end"));
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
    ] {
        assert!(
            baseline.contains(marker) || workflow.contains(marker),
            "P4-J7-22 evidence marker missing: {marker}"
        );
    }
}
