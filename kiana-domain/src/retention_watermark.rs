//! Bounded retention sweep and watermark contract.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const RETENTION_WATERMARK_SCHEMA: &str = "kiana.retention-watermark.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetentionWatermarkState {
    Planned,
    Committed,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionWatermark {
    pub schema: String,
    pub store_id: String,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub source_cursor: u64,
    pub upper_bound_cursor: u64,
    pub next_cursor: u64,
    pub batch_limit: u32,
    pub legal_hold_checked: bool,
    pub tombstone_committed: bool,
    pub state: RetentionWatermarkState,
    pub reason: Option<String>,
    pub watermark_digest: String,
}

impl RetentionWatermark {
    pub fn planned(
        store_id: impl Into<String>,
        policy_revision: u64,
        data_epoch: u64,
        source_cursor: u64,
        upper_bound_cursor: u64,
        batch_limit: u32,
    ) -> Result<Self, String> {
        let mut watermark = Self {
            schema: RETENTION_WATERMARK_SCHEMA.to_owned(),
            store_id: store_id.into(),
            policy_revision,
            data_epoch,
            source_cursor,
            upper_bound_cursor,
            next_cursor: source_cursor,
            batch_limit,
            legal_hold_checked: false,
            tombstone_committed: false,
            state: RetentionWatermarkState::Planned,
            reason: None,
            watermark_digest: String::new(),
        };
        watermark.watermark_digest = watermark.digest();
        watermark.validate()?;
        Ok(watermark)
    }

    pub fn commit(
        mut self,
        legal_hold_checked: bool,
        tombstone_committed: bool,
    ) -> Result<Self, String> {
        self.validate()?;
        if !legal_hold_checked || !tombstone_committed {
            self.state = RetentionWatermarkState::Blocked;
            self.reason = Some("retention_tombstone_or_hold_gate_missing".to_owned());
            self.watermark_digest = self.digest();
            self.validate()?;
            return Ok(self);
        }
        self.legal_hold_checked = true;
        self.tombstone_committed = true;
        self.state = RetentionWatermarkState::Committed;
        self.watermark_digest = self.digest();
        self.validate()?;
        Ok(self)
    }

    pub fn advance(&self, next_cursor: u64) -> Result<Self, String> {
        self.validate()?;
        if self.state != RetentionWatermarkState::Committed
            || next_cursor < self.next_cursor
            || next_cursor > self.upper_bound_cursor
        {
            return Err("retention_watermark_advance_invalid".to_owned());
        }
        let mut next = self.clone();
        next.next_cursor = next_cursor;
        next.watermark_digest = next.digest();
        next.validate()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RETENTION_WATERMARK_SCHEMA
            || self.store_id.trim().is_empty()
            || self.policy_revision == 0
            || self.data_epoch == 0
            || self.source_cursor == 0
            || self.upper_bound_cursor < self.source_cursor
            || self.next_cursor < self.source_cursor
            || self.next_cursor > self.upper_bound_cursor
            || self.batch_limit == 0
            || self.batch_limit > 10_000
        {
            return Err("retention_watermark_header_invalid".to_owned());
        }
        if self.state == RetentionWatermarkState::Committed
            && (!self.legal_hold_checked || !self.tombstone_committed || self.reason.is_some())
        {
            return Err("retention_watermark_commit_gate_invalid".to_owned());
        }
        if self.state == RetentionWatermarkState::Blocked
            && self.reason.as_deref().is_none_or(str::is_empty)
        {
            return Err("retention_watermark_block_reason_required".to_owned());
        }
        let Some(hex) = self.watermark_digest.strip_prefix("sha256:") else {
            return Err("retention_watermark_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("retention_watermark_digest_invalid".to_owned());
        }
        if self.watermark_digest != self.digest() {
            return Err("retention_watermark_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "store_id": self.store_id,
            "policy_revision": self.policy_revision,
            "data_epoch": self.data_epoch,
            "source_cursor": self.source_cursor,
            "upper_bound_cursor": self.upper_bound_cursor,
            "next_cursor": self.next_cursor,
            "batch_limit": self.batch_limit,
            "legal_hold_checked": self.legal_hold_checked,
            "tombstone_committed": self.tombstone_committed,
            "state": self.state,
            "reason": self.reason,
        }))
    }
}
