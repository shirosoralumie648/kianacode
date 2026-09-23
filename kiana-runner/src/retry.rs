use kiana_domain::{ModelError, ModelRetryClass, ModelSideEffectState, RequestId};
use std::time::Duration;

pub(crate) const MAX_PROVIDER_ATTEMPTS: u32 = 3;

pub(crate) fn is_safe_to_retry(error: &ModelError, observed_delta: bool) -> bool {
    !observed_delta
        && error.phase == "transport"
        && match error.retry_class {
            ModelRetryClass::BeforeSend => {
                !error.request_sent && error.side_effect_state == ModelSideEffectState::None
            }
            ModelRetryClass::Rejected => {
                error.request_sent
                    && error.side_effect_state == ModelSideEffectState::None
                    && matches!(
                        error.code.as_str(),
                        "provider_http_429" | "provider_http_503"
                    )
            }
            ModelRetryClass::Never => false,
        }
}

pub(crate) fn retry_delay(error: &ModelError, retry_index: u32, attempt_id: RequestId) -> Duration {
    let exponent = retry_index.min(5);
    let backoff_ms = 100u64.saturating_mul(1u64 << exponent).min(5_000);
    let server_delay_ms = error.retry_after_ms.unwrap_or_default();
    let base_ms = backoff_ms.max(server_delay_ms);
    let bytes = attempt_id.as_uuid().as_bytes().to_owned();
    let jitter_ms = (u16::from(bytes[0]) << 8 | u16::from(bytes[1])) % 101;
    Duration::from_millis(base_ms.saturating_add(u64::from(jitter_ms)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_requires_typed_pre_send_or_explicit_rejection_without_unknown_effect() {
        let before_send =
            ModelError::transport("connect_failed", ModelRetryClass::BeforeSend, false);
        assert!(is_safe_to_retry(&before_send, false));

        let mut rejected =
            ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
        rejected.side_effect_state = ModelSideEffectState::None;
        assert!(is_safe_to_retry(&rejected, false));
        rejected.code = "provider_http_500".to_owned();
        assert!(!is_safe_to_retry(&rejected, false));
        rejected.code = "provider_http_429".to_owned();

        let unknown =
            ModelError::transport("response_lost_after_send", ModelRetryClass::Never, true);
        assert!(!is_safe_to_retry(&unknown, false));
        assert!(!is_safe_to_retry(&rejected, true));

        rejected.side_effect_state = ModelSideEffectState::Unknown;
        assert!(!is_safe_to_retry(&rejected, false));
    }

    #[test]
    fn retry_delay_never_shortens_retry_after_and_is_bounded_jitter() {
        let mut error = ModelError::transport("provider_http_429", ModelRetryClass::Rejected, true);
        error.side_effect_state = ModelSideEffectState::None;
        error.retry_after_ms = Some(8_000);

        let delay = retry_delay(&error, 0, RequestId::new());
        assert!((Duration::from_millis(8_000)..=Duration::from_millis(8_100)).contains(&delay));
    }
}
