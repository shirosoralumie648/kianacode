use crate::{
    config::Connection,
    response::{decode, Accumulator},
};
use futures::StreamExt;
use kiana_domain::*;
use std::time::{Duration, Instant, SystemTime};

pub(crate) async fn send(
    connection: &Connection,
    prepared: PreparedModelCall,
    sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
) -> Result<ModelReply, ModelError> {
    let now = unix_ms()?;
    if now >= prepared.spec.deadline_unix_ms {
        return Err(ModelError::invalid("model_deadline_expired"));
    }
    let remaining =
        Duration::from_millis(prepared.spec.deadline_unix_ms - now).min(connection.limits.total);
    tokio::time::timeout(remaining, send_inner(connection, prepared, sink))
        .await
        .map_err(|_| {
            ModelError::transport("model_attempt_deadline", ModelRetryClass::Never, true)
        })?
}
async fn send_inner(
    connection: &Connection,
    prepared: PreparedModelCall,
    sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
) -> Result<ModelReply, ModelError> {
    let _capacity = connection
        .capacity
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| {
            ModelError::transport("provider_capacity_closed", ModelRetryClass::Never, false)
        })?;
    let lease_now = unix_ms()?;
    if lease_now >= prepared.spec.deadline_unix_ms {
        return Err(ModelError::invalid("model_deadline_expired"));
    }
    let endpoint_digest = json_digest(&serde_json::json!(connection.endpoint.as_str()));
    let mut material = connection
        .credential_ref
        .as_ref()
        .map(|secret_ref| {
            connection.credential_store.issue(
                secret_ref,
                &connection.provider_account,
                &connection.route.provider_id,
                &endpoint_digest,
                lease_now,
            )
        })
        .transpose()?;
    if let (Some(secret_ref), Some(material)) =
        (connection.credential_ref.as_ref(), material.as_mut())
    {
        material
            .lease
            .validate_for(
                lease_now,
                &connection.provider_account,
                "provider.request",
                &connection.route.provider_id,
                &endpoint_digest,
            )
            .map_err(ModelError::invalid)?;
        if &material.lease.secret_ref != secret_ref {
            return Err(ModelError::invalid("credential_lease_reference_mismatch"));
        }
        if material.credential_revision != connection.credential_revision {
            return Err(ModelError::invalid("model_credential_revision_changed"));
        }
        material
            .lease
            .consume(lease_now)
            .map_err(ModelError::invalid)?;
    }
    let mut request = connection
        .client
        .post(connection.endpoint.clone())
        .header("x-kiana-provider-account", &connection.provider_account)
        .json(&prepared.wire_body);
    if let Some(material) = material.as_ref() {
        let key = &material.value;
        request = match connection.route.protocol {
            ModelProtocol::AnthropicMessages => request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
            ModelProtocol::GeminiInteractions => request.header("x-goog-api-key", key),
            _ => request.bearer_auth(key),
        };
    }
    let response = tokio::time::timeout(connection.limits.headers, request.send())
        .await
        .map_err(|_| {
            ModelError::transport("provider_headers_timeout", ModelRetryClass::Never, true)
        })?
        .map_err(|err| {
            let retry = classify_connect_failure(err.is_connect(), io_error_kind(&err));
            ModelError::transport(
                "provider_connection_failed",
                retry,
                retry != ModelRetryClass::BeforeSend,
            )
        })?;
    let status = response.status();
    if !status.is_success() {
        let mut error = ModelError::transport(
            &format!("provider_http_{}", status.as_u16()),
            if matches!(status.as_u16(), 429 | 503) {
                ModelRetryClass::Rejected
            } else {
                ModelRetryClass::Never
            },
            true,
        );
        error.side_effect_state = ModelSideEffectState::None;
        error.retry_after_ms = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| parse_retry_after(value, SystemTime::now()));
        // Error bodies and authentication headers are never copied into logs or public errors.
        return Err(error);
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    let provider_request_id = ["request-id", "x-request-id", "x-goog-request-id"]
        .iter()
        .find_map(|key| {
            response
                .headers()
                .get(*key)
                .and_then(|v| v.to_str().ok())
                .filter(|v| v.len() < 256)
                .map(str::to_owned)
        });
    if prepared.route.streaming {
        let ndjson = prepared.route.protocol == ModelProtocol::OllamaChat;
        if !(if ndjson {
            content_type.starts_with("application/x-ndjson")
                || content_type.starts_with("application/json")
        } else {
            content_type.starts_with("text/event-stream")
        }) {
            return Err(ModelError::invalid("provider_content_type_invalid"));
        }
        let mut chunks = response.bytes_stream();
        let mut framer = Framer::new(ndjson, connection.limits.max_frame);
        let mut accumulator = Accumulator::new(prepared.route.protocol);
        let mut total = 0usize;
        let mut semantic = false;
        let started = Instant::now();
        loop {
            let idle = if semantic {
                connection.limits.idle
            } else {
                connection
                    .limits
                    .first_event
                    .saturating_sub(started.elapsed())
            };
            if idle.is_zero() {
                return Err(ModelError::transport(
                    "provider_first_semantic_timeout",
                    ModelRetryClass::Never,
                    true,
                ));
            }
            let next = tokio::time::timeout(idle, chunks.next())
                .await
                .map_err(|_| {
                    ModelError::transport(
                        "provider_read_idle_timeout",
                        ModelRetryClass::Never,
                        true,
                    )
                })?;
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(|_| {
                ModelError::transport("provider_stream_read_failed", ModelRetryClass::Never, true)
            })?;
            total = total
                .checked_add(chunk.len())
                .ok_or_else(|| ModelError::invalid("provider_body_limit"))?;
            if total > connection.limits.max_body {
                return Err(ModelError::invalid("provider_body_limit"));
            }
            for data in framer.push(&chunk)? {
                if !is_heartbeat(&data) {
                    semantic = true;
                }
                if accumulator.push(&data, sink)? {
                    let mut reply = accumulator.finish(&prepared)?;
                    reply.provider_request_id = provider_request_id;
                    return Ok(reply);
                }
            }
        }
        for data in framer.finish()? {
            if accumulator.push(&data, sink)? {
                let mut reply = accumulator.finish(&prepared)?;
                reply.provider_request_id = provider_request_id;
                return Ok(reply);
            }
        }
        Err(ModelError::transport(
            "provider_stream_incomplete",
            ModelRetryClass::Never,
            true,
        ))
    } else {
        if !content_type.starts_with("application/json") {
            return Err(ModelError::invalid("provider_content_type_invalid"));
        }
        let mut chunks = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = tokio::time::timeout(connection.limits.idle, chunks.next())
            .await
            .map_err(|_| {
                ModelError::transport("provider_read_idle_timeout", ModelRetryClass::Never, true)
            })?
        {
            let chunk = chunk.map_err(|_| {
                ModelError::transport(
                    "provider_response_read_failed",
                    ModelRetryClass::Never,
                    true,
                )
            })?;
            if bytes.len().saturating_add(chunk.len()) > connection.limits.max_body {
                return Err(ModelError::invalid("provider_body_limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| ModelError::invalid("provider_response_json_invalid"))?;
        let mut reply = decode(value, &prepared)?;
        reply.provider_request_id = provider_request_id;
        if !reply.output.text.is_empty() {
            sink(ModelDelta::Text {
                text: reply.output.text.clone(),
            })
            .map_err(ModelError::invalid)?;
        }
        Ok(reply)
    }
}
fn parse_retry_after(value: &str, now: SystemTime) -> Option<u64> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1000));
    }
    let when = httpdate::parse_http_date(value).ok()?;
    let duration = when.duration_since(now).ok()?;
    Some(duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

fn io_error_kind(error: &reqwest::Error) -> Option<std::io::ErrorKind> {
    let mut cause = Some(error as &(dyn std::error::Error + 'static));
    while let Some(error) = cause {
        if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
            return Some(io_error.kind());
        }
        cause = error.source();
    }
    None
}

fn classify_connect_failure(is_connect: bool, kind: Option<std::io::ErrorKind>) -> ModelRetryClass {
    if is_connect
        && matches!(
            kind,
            Some(
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::AddrNotAvailable
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::NetworkUnreachable
                    | std::io::ErrorKind::HostUnreachable
            )
        )
    {
        ModelRetryClass::BeforeSend
    } else {
        ModelRetryClass::Never
    }
}

fn is_heartbeat(data: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(data)
        .ok()
        .is_some_and(|v| v["type"] == "ping")
}
pub(crate) fn unix_ms() -> Result<u64, ModelError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|v| u64::try_from(v.as_millis()).ok())
        .ok_or_else(|| ModelError::invalid("model_clock_untrusted"))
}

