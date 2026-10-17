//! P4-J7-22 Domain 图片准入合同测试。
//!
//! fixtures 用 'fixture-png' 等普通字节串构造 ArtifactRef/ProcessingGrant，不是真实可解码 PNG 文件；
//! 这些用例验证摘要、路径、scope、MIME 标签和 capability 声明，不验证图片格式解析、ArtifactStore 授权或 Provider wire 编码。

use kiana_domain::{
    image_payload_digest, validate_image_attachment_ref, ArtifactId, ArtifactProvenance,
    ArtifactRef, CapabilitySupport, DataClass, ImageInputAdmission, ModelCapabilities,
    ModelProtocol, ModelRoute, ProcessingGrant, Purpose, Retention,
};

/// 提供固定的 OpenAI Chat route fixture；不连接 provider。
fn route() -> ModelRoute {
    ModelRoute {
        provider_id: "fixture-provider".to_owned(),
        protocol: ModelProtocol::OpenAiChat,
        connection_id: "fixture-connection".to_owned(),
        model_id: "vision-model".to_owned(),
        profile: "default".to_owned(),
        configuration_revision: "fixture.v1".to_owned(),
        streaming: true,
    }
}

/// 构造可控 images capability 的合成模型能力；窗口/输出数值只为测试 route 填充。
fn capabilities(images: CapabilitySupport) -> ModelCapabilities {
    ModelCapabilities {
        tools: CapabilitySupport::Supported,
        streaming: CapabilitySupport::Supported,
        structured_output: CapabilitySupport::Supported,
        images,
        reasoning_replay: CapabilitySupport::Unsupported,
        context_window: 16_384,
        max_output: 1_024,
        source: "fixture".to_owned(),
        revision: "fixture.v1".to_owned(),
    }
}

/// 将给定 bytes 摘要绑定进合成 ArtifactRef；scope/provenance 也只是 fixture metadata。
fn artifact(bytes: &[u8]) -> ArtifactRef {
    ArtifactRef {
        schema: "kiana.artifact-ref.v1".to_owned(),
        artifact_id: ArtifactId::new(),
        version: 1,
        artifact_schema: "image/png".to_owned(),
        content_hash: image_payload_digest(bytes),
        scope_digest: format!("sha256:{}", "b".repeat(64)),
        provenance: ArtifactProvenance {
            producer_kind: "fixture".to_owned(),
            producer_id: "image-fixture".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "fixture".to_owned(),
        },
    }
}

/// 构造含指定内容 hash、目的和 retention 时间窗的测试 ProcessingGrant。
fn grant(bytes: &[u8], revoked: bool) -> ProcessingGrant {
    ProcessingGrant {
        id: "grant-image".to_owned(),
        source_path: "assets/image.png".to_owned(),
        content_hash: image_payload_digest(bytes),
        class: DataClass::Restricted,
        purpose: Purpose {
            id: "vision-review".to_owned(),
            description: "fixture vision review".to_owned(),
        },
        retention: Retention {
            expires_at_ms: Some(10_000),
            retain_audit_metadata: true,
        },
        parent_ids: Vec::new(),
        created_by: "principal:fixture".to_owned(),
        revoked,
    }
}

/// 用一组固定 route/policy/purpose/class 参数建立短期图片准入 fixture。
fn admission(bytes: &[u8], revoked: bool) -> Result<ImageInputAdmission, String> {
    ImageInputAdmission::new(
        artifact(bytes),
        grant(bytes, revoked),
        "assets/image.png",
        "image/png",
        bytes.to_vec(),
        route(),
        capabilities(CapabilitySupport::Supported),
        format!("sha256:{}", "c".repeat(64)),
        1,
        1,
        "vision-review",
        vec![DataClass::Restricted],
        100,
    )
}

