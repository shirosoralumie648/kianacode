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

/// Schema of the Web timeline component directory.  A component is a declared presentation
/// boundary (thread, turn, item, status bar, feed indicator); it may project a server item but it
/// may never execute a command, decide an approval or own a second copy of the projection.
pub const WEB_TIMELINE_COMPONENT_SCHEMA: &str = "kiana.web-timeline-components.v1";
/// Schema of the server-owned migration switch that selects the reviewed timeline renderer.
pub const WEB_TIMELINE_MIGRATION_SCHEMA: &str = "kiana.web-timeline-migration.v1";
/// Schema of the measured bundle baseline published with the migration contract.
pub const WEB_TIMELINE_BUNDLE_SCHEMA: &str = "kiana.web-timeline-bundle.v1";

/// The only renderer the Web timeline may select: the typed inline projection that maps an
/// explicit server `kind` into a DOM node and never infers a kind from text.
pub const WEB_TIMELINE_RENDERER_TYPED_INLINE: &str = "typed_inline";
/// A renderer request for text-based kind inference.  It is never selected: guessing an item kind
/// from a title or body is the first denial of this step, so the value stays a refused request.
pub const WEB_TIMELINE_RENDERER_TEXT_GUESS: &str = "legacy_text";
/// Reviewed renderer allowlist published to a browser surface.
pub const WEB_TIMELINE_REVIEWED_RENDERERS: &[&str] = &[WEB_TIMELINE_RENDERER_TYPED_INLINE];

/// Byte budget for the embedded Web page artifact.  The number is a review baseline, not a claim
/// about a browser bundle: a page above it is published as an unavailable migration so the
/// renderer fails closed instead of painting an unreviewed payload.
pub const WEB_TIMELINE_PAGE_MAX_BYTES: usize = 160 * 1024;
/// Byte budget for the marked timeline component region inside that artifact.
pub const WEB_TIMELINE_COMPONENT_MAX_BYTES: usize = 64 * 1024;
/// Markers that delimit the timeline component region inside the served page.
pub const WEB_TIMELINE_COMPONENT_START_MARKER: &str = "// >>> UI-20 timeline components";
pub const WEB_TIMELINE_COMPONENT_END_MARKER: &str = "// <<< UI-20 timeline components";

/// Reviewed component directory: `(name, selector, dataset marker)`.
pub const WEB_TIMELINE_COMPONENTS: &[(&str, &str, &str)] = &[
    ("thread", "#chat", "thread"),
    ("turn", "section.timeline-turn", "turn"),
    ("item", "article.timeline-item", "item"),
    ("status-bar", "#timelineState", "status-bar"),
    ("feed-indicator", "#streamBanner", "feed-indicator"),
];

/// One declared presentation boundary of the Web timeline.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebTimelineComponent {
    pub name: String,
    pub selector: String,
    /// Value the component must carry in `data-component` for a DOM golden/probe.
    pub dataset_component: String,
    /// The reviewed renderer never grants a component execution or decision authority.
    pub may_execute: bool,
}

impl WebTimelineComponent {
    pub fn validate(&self) -> Result<(), String> {
        if self.may_execute {
            return Err("web_timeline_component_authority_denied".to_owned());
        }
        required(&self.name, "web_timeline_component_name", 64)?;
        required(&self.selector, "web_timeline_component_selector", 128)?;
        required(
            &self.dataset_component,
            "web_timeline_component_dataset",
            64,
        )
    }
}

/// The reviewed component directory as a serializable contract.
pub fn web_timeline_components() -> Vec<WebTimelineComponent> {
    WEB_TIMELINE_COMPONENTS
        .iter()
        .map(|(name, selector, dataset_component)| WebTimelineComponent {
            name: (*name).to_owned(),
            selector: (*selector).to_owned(),
            dataset_component: (*dataset_component).to_owned(),
            may_execute: false,
        })
        .collect()
}

/// Server-published bounds of the timeline window.  A presenter whose own constants differ from
/// these bounds fails closed instead of quietly windowing by another rule.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebTimelineWindowBounds {
    pub size: usize,
    pub max_items: usize,
    pub max_body_bytes: usize,
    pub protected: Vec<String>,
    pub older_page: String,
}

pub fn web_timeline_window_bounds() -> WebTimelineWindowBounds {
    WebTimelineWindowBounds {
        size: WEB_TIMELINE_WINDOW_SIZE,
        max_items: WEB_TIMELINE_MAX_ITEMS,
        max_body_bytes: WEB_TIMELINE_MAX_BODY_BYTES,
        protected: ["pending", "unknown"].map(str::to_owned).to_vec(),
        older_page: "hydrate_or_history_cursor".to_owned(),
    }
}

/// Measured bundle baseline of the embedded page artifact.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebTimelineBundle {
    pub schema: String,
    pub page_bytes: usize,
    pub component_bytes: usize,
    pub max_page_bytes: usize,
    pub max_component_bytes: usize,
    pub within_budget: bool,
}

