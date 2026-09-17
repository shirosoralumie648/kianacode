//! Safe provider-side metadata for model-attempt instrumentation.
//!
//! The provider owns request compilation and therefore is the last boundary that can prove a
//! route was selected without accidentally serializing the wire request.  This helper returns a
//! small allow-listed summary.  Prompt text, tool arguments, authentication headers, endpoint
//! URLs and response bodies are intentionally not representable in the returned value.

use kiana_domain::{ModelError, PreparedModelCall};
use serde_json::{json, Value};

pub const MODEL_ATTEMPT_TELEMETRY_SCHEMA: &str = "kiana.model-attempt.v1";

/// Produce the provider-side safe request summary used by the existing model-turn event path.
///
/// Calling this function validates the prepared request before exposing any metadata.  The
/// returned JSON is suitable for diagnostics, but it is not an authorization token or a provider
/// receipt; the ControlPlane still derives the durable projection from committed events.
pub fn safe_prepared_metadata(prepared: &PreparedModelCall) -> Result<Value, ModelError> {
    prepared.validate()?;
    let route = &prepared.route;
    Ok(json!({
        "schema": MODEL_ATTEMPT_TELEMETRY_SCHEMA,
        "model_call_id": prepared.spec.call_id,
        "model_request_id": prepared.spec.attempt_id,
        "run_id": prepared.spec.assignment.as_ref().map(|assignment| assignment.run_id),
        "purpose": prepared.spec.purpose,
        "provider_id": route.provider_id,
        "model_id": route.model_id,
        "streaming": route.streaming,
        "route_digest": route.digest(),
        "prompt_version": prepared.request_hash,
        "tool_catalog_hash": prepared.tool_catalog_hash,
        "budget": prepared.budget,
        "provider_account": prepared.provider_account,
        "credential_revision": prepared.credential_revision,
    }))
}
