//! Workbench inbox, server supplied diff and receipt display contracts.
//!
//! The types in this module are disposable read models.  They retain the server supplied
//! approval payload, artifact reference, revisions and receipt provenance so a presenter can
//! show the same facts in a Workbench panel.  They never approve, edit, restore, execute or
//! reconcile anything.  An eventual client must send the returned decision/action through the
//! existing `WorkbenchController` and `DaemonHost -> ControlPlane` path.

use kiana_protocol::{
    ArtifactRef, ArtifactVersion, ExecutionStatus, HumanActionCard, RequestId, RunId, UiActionV1,
    UI_ACTION_SCHEMA,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const WORKBENCH_REVIEW_SCHEMA: &str = "kiana.workbench-review.v1";
pub const WORKBENCH_RECEIPT_SCHEMA: &str = "kiana.workbench-receipt.v1";
pub const MAX_INBOX_CARDS: usize = 256;
pub const MAX_INBOX_FIELDS: usize = 32;
pub const MAX_INBOX_DECISIONS: usize = 16;
pub const MAX_SCOPE_PATHS: usize = 256;
pub const MAX_ARTIFACT_PAGES: usize = 1_024;
pub const MAX_ARTIFACT_PAGE_BYTES: usize = 256 * 1024;
pub const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_RECEIPT_FILES: usize = 1_024;
pub const MAX_RECEIPT_LIMITATIONS: usize = 128;
pub const MAX_RECEIPT_PROVENANCE: usize = 256;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn bounded_list(len: usize, max: usize, field: &str) -> Result<(), String> {
    if len > max {
        Err(format!("{field}_limit"))
    } else {
        Ok(())
    }
}

/// A bounded, server-derived scope summary shown on an approval card.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxScope {
    pub scope_digest: String,
    pub summary: String,
    #[serde(default)]
    pub paths: Vec<String>,
}

impl InboxScope {
    pub fn validate(&self) -> Result<(), String> {
        digest(&self.scope_digest, "inbox_scope_digest")?;
        required(&self.summary, "inbox_scope_summary", 2_048)?;
        bounded_list(self.paths.len(), MAX_SCOPE_PATHS, "inbox_scope_paths")?;
        for path in &self.paths {
            required(path, "inbox_scope_path", 1_024)?;
            if path.starts_with("http://") || path.starts_with("https://") {
                return Err("inbox_scope_path_url_forbidden".to_owned());
            }
        }
        Ok(())
    }
}

/// A required approval field.  The values in `allowed_values` are display/validation metadata;
/// they never widen the server-owned action payload or permission scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxField {
    pub name: String,
    pub label: String,
    pub field_type: String,
    pub required: bool,
    #[serde(default)]
    pub allowed_values: Vec<String>,
}

impl InboxField {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.name, "inbox_field_name", 128)?;
        required(&self.label, "inbox_field_label", 512)?;
        required(&self.field_type, "inbox_field_type", 64)?;
        bounded_list(
            self.allowed_values.len(),
            MAX_INBOX_DECISIONS,
            "inbox_field_allowed_values",
        )?;
        for value in &self.allowed_values {
            required(value, "inbox_field_allowed_value", 256)?;
        }
        Ok(())
    }
}

/// The immutable inbox card presented by Workbench.  `payload` is the exact server supplied
/// approval payload; it is retained only to detect client-side edits before an action is formed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchInboxCard {
    pub schema: String,
    pub action_id: String,
    pub command: String,
    pub target_id: String,
    pub reason: String,
    pub scope: InboxScope,
    pub expected_revision: Option<u64>,
    pub expires_at_unix_ms: Option<u64>,
    #[serde(default)]
    pub revoked: bool,
    pub fields: Vec<InboxField>,
    pub allowed_decisions: Vec<String>,
    pub payload: Value,
    pub payload_digest: String,
    #[serde(default)]
    pub artifact_refs: Vec<ArtifactRef>,
}

