//! EQ-19 explicitly declared volatile-value normalization.

use crate::{ArrayPolicy, CanonicalEvent, CanonicalEventTrace, CANONICAL_NORMALIZATION_VERSION};
use kiana_domain::{canonical_journal_bytes, RequestId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const VOLATILE_NORMALIZATION_VERSION: &str = "eq19.volatile-events.v1";
pub const MAX_VOLATILE_RULES: usize = 128;
pub const MAX_VOLATILE_REPLACEMENTS: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VolatileKind {
    Timestamp,
    Uuid,
    TempPath,
    Actor,
}

impl VolatileKind {
    fn token(self) -> &'static str {
        match self {
            Self::Timestamp => "<TS>",
            Self::Uuid => "<UUID>",
            Self::TempPath => "<TEMP_PATH>",
            Self::Actor => "<ACTOR>",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolatileRule {
    /// Dot-separated path rooted at one canonical event; `*` matches one object-array index.
    pub path: String,
    pub kind: VolatileKind,
}

impl VolatileRule {
    pub fn new(path: impl Into<String>, kind: VolatileKind) -> Result<Self, VolatileError> {
        let rule = Self {
            path: path.into(),
            kind,
        };
        rule.validate()?;
        Ok(rule)
    }

    pub fn validate(&self) -> Result<(), VolatileError> {
        if self.path.trim().is_empty()
            || self.path.len() > 512
            || self.path.starts_with('.')
            || self.path.ends_with('.')
            || self.path.split('.').any(|part| part.is_empty())
            || self
                .path
                .split('.')
                .any(|part| part != "*" && part.contains(['/', '\0']))
        {
            return Err(VolatileError::RuleInvalid {
                path: self.path.clone(),
            });
        }
        Ok(())
    }

    fn matches(&self, path: &[String]) -> bool {
        let parts = self.path.split('.').collect::<Vec<_>>();
        parts.len() == path.len()
            && parts
                .iter()
                .zip(path)
                .all(|(expected, actual)| *expected == "*" || *expected == actual)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolatilePolicy {
    pub rules: Vec<VolatileRule>,
    pub max_replacements: usize,
}

impl VolatilePolicy {
    pub fn new(mut rules: Vec<VolatileRule>) -> Result<Self, VolatileError> {
        let policy = Self {
            rules: {
                rules.sort();
                rules
            },
            max_replacements: MAX_VOLATILE_REPLACEMENTS,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn with_max_replacements(
        mut rules: Vec<VolatileRule>,
        max_replacements: usize,
    ) -> Result<Self, VolatileError> {
        rules.sort();
        let policy = Self {
            rules,
            max_replacements,
        };
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), VolatileError> {
        if self.rules.len() > MAX_VOLATILE_RULES
            || self.max_replacements == 0
            || self.max_replacements > MAX_VOLATILE_REPLACEMENTS
        {
            return Err(VolatileError::PolicyInvalid);
        }
        let mut paths = BTreeSet::new();
        for rule in &self.rules {
            rule.validate()?;
            if !paths.insert((rule.path.clone(), rule.kind)) {
                return Err(VolatileError::DuplicateRule {
                    path: rule.path.clone(),
                });
            }
        }
        Ok(())
    }

    fn matching_rule(&self, path: &[String]) -> Option<&VolatileRule> {
        self.rules.iter().find(|rule| rule.matches(path))
    }
}

impl Default for VolatilePolicy {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            max_replacements: MAX_VOLATILE_REPLACEMENTS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolatileReplacement {
    pub path: String,
    pub kind: VolatileKind,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolatileEvent {
    pub source_cursor: u64,
    pub kind: String,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VolatileEventTrace {
    pub normalization_version: String,
    pub source_normalization_version: String,
    pub array_policy: ArrayPolicy,
    pub source_cursor_start: u64,
    pub source_cursor_end: u64,
    pub correlation_id: RequestId,
    pub events: Vec<VolatileEvent>,
    pub terminal_event_indexes: Vec<usize>,
    pub replacement_count: usize,
    pub replacements: Vec<VolatileReplacement>,
}

impl VolatileEventTrace {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, VolatileError> {
        let normalized_values = self
            .events
            .iter()
            .map(|event| &event.value)
            .collect::<Vec<_>>();
        canonical_journal_bytes(&normalized_values).map_err(VolatileError::CanonicalEncodingFailed)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum VolatileError {
    #[error("volatile_rule_invalid:{path}")]
    RuleInvalid { path: String },
    #[error("volatile_policy_invalid")]
    PolicyInvalid,
    #[error("volatile_rule_duplicate:{path}")]
    DuplicateRule { path: String },
    #[error("volatile_source_version_invalid")]
    SourceVersionInvalid,
    #[error("volatile_value_type_mismatch:{path}:{kind:?}")]
    ValueTypeMismatch { path: String, kind: VolatileKind },
    #[error("volatile_undeclared:{path}:{kind:?}")]
    UndeclaredVolatile { path: String, kind: VolatileKind },
    #[error("volatile_replacement_limit_exceeded")]
    ReplacementLimitExceeded,
    #[error("volatile_canonical_encoding:{0}")]
    CanonicalEncodingFailed(String),
    #[error("volatile_canonical:{0}")]
    Canonicalization(String),
}

fn is_uuid_like(value: &str) -> bool {
    value.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| value.as_bytes().get(index) == Some(&b'-'))
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| [8, 13, 18, 23].contains(&index) || byte.is_ascii_hexdigit())
}

fn is_temp_path(value: &str) -> bool {
    value.starts_with("/tmp/")
        || value.starts_with("/private/tmp/")
        || value.contains("/tmp/")
        || value.contains("\\Temp\\")
        || value.starts_with("$TMPDIR/")
}

fn is_timestamp_field(field: &str, value: &Value) -> bool {
    let field = field.to_ascii_lowercase();
    let named = field.ends_with("_at_unix_ms")
        || field.contains("timestamp")
        || field.ends_with("_created_at")
        || field.ends_with("_updated_at")
        || field.ends_with("_expires_at")
        || field.ends_with("_deadline");
    named && matches!(value, Value::Number(_) | Value::String(_))
}

fn is_actor_field(field: &str, value: &Value) -> bool {
    let field = field.to_ascii_lowercase();
    let named = matches!(
        field.as_str(),
        "actor" | "actor_id" | "owner_id" | "principal_id" | "approver_id"
    ) || field.ends_with("_actor_id");
    named && matches!(value, Value::String(_))
}

fn inferred_kind(field: &str, value: &Value) -> Option<VolatileKind> {
    match value {
        Value::String(text) if is_uuid_like(text) => Some(VolatileKind::Uuid),
        Value::String(text) if is_temp_path(text) => Some(VolatileKind::TempPath),
        _ if is_timestamp_field(field, value) => Some(VolatileKind::Timestamp),
        _ if is_actor_field(field, value) => Some(VolatileKind::Actor),
        _ => None,
    }
}

fn apply_value(
    value: Value,
    path: &mut Vec<String>,
    policy: &VolatilePolicy,
    replacements: &mut Vec<VolatileReplacement>,
    event_index: usize,
) -> Result<Value, VolatileError> {
    let display_path = || {
        let suffix = path.join(".");
        if suffix.is_empty() {
            format!("events[{event_index}]")
        } else {
            format!("events[{event_index}].{suffix}")
        }
    };

    if let Some(rule) = policy.matching_rule(path) {
        let valid_type = match rule.kind {
            VolatileKind::Timestamp => matches!(value, Value::Number(_) | Value::String(_)),
            VolatileKind::Uuid | VolatileKind::TempPath | VolatileKind::Actor => {
                matches!(value, Value::String(_))
            }
        };
        if !valid_type {
            return Err(VolatileError::ValueTypeMismatch {
                path: display_path(),
                kind: rule.kind,
            });
        }
        if replacements.len() >= policy.max_replacements {
            return Err(VolatileError::ReplacementLimitExceeded);
        }
        replacements.push(VolatileReplacement {
            path: display_path(),
            kind: rule.kind,
        });
        return Ok(Value::String(rule.kind.token().to_owned()));
    }

    if let Value::Object(fields) = &value {
        let mut normalized = serde_json::Map::new();
        for (field, child) in fields {
            path.push(field.clone());
            normalized.insert(
                field.clone(),
                apply_value(child.clone(), path, policy, replacements, event_index)?,
            );
            path.pop();
        }
        return Ok(Value::Object(normalized));
    }

    if let Value::Array(values) = value {
        let mut normalized = Vec::with_capacity(values.len());
        for (index, child) in values.into_iter().enumerate() {
            path.push(index.to_string());
            normalized.push(apply_value(child, path, policy, replacements, event_index)?);
            path.pop();
        }
        return Ok(Value::Array(normalized));
    }

    if let Some(kind) = inferred_kind(path.last().map(String::as_str).unwrap_or_default(), &value) {
        return Err(VolatileError::UndeclaredVolatile {
            path: display_path(),
            kind,
        });
    }
    Ok(value)
}

fn normalize_event(
    event: &CanonicalEvent,
    event_index: usize,
    policy: &VolatilePolicy,
    replacements: &mut Vec<VolatileReplacement>,
) -> Result<VolatileEvent, VolatileError> {
    let mut path = Vec::new();
    let value = apply_value(
        event.value.clone(),
        &mut path,
        policy,
        replacements,
        event_index,
    )?;
    Ok(VolatileEvent {
        source_cursor: event.source_cursor,
        kind: event.kind.clone(),
        value,
    })
}

pub fn normalize_volatile_trace(
    trace: &CanonicalEventTrace,
    policy: &VolatilePolicy,
) -> Result<VolatileEventTrace, VolatileError> {
    policy.validate()?;
    if trace.normalization_version != CANONICAL_NORMALIZATION_VERSION {
        return Err(VolatileError::SourceVersionInvalid);
    }
    let mut replacements = Vec::new();
    let events = trace
        .events
        .iter()
        .enumerate()
        .map(|(index, event)| normalize_event(event, index, policy, &mut replacements))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VolatileEventTrace {
        normalization_version: VOLATILE_NORMALIZATION_VERSION.to_owned(),
        source_normalization_version: trace.normalization_version.clone(),
        array_policy: trace.array_policy,
        source_cursor_start: trace.source_cursor_start,
        source_cursor_end: trace.source_cursor_end,
        correlation_id: trace.correlation_id,
        events,
        terminal_event_indexes: trace.terminal_event_indexes.clone(),
        replacement_count: replacements.len(),
        replacements,
    })
}

pub fn normalize_volatile(
    trace: &CanonicalEventTrace,
    policy: &VolatilePolicy,
) -> Result<VolatileEventTrace, VolatileError> {
    normalize_volatile_trace(trace, policy)
}

pub fn standard_uuid_rule_paths() -> Vec<VolatileRule> {
    [
        "event_id",
        "request_id",
        "correlation_id",
        "causation_event_id",
        "parent_event_id",
    ]
    .into_iter()
    .map(|path| VolatileRule {
        path: path.to_owned(),
        kind: VolatileKind::Uuid,
    })
    .collect()
}
