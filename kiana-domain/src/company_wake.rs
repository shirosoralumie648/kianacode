//! Durable Company intent wakeup and dispatch-consumption contracts.
//!
//! A wake is a durable projection of a committed process intent.  Delivery is only a hint; the
//! ledger is rebuilt from committed intent/source-cursor facts after restart.  Claiming a wake
//! never grants authority, and a crash after an intent has been claimed is preserved as Unknown
//! until an explicit reconciliation receipt arrives.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const COMPANY_WAKE_SCHEMA: &str = "kiana.company-wake.v1";
pub const COMPANY_WAKE_LEDGER_SCHEMA: &str = "kiana.company-wake-ledger.v1";
pub const COMPANY_DISPATCH_RECEIPT_SCHEMA: &str = "kiana.company-dispatch-receipt.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyWakeKind {
    IntentReady,
    HumanDecision,
    Reconcile,
    Retry,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompanyWakeStatus {
    Pending,
    Claimed,
    Consumed,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyDispatchReceipt {
    pub schema: String,
    pub receipt_id: String,
    pub intent_id: String,
    pub dispatch_id: String,
    pub status: CompanyWakeStatus,
    pub effect_known: bool,
    pub source_cursor: u64,
    pub observed_at: u64,
    pub digest: String,
}

impl CompanyDispatchReceipt {
    pub fn new(
        receipt_id: impl Into<String>,
        intent_id: impl Into<String>,
        dispatch_id: impl Into<String>,
        status: CompanyWakeStatus,
        effect_known: bool,
        source_cursor: u64,
        observed_at: u64,
    ) -> Result<Self, &'static str> {
        let mut receipt = Self {
            schema: COMPANY_DISPATCH_RECEIPT_SCHEMA.to_owned(),
            receipt_id: receipt_id.into(),
            intent_id: intent_id.into(),
            dispatch_id: dispatch_id.into(),
            status,
            effect_known,
            source_cursor,
            observed_at,
            digest: String::new(),
        };
        receipt.digest = receipt.canonical_digest();
        receipt.validate()?;
        Ok(receipt)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_DISPATCH_RECEIPT_SCHEMA
            || self.source_cursor == 0
            || self.observed_at == 0
        {
            return Err("company_dispatch_receipt_invalid");
        }
        required(&self.receipt_id, "company_dispatch_receipt_id_required")?;
        required(&self.intent_id, "company_dispatch_receipt_intent_required")?;
        required(
            &self.dispatch_id,
            "company_dispatch_receipt_dispatch_required",
        )?;
        if self.status == CompanyWakeStatus::Consumed && !self.effect_known {
            return Err("company_dispatch_consumed_effect_unknown");
        }
        if self.digest != self.canonical_digest() {
            return Err("company_dispatch_receipt_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "receipt_id": self.receipt_id,
            "intent_id": self.intent_id,
            "dispatch_id": self.dispatch_id,
            "status": self.status,
            "effect_known": self.effect_known,
            "source_cursor": self.source_cursor,
            "observed_at": self.observed_at,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyWake {
    pub schema: String,
    pub wake_id: String,
    pub intent_id: String,
    pub process_id: String,
    pub kind: CompanyWakeKind,
    pub source_cursor: u64,
    pub trigger_digest: String,
    pub due_at: u64,
    pub status: CompanyWakeStatus,
    pub claim_owner: Option<String>,
    pub claim_id: Option<String>,
    pub dispatch_id: Option<String>,
    pub attempts: u32,
    pub receipt_id: Option<String>,
    pub digest: String,
}

impl CompanyWake {
    pub fn new(
        intent_id: impl Into<String>,
        process_id: impl Into<String>,
        kind: CompanyWakeKind,
        source_cursor: u64,
        trigger_digest: impl Into<String>,
        due_at: u64,
    ) -> Result<Self, &'static str> {
        let intent_id = intent_id.into();
        let mut wake = Self {
            schema: COMPANY_WAKE_SCHEMA.to_owned(),
            wake_id: format!("wake:{intent_id}"),
            intent_id,
            process_id: process_id.into(),
            kind,
            source_cursor,
            trigger_digest: trigger_digest.into(),
            due_at,
            status: CompanyWakeStatus::Pending,
            claim_owner: None,
            claim_id: None,
            dispatch_id: None,
            attempts: 0,
            receipt_id: None,
            digest: String::new(),
        };
        wake.digest = wake.canonical_digest();
        wake.validate()?;
        Ok(wake)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_WAKE_SCHEMA || self.source_cursor == 0 || self.due_at == 0 {
            return Err("company_wake_invalid");
        }
        required(&self.wake_id, "company_wake_id_required")?;
        required(&self.intent_id, "company_wake_intent_required")?;
        required(&self.process_id, "company_wake_process_required")?;
        digest(&self.trigger_digest, "company_wake_trigger_digest_invalid")?;
        if self.status == CompanyWakeStatus::Claimed
            && (self.claim_owner.is_none() || self.claim_id.is_none())
        {
            return Err("company_wake_claim_required");
        }
        if self.status == CompanyWakeStatus::Consumed && self.receipt_id.is_none() {
            return Err("company_wake_receipt_required");
        }
        if self.status == CompanyWakeStatus::Unknown && self.receipt_id.is_some() {
            return Err("company_wake_unknown_receipt_conflict");
        }
        if self.digest != self.canonical_digest() {
            return Err("company_wake_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "wake_id": self.wake_id,
            "intent_id": self.intent_id,
            "process_id": self.process_id,
            "kind": self.kind,
            "source_cursor": self.source_cursor,
            "trigger_digest": self.trigger_digest,
            "due_at": self.due_at,
            "status": self.status,
            "claim_owner": self.claim_owner,
            "claim_id": self.claim_id,
            "dispatch_id": self.dispatch_id,
            "attempts": self.attempts,
            "receipt_id": self.receipt_id,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompanyWakeLedger {
    pub schema: String,
    pub cursor: u64,
    #[serde(default)]
    pub wakes: BTreeMap<String, CompanyWake>,
}

impl Default for CompanyWakeLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl CompanyWakeLedger {
    pub fn new() -> Self {
        Self {
            schema: COMPANY_WAKE_LEDGER_SCHEMA.to_owned(),
            cursor: 0,
            wakes: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != COMPANY_WAKE_LEDGER_SCHEMA {
            return Err("company_wake_ledger_schema_invalid");
        }
        for (key, wake) in &self.wakes {
            wake.validate()?;
            if key != &wake.intent_id {
                return Err("company_wake_key_mismatch");
            }
            if wake.source_cursor > self.cursor {
                return Err("company_wake_cursor_regression");
            }
        }
        Ok(())
    }

    pub fn enqueue(&mut self, wake: CompanyWake) -> Result<(), &'static str> {
        wake.validate()?;
        if let Some(existing) = self.wakes.get(&wake.intent_id) {
            if existing.trigger_digest == wake.trigger_digest
                && existing.process_id == wake.process_id
            {
                self.cursor = self.cursor.max(wake.source_cursor);
                return Ok(());
            }
            return Err("company_wake_intent_conflict");
        }
        self.cursor = self.cursor.max(wake.source_cursor);
        self.wakes.insert(wake.intent_id.clone(), wake);
        self.schema = COMPANY_WAKE_LEDGER_SCHEMA.to_owned();
        Ok(())
    }

    pub fn scan_committed_intents(
        &mut self,
        intents: impl IntoIterator<Item = (u64, CompanyWake)>,
    ) -> Result<u64, &'static str> {
        for (cursor, mut wake) in intents {
            if cursor == 0 {
                return Err("company_wake_source_cursor_invalid");
            }
            wake.source_cursor = cursor;
            wake.digest = wake.canonical_digest();
            self.enqueue(wake)?;
        }
        Ok(self.cursor)
    }

    pub fn claim(
        &mut self,
        intent_id: &str,
        owner: impl Into<String>,
        claim_id: impl Into<String>,
        now_ms: u64,
    ) -> Result<CompanyWake, &'static str> {
        let wake = self
            .wakes
            .get_mut(intent_id)
            .ok_or("company_wake_not_found")?;
        if now_ms < wake.due_at {
            return Err("company_wake_not_due");
        }
        if wake.status != CompanyWakeStatus::Pending {
            return Err("company_wake_not_pending");
        }
        let owner = owner.into();
        let claim_id = claim_id.into();
        required(&owner, "company_wake_owner_required")?;
        required(&claim_id, "company_wake_claim_id_required")?;
        wake.status = CompanyWakeStatus::Claimed;
        wake.claim_owner = Some(owner);
        wake.claim_id = Some(claim_id);
        wake.attempts = wake.attempts.saturating_add(1);
        wake.digest = wake.canonical_digest();
        wake.validate()?;
        Ok(wake.clone())
    }

    pub fn consume(
        &mut self,
        intent_id: &str,
        claim_id: &str,
        receipt: CompanyDispatchReceipt,
    ) -> Result<(), &'static str> {
        receipt.validate()?;
        let wake = self
            .wakes
            .get_mut(intent_id)
            .ok_or("company_wake_not_found")?;
        if receipt.intent_id != wake.intent_id || wake.claim_id.as_deref() != Some(claim_id) {
            return Err("company_wake_claim_mismatch");
        }
        if wake.status == CompanyWakeStatus::Consumed {
            if wake.receipt_id.as_deref() == Some(receipt.receipt_id.as_str()) {
                return Ok(());
            }
            return Err("company_wake_already_consumed");
        }
        if wake.status != CompanyWakeStatus::Claimed {
            return Err("company_wake_not_claimed");
        }
        if receipt.status == CompanyWakeStatus::Unknown || !receipt.effect_known {
            wake.status = CompanyWakeStatus::Unknown;
            wake.dispatch_id = Some(receipt.dispatch_id);
            wake.digest = wake.canonical_digest();
            return wake.validate();
        }
        if receipt.status != CompanyWakeStatus::Consumed {
            return Err("company_wake_receipt_status_invalid");
        }
        wake.status = CompanyWakeStatus::Consumed;
        wake.dispatch_id = Some(receipt.dispatch_id);
        wake.receipt_id = Some(receipt.receipt_id);
        wake.digest = wake.canonical_digest();
        wake.validate()
    }

    pub fn mark_unknown(
        &mut self,
        intent_id: &str,
        claim_id: &str,
        dispatch_id: impl Into<String>,
    ) -> Result<(), &'static str> {
        let wake = self
            .wakes
            .get_mut(intent_id)
            .ok_or("company_wake_not_found")?;
        if wake.status != CompanyWakeStatus::Claimed || wake.claim_id.as_deref() != Some(claim_id) {
            return Err("company_wake_claim_mismatch");
        }
        let dispatch_id = dispatch_id.into();
        required(&dispatch_id, "company_wake_dispatch_required")?;
        wake.status = CompanyWakeStatus::Unknown;
        wake.dispatch_id = Some(dispatch_id);
        wake.digest = wake.canonical_digest();
        wake.validate()
    }

    pub fn pending(&self, now_ms: u64) -> Vec<&CompanyWake> {
        let mut pending = self
            .wakes
            .values()
            .filter(|wake| wake.status == CompanyWakeStatus::Pending && wake.due_at <= now_ms)
            .collect::<Vec<_>>();
        pending.sort_by(|left, right| {
            left.due_at
                .cmp(&right.due_at)
                .then_with(|| left.intent_id.cmp(&right.intent_id))
        });
        pending
    }
}