struct Framer {
    buffer: Vec<u8>,
    data: String,
    ndjson: bool,
    limit: usize,
}
impl Framer {
    fn new(ndjson: bool, limit: usize) -> Self {
        Self {
            buffer: Vec::new(),
            data: String::new(),
            ndjson,
            limit,
        }
    }
    fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, ModelError> {
        let mut frames = Vec::new();
        // Limit each line while receiving it, without copying an oversized chunk into our buffer.
        for byte in chunk {
            if self.buffer.len() >= self.limit {
                return Err(ModelError::invalid("provider_frame_limit"));
            }
            self.buffer.push(*byte);
            if *byte == b'\n' {
                let bytes = std::mem::take(&mut self.buffer);
                let line = std::str::from_utf8(&bytes)
                    .map_err(|_| ModelError::invalid("provider_frame_utf8_invalid"))?
                    .trim_end_matches(['\r', '\n']);
                if self.ndjson {
                    if !line.trim().is_empty() {
                        frames.push(line.to_owned());
                    }
                } else if line.is_empty() {
                    if !self.data.is_empty() {
                        frames.push(std::mem::take(&mut self.data));
                    }
                } else if let Some(value) = line.strip_prefix("data:") {
                    let value = value.strip_prefix(' ').unwrap_or(value);
                    if self
                        .data
                        .len()
                        .saturating_add(value.len())
                        .saturating_add(1)
                        > self.limit
                    {
                        return Err(ModelError::invalid("provider_frame_limit"));
                    }
                    if !self.data.is_empty() {
                        self.data.push('\n');
                    }
                    self.data.push_str(value);
                } else if line.starts_with(':')
                    || line.starts_with("event:")
                    || line.starts_with("id:")
                    || line.starts_with("retry:")
                {
                } else {
                    return Err(ModelError::invalid("provider_sse_field_invalid"));
                }
            }
        }
        Ok(frames)
    }
    fn finish(&mut self) -> Result<Vec<String>, ModelError> {
        if self.ndjson && !self.buffer.is_empty() {
            let bytes = std::mem::take(&mut self.buffer);
            let value = String::from_utf8(bytes)
                .map_err(|_| ModelError::invalid("provider_frame_utf8_invalid"))?;
            return Ok(vec![value]);
        }
        if !self.buffer.is_empty() || !self.data.is_empty() {
            return Err(ModelError::invalid("provider_frame_truncated"));
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn retry_after_http_date_uses_injected_clock() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let date = httpdate::fmt_http_date(now + Duration::from_secs(7));
        assert_eq!(parse_retry_after(&date, now), Some(7_000));
        assert_eq!(parse_retry_after(&date, now + Duration::from_secs(8)), None);
        assert_eq!(
            parse_retry_after("18446744073709551615", now),
            Some(u64::MAX)
        );
    }

    #[test]
    fn retry_after_large_delta_is_preserved_for_absolute_deadline_check() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(parse_retry_after("3600", now), Some(3_600_000));
    }

