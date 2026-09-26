//! Packet attempt, lease epoch and reclaim fencing for CompanyOS.
//!
//! The legacy `PacketClaim` remains wire-compatible.  CO-19 adds a separate, append-only attempt
//! history so a short lease cannot be mistaken for the long-lived packet identity.  A new claim
//! is admitted only after the previous epoch is terminal and its effect is known to be absent or
//! stopped.  This module is pure state validation; the ControlPlane/EventLog owns CAS commit.

use crate::{json_digest, AttemptId, CellId, RequestId, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const PACKET_ATTEMPT_SCHEMA: &str = "kiana.packet-attempt.v1";
pub const PACKET_ATTEMPT_LEDGER_SCHEMA: &str = "kiana.packet-attempt-ledger.v1";

fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field);
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(field);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketAttemptStatus {
    Active,
    Fenced,
    Succeeded,
    Failed,
    Cancelled,
    ResultUnknown,
}

impl PacketAttemptStatus {
    pub fn terminal(self) -> bool {
        !matches!(self, Self::Active)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PacketEffectState {
    NotStarted,
    Started,
    Stopped,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketAttempt {
    pub schema: String,
    pub attempt_id: AttemptId,
    pub packet_id: String,
    pub packet_version: u64,
    pub worker_cell_id: CellId,
    pub worker_session_id: SessionId,
    pub execution_request_id: RequestId,
    pub epoch: u64,
    pub issued_at: u64,
    pub lease_expires_at: u64,
    pub status: PacketAttemptStatus,
    pub effect: PacketEffectState,
    pub stop_confirmed: bool,
    pub terminal_at: Option<u64>,
    pub result_digest: Option<String>,
    pub digest: String,
}

impl PacketAttempt {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        packet_id: impl Into<String>,
        packet_version: u64,
        worker_cell_id: CellId,
        worker_session_id: SessionId,
        execution_request_id: RequestId,
        epoch: u64,
        issued_at: u64,
        lease_expires_at: u64,
    ) -> Result<Self, &'static str> {
        let mut attempt = Self {
            schema: PACKET_ATTEMPT_SCHEMA.to_owned(),
            attempt_id: AttemptId::new(),
            packet_id: packet_id.into(),
            packet_version,
            worker_cell_id,
            worker_session_id,
            execution_request_id,
            epoch,
            issued_at,
            lease_expires_at,
            status: PacketAttemptStatus::Active,
            effect: PacketEffectState::NotStarted,
            stop_confirmed: false,
            terminal_at: None,
            result_digest: None,
            digest: String::new(),
        };
        attempt.digest = attempt.canonical_digest();
        attempt.validate()?;
        Ok(attempt)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PACKET_ATTEMPT_SCHEMA
            || self.attempt_id.as_uuid().is_nil()
            || self.worker_cell_id.as_uuid().is_nil()
            || self.execution_request_id.as_uuid().is_nil()
        {
            return Err("packet_attempt_identity_invalid");
        }
        required(&self.packet_id, "packet_attempt_packet_required")?;
        if self.packet_version == 0
            || self.epoch == 0
            || self.issued_at == 0
            || self.lease_expires_at <= self.issued_at
            || self.worker_session_id.is_empty()
        {
            return Err("packet_attempt_window_invalid");
        }
        if self.stop_confirmed && self.effect != PacketEffectState::Stopped {
            return Err("packet_attempt_stop_effect_mismatch");
        }
        if self.effect == PacketEffectState::Unknown
            && self.status != PacketAttemptStatus::ResultUnknown
        {
            return Err("packet_attempt_unknown_effect_status_mismatch");
        }
        if self.status == PacketAttemptStatus::ResultUnknown
            && self.effect != PacketEffectState::Unknown
        {
            return Err("packet_attempt_unknown_effect_required");
        }
        if self.status.terminal() && self.terminal_at.is_none() {
            return Err("packet_attempt_terminal_time_required");
        }
        if let Some(result) = &self.result_digest {
            digest(result, "packet_attempt_result_digest_invalid")?;
        }
        if self.digest != self.canonical_digest() {
            return Err("packet_attempt_digest_mismatch");
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "attempt_id": self.attempt_id,
            "packet_id": self.packet_id,
            "packet_version": self.packet_version,
            "worker_cell_id": self.worker_cell_id,
            "worker_session_id": self.worker_session_id,
            "execution_request_id": self.execution_request_id,
            "epoch": self.epoch,
            "issued_at": self.issued_at,
            "lease_expires_at": self.lease_expires_at,
            "status": self.status,
            "effect": self.effect,
            "stop_confirmed": self.stop_confirmed,
            "terminal_at": self.terminal_at,
            "result_digest": self.result_digest,
        }))
    }

    pub fn active_at(&self, now_ms: u64) -> bool {
        self.status == PacketAttemptStatus::Active && now_ms < self.lease_expires_at
    }

    fn ensure_token(
        &self,
        attempt_id: AttemptId,
        epoch: u64,
        execution_request_id: RequestId,
    ) -> Result<(), &'static str> {
        if self.attempt_id != attempt_id
            || self.epoch != epoch
            || self.execution_request_id != execution_request_id
        {
            return Err("packet_attempt_fence_mismatch");
        }
        Ok(())
    }

    pub fn mark_started(
        &mut self,
        attempt_id: AttemptId,
        epoch: u64,
        execution_request_id: RequestId,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.ensure_token(attempt_id, epoch, execution_request_id)?;
        if !self.active_at(now_ms) {
            return Err("packet_attempt_expired_or_terminal");
        }
        self.effect = PacketEffectState::Started;
        self.digest = self.canonical_digest();
        self.validate()
    }

    pub fn confirm_stopped(
        &mut self,
        attempt_id: AttemptId,
        epoch: u64,
        execution_request_id: RequestId,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.ensure_token(attempt_id, epoch, execution_request_id)?;
        if self.status != PacketAttemptStatus::Active
            || (now_ms < self.lease_expires_at && self.effect == PacketEffectState::Started)
        {
            return Err("packet_attempt_stop_not_confirmable");
        }
        self.effect = PacketEffectState::Stopped;
        self.stop_confirmed = true;
        self.digest = self.canonical_digest();
        self.validate()
    }

    pub fn complete(
        &mut self,
        attempt_id: AttemptId,
        epoch: u64,
        execution_request_id: RequestId,
        status: PacketAttemptStatus,
        result_digest: Option<String>,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.ensure_token(attempt_id, epoch, execution_request_id)?;
        if !self.active_at(now_ms) {
            return Err("packet_attempt_expired_or_terminal");
        }
        if !matches!(
            status,
            PacketAttemptStatus::Succeeded
                | PacketAttemptStatus::Failed
                | PacketAttemptStatus::Cancelled
        ) {
            return Err("packet_attempt_terminal_status_invalid");
        }
        if let Some(result) = &result_digest {
            digest(result, "packet_attempt_result_digest_invalid")?;
        }
        self.status = status;
        self.terminal_at = Some(now_ms);
        self.result_digest = result_digest;
        self.digest = self.canonical_digest();
        self.validate()
    }

    /// Fence an expired attempt. Unknown/started effects become ResultUnknown and block reuse;
    /// never-started or confirmed-stopped attempts become safely reclaimable.
    pub fn fence_expired(&mut self, now_ms: u64) -> Result<(), &'static str> {
        if self.status != PacketAttemptStatus::Active {
            return Err("packet_attempt_not_active");
        }
        if now_ms < self.lease_expires_at {
            return Err("packet_attempt_not_expired");
        }
        if self.effect == PacketEffectState::Started || !self.stop_confirmed {
            self.effect = PacketEffectState::Unknown;
            self.status = PacketAttemptStatus::ResultUnknown;
        } else {
            self.status = PacketAttemptStatus::Fenced;
        }
        self.terminal_at = Some(now_ms);
        self.digest = self.canonical_digest();
        self.validate()
    }

    pub fn safely_reclaimable(&self) -> bool {
        matches!(self.status, PacketAttemptStatus::Fenced)
            && matches!(
                self.effect,
                PacketEffectState::NotStarted | PacketEffectState::Stopped
            )
            && self.stop_confirmed
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PacketAttemptLedger {
    pub schema: String,
    #[serde(default)]
    pub attempts: BTreeMap<String, Vec<PacketAttempt>>,
}

impl Default for PacketAttemptLedger {
    fn default() -> Self {
        Self::new()
    }
}

impl PacketAttemptLedger {
    pub fn new() -> Self {
        Self {
            schema: PACKET_ATTEMPT_LEDGER_SCHEMA.to_owned(),
            attempts: BTreeMap::new(),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != PACKET_ATTEMPT_LEDGER_SCHEMA {
            return Err("packet_attempt_ledger_schema_invalid");
        }
        for (packet_id, history) in &self.attempts {
            required(packet_id, "packet_attempt_packet_required")?;
            let mut previous_epoch = 0;
            for attempt in history {
                attempt.validate()?;
                if attempt.packet_id != *packet_id || attempt.epoch <= previous_epoch {
                    return Err("packet_attempt_epoch_order_invalid");
                }
                previous_epoch = attempt.epoch;
            }
            if history
                .iter()
                .filter(|attempt| attempt.status == PacketAttemptStatus::Active)
                .count()
                > 1
            {
                return Err("packet_attempt_multiple_active_invalid");
            }
        }
        Ok(())
    }

    pub fn claim(
        &mut self,
        packet_id: impl Into<String>,
        packet_version: u64,
        worker_cell_id: CellId,
        worker_session_id: SessionId,
        execution_request_id: RequestId,
        now_ms: u64,
        lease_expires_at: u64,
    ) -> Result<PacketAttempt, &'static str> {
        let packet_id = packet_id.into();
        required(&packet_id, "packet_attempt_packet_required")?;
        if packet_version == 0 || now_ms == 0 || lease_expires_at <= now_ms {
            return Err("packet_attempt_claim_window_invalid");
        }
        let history = self.attempts.entry(packet_id.clone()).or_default();
        if let Some(current) = history.last() {
            if current.status == PacketAttemptStatus::Active {
                if current.worker_session_id == worker_session_id
                    && current.execution_request_id == execution_request_id
                {
                    return Ok(current.clone());
                }
                return Err("packet_attempt_already_claimed");
            }
            if current.status == PacketAttemptStatus::ResultUnknown
                || current.effect == PacketEffectState::Unknown
            {
                return Err("packet_attempt_result_unknown_requires_reconcile");
            }
            if current.status != PacketAttemptStatus::Fenced && !current.safely_reclaimable() {
                return Err("packet_attempt_previous_not_reclaimable");
            }
        }
        let epoch = history
            .last()
            .map_or(1, |attempt| attempt.epoch.saturating_add(1));
        if epoch == 0 {
            return Err("packet_attempt_epoch_exhausted");
        }
        let attempt = PacketAttempt::new(
            packet_id.clone(),
            packet_version,
            worker_cell_id,
            worker_session_id,
            execution_request_id,
            epoch,
            now_ms,
            lease_expires_at,
        )?;
        history.push(attempt.clone());
        self.schema = PACKET_ATTEMPT_LEDGER_SCHEMA.to_owned();
        Ok(attempt)
    }

    pub fn fence_expired(
        &mut self,
        packet_id: &str,
        attempt_id: AttemptId,
        epoch: u64,
        now_ms: u64,
    ) -> Result<PacketAttemptStatus, &'static str> {
        self.ensure_current(packet_id, attempt_id, epoch)?;
        let attempt = self
            .attempt_mut(packet_id, attempt_id, epoch)
            .ok_or("packet_attempt_not_found")?;
        attempt.fence_expired(now_ms)?;
        Ok(attempt.status)
    }

    pub fn mark_started(
        &mut self,
        packet_id: &str,
        attempt_id: AttemptId,
        epoch: u64,
        execution_request_id: RequestId,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.ensure_current(packet_id, attempt_id, epoch)?;
        self.attempt_mut(packet_id, attempt_id, epoch)
            .ok_or("packet_attempt_not_found")?
            .mark_started(attempt_id, epoch, execution_request_id, now_ms)
    }

    pub fn confirm_stopped(
        &mut self,
        packet_id: &str,
        attempt_id: AttemptId,
        epoch: u64,
        execution_request_id: RequestId,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.ensure_current(packet_id, attempt_id, epoch)?;
        self.attempt_mut(packet_id, attempt_id, epoch)
            .ok_or("packet_attempt_not_found")?
            .confirm_stopped(attempt_id, epoch, execution_request_id, now_ms)
    }

    pub fn complete(
        &mut self,
        packet_id: &str,
        attempt_id: AttemptId,
        epoch: u64,
        execution_request_id: RequestId,
        status: PacketAttemptStatus,
        result_digest: Option<String>,
        now_ms: u64,
    ) -> Result<(), &'static str> {
        self.ensure_current(packet_id, attempt_id, epoch)?;
        self.attempt_mut(packet_id, attempt_id, epoch)
            .ok_or("packet_attempt_not_found")?
            .complete(
                attempt_id,
                epoch,
                execution_request_id,
                status,
                result_digest,
                now_ms,
            )
    }

    pub fn get(&self, packet_id: &str, attempt_id: AttemptId) -> Option<&PacketAttempt> {
        self.attempts.get(packet_id).and_then(|history| {
            history
                .iter()
                .find(|attempt| attempt.attempt_id == attempt_id)
        })
    }

    pub fn history(&self, packet_id: &str) -> &[PacketAttempt] {
        self.attempts.get(packet_id).map_or(&[], Vec::as_slice)
    }

    fn attempt_mut(
        &mut self,
        packet_id: &str,
        attempt_id: AttemptId,
        epoch: u64,
    ) -> Option<&mut PacketAttempt> {
        self.attempts.get_mut(packet_id).and_then(|history| {
            history
                .iter_mut()
                .find(|attempt| attempt.attempt_id == attempt_id && attempt.epoch == epoch)
        })
    }

    fn ensure_current(
        &self,
        packet_id: &str,
        attempt_id: AttemptId,
        epoch: u64,
    ) -> Result<(), &'static str> {
        let current = self
            .attempts
            .get(packet_id)
            .and_then(|history| history.last())
            .ok_or("packet_attempt_not_found")?;
        if current.attempt_id != attempt_id || current.epoch != epoch {
            return Err("packet_attempt_fence_mismatch");
        }
        Ok(())
    }
}
