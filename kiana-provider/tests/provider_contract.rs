use kiana_domain::{
    CapabilitySupport, ModelCallSpec, ModelContent, ModelMessage, ModelProtocol, ModelPurpose,
    ModelRequest, ModelResponseFormat, ProviderCassette, ProviderCassetteMode,
    ProviderCassetteSource, ProviderContractCapability, ProviderContractCell,
    ProviderContractMatrix, ProviderContractSupport, RequestId,
};
use kiana_ports::ModelClient;
use kiana_provider::{ProviderConfig, ProviderGateway};
use std::ffi::OsString;

struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    fn new(keys: &[&'static str]) -> Self {
        let values = keys
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect::<Vec<_>>();
        for key in keys {
            std::env::remove_var(key);
        }
        Self(values)
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn fixture_matrix() -> ProviderContractMatrix {
    let protocols = [
        ModelProtocol::AnthropicMessages,
        ModelProtocol::OpenAiChat,
        ModelProtocol::OpenAiResponses,
        ModelProtocol::OllamaChat,
        ModelProtocol::GeminiInteractions,
    ];
    let mut cells = Vec::new();
    for (index, protocol) in protocols.into_iter().enumerate() {
        for (capability_index, capability) in
            ProviderContractCapability::ALL.into_iter().enumerate()
        {
            let (support, fixture_ids, preflight_error) = match capability {
                ProviderContractCapability::Tools
                | ProviderContractCapability::Reasoning
                | ProviderContractCapability::Structured => {
                    (ProviderContractSupport::Unknown, Vec::new(), None)
                }
                ProviderContractCapability::Image => (
                    ProviderContractSupport::Unsupported,
                    Vec::new(),
                    Some("unsupported_content_block_fails_before_request".to_owned()),
                ),
                _ => (
                    ProviderContractSupport::Supported,
                    vec![format!("fixture-{index}-{capability_index}")],
                    None,
                ),
            };
            cells.push(
                ProviderContractCell::new(
                    protocol,
                    capability,
                    support,
                    fixture_ids,
                    preflight_error,
                )
                .expect("matrix cell"),
            );
        }
    }
    ProviderContractMatrix::new("offline-fixture", "p4-j7-29.v1", cells).expect("matrix")
}

#[test]
fn matrix_requires_explicit_support_fixture_or_preflight_error() {
    let matrix = fixture_matrix();
    matrix.validate().expect("valid contract matrix");
    assert_eq!(
        matrix
            .unsupported_preflight_error(
                ModelProtocol::OpenAiChat,
                ProviderContractCapability::Image,
            )
            .expect("unsupported image error"),
        "unsupported_content_block_fails_before_request"
    );
    assert!(matrix
        .cell(
            ModelProtocol::GeminiInteractions,
            ProviderContractCapability::Streaming,
        )
        .is_some_and(|cell| cell.support == ProviderContractSupport::Supported));
    assert!(matrix.canonical_bytes().expect("canonical matrix").len() > 64);
}

#[test]
fn unsupported_capability_matrix_matches_preflight_error_without_network() {
    let _env = EnvRestore::new(&["KIANA_MODEL_PROFILES_JSON", "KIANA_STREAMING"]);
    let gateway = ProviderGateway::from_env(ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: Some("fixture-model".to_owned()),
        base_url: Some("http://127.0.0.1:22114".to_owned()),
        api_key: None,
    })
    .expect("offline gateway configuration");
    let entry = &gateway.model_catalog().expect("catalog").entries[0];
    assert_eq!(
        ProviderContractCapability::Image.model_support(&entry.capabilities),
        CapabilitySupport::Unsupported
    );
    let message = ModelMessage::with_content(
        kiana_domain::ModelRole::User,
        vec![ModelContent::AttachmentRef {
            artifact_ref: "artifact:fixture-image".to_owned(),
            media_type: "image/png".to_owned(),
            digest: digest('a'),
        }],
    )
    .expect("valid attachment reference");
    let error = gateway
        .prepare_call(
            ModelRequest {
                messages: vec![message],
                tools: Vec::new(),
                sandbox: "read-only".to_owned(),
            },
            ModelCallSpec {
                call_id: RequestId::new(),
                attempt_id: RequestId::new(),
                model_attempt_id: None,
                step_id: None,
                step: 1,
                purpose: ModelPurpose::Task,
                assignment: None,
                response_format: ModelResponseFormat::Text,
                replay: Vec::new(),
                deadline_unix_ms: u64::MAX,
            },
        )
        .expect_err("unsupported image must fail before request");
    assert_eq!(error.code, "unsupported_content_block_fails_before_request");
}

#[test]
fn fixture_replay_is_offline_and_fake_cannot_become_live_implicitly() {
    let raw = include_str!("../fixtures/provider/p4-j7-29-cassette.json");
    let value: serde_json::Value = serde_json::from_str(raw).expect("cassette fixture json");
    assert_eq!(value["mode"], "replay");
    assert_eq!(value["source"], "synthetic");
    assert_eq!(value["external_connection_allowed"], false);
    assert_eq!(value["live_opt_in"], false);
    assert!(!raw.contains("http://") && !raw.contains("https://"));

    let replay = ProviderCassette::new(
        "fixture-cassette",
        ModelProtocol::OpenAiChat,
        ProviderCassetteMode::Replay,
        ProviderCassetteSource::Synthetic,
        digest('b'),
        digest('c'),
        false,
        false,
    )
    .expect("offline replay cassette");
    replay.validate().expect("valid replay cassette");
    assert!(ProviderCassette::new(
        "synthetic-record",
        ModelProtocol::OpenAiChat,
        ProviderCassetteMode::Record,
        ProviderCassetteSource::Synthetic,
        digest('b'),
        digest('c'),
        false,
        false,
    )
    .is_err());
}
