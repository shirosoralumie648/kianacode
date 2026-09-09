use serde_json::Value;

const REDACTED: &str = "[REDACTED]";

/// Streaming redaction retains at most this many bytes of already-emitted text as overlap so a
/// sensitive marker split across chunks can still be recognized. The value is deliberately larger
/// than the longest marker below.
pub const STREAM_REDACTION_BUFFER_LIMIT: usize = 64;

#[derive(Clone, Copy, Debug)]
struct SensitiveMarker {
    literal: &'static str,
    quoted_value: bool,
}

const SENSITIVE_MARKERS: &[SensitiveMarker] = &[
    SensitiveMarker {
        literal: "token=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "password=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "api_key=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "access_key=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "private_key=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "secret=",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "bearer ",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "authorization: bearer ",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "authorization: basic ",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "x-api-key:",
        quoted_value: false,
    },
    SensitiveMarker {
        literal: "\"token\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"password\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"api_key\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"access_key\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"private_key\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"secret\":\"",
        quoted_value: true,
    },
    SensitiveMarker {
        literal: "\"authorization\":\"bearer ",
        quoted_value: true,
    },
];

/// Redact secrets in one complete text value.
pub fn redact_text(text: &str) -> String {
    let trimmed = text.trim();
    if matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'[')) {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return redact_value(&value).to_string();
        }
    }

    let mut redacted = text.to_owned();
    for marker in SENSITIVE_MARKERS {
        let marker_lower = marker.literal.to_ascii_lowercase();
        let mut search_from = 0;
        while search_from < redacted.len() {
            let lower = redacted.to_ascii_lowercase();
            let Some(relative_start) = lower[search_from..].find(&marker_lower) else {
                break;
            };
            let start = search_from + relative_start + marker.literal.len();
            let value_start = if marker.quoted_value {
                start
            } else {
                start
                    + redacted[start..]
                        .chars()
                        .take_while(|character| character.is_whitespace())
                        .map(char::len_utf8)
                        .sum::<usize>()
            };
            let end = if marker.quoted_value {
                redacted[value_start..]
                    .find('"')
                    .map_or(redacted.len(), |relative_end| value_start + relative_end)
            } else {
                redacted[value_start..]
                    .find(|character: char| {
                        character.is_whitespace()
                            || matches!(character, '&' | ',' | ';' | '"' | '}')
                    })
                    .map_or(redacted.len(), |relative_end| value_start + relative_end)
            };
            if end <= value_start {
                break;
            }
            redacted.replace_range(value_start..end, REDACTED);
            search_from = value_start + REDACTED.len();
        }
    }
    redacted
}

/// Redact sensitive object keys and text values recursively.
pub fn redact_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    let normalized = key.to_ascii_lowercase();
                    let token_metric =
                        matches!(
                            normalized.as_str(),
                            "tokens_before"
                                | "tokens_after"
                                | "input_tokens"
                                | "output_tokens"
                                | "total_tokens"
                                | "cached_tokens"
                                | "reasoning_tokens"
                                | "max_tokens"
                                | "min_tokens"
                                | "token_count"
                                | "token_budget"
                                | "estimated_tokens"
                                | "token_overlap"
                        ) && matches!(value, Value::Number(_) | Value::Bool(_) | Value::Null);
                    let sensitive = normalized != "secret_ref"
                        && ((normalized.contains("token") && !token_metric)
                            || normalized.contains("password")
                            || normalized.contains("api_key")
                            || normalized.contains("access_key")
                            || normalized.contains("private_key")
                            || normalized.contains("secret"));
                    let value = if sensitive {
                        Value::String(REDACTED.to_owned())
                    } else {
                        redact_value(value)
                    };
                    (key.clone(), value)
                })
                .collect(),
        ),
        Value::String(text) => Value::String(redact_text(text)),
        _ => value.clone(),
    }
}

#[derive(Clone, Copy, Debug)]
enum Suppression {
    Quoted,
    UnquotedAwaitingValue,
    UnquotedValue,
}

impl Suppression {
    fn is_delimiter(self, character: char) -> bool {
        match self {
            Self::Quoted => character == '"',
            Self::UnquotedValue => {
                character.is_whitespace() || matches!(character, '&' | ',' | ';' | '"' | '}')
            }
            Self::UnquotedAwaitingValue => false,
        }
    }
}

/// Stateful redaction for a text stream.
///
/// The redactor emits ordinary text immediately and retains only a bounded overlap of already
/// emitted text so a sensitive marker split across chunks can still be recognized. Once a marker
/// is recognized, the following value is suppressed until its delimiter; if the stream ends first,
/// [`Self::finish`] emits the replacement marker instead of the buffered secret.
#[derive(Debug)]
pub struct StreamingRedactor {
    overlap: String,
    suppression: Option<Suppression>,
    suppressed_any: bool,
    finished: bool,
}

impl Default for StreamingRedactor {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamingRedactor {
    /// Construct an empty streaming redactor.
    pub fn new() -> Self {
        debug_assert!(SENSITIVE_MARKERS
            .iter()
            .all(|marker| marker.literal.len() <= STREAM_REDACTION_BUFFER_LIMIT));
        Self {
            overlap: String::new(),
            suppression: None,
            suppressed_any: false,
            finished: false,
        }
    }

