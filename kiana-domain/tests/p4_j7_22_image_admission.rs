use kiana_domain::{
    image_payload_digest, validate_image_attachment_ref, ArtifactId, ArtifactProvenance,
    ArtifactRef, CapabilitySupport, DataClass, ImageInputAdmission, ModelCapabilities,
    ModelProtocol, ModelRoute, ProcessingGrant, Purpose, Retention,
};

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

#[test]
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
    assert_eq!(result.unwrap_err(), "untrusted_image_path_never_reaches_provider");
}

#[test]
fn revoked_artifact_grant_blocks_send() {
    let error = admission(b"fixture-png", true).unwrap_err();
    assert_eq!(error, "revoked_artifact_grant_blocks_send");
}

#[test]
fn remote_image_url_is_not_fetched_implicitly() {
    assert_eq!(
        validate_image_attachment_ref("https://example.invalid/image.png").unwrap_err(),
        "remote_image_url_is_not_fetched_implicitly"
    );
}

#[test]
fn authorized_image_input_reaches_vision_capable_provider() {
    let admitted = admission(b"fixture-png", false).expect("authorized image");
    assert_eq!(admitted.media_type, "image/png");
    assert_eq!(admitted.source_size_bytes(), b"fixture-png".len());
    assert_eq!(admitted.capabilities.images, CapabilitySupport::Supported);
}

#[test]
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
