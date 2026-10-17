//! P4-J7-22 provider 端到端图片准入夹具（离线，不发起任何网络请求）。
//!
//! `KIANA_MODEL_PROFILES_JSON` 声明三个 profile：vision 的 Anthropic、vision 的 Ollama，
//! 以及一个不具备 image capability 的 Anthropic。夹具证明：
//!
//! 1. 普通 `prepare_call` 对没有 `ImageInputAdmission` 的 `AttachmentRef` 在发送前拒绝；
//! 2. 已授权图片按协议图片块形状进入 wire body，且 wire payload 的摘要等于 Artifact hash；
//! 3. revoked grant、过期 retention、跨 route 复用、非 vision route、未引用 admission、
//!    超量 admission、remote URL / 任意路径、文档与音视频输入都有稳定拒绝码；
//! 4. 工具返回的图片复用同一准入通道：Anthropic 落在 `tool_result` 内，其它协议明确拒绝。
//!
//! `prepare_call_with_images` 只编译请求，不消费 permit、不建连；这些用例因此可以在 CI 里
//! 无网络运行。它们不证明真实 provider 行为、ArtifactStore 恢复或远端文件生命周期。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use kiana_domain::{
    image_payload_digest, ArtifactId, ArtifactProvenance, ArtifactRef, CapabilitySupport,
    DataClass, ImageInputAdmission, ModelAssignment, ModelCallSpec, ModelCapabilities,
    ModelContent, ModelMessage, ModelProtocol, ModelPurpose, ModelRequest, ModelResponseFormat,
    ModelRole, ModelRoute, ModelToolCall, ProcessingGrant, Purpose, RequestId, Retention, RoleSpec,
    RunId, SchemaVersion, TurnId,
};
use kiana_ports::ModelClient;
use kiana_provider::{ProviderConfig, ProviderGateway};
use serde_json::{json, Value};
use std::ffi::OsString;

/// Anthropic profile 的占位凭据变量；夹具只写入固定占位值，不接触任何真实密钥。
const FIXTURE_KEY_ENV: &str = "KIANA_P4_J7_22_FIXTURE_KEY";

/// 三个离线 profile：`executing` = vision Anthropic，`quality` = vision Ollama，
/// `analysis` = 声明不支持图片的 Anthropic。base_url 全是回环地址，夹具不会发请求。
const PROFILES: &str = r#"{
  "executing": {
    "provider": "anthropic",
    "model": "claude-vision-fixture",
    "base_url": "http://127.0.0.1:22971",
    "api_key_env": "KIANA_P4_J7_22_FIXTURE_KEY",
    "capabilities": {"tools": true, "images": true, "structured_output": true, "context_window": 200000, "max_output": 4096}
  },
  "quality": {
    "provider": "ollama",
    "model": "llava-fixture",
    "base_url": "http://127.0.0.1:22972",
    "capabilities": {"tools": true, "images": true, "structured_output": true, "context_window": 128000, "max_output": 4096}
  },
  "analysis": {
    "provider": "anthropic",
    "model": "claude-text-only-fixture",
    "base_url": "http://127.0.0.1:22973",
    "api_key_env": "KIANA_P4_J7_22_FIXTURE_KEY",
    "capabilities": {"tools": true, "images": false, "structured_output": true, "context_window": 200000, "max_output": 4096}
  }
}"#;

/// 与工作区/环境隔离的 provider 夹具环境变量，Drop 时逐个还原。
struct FixtureEnv {
    previous: Vec<(&'static str, Option<OsString>)>,
}

impl FixtureEnv {
    fn new() -> Self {
        let values = [
            ("KIANA_MODEL_PROFILES_JSON", PROFILES),
            ("KIANA_PROVIDER", "ollama"),
            ("KIANA_OLLAMA_MODEL", "fixture-default"),
            ("KIANA_OLLAMA_BASE_URL", "http://127.0.0.1:22970"),
            ("KIANA_STREAMING", "off"),
            (FIXTURE_KEY_ENV, "fixture-key"),
        ];
        let mut previous = Vec::new();
        for (key, value) in values {
            previous.push((key, std::env::var_os(key)));
            std::env::set_var(key, value);
        }
        Self { previous }
    }