/// 与 `admission` 相同，但可指定媒体类型和 Artifact schema，用于未支持输入的拒绝路径。
fn admission_with_media(
    bytes: &[u8],
    media_type: &str,
    artifact_schema: &str,
) -> Result<ImageInputAdmission, String> {
    let mut artifact = artifact(bytes);
    artifact.artifact_schema = artifact_schema.to_owned();
    ImageInputAdmission::new(
        artifact,
        grant(bytes, false),
        "assets/image.png",
        media_type,
        bytes.to_vec(),
        route(),
        capabilities(CapabilitySupport::Supported),
        format!("sha256:{}", "c".repeat(64)),
        1,
        1,
        "vision-review",
        vec![DataClass::Restricted],
        100,
    )
}

#[test]
/// 未受控绝对路径与 grant source path 不同，必须在 admission 创建阶段拒绝。
fn untrusted_image_path_never_reaches_provider() {
    let bytes = b"fixture-png";
    let result = ImageInputAdmission::new(
        artifact(bytes),
        grant(bytes, false),
        "/tmp/image.png",
        "image/png",
        bytes.to_vec(),
        route(),
        capabilities(CapabilitySupport::Supported),
        format!("sha256:{}", "c".repeat(64)),
        1,
        1,
        "vision-review",
        vec![DataClass::Restricted],
        100,
    );
    assert_eq!(
        result.unwrap_err(),
        "untrusted_image_path_never_reaches_provider"
    );
}

#[test]
/// revoked ProcessingGrant 必须在图片可进入 provider 编码前拒绝。
fn revoked_artifact_grant_blocks_send() {
    let error = admission(b"fixture-png", true).unwrap_err();
    assert_eq!(error, "revoked_artifact_grant_blocks_send");
}

#[test]
/// URL 不是 Artifact reference；准入合同不会隐式抓取网络图片。
fn remote_image_url_is_not_fetched_implicitly() {
    assert_eq!(
        validate_image_attachment_ref("https://example.invalid/image.png").unwrap_err(),
        "remote_image_url_is_not_fetched_implicitly"
    );
}

#[test]
/// 一份字段绑定一致且声明 vision capability 的 admission 可通过 Domain 合同。
/// 这里使用的字节并非有效 PNG，因此不证明 MIME 与真实文件格式匹配。
fn authorized_image_input_reaches_vision_capable_provider() {
    let admitted = admission(b"fixture-png", false).expect("authorized image");
    assert_eq!(admitted.media_type, "image/png");
    assert_eq!(admitted.source_size_bytes(), b"fixture-png".len());
    assert_eq!(admitted.capabilities.images, CapabilitySupport::Supported);
}

#[test]
/// admission 中的 bytes digest 与 Artifact hash 一致，并可匹配请求 AttachmentRef metadata。
fn image_hash_matches_admitted_payload() {
    let bytes = b"fixture-png";
    let admitted = admission(bytes, false).expect("authorized image");
    assert_eq!(admitted.source_digest(), admitted.artifact.content_hash);
    assert!(admitted.matches_attachment(
        &admitted.attachment_ref(),
        "image/png",
        &admitted.artifact.content_hash
    ));
}

#[test]
/// 文档、音频、视频和未列入 allowlist 的图片格式必须给出准确的 unsupported，而不是降级成图片。
fn document_audio_and_video_inputs_are_explicitly_unsupported() {
    for (media_type, artifact_schema, code) in [
        (
            "application/pdf",
            "application/pdf",
            "unsupported_document_upload",
        ),
        ("text/plain", "text/plain", "unsupported_document_upload"),
        (
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "unsupported_document_upload",
        ),
        ("audio/mpeg", "audio/mpeg", "unsupported_audio_video_input"),
        ("video/mp4", "video/mp4", "unsupported_audio_video_input"),
        (
            "image/svg+xml",
            "image/svg+xml",
            "image_media_type_unsupported",
        ),
    ] {
        let error = admission_with_media(b"fixture-payload", media_type, artifact_schema)
            .expect_err("unsupported media type");

        assert_eq!(error, code, "{media_type}");
    }
}