impl WorkbenchInboxCard {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKBENCH_REVIEW_SCHEMA {
            return Err("workbench_inbox_schema_invalid".to_owned());
        }
        required(&self.action_id, "inbox_action_id", 256)?;
        required(&self.command, "inbox_command", 128)?;
        required(&self.target_id, "inbox_target_id", 256)?;
        required(&self.reason, "inbox_reason", 4_096)?;
        self.scope.validate()?;
        if self.expected_revision == Some(0) || self.expires_at_unix_ms == Some(0) {
            return Err("inbox_card_revision_invalid".to_owned());
        }
        bounded_list(self.fields.len(), MAX_INBOX_FIELDS, "inbox_fields")?;
        for field in &self.fields {
            field.validate()?;
        }
        bounded_list(
            self.allowed_decisions.len(),
            MAX_INBOX_DECISIONS,
            "inbox_allowed_decisions",
        )?;
        if self.allowed_decisions.is_empty() {
            return Err("inbox_allowed_decisions_missing".to_owned());
        }
        let mut decisions = BTreeSet::new();
        for decision in &self.allowed_decisions {
            required(decision, "inbox_decision", 64)?;
            if !decisions.insert(decision) {
                return Err("inbox_allowed_decision_duplicate".to_owned());
            }
        }
        // The client never gets to replace the server payload with a locally edited object.  The
        // wire digest is still checked for shape here; ControlPlane performs the authoritative
        // digest and policy check when the action is submitted.
        digest(&self.payload_digest, "inbox_payload_digest")?;
        if serde_json::to_vec(&self.payload)
            .map(|bytes| bytes.len() > 128 * 1024)
            .unwrap_or(true)
        {
            return Err("inbox_payload_limit".to_owned());
        }
        bounded_list(
            self.artifact_refs.len(),
            MAX_ARTIFACT_PAGES,
            "inbox_artifact_refs",
        )?;
        for artifact in &self.artifact_refs {
            artifact.validate()?;
        }
        Ok(())
    }

    pub fn from_protocol(
        card: HumanActionCard,
        reason: impl Into<String>,
        scope: InboxScope,
        fields: Vec<InboxField>,
        payload: Value,
    ) -> Result<Self, String> {
        let card_payload = payload.clone();
        let value = Self {
            schema: WORKBENCH_REVIEW_SCHEMA.to_owned(),
            action_id: card.action_id,
            command: card.command,
            target_id: card.target_id,
            reason: reason.into(),
            scope,
            expected_revision: card.expected_revision,
            expires_at_unix_ms: card.expires_at_unix_ms,
            revoked: false,
            fields,
            allowed_decisions: card.allowed_decisions,
            payload: card_payload,
            payload_digest: card.payload_digest,
            artifact_refs: Vec::new(),
        };
        // Do not silently accept a malformed card at the presenter boundary.
        value.validate()?;
        Ok(value)
    }

    pub fn as_protocol_card(&self) -> Result<HumanActionCard, String> {
        self.validate()?;
        Ok(HumanActionCard {
            action_id: self.action_id.clone(),
            command: self.command.clone(),
            target_id: self.target_id.clone(),
            expected_revision: self.expected_revision,
            expires_at_unix_ms: self.expires_at_unix_ms,
            allowed_decisions: self.allowed_decisions.clone(),
            payload_digest: self.payload_digest.clone(),
        })
    }

    pub fn is_expired(&self, now_unix_ms: u64) -> bool {
        self.expires_at_unix_ms
            .is_some_and(|expires| now_unix_ms >= expires)
    }

    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    pub fn allows(&self, decision: &str) -> bool {
        self.allowed_decisions.iter().any(|item| item == decision)
    }

    /// Build a decision without executing it.  All server preconditions are repeated before the
    /// action crosses the typed client boundary.
    pub fn prepare_decision(
        &self,
        decision: impl Into<String>,
        payload: Value,
        observed_revision: Option<u64>,
        now_unix_ms: u64,
    ) -> Result<WorkbenchInboxDecision, String> {
        self.validate()?;
        if self.revoked {
            return Err("approval_revoked".to_owned());
        }
        if self.is_expired(now_unix_ms) {
            return Err("approval_expired".to_owned());
        }
        if self.expected_revision != observed_revision {
            return Err("approval_revision_mismatch".to_owned());
        }
        let decision = decision.into();
        required(&decision, "inbox_decision", 64)?;
        if !self.allows(&decision) {
            return Err("approval_decision_not_allowed".to_owned());
        }
        if payload != self.payload {
            return Err("client_payload_mutation".to_owned());
        }
        Ok(WorkbenchInboxDecision {
            action_id: self.action_id.clone(),
            command: self.command.clone(),
            target_id: self.target_id.clone(),
            decision,
            expected_revision: self.expected_revision,
            payload,
            payload_digest: self.payload_digest.clone(),
        })
    }
}

/// Decision intent produced by the inbox presenter; this is not a permit and has no execution
/// method.  The controller/client adds command identity and CAS fields before transport.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchInboxDecision {
    pub action_id: String,
    pub command: String,
    pub target_id: String,
    pub decision: String,
    pub expected_revision: Option<u64>,
    pub payload: Value,
    pub payload_digest: String,
}

