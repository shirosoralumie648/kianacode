//! Provider wire usage adapters for the neutral billing contract.
//!
//! The response decoders own protocol syntax; this module owns the narrower accounting
//! boundary.  It copies only bounded numeric usage and the provider's served-model observation
//! into [`kiana_domain::NormalizedUsage`].  Requested model, attempt, run and route identity stay
//! server-owned by the admitted [`PreparedModelCall`].  Missing fields remain unknown and are
//! never filled with zero.

use kiana_domain::{
    json_digest, AttemptId, BillingUnknownReason, ModelProtocol, NormalizedUsage,
    PreparedModelCall, RunId, UsageConfidence, UsageId, UsageObservation, UsageSource, UsageVector,
};
use serde_json::{Map, Value};

pub const PROVIDER_USAGE_ADAPTER_SCHEMA: &str = "kiana.provider-usage-adapter.v1";
pub const MAX_PROVIDER_USAGE_TOKENS: u64 = 1_000_000_000;

fn invalid(field: &str) -> String {
    format!("provider_usage_{field}_invalid")
}

fn bounded_text(value: &str, field: &str, max: usize) -> Result<String, String> {
    if value.trim().is_empty()
        || value.len() > max
        || value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(invalid(field));
    }
    Ok(value.to_owned())
}

fn optional_u64(value: Option<&Value>, field: &str) -> Result<Option<u64>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let number = value.as_u64().ok_or_else(|| invalid(field))?;
            if number > MAX_PROVIDER_USAGE_TOKENS {
                return Err(invalid(field));
            }
            Ok(Some(number))
        }
    }
}

fn object<'a>(value: &'a Value, field: &str) -> Result<Option<&'a Map<String, Value>>, String> {
    match value {
        Value::Null => Ok(None),
        Value::Object(object) => Ok(Some(object)),
        _ => Err(invalid(field)),
    }
}

fn nested_u64(
    value: Option<&Map<String, Value>>,
    field: &str,
    nested: &str,
) -> Result<Option<u64>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(value) = value.get(nested) else {
        return Ok(None);
    };
    optional_u64(Some(value), field)
}

fn served_model(response: &Map<String, Value>) -> Result<Option<String>, String> {
    let value = response
        .get("model")
        .or_else(|| response.get("model_version"))
        .or_else(|| response.get("modelVersion"));
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => bounded_text(
            value.as_str().ok_or_else(|| invalid("served_model"))?,
            "served_model",
            256,
        )
        .map(Some),
    }
}

fn usage_object<'a>(
    response: &'a Map<String, Value>,
    protocol: ModelProtocol,
) -> Result<Option<&'a Map<String, Value>>, String> {
    match protocol {
        ModelProtocol::OllamaChat => Ok(Some(response)),
        ModelProtocol::GeminiInteractions => {
            let usage = response
                .get("usage_metadata")
                .or_else(|| response.get("usageMetadata"));
            usage
                .map(|value| object(value, "usage"))
                .transpose()?
                .flatten()
                .map(Ok)
                .transpose()
        }
        ModelProtocol::AnthropicMessages
        | ModelProtocol::OpenAiChat
        | ModelProtocol::OpenAiResponses
        | ModelProtocol::Legacy => response
            .get("usage")
            .map(|value| object(value, "usage"))
            .transpose()?
            .flatten()
            .map(Ok)
            .transpose(),
    }
}

fn protocol_fields(
    response: &Map<String, Value>,
    protocol: ModelProtocol,
) -> Result<
    (
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
    ),
    String,
