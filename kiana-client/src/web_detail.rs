//! Read-only Web artifact, diff and receipt detail projections.
//!
//! The browser/client keeps only server supplied identifiers, pages and statistics.  It validates
//! revision, cursor scope and page digests before exposing content, while ControlPlane remains the
//! authority for authorization and the EventLog/Receipt remains the authority for facts.  This
//! module never opens a path, fetches a URL, computes a diff or retries an Unknown result.

use kiana_domain::{journal_sha256, ArtifactId, SessionId};
use kiana_protocol::{
    UiArtifactDetailV1, UiArtifactPageV1, UiArtifactRefV1, UiDetailScopeV1, UiDiffDetailV1,
    UiReceiptDetailV1,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const WEB_DETAIL_SCHEMA: &str = "kiana.web-detail.v1";
pub const WEB_ARTIFACT_DETAIL_SCHEMA: &str = "kiana.web-artifact-detail.v1";
pub const WEB_ARTIFACT_PAGE_SCHEMA: &str = "kiana.web-artifact-page.v2";
pub const WEB_DIFF_DETAIL_SCHEMA: &str = "kiana.web-diff-detail.v1";
pub const WEB_RECEIPT_DETAIL_SCHEMA: &str = "kiana.web-receipt-detail.v1";
pub const WEB_DETAIL_CURSOR_SCHEMA: &str = "kiana.web-detail-cursor.v1";
pub const WEB_DETAIL_MAX_PAGES: usize = 512;
pub const WEB_DETAIL_MAX_PAGE_BYTES: usize = 64 * 1024;

pub type WebArtifactRef = UiArtifactRefV1;
pub type WebArtifactPage = UiArtifactPageV1;
pub type WebDiffDetail = UiDiffDetailV1;
pub type WebReceiptDetail = UiReceiptDetailV1;

/// Cursor state is server-issued and tab/session scoped.  A client can retain it for a read-only
/// page request, but cannot edit the owner/session/epoch fields and remain valid.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDetailCursor {
    pub schema: String,
    pub token: String,
    pub kind: String,
    pub session_id: SessionId,
    pub tab_id: String,
    pub instance_id: String,
    pub epoch: String,
    pub source_cursor: u64,
    pub revision: u64,
}

impl WebDetailCursor {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_DETAIL_CURSOR_SCHEMA
            || self.token.trim().is_empty()
            || self.token.len() > 512
            || self.kind.trim().is_empty()
            || self.kind.len() > 64
            || self.source_cursor == 0
            || self.revision == 0
        {
            return Err("web_detail_cursor_invalid".to_owned());
        }
        if self.token.contains('\0')
            || self.kind.contains('\0')
            || self.tab_id.trim().is_empty()
            || self.tab_id.len() > 256
            || self.instance_id.trim().is_empty()
            || self.epoch.trim().is_empty()
        {
            return Err("web_detail_cursor_scope_invalid".to_owned());
        }
        Ok(())
    }

    pub fn matches_scope(&self, scope: &UiDetailScopeV1) -> bool {
        self.session_id == scope.session_id
            && self.tab_id == scope.tab_id
            && self.instance_id == scope.instance_id
            && self.epoch == scope.epoch
            && self.source_cursor == scope.source_cursor
            && self.revision == scope.revision
    }
}

/// Client-side state for server pages.  Pages from another artifact, session, tab or revision
/// are denied before they can be appended; an incomplete/unknown page remains incomplete.
#[derive(Clone, Debug, PartialEq)]
pub struct WebArtifactViewer {
    artifact: WebArtifactRef,
    expected_revision: u64,
    expected_page_count: Option<u32>,
    pages: BTreeMap<u32, WebArtifactPage>,
    total_bytes: usize,
    complete: bool,
    status: WebDetailStatus,
    limitations: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebDetailStatus {
    Ready,
    Partial,
    Unknown,
}

impl WebArtifactViewer {
    pub fn new(artifact: WebArtifactRef, expected_revision: u64) -> Result<Self, String> {
        artifact.validate()?;
        if expected_revision == 0 || artifact.revision != expected_revision {
            return Err("web_artifact_expected_revision_invalid".to_owned());
        }
        Ok(Self {
            artifact,
            expected_revision,
            expected_page_count: None,
            pages: BTreeMap::new(),
            total_bytes: 0,
            complete: false,
            status: WebDetailStatus::Partial,
            limitations: Vec::new(),
        })
    }

    pub fn artifact(&self) -> &WebArtifactRef {
        &self.artifact
    }

    pub fn status(&self) -> WebDetailStatus {
        self.status
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }

    /// Accept one server page after checking server digest, MIME/content policy, revision and
    /// scope.  No client-side diff or authorization decision is made here.
    pub fn accept_page(&mut self, page: WebArtifactPage) -> Result<(), String> {
        page.validate()?;
        if page.revision != self.expected_revision || page.artifact != self.artifact {
            return Err("web_artifact_revision_mismatch".to_owned());
        }
        if page.scope.session_id != self.artifact.session_id
            || page.scope.revision != self.expected_revision
        {
            return Err("web_artifact_scope_mismatch".to_owned());
        }
        if page.content_encoding == "text/html"
            || page.artifact.mime == "text/html"
            || page.artifact.mime == "application/xhtml+xml"
            || page.artifact.mime == "image/svg+xml"
        {
            return Err("web_artifact_content_type_denied".to_owned());
        }
        if let Some(content) = &page.content {
            if content.len() > WEB_DETAIL_MAX_PAGE_BYTES || content.contains('\0') {
                return Err("web_artifact_page_too_large".to_owned());
            }
            let computed = format!("sha256:{}", journal_sha256(content.as_bytes()));
            if computed != page.page_digest {
                return Err("web_artifact_page_digest_mismatch".to_owned());
            }
        } else if page.content_encoding != "none" {
            return Err("web_artifact_page_content_missing".to_owned());
        }
        if let Some(expected) = self.expected_page_count {
            if expected != page.page_count {
                return Err("web_artifact_page_count_mismatch".to_owned());
            }
        } else {
            self.expected_page_count = Some(page.page_count);
        }
        if let Some(existing) = self.pages.get(&page.page_index) {
            if existing != &page {
                return Err("web_artifact_page_conflict".to_owned());
            }
            return Ok(());
        }
        let page_bytes = page.content.as_ref().map_or(0, String::len);
        let total = self
            .total_bytes
            .checked_add(page_bytes)
            .ok_or_else(|| "web_artifact_bytes_overflow".to_owned())?;
        if total > self.artifact.size_bytes as usize && self.artifact.size_bytes != 0 {
            return Err("web_artifact_size_mismatch".to_owned());
        }
        self.pages.insert(page.page_index, page);
        self.total_bytes = total;
        let expected = self.expected_page_count.unwrap_or_default() as usize;
        if expected > 0 && self.pages.len() == expected {
            let contiguous = self.pages.keys().copied().eq(0..expected as u32);
            if contiguous {
                self.complete = true;
                self.status = WebDetailStatus::Ready;
            }
        }
        if !self.complete {
            self.status = WebDetailStatus::Partial;
        }
        Ok(())
    }

    pub fn mark_unknown(&mut self, limitation: impl Into<String>) {
        let limitation = limitation.into();
        if !limitation.trim().is_empty() && limitation.len() <= 512 {
            self.limitations.push(limitation);
        }
        self.status = WebDetailStatus::Unknown;
        self.complete = false;
    }

    pub fn content_text(&self) -> Option<String> {
        if !self.complete {
            return None;
        }
        let mut content = String::with_capacity(self.total_bytes);
        for index in 0..self.expected_page_count.unwrap_or_default() {
            let page = self.pages.get(&index)?;
            content.push_str(page.content.as_deref()?);
        }
        let digest = format!("sha256:{}", journal_sha256(content.as_bytes()));
        (digest == self.artifact.content_hash).then_some(content)
    }
}

/// Read-only aggregate projection.  The client accepts server statistics and links but never
/// derives additions/deletions or promotes an Unknown receipt to success.
#[derive(Clone, Debug, PartialEq)]
pub struct WebDetailProjection {
    scope: UiDetailScopeV1,
    artifact: Option<WebArtifactViewer>,
    diff: Option<WebDiffDetail>,
    receipt: Option<WebReceiptDetail>,
    status: WebDetailStatus,
}

impl WebDetailProjection {
    pub fn from_server(detail: UiArtifactDetailV1) -> Result<Self, String> {
        detail.validate()?;
        let status = match detail.status.as_str() {
            "ready" | "completed" => WebDetailStatus::Ready,
            "partial" | "loading" => WebDetailStatus::Partial,
            "unknown" | "result_unknown" | "incident" => WebDetailStatus::Unknown,
            _ => WebDetailStatus::Unknown,
        };
        let artifact = WebArtifactViewer::new(
            detail.artifact_page.artifact.clone(),
            detail.artifact_page.revision,
        )?;
        Ok(Self {
            scope: detail.scope,
            artifact: Some(artifact),
            diff: detail.diff,
            receipt: detail.receipt,
            status,
        })
    }

    pub fn scope(&self) -> &UiDetailScopeV1 {
        &self.scope
    }

    pub fn artifact(&self) -> Option<&WebArtifactViewer> {
        self.artifact.as_ref()
    }

    pub fn diff(&self) -> Option<&WebDiffDetail> {
        self.diff.as_ref()
    }

    pub fn receipt(&self) -> Option<&WebReceiptDetail> {
        self.receipt.as_ref()
    }

    pub fn status(&self) -> WebDetailStatus {
        self.status
    }

    pub fn accepts_server_stats_only(&self) -> bool {
        self.diff.as_ref().is_none_or(|diff| {
            diff.files
                .iter()
                .all(|file| file.patch.is_some() || file.patch_digest.starts_with("sha256:"))
        })
    }

    pub fn receipt_is_unknown(&self) -> bool {
        self.receipt.as_ref().is_some_and(|receipt| receipt.unknown)
            || self.status == WebDetailStatus::Unknown
    }
}

/// Lightweight server ref used by a detail request.  It intentionally has no URL/path field.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebDetailRequest {
    pub session_id: SessionId,
    pub tab_id: String,
    pub artifact_id: ArtifactId,
    pub revision: u64,
    #[serde(default)]
    pub after: Option<String>,
}

impl WebDetailRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.session_id.is_empty()
            || self.tab_id.trim().is_empty()
            || self.tab_id.len() > 256
            || self.artifact_id.as_uuid().is_nil()
            || self.revision == 0
            || self.after.as_deref().is_some_and(|cursor| {
                cursor.trim().is_empty() || cursor.len() > 512 || cursor.contains('\0')
            })
        {
            return Err("web_detail_request_invalid".to_owned());
        }
        Ok(())
    }
}