    fn gateway(&self) -> ProviderGateway {
        ProviderGateway::from_env(ProviderConfig {
            provider: None,
            model: None,
            base_url: None,
            api_key: None,
        })
        .expect("offline fixture gateway")
    }
}

impl Drop for FixtureEnv {
    fn drop(&mut self) {
        for (key, value) in &self.previous {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// 从 catalog 里取出某个 profile 的完整 route（含配置摘要），供 admission 绑定。
fn route_for(gateway: &ProviderGateway, profile: &str) -> ModelRoute {
    let catalog = gateway.catalog();
    let route = catalog["connections"]
        .as_array()
        .expect("catalog connections")
        .iter()
        .map(|connection| &connection["route"])
        .find(|route| route["profile"] == profile)
        .expect("fixture route");
    serde_json::from_value(route.clone()).expect("model route fixture")
}

/// 为指定 profile 构造一个已授权面的模型调用 spec（含 server-owned assignment）。
fn spec(role: RoleSpec) -> ModelCallSpec {
    ModelCallSpec {
        call_id: RequestId::new(),
        attempt_id: RequestId::new(),
        model_attempt_id: None,
        step_id: None,
        step: 1,
        purpose: ModelPurpose::Task,
        assignment: Some(ModelAssignment {
            schema: "kiana.model-assignment.v1".to_owned(),
            run_id: RunId::new(),
            turn_id: TurnId::new(),
            role_id: role.role_id.clone(),
            role_version: Some(role.version),
            catalog_version: Some(SchemaVersion::new(1, 0)),
            prompt_hash: Some(role.prompt_hash.clone()),
            input_schema: Some(role.input_schema.clone()),
            output_schema: Some(role.output_schema.clone()),
            profile: role.model_profile.clone(),
            project_root: "/repo".to_owned(),
            project_trusted: true,
            authority_revision: None,
            max_wall_time_ms: 10_000,
            runtime_budget: None,
        }),
        response_format: ModelResponseFormat::Text,
        replay: Vec::new(),
        deadline_unix_ms: u64::MAX,
    }
}

/// admission 的媒体类型选项；artifact schema 与 media type 保持一致，避免先撞上 mismatch。
struct AdmissionOptions {
    media_type: &'static str,
    artifact_schema: &'static str,
}

impl AdmissionOptions {
    fn png() -> Self {
        Self {
            media_type: "image/png",
            artifact_schema: "image/png",
        }
    }
}

/// 把一份已准入的图片改写成另一种媒体类型。
///
/// `ImageInputAdmission::new` 只接受 allowlist 内的图片格式，因此“文档/音视频/未支持格式必须
/// 拒绝”只能在准入创建之后、派发之前改写字段来覆盖；派发校验会重新读取这些 public metadata。
fn with_media_type(mut image: ImageInputAdmission, media_type: &str) -> ImageInputAdmission {
    image.media_type = media_type.to_owned();
    image.artifact.artifact_schema = media_type.to_owned();
    image
}

/// 构造一份绑定到 `profile` 真实 route 的图片准入；bytes 固定为 `fixture-image-payload`。
fn admitted(
    gateway: &ProviderGateway,
    profile: &str,
    options: AdmissionOptions,
) -> ImageInputAdmission {
    let bytes = b"fixture-image-payload".to_vec();
    let digest = image_payload_digest(&bytes);
    let artifact = ArtifactRef {
        schema: "kiana.artifact-ref.v1".to_owned(),
        artifact_id: ArtifactId::new(),
        version: 1,
        artifact_schema: options.artifact_schema.to_owned(),
        content_hash: digest.clone(),
        scope_digest: format!("sha256:{}", "b".repeat(64)),
        provenance: ArtifactProvenance {
            producer_kind: "fixture".to_owned(),
            producer_id: "image-fixture".to_owned(),
            source_event_id: None,
            source_run_id: None,
            recorded_by: "fixture".to_owned(),
        },
    };
    let grant = ProcessingGrant {
        id: "grant-image".to_owned(),
        source_path: "assets/image.png".to_owned(),
        content_hash: digest,
        class: DataClass::Restricted,
        purpose: Purpose {
            id: "vision-review".to_owned(),
            description: "fixture vision review".to_owned(),
        },
        retention: Retention {
            expires_at_ms: None,
            retain_audit_metadata: true,
        },
        parent_ids: Vec::new(),
        created_by: "principal:fixture".to_owned(),
        revoked: false,
    };
    let capabilities = ModelCapabilities {
        tools: CapabilitySupport::Supported,
        streaming: CapabilitySupport::Supported,
        structured_output: CapabilitySupport::Supported,
        images: CapabilitySupport::Supported,
        reasoning_replay: CapabilitySupport::Unsupported,
        context_window: 200_000,
        max_output: 4_096,
        source: "fixture".to_owned(),
        revision: "fixture.v1".to_owned(),
    };
    ImageInputAdmission::new(
        artifact,
        grant,
        "assets/image.png",
        options.media_type,
        bytes,
        route_for(gateway, profile),
        capabilities,
        format!("sha256:{}", "c".repeat(64)),
        1,
        1,
        "vision-review",
        vec![DataClass::Restricted],
        100,
    )
    .expect("image admission fixture")
}

/// 引用 admission 的附件块。
fn attachment_block(image: &ImageInputAdmission) -> ModelContent {
    ModelContent::AttachmentRef {
        artifact_ref: image.attachment_ref(),
        media_type: image.media_type.clone(),
        digest: image.artifact.content_hash.clone(),
    }
}

/// 一个引用图片的用户请求。
fn image_request(image: &ImageInputAdmission) -> ModelRequest {
    ModelRequest {
        messages: vec![ModelMessage::with_content(
            ModelRole::User,
            vec![
                ModelContent::Text {
                    text: "describe the fixture image".to_owned(),
                },
                attachment_block(image),
            ],
        )
        .expect("user image message")],
        tools: Vec::new(),
        sandbox: "read-only".to_owned(),
    }
}

/// 五个规范工具之一的最小 schema，供工具名映射使用。
fn fixture_tools() -> Vec<Value> {
    vec![json!({
        "name":"shell",
        "description":"fixture shell",
        "parameters":{"type":"object","properties":{"command":{"type":"string"}}}
    })]
}

/// 一个工具返回图片的请求：assistant tool_call + tool 结果（文本 + 附件引用）。
fn tool_image_request(image: &ImageInputAdmission) -> ModelRequest {
    let call = ModelToolCall {
        id: "call-image".to_owned(),
        name: "shell".to_owned(),
        arguments: json!({"command":"fixture-screenshot"}),
    };
    ModelRequest {
        messages: vec![
            ModelMessage::with_content(
                ModelRole::Assistant,
                vec![ModelContent::ToolCall { call: call.clone() }],
            )
            .expect("assistant tool call"),
            ModelMessage::with_content(
                ModelRole::Tool,
                vec![
                    ModelContent::ToolResult {
                        call_id: call.id.clone(),
                        content: "{\"status\":\"succeeded\"}".to_owned(),
                        is_error: false,
                    },
                    attachment_block(image),
                ],
            )
            .expect("tool result image"),
        ],
        tools: fixture_tools(),
        sandbox: "read-only".to_owned(),
    }
}

#[test]
fn untrusted_attachment_without_admission_never_reaches_provider() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let image = admitted(&gateway, "executing", AdmissionOptions::png());

    let error = gateway
        .prepare_call(image_request(&image), spec(RoleSpec::builder()))
        .expect_err("unadmitted attachment");

    assert_eq!(error.code, "image_admission_required");
    assert!(!error.request_sent);
}

#[test]
fn untrusted_image_path_and_remote_url_never_reach_provider() {
    let digest = format!("sha256:{}", "a".repeat(64));
    for (artifact_ref, code) in [
        (
            "https://example.invalid/image.png",
            "remote_image_url_is_not_fetched_implicitly",
        ),
        (
            "/tmp/secret/image.png",
            "untrusted_image_path_never_reaches_provider",
        ),
    ] {
        let error = ModelMessage::with_content(
            ModelRole::User,
            vec![ModelContent::AttachmentRef {
                artifact_ref: artifact_ref.to_owned(),
                media_type: "image/png".to_owned(),
                digest: digest.clone(),
            }],
        )
        .expect_err("untrusted reference");

        assert_eq!(error.code, code, "{artifact_ref}");
    }
}

#[test]
fn authorized_image_input_reaches_vision_capable_provider() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let image = admitted(&gateway, "executing", AdmissionOptions::png());

    let prepared = gateway
        .prepare_call_with_images(
            image_request(&image),
            spec(RoleSpec::builder()),
            vec![image.clone()],
        )
        .expect("admitted image");

    assert_eq!(prepared.route.protocol, ModelProtocol::AnthropicMessages);
    let messages = prepared.wire_body["messages"]
        .as_array()
        .expect("anthropic messages");
    let content = messages[0]["content"]
        .as_array()
        .expect("user content blocks");
    let block = content
        .iter()
        .find(|block| block["type"] == "image")
        .expect("image block");
    assert_eq!(block["source"]["type"], "base64");
    assert_eq!(block["source"]["media_type"], "image/png");
    let encoded = block["source"]["data"].as_str().expect("base64 data");
    assert!(!encoded.contains("assets/image.png"));
    assert_eq!(
        STANDARD.decode(encoded).expect("base64 image"),
        b"fixture-image-payload".to_vec()
    );
}

#[test]
fn image_hash_matches_admitted_payload_after_encoding() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let image = admitted(&gateway, "executing", AdmissionOptions::png());
    assert_eq!(image.source_digest(), image.artifact.content_hash);

