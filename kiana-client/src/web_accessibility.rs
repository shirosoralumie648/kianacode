//! Typed Web accessibility and text-only presentation contracts.
//!
//! This module is a client-side validation boundary only.  It records the server scope that a
//! focus restoration or status projection belongs to and rejects control/URL escape sequences
//! before a presenter can put them in the DOM.  It does not authorize actions, create a modal
//! decision, or replace the server-owned timeline, inbox, detail, session, or SSE contracts.

use kiana_domain::SessionId;
use serde::{Deserialize, Serialize};

pub const WEB_ACCESSIBILITY_SCHEMA: &str = "kiana.web-accessibility.v1";
pub const WEB_FOCUS_SCOPE_SCHEMA: &str = "kiana.web-focus-scope.v1";
pub const WEB_MAX_TEXT_BYTES: usize = 256 * 1024;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

/// Server-bound identity carried with a focus return target.  A focus target from a previous
/// session, tab or authority epoch is never reused after hydrate/reconnect.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebFocusScope {
    pub schema: String,
    pub session_id: SessionId,
    pub tab_id: String,
    pub epoch: String,
    pub return_focus_id: String,
}

impl WebFocusScope {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_FOCUS_SCOPE_SCHEMA {
            return Err("web_focus_scope_schema_invalid".to_owned());
        }
        required(self.session_id.as_str(), "web_focus_scope_session", 256)?;
        required(&self.tab_id, "web_focus_scope_tab", 256)?;
        required(&self.epoch, "web_focus_scope_epoch", 256)?;
        required(&self.return_focus_id, "web_focus_scope_return_target", 256)
    }

    pub fn matches(&self, session_id: &SessionId, tab_id: &str, epoch: &str) -> bool {
        self.session_id == *session_id && self.tab_id == tab_id && self.epoch == epoch
    }
}

/// A bounded presentation state.  `Unknown` and `Offline` are visible states and cannot be
/// converted into a successful action by a browser presenter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebAccessibilityState {
    Loading,
    Ready,
    Partial,
    Unknown,
    Offline,
}

impl WebAccessibilityState {
    pub fn is_terminal_success(self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Text-only DOM presenters may display literal markup as text, but they must not accept control
/// sequences or executable URL schemes.  HTML is intentionally not parsed or interpreted here.
pub fn validate_text_only(value: &str, max_bytes: usize) -> Result<(), String> {
    if value.len() > max_bytes || value.len() > WEB_MAX_TEXT_BYTES {
        return Err("web_text_too_large".to_owned());
    }
    if value.contains('\0') {
        return Err("web_text_nul_denied".to_owned());
    }
    if value.contains('\u{1b}') || value.contains('\u{7}') {
        return Err("web_text_ansi_or_osc_denied".to_owned());
    }
    if value
        .split_whitespace()
        .any(|token| is_executable_url_scheme(token))
    {
        return Err("web_text_executable_url_denied".to_owned());
    }
    Ok(())
}

/// Preserve user/tool output as text after the same deny-first validation used by the browser.
/// Callers must still use a text sink such as `textContent`; this function does not produce HTML.
pub fn sanitize_text_only(value: &str, max_bytes: usize) -> Result<String, String> {
    validate_text_only(value, max_bytes)?;
    Ok(value.to_owned())
}

fn is_executable_url_scheme(value: &str) -> bool {
    let candidate = value
        .trim_matches(|character: char| {
            matches!(
                character,
                '`' | '\'' | '"' | '(' | ')' | '[' | ']' | '<' | '>' | ','
            )
        })
        .to_ascii_lowercase();
    candidate.starts_with("javascript:")
        || candidate.starts_with("vbscript:")
        || candidate.starts_with("data:text/html")
        || candidate.starts_with("data:application/xhtml+xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> WebFocusScope {
        WebFocusScope {
            schema: WEB_FOCUS_SCOPE_SCHEMA.to_owned(),
            session_id: SessionId::new("session-1"),
            tab_id: "tab-1".to_owned(),
            epoch: "epoch-1".to_owned(),
            return_focus_id: "approval-button".to_owned(),
        }
    }

    #[test]
    fn focus_scope_is_session_tab_and_epoch_bound() {
        let value = scope();
        assert!(value.validate().is_ok());
        assert!(value.matches(&SessionId::new("session-1"), "tab-1", "epoch-1"));
        assert!(!value.matches(&SessionId::new("session-2"), "tab-1", "epoch-1"));
        assert!(!value.matches(&SessionId::new("session-1"), "tab-2", "epoch-1"));
        assert!(!value.matches(&SessionId::new("session-1"), "tab-1", "epoch-2"));
    }

    #[test]
    fn text_only_contract_rejects_control_ansi_and_executable_url() {
        assert!(sanitize_text_only("<script>alert(1)</script>", 128).is_ok());
        assert_eq!(
            sanitize_text_only("hello\nworld", 128).unwrap(),
            "hello\nworld"
        );
        assert_eq!(
            validate_text_only("\u{1b}]8;;https://evil.example\u{7}click", 128),
            Err("web_text_ansi_or_osc_denied".to_owned())
        );
        assert_eq!(
            validate_text_only("javascript:alert(1)", 128),
            Err("web_text_executable_url_denied".to_owned())
        );
    }
}
