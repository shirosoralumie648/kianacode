//! Unified Company Human Inbox decision-card projection.
//!
//! Cards bind one exact target and an authorized decider. Consuming a card records a decision fact
//! but does not execute the referenced command; the ControlPlane must recheck the same target.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const COMPANY_INBOX_CARD_SCHEMA: &str = "kiana.company-inbox-card.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains(['\0', '\r', '\n']) {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyInboxCardKind {
    Charter,
    Change,
    RuntimeApproval,
    Acceptance,
    Delivery,
    Incident,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyInboxCardStatus {
    Pending,
    Decided,
    Expired,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyInboxCard {
    pub schema: String,
    pub card_id: String,
    pub task_id: String,
    pub kind: CompanyInboxCardKind,
    pub project_id: String,
    pub target_kind: String,
    pub target_id: String,
    pub target_revision: u64,
    pub target_digest: String,
    pub scope_digest: String,
    pub decider_ref: String,
    pub options: Vec<String>,
    pub evidence_refs: Vec<String>,
    pub created_at: u64,
    pub due_at: u64,
    pub expires_at: u64,
    pub status: CompanyInboxCardStatus,
    #[serde(default)]
    pub decision_ref: Option<String>,
    #[serde(default)]
    pub decided_by: Option<String>,
    pub digest: String,
}

impl CompanyInboxCard {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_INBOX_CARD_SCHEMA
            || self.target_revision == 0
            || self.created_at == 0
            || self.due_at < self.created_at
            || self.expires_at <= self.due_at
            || self.options.is_empty()
            || self.options.len() > 16
            || self.evidence_refs.is_empty()
        {
            return Err("company_inbox_card_header_invalid");
        }
        for (value, field) in [
            (&self.card_id, "company_inbox_card_id_required"),
            (&self.task_id, "company_inbox_task_id_required"),
            (&self.project_id, "company_inbox_project_required"),
            (&self.target_kind, "company_inbox_target_kind_required"),
            (&self.target_id, "company_inbox_target_id_required"),
            (&self.decider_ref, "company_inbox_decider_required"),
        ] {
            required(value, field)?;
        }
        digest(&self.target_digest, "company_inbox_target_digest_invalid")?;
        digest(&self.scope_digest, "company_inbox_scope_digest_invalid")?;
        for option in &self.options {
            required(option, "company_inbox_option_invalid")?;
        }
        for evidence in &self.evidence_refs {
            required(evidence, "company_inbox_evidence_invalid")?;
        }
        if self.options.windows(2).any(|pair| pair[0] >= pair[1])
            || self.evidence_refs.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("company_inbox_refs_noncanonical");
        }
        match self.status {
            CompanyInboxCardStatus::Pending => {
                if self.decision_ref.is_some() || self.decided_by.is_some() {
                    return Err("company_inbox_pending_decision_invalid");
                }
            }
            CompanyInboxCardStatus::Decided => {
                if self.decision_ref.as_deref().is_none_or(str::is_empty)
                    || self.decided_by.as_deref().is_none_or(str::is_empty)
                {
                    return Err("company_inbox_decision_required");
                }
            }
            CompanyInboxCardStatus::Expired | CompanyInboxCardStatus::Cancelled => {
                if self.decision_ref.is_some() || self.decided_by.is_some() {
                    return Err("company_inbox_terminal_decision_invalid");
                }
            }
        }
        digest(&self.digest, "company_inbox_card_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("company_inbox_card_digest_mismatch");
        }
        Ok(())
    }

    pub fn consume(
        &self,
        decider_ref: &str,
        option: &str,
        decision_ref: &str,
        now: u64,
        target_revision: u64,
        target_digest: &str,
        scope_digest: &str,
    ) -> Result<Self, &'static str> {
        self.validate()?;
        if self.status != CompanyInboxCardStatus::Pending {
            return Err("company_inbox_double_consumption");
        }
        if now >= self.expires_at {
            return Err("company_inbox_target_expired");
        }
        if decider_ref != self.decider_ref {
            return Err("company_inbox_wrong_decider");
        }
        if option.trim().is_empty() || !self.options.iter().any(|allowed| allowed == option) {
            return Err("company_inbox_option_invalid");
        }
        if target_revision != self.target_revision
            || target_digest != self.target_digest
            || scope_digest != self.scope_digest
        {
            return Err("company_inbox_stale_target");
        }
        required(decision_ref, "company_inbox_decision_ref_required")?;
        let mut next = self.clone();
        next.status = CompanyInboxCardStatus::Decided;
        next.decision_ref = Some(decision_ref.to_owned());
        next.decided_by = Some(decider_ref.to_owned());
        next.digest = next.canonical_digest();
        Ok(next)
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "card_id": self.card_id,
            "task_id": self.task_id,
            "kind": self.kind,
            "project_id": self.project_id,
            "target_kind": self.target_kind,
            "target_id": self.target_id,
            "target_revision": self.target_revision,
            "target_digest": self.target_digest,
            "scope_digest": self.scope_digest,
            "decider_ref": self.decider_ref,
            "options": self.options,
            "evidence_refs": self.evidence_refs,
            "created_at": self.created_at,
            "due_at": self.due_at,
            "expires_at": self.expires_at,
            "status": self.status,
            "decision_ref": self.decision_ref,
            "decided_by": self.decided_by,
        }))
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompanyInboxLedger {
    pub cards: BTreeMap<String, CompanyInboxCard>,
}

impl CompanyInboxLedger {
    pub fn publish(&mut self, card: CompanyInboxCard) -> Result<(), &'static str> {
        card.validate()?;
        if let Some(existing) = self.cards.get(&card.card_id) {
            if existing.digest == card.digest {
                return Ok(());
            }
            return Err("company_inbox_duplicate_digest_mismatch");
        }
        self.cards.insert(card.card_id.clone(), card);
        Ok(())
    }

    pub fn consume(
        &mut self,
        card_id: &str,
        decider_ref: &str,
        option: &str,
        decision_ref: &str,
        now: u64,
        target_revision: u64,
        target_digest: &str,
        scope_digest: &str,
    ) -> Result<(), &'static str> {
        let card = self
            .cards
            .get(card_id)
            .ok_or("company_inbox_card_not_found")?
            .clone();
        let next = card.consume(
            decider_ref,
            option,
            decision_ref,
            now,
            target_revision,
            target_digest,
            scope_digest,
        )?;
        self.cards.insert(card_id.to_owned(), next);
        Ok(())
    }

    pub fn pending_for(&self, decider_ref: &str, now: u64) -> Vec<&CompanyInboxCard> {
        let mut cards = self
            .cards
            .values()
            .filter(|card| {
                card.decider_ref == decider_ref
                    && card.status == CompanyInboxCardStatus::Pending
                    && now < card.expires_at
            })
            .collect::<Vec<_>>();
        cards.sort_by_key(|card| (card.due_at, card.created_at, card.card_id.as_str()));
        cards
    }
}