    let prepared = gateway
        .prepare_call_with_images(
            image_request(&image),
            spec(RoleSpec::builder()),
            vec![image.clone()],
        )
        .expect("admitted image");
    let encoded = prepared.wire_body["messages"][0]["content"]
        .as_array()
        .expect("user content blocks")
        .iter()
        .find(|block| block["type"] == "image")
        .expect("image block")["source"]["data"]
        .as_str()
        .expect("base64 data")
        .to_owned();
    let decoded = STANDARD.decode(encoded).expect("base64 image");

    assert_eq!(image_payload_digest(&decoded), image.artifact.content_hash);
    assert_eq!(decoded.len(), image.source_size_bytes());
}

#[test]
fn revoked_artifact_grant_blocks_send() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let mut image = admitted(&gateway, "executing", AdmissionOptions::png());
    image.grant.revoked = true;

    let error = gateway
        .prepare_call_with_images(
            image_request(&image),
            spec(RoleSpec::builder()),
            vec![image.clone()],
        )
        .expect_err("revoked grant");

    assert_eq!(error.code, "revoked_artifact_grant_blocks_send");
    assert!(!error.request_sent);
}

#[test]
fn retention_expiry_is_checked_at_dispatch_time() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    // 创建时尚未过期（expires 0 非法，这里用过期之后才被 dispatch 校验的时间戳）。
    let mut image = admitted(&gateway, "executing", AdmissionOptions::png());
    image.grant.retention.expires_at_ms = Some(1);

    let error = gateway
        .prepare_call_with_images(
            image_request(&image),
            spec(RoleSpec::builder()),
            vec![image.clone()],
        )
        .expect_err("expired retention");

    assert_eq!(error.code, "image_processing_grant_expired");
    assert!(!error.request_sent);
}