    #[test]
    fn only_typed_network_connect_failures_are_retryable() {
        assert_eq!(
            classify_connect_failure(true, Some(std::io::ErrorKind::ConnectionRefused)),
            ModelRetryClass::BeforeSend
        );
        assert_eq!(
            classify_connect_failure(true, Some(std::io::ErrorKind::TimedOut)),
            ModelRetryClass::BeforeSend
        );
        assert_eq!(
            classify_connect_failure(true, Some(std::io::ErrorKind::InvalidData)),
            ModelRetryClass::Never
        );
        assert_eq!(
            classify_connect_failure(false, Some(std::io::ErrorKind::ConnectionRefused)),
            ModelRetryClass::Never
        );
    }

    #[test]
    fn tls_failure_is_not_transient() {
        assert_eq!(classify_connect_failure(true, None), ModelRetryClass::Never);
        assert_eq!(
            classify_connect_failure(true, Some(std::io::ErrorKind::InvalidData)),
            ModelRetryClass::Never
        );
    }

    #[test]
    fn sse_survives_arbitrary_utf8_chunking_and_crlf() {
        let payload = "data: {\"text\":\"你好\"}\r\n\r\n";
        let mut framer = Framer::new(false, 128);
        let mut frames = Vec::new();
        for chunk in payload.as_bytes().chunks(1) {
            frames.extend(framer.push(chunk).expect("sse frame"));
        }
        frames.extend(framer.finish().expect("sse complete"));
        assert_eq!(frames, vec![r#"{"text":"你好"}"#.to_owned()]);
    }

    #[test]
    fn sse_multiline_data_and_comments_are_bounded() {
        let mut framer = Framer::new(false, 128);
        let mut frames = Vec::new();
        frames.extend(
            framer
                .push(b": heartbeat\r\ndata: first\r\ndata: second\r\n\r\n")
                .expect("sse frame"),
        );
        assert_eq!(frames, vec!["first\nsecond".to_owned()]);
        assert!(framer.finish().expect("sse complete").is_empty());
    }

    #[test]
    fn ndjson_flushes_complete_lines_and_one_bounded_tail() {
        let mut framer = Framer::new(true, 64);
        let mut frames = Vec::new();
        for chunk in b"{\"a\":1}\n{\"b\":2}".chunks(2) {
            frames.extend(framer.push(chunk).expect("ndjson frame"));
        }
        frames.extend(framer.finish().expect("ndjson tail"));
        assert_eq!(
            frames,
            vec![r#"{"a":1}"#.to_owned(), r#"{"b":2}"#.to_owned()]
        );
    }

    #[test]
    fn oversized_and_truncated_frames_fail_closed() {
        let mut oversized = Framer::new(true, 4);
        assert_eq!(
            oversized.push(b"12345").unwrap_err().code,
            "provider_frame_limit"
        );

        let mut invalid_utf8 = Framer::new(true, 16);
        assert_eq!(
            invalid_utf8.push(&[0xff, b'\n']).unwrap_err().code,
            "provider_frame_utf8_invalid"
        );

        let mut truncated = Framer::new(false, 64);
        truncated.push(b"data: partial\n").expect("partial sse");
        assert_eq!(
            truncated.finish().unwrap_err().code,
            "provider_frame_truncated"
        );
    }
}
