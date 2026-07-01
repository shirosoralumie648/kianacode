use tracing::{info, warn};

pub fn log_api_query(model: &str, messages_length: usize, temperature: f32) {
    info!(
        model = model,
        messages_length = messages_length,
        temperature = temperature,
        "API query started"
    );
}

pub fn log_api_success(model: &str, duration_ms: u64, tokens_used: u32) {
    info!(
        model = model,
        duration_ms = duration_ms,
        tokens_used = tokens_used,
        "API query succeeded"
    );
}

pub fn log_api_error(model: &str, error_type: &str, attempt: u32) {
    warn!(
        model = model,
        error_type = error_type,
        attempt = attempt,
        "API query failed"
    );
}
