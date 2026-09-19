//! Versioned, evidence-only compact summary for the runner context view.
//!
//! A summary is not a business fact and cannot claim a tool or approval completed from prose. It
//! may carry only bounded goal/constraint/pending text plus typed evidence references; the runner
//! still preserves the latest complete assistant/tool group alongside it.

use crate::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const COMPACT_SUMMARY_SCHEMA: &str = "kiana.compact-summary.v1";
pub const COMPACT_SUMMARY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_COMPACT_SUMMARY_ITEMS: usize = 64;
pub const MAX_COMPACT_SUMMARY_EVIDENCE: usize = 128;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
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

fn evidence_ref(value: &str) -> bool {
    ["event:", "artifact:", "file:", "receipt:"]
        .iter()
        .any(|prefix| value.starts_with(prefix))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactEvidenceKind {
    Decision,
    Approval,
    CompletedAction,
    Verification,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactEvidenceStatus {
    Confirmed,
    Pending,
    Denied,
    Unknown,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactSummaryEvidence {
    pub reference: String,
    pub kind: CompactEvidenceKind,
    pub status: CompactEvidenceStatus,
    pub fact_digest: String,
}

impl CompactSummaryEvidence {
    pub fn new(
        reference: impl Into<String>,
        kind: CompactEvidenceKind,
        status: CompactEvidenceStatus,
        fact_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let evidence = Self {
            reference: reference.into(),
            kind,
            status,
            fact_digest: fact_digest.into(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !evidence_ref(&self.reference) {
            return Err("compact_summary_evidence_ref_invalid".to_owned());
        }
        digest(&self.fact_digest, "compact_summary_fact_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactSummary {
    pub schema: String,
    pub version: SchemaVersion,
    pub goal: String,
    pub constraints: Vec<String>,
    pub decision_refs: Vec<String>,
    pub completed_action_refs: Vec<String>,
    pub verification_refs: Vec<String>,
    pub workspace_revisions: Vec<String>,
    pub pending_work: Vec<String>,
    pub next_action: String,
    pub source_event_start: Option<u64>,
    pub source_event_end: Option<u64>,
    pub source_message_count: u64,
    #[serde(default)]
    pub evidence: Vec<CompactSummaryEvidence>,
    pub summary_digest: String,
}

impl CompactSummary {
    pub fn from_messages(messages: &[crate::ModelMessage]) -> Result<Self, String> {
        let goal = messages
            .iter()
            .rev()
            .find(|message| message.role == crate::ModelRole::User)
            .map(|message| message.text.clone())
            .filter(|text| !text.trim().is_empty())
            .unwrap_or_else(|| "continue from preserved runner state".to_owned());
        let pending = messages.last().is_some_and(|message| {
            message.role == crate::ModelRole::Tool
                || (message.role == crate::ModelRole::Assistant && !message.tool_calls.is_empty())
        });
        let mut summary = Self {
            schema: COMPACT_SUMMARY_SCHEMA.to_owned(),
            version: COMPACT_SUMMARY_VERSION,
            goal: bounded_text(&goal, 1_024),
            constraints: Vec::new(),
            decision_refs: Vec::new(),
            completed_action_refs: Vec::new(),
            verification_refs: Vec::new(),
            workspace_revisions: Vec::new(),
            pending_work: if pending {
                vec!["pending assistant/tool pair preserved in recent message group".to_owned()]
            } else {
                Vec::new()
            },
            next_action: "continue from the preserved recent message group".to_owned(),
            source_event_start: None,
            source_event_end: None,
            source_message_count: messages.len() as u64,
            evidence: Vec::new(),
            summary_digest: String::new(),
        };
        summary.summary_digest = summary.digest();
        summary.validate()?;
        Ok(summary)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPACT_SUMMARY_SCHEMA
            || !self.version.is_compatible_with(&COMPACT_SUMMARY_VERSION)
            || self.source_message_count == 0
        {
            return Err("compact_summary_header_invalid".to_owned());
        }
        if self.evidence.len() > MAX_COMPACT_SUMMARY_EVIDENCE {
            return Err("compact_summary_evidence_limit".to_owned());
        }
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        required(&self.goal, "compact_summary_goal", 8_192)?;
        required(&self.next_action, "compact_summary_next_action", 2_048)?;
        for (items, field) in [
            (&self.constraints, "compact_summary_constraints"),
            (&self.decision_refs, "compact_summary_decisions"),
            (&self.completed_action_refs, "compact_summary_completed"),
            (&self.verification_refs, "compact_summary_verification"),
            (&self.workspace_revisions, "compact_summary_workspace"),
            (&self.pending_work, "compact_summary_pending"),
        ] {
            if items.len() > MAX_COMPACT_SUMMARY_ITEMS
                || items
                    .iter()
                    .any(|item| item.trim().is_empty() || item.len() > 2_048)
            {
                return Err(format!("{field}_invalid"));
            }
        }
        for reference in self
            .decision_refs
            .iter()
            .chain(self.completed_action_refs.iter())
            .chain(self.verification_refs.iter())
        {
            if !evidence_ref(reference) {
                return Err("compact_summary_evidence_ref_invalid".to_owned());
            }
        }
        match (self.source_event_start, self.source_event_end) {
            (Some(start), Some(end)) if start > 0 && start <= end => {}
            (None, None) => {}
            _ => return Err("compact_summary_source_range_invalid".to_owned()),
        }
        validate_confirmed_refs(
            &self.decision_refs,
            &self.evidence,
            &[CompactEvidenceKind::Decision, CompactEvidenceKind::Approval],
            "compact_summary_decision_evidence_missing",
            "compact_summary_decision_fact_not_confirmed",
        )?;
        validate_confirmed_refs(
            &self.completed_action_refs,
            &self.evidence,
            &[CompactEvidenceKind::CompletedAction],
            "compact_summary_completed_evidence_missing",
            "compact_summary_completed_fact_not_confirmed",
        )?;
        validate_confirmed_refs(
            &self.verification_refs,
            &self.evidence,
            &[CompactEvidenceKind::Verification],
            "compact_summary_verification_evidence_missing",
            "compact_summary_verification_fact_not_confirmed",
        )?;
        digest(&self.summary_digest, "compact_summary_digest")?;
        if self.summary_digest != self.digest() {
            return Err("compact_summary_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Attach server-derived event/receipt evidence to an otherwise bounded summary.
    /// Model prose alone cannot create a completed action, decision or verification reference.
    pub fn with_evidence(mut self, evidence: Vec<CompactSummaryEvidence>) -> Result<Self, String> {
        self.evidence = evidence;
        self.summary_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    /// Recheck that every evidence item in the summary came from the current authoritative
    /// event/receipt projection before the summary is admitted to a context view.
    pub fn validate_against_evidence(
        &self,
        authoritative: &[CompactSummaryEvidence],
    ) -> Result<(), String> {
        self.validate()?;
        for evidence in &self.evidence {
            if !authoritative.iter().any(|candidate| candidate == evidence) {
                return Err("compact_summary_evidence_not_authoritative".to_owned());
            }
        }
        Ok(())
    }

    pub fn render(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string(self).map_err(|_| "compact_summary_encode_failed".to_owned())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "summary_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}

fn validate_confirmed_refs(
    references: &[String],
    evidence: &[CompactSummaryEvidence],
    allowed_kinds: &[CompactEvidenceKind],
    missing_reason: &str,
    unconfirmed_reason: &str,
) -> Result<(), String> {
    for reference in references {
        let Some(fact) = evidence.iter().find(|fact| fact.reference == *reference) else {
            return Err(missing_reason.to_owned());
        };
        if !allowed_kinds.contains(&fact.kind) || fact.status != CompactEvidenceStatus::Confirmed {
            return Err(unconfirmed_reason.to_owned());
        }
    }
    Ok(())
}

fn bounded_text(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut end = max;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}