impl WebTimelineBundle {
    /// Measure one served artifact.  `None` means the component region is not marked at all, which
    /// is an unreviewed artifact rather than a zero-byte component.
    pub fn measure(page: &str) -> Option<Self> {
        let start = page.find(WEB_TIMELINE_COMPONENT_START_MARKER)?;
        let end = page[start..].find(WEB_TIMELINE_COMPONENT_END_MARKER)? + start;
        let end = end + WEB_TIMELINE_COMPONENT_END_MARKER.len();
        let page_bytes = page.len();
        let component_bytes = end - start;
        Some(Self {
            schema: WEB_TIMELINE_BUNDLE_SCHEMA.to_owned(),
            page_bytes,
            component_bytes,
            max_page_bytes: WEB_TIMELINE_PAGE_MAX_BYTES,
            max_component_bytes: WEB_TIMELINE_COMPONENT_MAX_BYTES,
            within_budget: page_bytes <= WEB_TIMELINE_PAGE_MAX_BYTES
                && component_bytes <= WEB_TIMELINE_COMPONENT_MAX_BYTES,
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_TIMELINE_BUNDLE_SCHEMA || self.component_bytes == 0 {
            return Err("web_timeline_bundle_invalid".to_owned());
        }
        if !self.within_budget {
            return Err("web_timeline_bundle_over_budget".to_owned());
        }
        Ok(())
    }
}

/// Disposition of the server-owned renderer migration.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebTimelineMigrationDisposition {
    /// The reviewed renderer is selected for an artifact inside its budgets.
    Active,
    /// The artifact could not be reviewed (missing component region or over budget).  A browser
    /// must stay `partial` and keep only pending/unknown items instead of guessing a renderer.
    Unavailable,
}

/// The migration switch published with a Web snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebTimelineMigration {
    pub schema: String,
    pub selected: String,
    pub disposition: WebTimelineMigrationDisposition,
    pub reason: String,
    pub renderers: Vec<String>,
    pub components: Vec<WebTimelineComponent>,
    pub window: WebTimelineWindowBounds,
    pub bundle: WebTimelineBundle,
    pub server_item_kinds: Vec<String>,
    pub visible_states: Vec<String>,
}

impl WebTimelineMigration {
    /// Select the renderer for one served artifact.  Reviewing failure is never a fallback: the
    /// migration is published as `unavailable` so the browser keeps a visible partial state.
    pub fn select(page: &str) -> Self {
        let components = web_timeline_components();
        let window = web_timeline_window_bounds();
        let reviewed = components.iter().all(|entry| entry.validate().is_ok());
        let bundle = WebTimelineBundle::measure(page);
        let (selected, disposition, reason) = match &bundle {
            None => (
                "unavailable",
                WebTimelineMigrationDisposition::Unavailable,
                "web_timeline_component_region_missing",
            ),
            Some(bundle) if bundle.validate().is_err() => (
                "unavailable",
                WebTimelineMigrationDisposition::Unavailable,
                "web_timeline_bundle_over_budget",
            ),
            Some(_) if !reviewed => (
                "unavailable",
                WebTimelineMigrationDisposition::Unavailable,
                "web_timeline_component_authority_denied",
            ),
            Some(_) => (
                WEB_TIMELINE_RENDERER_TYPED_INLINE,
                WebTimelineMigrationDisposition::Active,
                "web_timeline_renderer_reviewed",
            ),
        };
        Self {
            schema: WEB_TIMELINE_MIGRATION_SCHEMA.to_owned(),
            selected: selected.to_owned(),
            disposition,
            reason: reason.to_owned(),
            renderers: WEB_TIMELINE_REVIEWED_RENDERERS
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            components,
            window,
            bundle: bundle.unwrap_or_else(|| WebTimelineBundle {
                schema: WEB_TIMELINE_BUNDLE_SCHEMA.to_owned(),
                page_bytes: page.len(),
                component_bytes: 0,
                max_page_bytes: WEB_TIMELINE_PAGE_MAX_BYTES,
                max_component_bytes: WEB_TIMELINE_COMPONENT_MAX_BYTES,
                within_budget: false,
            }),
            server_item_kinds: ["delta", "tool", "approval", "error", "unknown", "terminal"]
                .map(str::to_owned)
                .to_vec(),
            visible_states: ["loading", "partial", "replay", "ready", "offline"]
                .map(str::to_owned)
                .to_vec(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.disposition == WebTimelineMigrationDisposition::Active
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_TIMELINE_MIGRATION_SCHEMA {
            return Err("web_timeline_migration_schema_invalid".to_owned());
        }
        if !self.is_active() {
            return Err("web_timeline_migration_unavailable".to_owned());
        }
        if self.selected != WEB_TIMELINE_RENDERER_TYPED_INLINE {
            return Err("web_timeline_renderer_not_reviewed".to_owned());
        }
        if self.reason.trim().is_empty() {
            return Err("web_timeline_migration_reason_missing".to_owned());
        }
        if self.components.len() != WEB_TIMELINE_COMPONENTS.len() {
            return Err("web_timeline_component_directory_incomplete".to_owned());
        }
        for component in &self.components {
            component.validate()?;
        }
        if self.window.size != WEB_TIMELINE_WINDOW_SIZE
            || self.window.max_items != WEB_TIMELINE_MAX_ITEMS
            || self.window.max_body_bytes != WEB_TIMELINE_MAX_BODY_BYTES
        {
            return Err("web_timeline_window_bounds_drifted".to_owned());
        }
        for protected in ["pending", "unknown"] {
            if !self.window.protected.iter().any(|value| value == protected) {
                return Err("web_timeline_protected_kinds_incomplete".to_owned());
            }
        }
        self.bundle.validate()?;
        if self.server_item_kinds.len() != 6 || self.visible_states.len() != 5 {
            return Err("web_timeline_kind_or_state_matrix_incomplete".to_owned());
        }
        Ok(())
    }
}
