use kiana_domain::{
    ModelProtocol, ModelUsage, ProviderConnectionKind, ProviderLiveConnectionEvidence,
    ProviderLiveEvidenceSource, ProviderLiveEvidenceStatus, ProviderLiveUsage,
};

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn evidence(
    source: ProviderLiveEvidenceSource,
    status: ProviderLiveEvidenceStatus,
    approval: Option<String>,
    limitations: Vec<String>,
) -> ProviderLiveConnectionEvidence {
    ProviderLiveConnectionEvidence::new(
        "openai-compatible",
        "default",
        ProviderConnectionKind::CompatibleGateway,
        ModelProtocol::OpenAiChat,
        "default",
        "gpt-4.1-mini",
        Some("gpt-4.1-mini".to_owned()),
        digest('a'),
        digest('b'),
        digest('c'),
        source,
        status,
        approval,
        2,
        true,
        true,
        true,
        true,
        ProviderLiveUsage::Known {
            usage: ModelUsage {
                input_tokens: 12,
                output_tokens: 8,
            },
        },
        Some(digest('d')),
        Some(digest('e')),
        limitations,
    )
    .expect("evidence fixture")
}

#[test]
fn verified_connection_requires_real_operator_bound_evidence() {
    let value = evidence(
        ProviderLiveEvidenceSource::LiveNetwork,
        ProviderLiveEvidenceStatus::Verified,
        Some("approval:provider-live-1".to_owned()),
        Vec::new(),
    );
    value.validate().expect("verified evidence validates");
}

#[test]
fn synthetic_streaming_cannot_be_marked_verified() {
    let result = ProviderLiveConnectionEvidence::new(
        "openai-compatible",
        "default",
        ProviderConnectionKind::CompatibleGateway,
        ModelProtocol::OpenAiChat,
        "default",
        "gpt-4.1-mini",
        Some("gpt-4.1-mini".to_owned()),
        digest('a'),
        digest('b'),
        digest('c'),
        ProviderLiveEvidenceSource::Synthetic,
        ProviderLiveEvidenceStatus::Verified,
        Some("approval:provider-live-1".to_owned()),
        2,
        true,
        true,
        true,
        true,
        ProviderLiveUsage::Known {
            usage: ModelUsage {
                input_tokens: 1,
                output_tokens: 1,
            },
        },
        Some(digest('d')),
        Some(digest('e')),
        Vec::new(),
    );
    assert_eq!(
        result.expect_err("synthetic evidence must be rejected"),
        "provider_live_synthetic_evidence"
    );
}

#[test]
fn unverified_connection_must_explain_why_it_did_not_close() {
    let result = ProviderLiveConnectionEvidence::new(
        "ollama",
        "local",
        ProviderConnectionKind::Local,
        ModelProtocol::OllamaChat,
        "local",
        "llama3.2",
        None,
        digest('a'),
        "none",
        digest('c'),
        ProviderLiveEvidenceSource::LiveLocal,
        ProviderLiveEvidenceStatus::Unverified,
        None,
        0,
        false,
        false,
        false,
        false,
        ProviderLiveUsage::Unknown {
            reason: "no authorized local observation".to_owned(),
        },
        None,
        None,
        Vec::new(),
    );
    assert_eq!(
        result.expect_err("unverified evidence needs a limitation"),
        "provider_live_nonverified_reason_required"
    );
}

#[test]
fn local_connection_can_expose_a_credential_free_revision() {
    let value = ProviderLiveConnectionEvidence::new(
        "ollama",
        "local",
        ProviderConnectionKind::Local,
        ModelProtocol::OllamaChat,
        "local",
        "llama3.2",
        None,
        digest('a'),
        "none",
        digest('c'),
        ProviderLiveEvidenceSource::LiveLocal,
        ProviderLiveEvidenceStatus::Skipped,
        None,
        0,
        false,
        false,
        false,
        false,
        ProviderLiveUsage::Unknown {
            reason: "local provider is not configured".to_owned(),
        },
        None,
        None,
        vec!["no authorized local observation".to_owned()],
    )
    .expect("credential-free local evidence is valid");
    value.validate().expect("skipped evidence validates");
}

#[test]
fn verified_connection_requires_typed_operator_approval() {
    let mut value = evidence(
        ProviderLiveEvidenceSource::LiveNetwork,
        ProviderLiveEvidenceStatus::Verified,
        Some("operator-approval".to_owned()),
        Vec::new(),
    );
    value.evidence_digest = value.digest();
    assert_eq!(
        value.validate().expect_err("approval ref must be typed"),
        "provider_live_operator_approval_ref_invalid"
    );
}