> {
    let usage = usage_object(response, protocol)?;
    let fields = usage;
    match protocol {
        ModelProtocol::AnthropicMessages => Ok((
            optional_u64(
                fields.and_then(|value| value.get("input_tokens")),
                "input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("output_tokens")),
                "output_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("cache_read_input_tokens")),
                "cache_read_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("cache_creation_input_tokens")),
                "cache_write_tokens",
            )?,
            None,
            None,
            optional_u64(
                fields.and_then(|value| value.get("total_tokens")),
                "total_tokens",
            )?,
        )),
        ModelProtocol::OpenAiChat => {
            let details = fields.and_then(|value| value.get("prompt_tokens_details"));
            let completion_details =
                fields.and_then(|value| value.get("completion_tokens_details"));
            Ok((
                optional_u64(
                    fields.and_then(|value| value.get("prompt_tokens")),
                    "input_tokens",
                )?,
                optional_u64(
                    fields.and_then(|value| value.get("completion_tokens")),
                    "output_tokens",
                )?,
                nested_u64(
                    object_or_none(details, "prompt_tokens_details")?,
                    "cache_read_tokens",
                    "cached_tokens",
                )?,
                None,
                nested_u64(
                    object_or_none(completion_details, "completion_tokens_details")?,
                    "reasoning_tokens",
                    "reasoning_tokens",
                )?,
                None,
                optional_u64(
                    fields.and_then(|value| value.get("total_tokens")),
                    "total_tokens",
                )?,
            ))
        }
        ModelProtocol::OpenAiResponses => {
            let input_details = fields.and_then(|value| value.get("input_tokens_details"));
            Ok((
                optional_u64(
                    fields.and_then(|value| value.get("input_tokens")),
                    "input_tokens",
                )?,
                optional_u64(
                    fields.and_then(|value| value.get("output_tokens")),
                    "output_tokens",
                )?,
                nested_u64(
                    object_or_none(input_details, "input_tokens_details")?,
                    "cache_read_tokens",
                    "cached_tokens",
                )?,
                None,
                nested_u64(
                    object_or_none(
                        fields.and_then(|value| value.get("output_tokens_details")),
                        "output_tokens_details",
                    )?,
                    "reasoning_tokens",
                    "reasoning_tokens",
                )?,
                None,
                optional_u64(
                    fields.and_then(|value| value.get("total_tokens")),
                    "total_tokens",
                )?,
            ))
        }
        ModelProtocol::OllamaChat => Ok((
            optional_u64(
                fields.and_then(|value| value.get("prompt_eval_count")),
                "input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("eval_count")),
                "output_tokens",
            )?,
            None,
            None,
            None,
            None,
            None,
        )),
        ModelProtocol::GeminiInteractions => Ok((
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("prompt_token_count")
                        .or_else(|| value.get("promptTokenCount"))
                        .or_else(|| value.get("total_input_tokens"))
                }),
                "input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("candidates_token_count")
                        .or_else(|| value.get("candidatesTokenCount"))
                        .or_else(|| value.get("total_output_tokens"))
                }),
                "output_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("cached_content_token_count")
                        .or_else(|| value.get("cachedContentTokenCount"))
                }),
                "cache_read_tokens",
            )?,
            None,
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("thoughts_token_count")
                        .or_else(|| value.get("thoughtsTokenCount"))
                        .or_else(|| value.get("total_thought_tokens"))
                }),
                "reasoning_tokens",
            )?,
            None,
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("total_token_count")
                        .or_else(|| value.get("totalTokenCount"))
                        .or_else(|| value.get("total_tokens"))
                }),
                "total_tokens",
            )?,
        )),
        // The legacy route is the deterministic Fake adapter in offline fixtures.  It uses the
        // neutral field names directly and is never treated as a live provider claim.
        ModelProtocol::Legacy => Ok((
            optional_u64(
                fields.and_then(|value| value.get("input_tokens")),
                "input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("output_tokens")),
                "output_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("cache_read_tokens")),
                "cache_read_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("cache_write_tokens")),
                "cache_write_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("reasoning_output_tokens")),
                "reasoning_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("audio_input_tokens")),
                "audio_input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("total_tokens")),
                "total_tokens",
            )?,
        )),
    }
}

fn object_or_none<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<Option<&'a Map<String, Value>>, String> {
    match value {
        None => Ok(None),
        Some(value) => object(value, field),
    }
}

