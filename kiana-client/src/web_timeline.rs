//! Typed, bounded contracts for the Web timeline presenter.
//!
//! The Web page is a disposable renderer.  It may project a server item into a DOM node, but it
//! never decides what happened, authorizes a command, or promotes a missing stream frame to a
//! terminal result.  `WebTimelineItemKind` is deliberately an exact server-kind mapping: text in
//! `title` or `body` is never inspected to infer a kind.

use serde::{Deserialize, Serialize};

pub const WEB_TIMELINE_SCHEMA: &str = "kiana.web-timeline.v1";
pub const WEB_TIMELINE_ITEM_SCHEMA: &str = "kiana.web-timeline-item.v1";
pub const WEB_TIMELINE_MAX_ITEMS: usize = 512;
pub const WEB_TIMELINE_WINDOW_SIZE: usize = 64;
pub const WEB_TIMELINE_MAX_BODY_BYTES: usize = 16 * 1024;
pub const WEB_TIMELINE_MAX_ID_BYTES: usize = 512;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

/// Item kinds are selected from the server supplied kind field only.
///
/// The legacy Web names are accepted as an explicit compatibility mapping while the migration is
/// in progress.  Unknown values remain `Unknown`; no body/title matching is performed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebTimelineItemKind {
    Delta,
    Tool,
    Approval,
    Error,
    Unknown,
    Terminal,
}

impl WebTimelineItemKind {
    pub fn from_server_kind(value: &str) -> Self {
        match value {
            "delta" | "agentMessage" | "userMessage" => Self::Delta,
            "tool" | "commandExecution" | "fileChange" => Self::Tool,
            "approval" => Self::Approval,
            "error" => Self::Error,
            "terminal" => Self::Terminal,
            "unknown" => Self::Unknown,
            _ => Self::Unknown,
        }
    }

    pub fn is_protected(self, pending: bool) -> bool {
        pending || matches!(self, Self::Unknown)
    }
}

/// Presentation state is visible in the page and cannot be mistaken for an execution outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebTimelineViewState {
    Loading,
    Partial,
    Replay,
    Ready,
    Offline,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebTimelineItem {
    pub schema: String,
    pub id: String,
    pub kind: WebTimelineItemKind,
    pub status: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub replay: bool,
    #[serde(default)]
    pub sequence: Option<u64>,
}

impl WebTimelineItem {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_TIMELINE_ITEM_SCHEMA {
            return Err("web_timeline_item_schema_invalid".to_owned());
        }
        required(&self.id, "web_timeline_item_id", WEB_TIMELINE_MAX_ID_BYTES)?;
        required(&self.status, "web_timeline_item_status", 128)?;
        required(&self.title, "web_timeline_item_title", 512)?;
        if self.body.len() > WEB_TIMELINE_MAX_BODY_BYTES || self.body.contains('\0') {
            return Err("web_timeline_item_body_invalid".to_owned());
        }
        if self.kind == WebTimelineItemKind::Terminal && self.pending {
            return Err("web_timeline_terminal_pending".to_owned());
        }
        Ok(())
    }

    pub fn protected(&self) -> bool {
        self.kind.is_protected(self.pending)
    }
}

/// Bounded client projection metadata.  The renderer may include protected pending/unknown items
/// outside the recent window, so a reconnect or long stream cannot silently hide an unresolved
/// action or an event whose schema was not understood.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebTimelineWindow {
    pub schema: String,
    pub state: WebTimelineViewState,
    pub cursor: Option<String>,
    pub items: Vec<WebTimelineItem>,
    #[serde(default)]
    pub has_older: bool,
    #[serde(default)]
    pub replay: bool,
}

impl WebTimelineWindow {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_TIMELINE_SCHEMA || self.items.len() > WEB_TIMELINE_MAX_ITEMS {
            return Err("web_timeline_window_header_invalid".to_owned());
        }
        if self
            .cursor
            .as_deref()
            .is_some_and(|cursor| cursor.is_empty() || cursor.len() > WEB_TIMELINE_MAX_ID_BYTES)
        {
            return Err("web_timeline_window_cursor_invalid".to_owned());
        }
        let mut ids = std::collections::BTreeSet::new();
        for item in &self.items {
            item.validate()?;
            if !ids.insert(item.id.as_str()) {
                return Err("web_timeline_duplicate_item".to_owned());
            }
        }
        Ok(())
    }

    pub fn protected_item_ids(&self) -> impl Iterator<Item = &str> {
        self.items
            .iter()
            .filter(|item| item.protected())
            .map(|item| item.id.as_str())
    }
}