impl WorkbenchInboxDecision {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.action_id, "inbox_decision_action_id", 256)?;
        required(&self.command, "inbox_decision_command", 128)?;
        required(&self.target_id, "inbox_decision_target_id", 256)?;
        required(&self.decision, "inbox_decision_value", 64)?;
        if self.expected_revision == Some(0) {
            return Err("inbox_decision_revision_invalid".to_owned());
        }
        digest(&self.payload_digest, "inbox_decision_payload_digest")
    }
}

/// Bounded inbox projection.  Cards are retained by stable action ID so duplicate feed events do
/// not create duplicate approval buttons or accidentally discard a pending card.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkbenchInbox {
    cards: BTreeMap<String, WorkbenchInboxCard>,
}

/// Surface-oriented name used by Workbench presenters; the underlying projection remains the
/// bounded `WorkbenchInbox` contract.
pub type InboxPanel = WorkbenchInbox;

impl WorkbenchInbox {
    pub fn replace(&mut self, cards: Vec<WorkbenchInboxCard>) -> Result<(), String> {
        bounded_list(cards.len(), MAX_INBOX_CARDS, "inbox_cards")?;
        let mut next = BTreeMap::new();
        for card in cards {
            card.validate()?;
            if next.insert(card.action_id.clone(), card).is_some() {
                return Err("inbox_action_duplicate".to_owned());
            }
        }
        self.cards = next;
        Ok(())
    }

    pub fn cards(&self) -> impl Iterator<Item = &WorkbenchInboxCard> {
        self.cards.values()
    }

    pub fn get(&self, action_id: &str) -> Option<&WorkbenchInboxCard> {
        self.cards.get(action_id)
    }

    pub fn remove(&mut self, action_id: &str) -> Option<WorkbenchInboxCard> {
        self.cards.remove(action_id)
    }
}

/// One server-provided artifact page.  `page_digest` authenticates the bytes received for this
/// page; a complete one-page artifact must also match `artifact_ref.content_hash`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactPage {
    pub artifact_ref: ArtifactRef,
    pub revision: u64,
    pub page_index: u32,
    pub page_count: u32,
    pub page_digest: String,
    pub content: Vec<u8>,
}

