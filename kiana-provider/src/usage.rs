//! Provider-neutral usage normalization at the admitted model boundary.
//!
//! The adapter only turns a completed provider reply into a domain observation.  It does not
//! charge a project, reserve a permit, retry a request or append an EventLog fact.

use kiana_domain::{
    json_digest, AttemptId, ModelReply, NormalizedUsage, PreparedModelCall, UsageConfidence,
    UsageObservation, UsageSource, UsageVector, UsageId, BillingUnknownReason,
};
use serde_json::json;

/// Convert one admitted reply into a final provider usage observation.
///
/// Missing provider usage remains `Unknown`; it is never converted to zero.  The caller supplies
/// the server-owned attempt/run identity because provider wire data cannot become an authority for
/// those fields.
pub fn normalize_model_reply(
    prepared: &PreparedModelCall,
    reply: &ModelReply,
    usage_id: UsageId,
    attempt_id: AttemptId,
    run_id: kiana_domain::RunId,
) -> Result<NormalizedUsage, String> {
    prepared
        .validate()
        .map_err(|error| error.to_string())?;
    if prepared.spec.assignment.as_ref().is_some_and(|assignment| assignment.run_id != run_id) {
        return Err("provider_usage_run_mismatch".to_owned());
    }
    let (input_tokens, output_tokens, confidence, unknown_reason) = match &reply.output.usage {
        Some(usage) => (
            Some(usage.input_tokens),
            Some(usage.output_tokens),
            UsageConfidence::Known,
            None,
        ),
        None => (
            None,
            None,
            UsageConfidence::Unknown,
            Some(BillingUnknownReason::ProviderUnreported),
        ),
    };
    let vector = UsageVector {
        schema: kiana_domain::USAGE_VECTOR_SCHEMA.to_owned(),
        input_tokens,
        output_tokens,
        ..UsageVector::zero()
    };
    let raw_digest = json_digest(&json!({
        "provider": prepared.route.provider_id,
        "protocol": prepared.route.protocol,
        "request_id": reply.provider_request_id,
        "response_id": reply.provider_response_id,
        "usage": reply.output.usage,
        "model": reply.output.model_id,
    }));
    let mut normalized = NormalizedUsage::new(
        usage_id,
        attempt_id,
        run_id,
        Some(prepared.route.provider_id.clone()),
        prepared.route.model_id.clone(),
        prepared.route.connection_id.clone(),
        vector,
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        unknown_reason,
        format!("provider:{}", protocol_name(prepared.route.protocol)),
        raw_digest,
    )?;
    normalized.served_model_id = reply
        .output
        .model_id
        .clone()
        .or_else(|| Some(prepared.route.model_id.clone()));
    normalized.retry_ordinal = 0;
    normalized.usage_digest = normalized.digest();
    normalized.validate()?;
    Ok(normalized)
}

fn protocol_name(protocol: kiana_domain::ModelProtocol) -> &'static str {
    match protocol {
        kiana_domain::ModelProtocol::Legacy => "legacy",
        kiana_domain::ModelProtocol::AnthropicMessages => "anthropic_messages",
        kiana_domain::ModelProtocol::OpenAiChat => "openai_chat",
        kiana_domain::ModelProtocol::OpenAiResponses => "openai_responses",
        kiana_domain::ModelProtocol::OllamaChat => "ollama_chat",
        kiana_domain::ModelProtocol::GeminiInteractions => "gemini_interactions",
    }
}