fn check_total(input: Option<u64>, output: Option<u64>, total: Option<u64>) -> Result<(), String> {
    let Some(total) = total else {
        return Ok(());
    };
    if let (Some(input), Some(output)) = (input, output) {
        let calculated = input
            .checked_add(output)
            .ok_or_else(|| "provider_usage_total_overflow".to_owned())?;
        if calculated != total {
            return Err("provider_usage_total_inconsistent".to_owned());
        }
    } else if input.is_some_and(|input| input > total)
        || output.is_some_and(|output| output > total)
    {
        return Err("provider_usage_total_inconsistent".to_owned());
    }
    Ok(())
}

/// Normalize one server-owned provider response into the neutral billing usage vector.
///
/// The response may report a served model different from the requested route model.  The
/// requested model is always copied from `prepared.route.model_id`; the response cannot replace
/// it.  Provider totals are checked when all core components are present, while missing
/// components remain `Partial`/`Unknown` for later reconciliation.
#[allow(clippy::too_many_arguments)]
pub fn normalize_provider_usage(
    prepared: &PreparedModelCall,
    response: &Value,
    usage_id: UsageId,
    attempt_id: AttemptId,
    run_id: RunId,
    retry_ordinal: u32,
) -> Result<NormalizedUsage, String> {
    prepared.validate().map_err(|error| error.to_string())?;
    if prepared
        .spec
        .assignment
        .as_ref()
        .is_some_and(|assignment| assignment.run_id != run_id)
    {
        return Err("provider_usage_run_mismatch".to_owned());
    }
    if retry_ordinal > 1_000_000 {
        return Err("provider_usage_retry_ordinal_invalid".to_owned());
    }
    let raw_digest = json_digest(response);
    let response = response.as_object().ok_or_else(|| invalid("response"))?;
    let served_model_id = served_model(response)?;
    let (
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_output_tokens,
        audio_input_tokens,
        total_tokens,
    ) = protocol_fields(response, prepared.route.protocol)?;
    check_total(input_tokens, output_tokens, total_tokens)?;
    let has_core = input_tokens.is_some() && output_tokens.is_some();
    let has_any = input_tokens.is_some()
        || output_tokens.is_some()
        || cache_read_tokens.is_some()
        || cache_write_tokens.is_some()
        || reasoning_output_tokens.is_some()
        || audio_input_tokens.is_some()
        || total_tokens.is_some();
    let (confidence, unknown_reason) = if has_core {
        (UsageConfidence::Known, None)
    } else if has_any {
        (
            UsageConfidence::Partial,
            Some(BillingUnknownReason::Partial),
        )
    } else {
        (
            UsageConfidence::Unknown,
            Some(BillingUnknownReason::ProviderUnreported),
        )
    };
    let vector = UsageVector {
        schema: kiana_domain::USAGE_VECTOR_SCHEMA.to_owned(),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_output_tokens,
        audio_input_tokens,
        audio_output_tokens: None,
        tool_calls: 0,
        effect_count: 0,
        wall_time_ms: 0,
        output_bytes: 0,
        artifact_bytes: 0,
        storage_bytes: 0,
    };
    let basis = format!(
        "{}:{}",
        PROVIDER_USAGE_ADAPTER_SCHEMA,
        protocol_name(prepared.route.protocol)
    );
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
        basis,
        raw_digest,
    )?;
    normalized.served_model_id = served_model_id;
    normalized.retry_ordinal = retry_ordinal;
    normalized.usage_digest = normalized.digest();
    normalized.validate()?;
    Ok(normalized)
}

fn protocol_name(protocol: ModelProtocol) -> &'static str {
    match protocol {
        ModelProtocol::AnthropicMessages => "anthropic_messages",
        ModelProtocol::OpenAiChat => "openai_chat",
        ModelProtocol::OpenAiResponses => "openai_responses",
        ModelProtocol::OllamaChat => "ollama_chat",
        ModelProtocol::GeminiInteractions => "gemini_interactions",
        ModelProtocol::Legacy => "fake",
    }
}