impl ArtifactPage {
    pub fn validate(&self) -> Result<(), String> {
        self.artifact_ref.validate()?;
        if self.revision == 0
            || self.page_count == 0
            || self.page_count as usize > MAX_ARTIFACT_PAGES
            || self.page_index >= self.page_count
            || self.content.len() > MAX_ARTIFACT_PAGE_BYTES
        {
            return Err("artifact_page_bounds_invalid".to_owned());
        }
        digest(&self.page_digest, "artifact_page_digest")?;
        let computed = artifact_content_digest(&self.artifact_ref, &self.content)?;
        if self.page_digest != computed {
            return Err("artifact_page_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

/// Read-only artifact/diff viewer.  It accepts only server pages bound to the expected ref and
/// revision; it does not fetch paths, calculate a client-side diff, or apply changes.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtifactViewer {
    artifact_ref: ArtifactRef,
    expected_revision: u64,
    expected_page_count: Option<u32>,
    pages: BTreeMap<u32, ArtifactPage>,
    total_bytes: usize,
    complete: bool,
}

/// Surface-oriented alias for the read-only server artifact/diff viewer.
pub type DiffViewer = ArtifactViewer;

impl ArtifactViewer {
    pub fn new(artifact_ref: ArtifactRef, expected_revision: u64) -> Result<Self, String> {
        artifact_ref.validate()?;
        if expected_revision == 0 {
            return Err("artifact_expected_revision_invalid".to_owned());
        }
        Ok(Self {
            artifact_ref,
            expected_revision,
            expected_page_count: None,
            pages: BTreeMap::new(),
            total_bytes: 0,
            complete: false,
        })
    }

    pub fn artifact_ref(&self) -> &ArtifactRef {
        &self.artifact_ref
    }

    pub fn expected_revision(&self) -> u64 {
        self.expected_revision
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn accept_page(&mut self, page: ArtifactPage) -> Result<(), String> {
        page.validate()?;
        if page.revision != self.expected_revision {
            return Err("artifact_revision_mismatch".to_owned());
        }
        if page.artifact_ref != self.artifact_ref {
            return Err("artifact_reference_mismatch".to_owned());
        }
        let page_count = page.page_count;
        if let Some(expected) = self.expected_page_count {
            if expected != page_count {
                return Err("artifact_page_count_mismatch".to_owned());
            }
        } else {
            self.expected_page_count = Some(page_count);
        }
        if let Some(existing) = self.pages.get(&page.page_index) {
            if existing != &page {
                return Err("artifact_page_conflict".to_owned());
            }
            return Ok(());
        }
        let next_bytes = self
            .total_bytes
            .checked_add(page.content.len())
            .ok_or_else(|| "artifact_bytes_overflow".to_owned())?;
        if next_bytes > MAX_ARTIFACT_BYTES {
            return Err("artifact_content_too_large".to_owned());
        }
        let complete_after = self.pages.len() + 1 == page_count as usize
            && self
                .pages
                .keys()
                .copied()
                .chain(std::iter::once(page.page_index))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .eq(0..page_count);
        if complete_after {
            let mut content = Vec::with_capacity(next_bytes);
            for index in 0..page_count {
                if index == page.page_index {
                    content.extend_from_slice(&page.content);
                } else if let Some(existing) = self.pages.get(&index) {
                    content.extend_from_slice(&existing.content);
                } else {
                    return Err("artifact_pages_incomplete".to_owned());
                }
            }
            let computed = artifact_content_digest(&self.artifact_ref, &content)?;
            if computed != self.artifact_ref.content_hash {
                return Err("artifact_digest_mismatch".to_owned());
            }
        }
        self.pages.insert(page.page_index, page);
        self.total_bytes = next_bytes;
        if complete_after {
            self.complete = true;
        }
        Ok(())
    }

    pub fn content_bytes(&self) -> Option<Vec<u8>> {
        if self.pages.is_empty() {
            return None;
        }
        let mut content = Vec::with_capacity(self.total_bytes);
        for page in self.pages.values() {
            content.extend_from_slice(&page.content);
        }
        Some(content)
    }

    pub fn content(&self) -> Option<Vec<u8>> {
        self.complete.then(|| self.content_bytes()).flatten()
    }
}

/// A file shown in the receipt.  The path is metadata only; no local filesystem is opened by
/// this view.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptFile {
    pub path: String,
    pub status: String,
    pub revision: u64,
    #[serde(default)]
    pub artifact_ref: Option<ArtifactRef>,
}

impl ReceiptFile {
    fn validate(&self) -> Result<(), String> {
        required(&self.path, "receipt_file_path", 2_048)?;
        if self.path.starts_with("http://") || self.path.starts_with("https://") {
            return Err("receipt_file_url_forbidden".to_owned());
        }
        required(&self.status, "receipt_file_status", 64)?;
        if self.revision == 0 {
            return Err("receipt_file_revision_invalid".to_owned());
        }
        if let Some(reference) = &self.artifact_ref {
            reference.validate()?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptCost {
    pub currency: String,
    pub amount_micros: Option<u64>,
    pub usage_known: bool,
    pub cost_known: bool,
}

impl ReceiptCost {
    fn validate(&self) -> Result<(), String> {
        required(&self.currency, "receipt_cost_currency", 16)?;
        if self.cost_known != self.amount_micros.is_some() {
            return Err("receipt_cost_known_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptProvenance {
    pub receipt_digest: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<String>,
    #[serde(default)]
    pub artifact_digests: Vec<String>,
    #[serde(default)]
    pub producer: Option<String>,
}

impl ReceiptProvenance {
    fn validate(&self) -> Result<(), String> {
        digest(&self.receipt_digest, "receipt_digest")?;
        if self.source_cursor == 0 {
            return Err("receipt_source_cursor_invalid".to_owned());
        }
        if self.source_event_ids.is_empty() {
            return Err("receipt_source_events_missing".to_owned());
        }
        bounded_list(
            self.source_event_ids.len(),
            MAX_RECEIPT_PROVENANCE,
            "receipt_source_events",
        )?;
        let mut ids = BTreeSet::new();
        for event_id in &self.source_event_ids {
            required(event_id, "receipt_source_event_id", 256)?;
            if !ids.insert(event_id) {
                return Err("receipt_source_event_duplicate".to_owned());
            }
        }
        bounded_list(
            self.artifact_digests.len(),
            MAX_RECEIPT_PROVENANCE,
            "receipt_artifact_digests",
        )?;
        for value in &self.artifact_digests {
            digest(value, "receipt_artifact_digest")?;
        }
        if let Some(producer) = &self.producer {
            required(producer, "receipt_producer", 256)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptDisplayState {
    Completed,
    CompletedWithLimitations,
    Failed,
    Cancelled,
    ResultUnknown,
}

/// Receipt projection used by Workbench.  It intentionally keeps unknown/limitations visible;
/// a `ResultUnknown` run can never be represented as a green completed receipt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkbenchReceipt {
    pub schema: String,
    pub run_id: RunId,
    pub status: ExecutionStatus,
    pub files: Vec<ReceiptFile>,
    pub cost: ReceiptCost,
    pub unknown: bool,
    pub limitations: Vec<String>,
    pub provenance: ReceiptProvenance,
}

/// Surface-oriented alias for the receipt detail presenter.
pub type ReceiptView = WorkbenchReceipt;

impl WorkbenchReceipt {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WORKBENCH_RECEIPT_SCHEMA {
            return Err("workbench_receipt_schema_invalid".to_owned());
        }
        if self.run_id.as_uuid().is_nil() {
            return Err("receipt_run_id_invalid".to_owned());
        }
        bounded_list(self.files.len(), MAX_RECEIPT_FILES, "receipt_files")?;
        for file in &self.files {
            file.validate()?;
        }
        self.cost.validate()?;
        bounded_list(
            self.limitations.len(),
            MAX_RECEIPT_LIMITATIONS,
            "receipt_limitations",
        )?;
        for limitation in &self.limitations {
            required(limitation, "receipt_limitation", 1_024)?;
        }
        self.provenance.validate()?;
        if self.status == ExecutionStatus::ResultUnknown && !self.unknown {
            return Err("result_unknown_marker_missing".to_owned());
        }
        Ok(())
    }

    pub fn display_state(&self) -> ReceiptDisplayState {
        if self.unknown || self.status == ExecutionStatus::ResultUnknown {
            ReceiptDisplayState::ResultUnknown
        } else {
            match self.status {
                ExecutionStatus::Completed if self.limitations.is_empty() => {
                    ReceiptDisplayState::Completed
                }
                ExecutionStatus::Completed => ReceiptDisplayState::CompletedWithLimitations,
                ExecutionStatus::Cancelled => ReceiptDisplayState::Cancelled,
                ExecutionStatus::Failed | ExecutionStatus::Denied | ExecutionStatus::Blocked => {
                    ReceiptDisplayState::Failed
                }
                _ => ReceiptDisplayState::CompletedWithLimitations,
            }
        }
    }

    pub fn is_green(&self) -> bool {
        self.display_state() == ReceiptDisplayState::Completed
    }

    /// Recompute a display projection from a fresh server snapshot without changing its facts.
    /// The returned value remains untrusted until the new source receipt is validated.
    pub fn recompute(
        &self,
        status: ExecutionStatus,
        files: Vec<ReceiptFile>,
        cost: ReceiptCost,
    ) -> Self {
        Self {
            schema: self.schema.clone(),
            run_id: self.run_id,
            status,
            files,
            cost,
            unknown: status == ExecutionStatus::ResultUnknown,
            limitations: self.limitations.clone(),
            provenance: self.provenance.clone(),
        }
    }
}

fn artifact_content_digest(reference: &ArtifactRef, content: &[u8]) -> Result<String, String> {
    // ArtifactVersion owns the canonical SHA-256 implementation.  Constructing this immutable
    // value is a pure digest operation and does not stage, commit or read a filesystem artifact.
    ArtifactVersion::new(
        reference.artifact_id,
        reference.version,
        reference.artifact_schema.clone(),
        content,
        reference.scope_digest.clone(),
        reference.provenance.clone(),
        1,
    )
    .map(|version| version.content_hash)
    .map_err(|_| "artifact_content_digest_failed".to_owned())
}

/// Keep the protocol action digest validator close to this boundary for source guards and future
/// client adapters.  It does not submit the action or grant approval.
pub fn validate_decision_wire_payload(
    command_id: RequestId,
    target_id: impl Into<String>,
    payload: Value,
    payload_digest: impl Into<String>,
) -> Result<(), String> {
    let action = UiActionV1 {
        schema: UI_ACTION_SCHEMA.to_owned(),
        command_id,
        idempotency_key: "workbench-review-preview".to_owned(),
        target_id: target_id.into(),
        expected_epoch: "workbench-review".to_owned(),
        expected_cursor: 1,
        expected_revision: None,
        payload,
        payload_digest: payload_digest.into(),
        submitted_by: "workbench".to_owned(),
        deadline_unix_ms: None,
    };
    action.validate()
}
