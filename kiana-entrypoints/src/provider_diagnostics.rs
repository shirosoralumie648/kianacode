//! Provider authentication diagnostics for user-facing entrypoints.
//!
//! The command registry predates the provider policy boundary and may still expose compatibility
//! fields such as `key_preview`.  Every CLI/TUI/HTTP projection must pass through this module so
//! those fields become presence-only values and token-shaped text is redacted before it leaves an
//! entrypoint.  This is a display boundary, not an authorization decision.

use serde_json::{Map, Value};

const MAX_DIAGNOSTIC_DEPTH: usize = 32;

/// Sanitize a structured auth status projection for CLI, TUI and HTTP consumers.
pub fn sanitize_auth_status(value: &Value) -> Value {
    scrub(value, 0)
}

/// Sanitize the output of the compatibility `auth` command before printing it from the CLI.
/// Non-JSON output is still passed through the domain text redactor rather than returned verbatim.
pub fn sanitize_command_output(command: &str, output: &str) -> String {
    if command != "auth" {
        return output.to_owned();
    }
    match serde_json::from_str::<Value>(output) {
        Ok(value) => serde_json::to_string_pretty(&sanitize_auth_status(&value))
            .unwrap_or_else(|_| kiana_protocol::redact_text(output)),
        Err(_) => kiana_protocol::redact_text(output),
    }
}

fn scrub(value: &Value, depth: usize) -> Value {
    if depth > MAX_DIAGNOSTIC_DEPTH {
        return Value::String("[REDACTED]".to_owned());
    }
    match value {
        Value::Object(object) => {
            let mut sanitized = Map::with_capacity(object.len());
            for (key, value) in object {
                let normalized = key.to_ascii_lowercase();
                if normalized == "key_preview" {
                    sanitized.insert(key.clone(), Value::String("redacted".to_owned()));
                } else if is_sensitive_key(&normalized) {
                    sanitized.insert(key.clone(), presence(value));
                } else {
                    sanitized.insert(key.clone(), scrub(value, depth + 1));
                }
            }
            Value::Object(sanitized)
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| scrub(item, depth + 1)).collect())
        }
        Value::String(text) => Value::String(kiana_protocol::redact_text(text)),
        _ => value.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    if matches!(
        key,
        "api_key"
            | "access_token"
            | "refresh_token"
            | "auth_token"
            | "id_token"
            | "worker_jwt"
            | "client_secret"
            | "private_key"
            | "access_key"
            | "password"
            | "secret"
            | "credential"
            | "credential_value"
            | "authorization"
            | "bearer"
    ) {
        return true;
    }
    // Catch provider-specific names (`github_pat`, `oauth_token`, `foo_secret`) while keeping
    // numeric usage fields such as `token_count` and `token_budget` available to diagnostics.
    (key.contains("token") || key.contains("secret") || key.contains("password"))
        && !matches!(
            key,
            "token_count"
                | "token_budget"
                | "tokens"
                | "tokens_used"
                | "input_tokens"
                | "output_tokens"
                | "total_tokens"
        )
}

fn presence(value: &Value) -> Value {
    let present = match value {
        Value::Null => false,
        Value::String(text) => {
            let normalized = text.trim().to_ascii_lowercase();
            !normalized.is_empty()
                && !matches!(normalized.as_str(), "missing" | "none" | "null" | "unset")
        }
        Value::Bool(value) => *value,
        _ => true,
    };
    Value::String(if present { "configured" } else { "missing" }.to_owned())
}