#[test]
fn admission_bound_to_another_route_cannot_be_replayed() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let image = admitted(&gateway, "executing", AdmissionOptions::png());

    let error = gateway
        .prepare_call_with_images(
            image_request(&image),
            spec(RoleSpec::qa()),
            vec![image.clone()],
        )
        .expect_err("route replay");

    assert_eq!(error.code, "image_route_scope_mismatch");
}

#[test]
fn non_vision_route_rejects_attachment_before_send() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let image = admitted(&gateway, "executing", AdmissionOptions::png());

    let error = gateway
        .prepare_call_with_images(
            image_request(&image),
            spec(RoleSpec::analyst()),
            vec![image.clone()],
        )
        .expect_err("non-vision route");

    assert_eq!(error.code, "unsupported_content_block_fails_before_request");
}

#[test]
fn unreferenced_and_oversized_admission_lists_stay_before_send() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let image = admitted(&gateway, "executing", AdmissionOptions::png());

    let plain_request = ModelRequest {
        messages: vec![ModelMessage::with_content(
            ModelRole::User,
            vec![ModelContent::Text {
                text: "no image here".to_owned(),
            }],
        )
        .expect("plain user message")],
        tools: Vec::new(),
        sandbox: "read-only".to_owned(),
    };
    let error = gateway
        .prepare_call_with_images(
            plain_request,
            spec(RoleSpec::builder()),
            vec![image.clone()],
        )
        .expect_err("unreferenced admission");
    assert_eq!(error.code, "image_admission_without_attachment");

    // 请求引用了图片 A，但 admission 列表里混入了一份没有任何附件引用它的 admission B：
    // 不能带着无法对账的准入继续编码。
    let extra = admitted(&gateway, "executing", AdmissionOptions::png());
    let error = gateway
        .prepare_call_with_images(
            image_request(&image),
            spec(RoleSpec::builder()),
            vec![image.clone(), extra],
        )
        .expect_err("admission without a referencing attachment");
    assert_eq!(error.code, "image_admission_unreferenced");

    let error = gateway
        .prepare_call_with_images(
            image_request(&image),
            spec(RoleSpec::builder()),
            vec![image.clone(); 33],
        )
        .expect_err("oversized admission list");
    assert_eq!(error.code, "image_admission_count_limit");
}