    /// Redact a stream chunk and return the text that is safe to expose now.
    pub fn push(&mut self, text: &str) -> String {
        if self.finished {
            return redact_text(text);
        }
        let mut output = String::new();
        let mut remaining = text;
        while !remaining.is_empty() {
            if let Some(suppression) = self.suppression {
                if matches!(suppression, Suppression::UnquotedAwaitingValue) {
                    let whitespace_len = remaining
                        .chars()
                        .take_while(|character| character.is_whitespace())
                        .map(char::len_utf8)
                        .sum::<usize>();
                    output.push_str(&remaining[..whitespace_len]);
                    remaining = &remaining[whitespace_len..];
                    if remaining.is_empty() {
                        break;
                    }
                    self.suppression = Some(Suppression::UnquotedValue);
                    continue;
                }
                if let Some(index) = remaining.char_indices().find_map(|(index, character)| {
                    suppression.is_delimiter(character).then_some(index)
                }) {
                    if self.suppressed_any || index > 0 {
                        output.push_str(REDACTED);
                    }
                    self.suppression = None;
                    self.suppressed_any = false;
                    remaining = &remaining[index..];
                    continue;
                }
                self.suppressed_any = true;
                break;
            }

            let combined = format!("{}{}", self.overlap, remaining);
            let overlap_len = self.overlap.len();
            let Some((start, marker)) = earliest_marker(&combined) else {
                output.push_str(remaining);
                self.retain_overlap(&combined);
                break;
            };

            let marker_end = start + marker.literal.len();
            let emit_len = marker_end.saturating_sub(overlap_len).min(remaining.len());
            output.push_str(&remaining[..emit_len]);
            remaining = &remaining[emit_len..];
            self.overlap.clear();
            self.suppression = Some(if marker.quoted_value {
                Suppression::Quoted
            } else {
                Suppression::UnquotedAwaitingValue
            });
            self.suppressed_any = false;
        }
        output
    }

    /// Flush the stream tail. A pending sensitive value is replaced with `[REDACTED]`.
    pub fn finish(&mut self) -> String {
        let mut output = String::new();
        if self.finished {
            return output;
        }
        if self.suppression.is_some() {
            if self.suppressed_any {
                output.push_str(REDACTED);
            }
            self.suppression = None;
            self.suppressed_any = false;
        }
        self.overlap.clear();
        self.finished = true;
        output
    }

    /// Current number of retained overlap bytes. This is bounded by
    /// [`STREAM_REDACTION_BUFFER_LIMIT`] for ordinary stream processing.
    pub fn buffered_len(&self) -> usize {
        self.overlap.len()
    }

    fn retain_overlap(&mut self, emitted: &str) {
        let mut start = emitted.len().saturating_sub(STREAM_REDACTION_BUFFER_LIMIT);
        while start < emitted.len() && !emitted.is_char_boundary(start) {
            start += 1;
        }
        self.overlap.clear();
        self.overlap.push_str(&emitted[start..]);
        debug_assert!(self.overlap.len() <= STREAM_REDACTION_BUFFER_LIMIT);
    }
}

fn earliest_marker(text: &str) -> Option<(usize, SensitiveMarker)> {
    let lower = text.to_ascii_lowercase();
    SENSITIVE_MARKERS
        .iter()
        .filter_map(|marker| lower.find(marker.literal).map(|start| (start, *marker)))
        .min_by(|(left_start, left), (right_start, right)| {
            left_start
                .cmp(right_start)
                .then_with(|| right.literal.len().cmp(&left.literal.len()))
        })
}

#[cfg(test)]
mod tests {
    use super::{redact_text, StreamingRedactor, STREAM_REDACTION_BUFFER_LIMIT};

    #[test]
    fn streaming_redactor_masks_marker_and_value_split_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("Authorization: Bear"));
        output.push_str(&redactor.push("er stream-split-sentinel"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "Authorization: Bearer [REDACTED]");
        assert!(!output.contains("stream-split-sentinel"), "{output}");
    }

    #[test]
    fn streaming_redactor_masks_value_split_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("token=first"));
        output.push_str(&redactor.push("second, visible"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "token=[REDACTED], visible");
    }

    #[test]
    fn streaming_redactor_skips_marker_whitespace_across_chunks() {
        let mut redactor = StreamingRedactor::new();
        let mut output = String::new();
        output.push_str(&redactor.push("X-Api-Key:"));
        output.push_str(&redactor.push("  stream-split-sentinel"));
        output.push_str(&redactor.finish());

        assert_eq!(output, "X-Api-Key:  [REDACTED]");
        assert!(!output.contains("stream-split-sentinel"), "{output}");
    }

    #[test]
    fn streaming_redactor_emits_ordinary_text_immediately_and_stays_bounded() {
        let mut redactor = StreamingRedactor::new();
        let output = redactor.push("ordinary text Authorization: Bear");

        assert_eq!(output, "ordinary text Authorization: Bear");
        assert!(redactor.buffered_len() <= STREAM_REDACTION_BUFFER_LIMIT);
        assert!(redactor.finish().is_empty());
    }

    #[test]
    fn complete_redaction_and_streaming_redaction_agree_on_split_secret() {
        let text = "Authorization: Bearer stream-split-sentinel";
        let mut redactor = StreamingRedactor::new();
        let mut streamed = String::new();
        streamed.push_str(&redactor.push("Authorization: Bear"));
        streamed.push_str(&redactor.push("er stream-split-sentinel"));
        streamed.push_str(&redactor.finish());

        assert_eq!(streamed, redact_text(text));
    }
}
