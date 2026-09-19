//! EQ-22 deterministic declaration-based assertion DSL.

use kiana_domain::{canonical_journal_bytes, redact_text, redact_value};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const ASSERTION_SCHEMA: &str = "kiana.quality-assertion.v1";
pub const MAX_ASSERTIONS: usize = 256;
pub const MAX_ASSERTION_PATH_BYTES: usize = 512;
pub const MAX_REGEX_BYTES: usize = 2_048;
pub const MAX_ASSERTION_SUMMARY_BYTES: usize = 512;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionMode {
    Exact,
    Ordered,
    Multiset,
    NumericTolerance { absolute: f64, relative: f64 },
    Regex,
    Contains,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assertion {
    pub schema: String,
    /// Dot-separated path rooted at the value supplied to `evaluate_assertion`; an empty path
    /// targets the complete value.
    pub path: String,
    pub mode: AssertionMode,
    pub expected: Value,
}

impl Assertion {
    pub fn new(
        path: impl Into<String>,
        mode: AssertionMode,
        expected: Value,
    ) -> Result<Self, AssertionError> {
        let assertion = Self {
            schema: ASSERTION_SCHEMA.to_owned(),
            path: path.into(),
            mode,
            expected,
        };
        assertion.validate()?;
        Ok(assertion)
    }

    pub fn validate(&self) -> Result<(), AssertionError> {
        if self.schema != ASSERTION_SCHEMA
            || self.path.len() > MAX_ASSERTION_PATH_BYTES
            || self.path.contains(['\0', '\n', '\r'])
            || self
                .path
                .split('.')
                .any(|part| part.is_empty() && !self.path.is_empty())
        {
            return Err(AssertionError::SpecInvalid);
        }
        match &self.mode {
            AssertionMode::Ordered | AssertionMode::Multiset if !self.expected.is_array() => {
                Err(AssertionError::ExpectedArray)
            }
            AssertionMode::NumericTolerance { absolute, relative }
                if !absolute.is_finite()
                    || !relative.is_finite()
                    || *absolute < 0.0
                    || *relative < 0.0
                    || *absolute > 1_000_000_000.0
                    || *relative > 1_000_000_000.0
                    || !self.expected.is_number() =>
            {
                Err(AssertionError::NumericSpecInvalid)
            }
            AssertionMode::Regex => {
                let pattern = self
                    .expected
                    .as_str()
                    .ok_or(AssertionError::ExpectedString)?;
                if pattern.len() > MAX_REGEX_BYTES {
                    return Err(AssertionError::RegexTooLarge);
                }
                Regex::new(pattern).map_err(|_| AssertionError::RegexInvalid)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssertionResult {
    pub path: String,
    pub mode: AssertionMode,
    pub passed: bool,
    pub code: Option<String>,
    pub expected_summary: String,
    pub actual_summary: String,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AssertionError {
    #[error("quality_assertion_spec_invalid")]
    SpecInvalid,
    #[error("quality_assertion_expected_array")]
    ExpectedArray,
    #[error("quality_assertion_expected_string")]
    ExpectedString,
    #[error("quality_assertion_numeric_spec_invalid")]
    NumericSpecInvalid,
    #[error("quality_assertion_regex_invalid")]
    RegexInvalid,
    #[error("quality_assertion_regex_too_large")]
    RegexTooLarge,
    #[error("quality_assertion_limit_exceeded")]
    AssertionLimitExceeded,
}

pub fn evaluate_assertion(
    actual: &Value,
    assertion: &Assertion,
) -> Result<AssertionResult, AssertionError> {
    assertion.validate()?;
    let actual_value = resolve_path(actual, &assertion.path);
    let (passed, code) = match actual_value {
        None => (false, Some("assertion_path_missing".to_owned())),
        Some(value) => evaluate_mode(value, &assertion.mode, &assertion.expected),
    };
    Ok(AssertionResult {
        path: assertion.path.clone(),
        mode: assertion.mode.clone(),
        passed,
        code,
        expected_summary: summarize(&assertion.expected),
        actual_summary: actual_value.map_or_else(|| "<missing>".to_owned(), summarize),
    })
}

pub fn evaluate_assertions(
    actual: &Value,
    assertions: &[Assertion],
) -> Result<Vec<AssertionResult>, AssertionError> {
    if assertions.len() > MAX_ASSERTIONS {
        return Err(AssertionError::AssertionLimitExceeded);
    }
    assertions
        .iter()
        .map(|assertion| evaluate_assertion(actual, assertion))
        .collect()
}

fn resolve_path<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(value);
    }
    path.split('.')
        .try_fold(value, |current, segment| match current {
            Value::Object(fields) => fields.get(segment),
            Value::Array(items) => segment
                .parse::<usize>()
                .ok()
                .and_then(|index| items.get(index)),
            _ => None,
        })
}

fn evaluate_mode(actual: &Value, mode: &AssertionMode, expected: &Value) -> (bool, Option<String>) {
    let (passed, code) = match mode {
        AssertionMode::Exact => (actual == expected, "assertion_exact_mismatch"),
        AssertionMode::Ordered => (
            compare_ordered(actual, expected),
            "assertion_ordered_mismatch",
        ),
        AssertionMode::Multiset => (
            compare_multiset(actual, expected),
            "assertion_multiset_mismatch",
        ),
        AssertionMode::NumericTolerance { absolute, relative } => {
            let passed =
                actual
                    .as_f64()
                    .zip(expected.as_f64())
                    .is_some_and(|(actual, expected)| {
                        let difference = (actual - expected).abs();
                        difference <= *absolute + *relative * actual.abs().max(expected.abs())
                    });
            (passed, "assertion_numeric_tolerance_exceeded")
        }
        AssertionMode::Regex => {
            let passed = actual
                .as_str()
                .zip(expected.as_str())
                .is_some_and(|(actual, pattern)| {
                    Regex::new(pattern).is_ok_and(|regex| regex.is_match(actual))
                });
            (passed, "assertion_regex_mismatch")
        }
        AssertionMode::Contains => (contains(actual, expected), "assertion_contains_mismatch"),
    };
    (passed, (!passed).then_some(code.to_owned()))
}

fn compare_ordered(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            actual.len() == expected.len()
                && expected.iter().all(|(key, value)| {
                    actual
                        .get(key)
                        .is_some_and(|actual| compare_ordered(actual, value))
                })
        }
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual, expected)| compare_ordered(actual, expected))
        }
        _ => actual == expected,
    }
}