#[test]
fn document_audio_and_video_inputs_are_explicitly_unsupported() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    for (media_type, code) in [
        ("application/pdf", "unsupported_document_upload"),
        ("text/plain", "unsupported_document_upload"),
        ("audio/mpeg", "unsupported_audio_video_input"),
        ("video/mp4", "unsupported_audio_video_input"),
        ("image/svg+xml", "image_media_type_unsupported"),
    ] {
        let image = with_media_type(
            admitted(&gateway, "executing", AdmissionOptions::png()),
            media_type,
        );
        let error = gateway
            .prepare_call_with_images(
                image_request(&image),
                spec(RoleSpec::builder()),
                vec![image.clone()],
            )
            .expect_err("unsupported media type");

        assert_eq!(error.code, code, "{media_type}");
    }
}

#[test]
fn tool_result_image_reaches_anthropic_tool_result() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let image = admitted(&gateway, "executing", AdmissionOptions::png());

    let prepared = gateway
        .prepare_call_with_images(
            tool_image_request(&image),
            spec(RoleSpec::builder()),
            vec![image.clone()],
        )
        .expect("anthropic tool image");

    let messages = prepared.wire_body["messages"]
        .as_array()
        .expect("anthropic messages");
    let tool_result = messages
        .iter()
        .flat_map(|message| message["content"].as_array().into_iter().flatten())
        .find(|block| block["type"] == "tool_result")
        .expect("tool result block");
    assert_eq!(tool_result["tool_use_id"].as_str(), Some("call-image"));
    let content = tool_result["content"]
        .as_array()
        .expect("tool result content blocks");
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "{\"status\":\"succeeded\"}");
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
    assert_eq!(
        STANDARD
            .decode(content[1]["source"]["data"].as_str().expect("base64 data"))
            .expect("base64 image"),
        b"fixture-image-payload".to_vec()
    );
}

#[test]
fn tool_result_image_is_rejected_without_a_protocol_tool_image_shape() {
    let env = FixtureEnv::new();
    let gateway = env.gateway();
    let image = admitted(&gateway, "quality", AdmissionOptions::png());

    let error = gateway
        .prepare_call_with_images(
            tool_image_request(&image),
            spec(RoleSpec::qa()),
            vec![image.clone()],
        )
        .expect_err("ollama tool image");

    assert_eq!(error.code, "image_tool_result_protocol_unsupported");
    assert!(!error.request_sent);
}