fn compare_multiset(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => {
            actual.len() == expected.len()
                && expected.iter().all(|(key, value)| {
                    actual
                        .get(key)
                        .is_some_and(|actual| compare_multiset(actual, value))
                })
        }
        (Value::Array(actual), Value::Array(expected)) => {
            if actual.len() != expected.len() {
                return false;
            }
            let mut used = vec![false; actual.len()];
            expected.iter().all(|expected| {
                actual.iter().enumerate().any(|(index, actual)| {
                    !used[index] && compare_multiset(actual, expected) && {
                        used[index] = true;
                        true
                    }
                })
            })
        }
        _ => actual == expected,
    }
}

fn contains(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::String(actual), Value::String(expected)) => actual.contains(expected),
        (Value::Array(actual), Value::Array(expected)) => expected
            .iter()
            .all(|expected| actual.iter().any(|actual| contains(actual, expected))),
        (Value::Array(actual), expected) => actual.iter().any(|actual| contains(actual, expected)),
        (Value::Object(actual), Value::Object(expected)) => {
            expected.iter().all(|(key, expected)| {
                actual
                    .get(key)
                    .is_some_and(|actual| contains(actual, expected))
            })
        }
        _ => actual == expected,
    }
}

fn summarize(value: &Value) -> String {
    let redacted = redact_value(value);
    let bytes = canonical_journal_bytes(&redacted).unwrap_or_else(|_| b"<encode-error>".to_vec());
    let text = redact_text(&String::from_utf8_lossy(&bytes));
    if text.len() <= MAX_ASSERTION_SUMMARY_BYTES {
        return text;
    }
    let mut bounded = text
        .char_indices()
        .take_while(|(index, _)| *index < MAX_ASSERTION_SUMMARY_BYTES)
        .map(|(_, character)| character)
        .collect::<String>();
    bounded.push_str("…");
    bounded
}
