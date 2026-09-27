//! SW-05 atomic swarm admission contract.
//!
//! This bounded ledger is a deterministic source contract for one admission boundary.  It checks
//! budget, path/data locks, claim, grant, supervision and intent before mutating any reservation.
//! A replay with the same idempotency key and payload returns the original receipt; a payload
//! conflict or active work fingerprint is rejected.  It does not replace the EventLog/CAS
//! implementation used by the runtime and therefore cannot prove cross-process durability.

use crate::{json_digest, DispatchIntentId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SWARM_ADMISSION_SCHEMA: &str = "kiana.swarm-admission.v1";
pub const SWARM_ADMISSION_RECEIPT_SCHEMA: &str = "kiana.swarm-admission-receipt.v1";
const MAX_KEYS: usize = 64;
const MAX_TEXT: usize = 512;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmAdmissionResource {
    Budget,
    PathLock,
    DataLock,
    Claim,
    Grant,
    Supervision,
    Intent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmAdmissionRequest {
    pub schema: String,
    pub idempotency_key: String,
    pub work_fingerprint: String,
    pub payload_digest: String,
    pub budget_units: u64,
    pub path_locks: Vec<String>,
    pub data_locks: Vec<String>,
    pub claim_key: String,
    pub grant_digest: String,
    pub supervision_key: String,
    pub intent_key: String,
    pub authority_epoch: u64,
    pub config_revision: String,
    pub policy_revision: String,
}

impl SwarmAdmissionRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_ADMISSION_SCHEMA
            || !valid_text(&self.idempotency_key)
            || !valid_digest(&self.work_fingerprint)
            || !valid_digest(&self.payload_digest)
            || self.budget_units == 0
            || self.path_locks.is_empty()
            || self.path_locks.len() > MAX_KEYS
            || self.data_locks.is_empty()
            || self.data_locks.len() > MAX_KEYS
            || !valid_text(&self.claim_key)
            || !valid_digest(&self.grant_digest)
            || !valid_text(&self.supervision_key)
            || !valid_text(&self.intent_key)
            || self.authority_epoch == 0
            || !valid_text(&self.config_revision)
            || !valid_text(&self.policy_revision)
            || !canonical_keys(&self.path_locks)
            || !canonical_keys(&self.data_locks)
            || self.path_locks.iter().any(|key| !valid_lock(key))
            || self.data_locks.iter().any(|key| !valid_lock(key))
        {
            return Err("swarm_admission_request_invalid");
        }
        if self
            .path_locks
            .iter()
            .any(|path| self.data_locks.contains(path))
        {
            return Err("swarm_admission_path_data_overlap");
        }
        Ok(())
    }

    pub fn request_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "idempotency_key": self.idempotency_key,
            "work_fingerprint": self.work_fingerprint,
            "payload_digest": self.payload_digest,
            "budget_units": self.budget_units,
            "path_locks": self.path_locks,
            "data_locks": self.data_locks,
            "claim_key": self.claim_key,
            "grant_digest": self.grant_digest,
            "supervision_key": self.supervision_key,
            "intent_key": self.intent_key,
            "authority_epoch": self.authority_epoch,
            "config_revision": self.config_revision,
            "policy_revision": self.policy_revision,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmAdmissionOutcome {
    Reserved,
    Replayed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmAdmissionReceipt {
    pub schema: String,
    pub admission_id: DispatchIntentId,
    pub outcome: SwarmAdmissionOutcome,
    pub request_digest: String,
    pub work_fingerprint: String,
    pub authority_epoch: u64,
    pub config_revision: String,
    pub policy_revision: String,
    pub receipt_digest: String,
}

impl SwarmAdmissionReceipt {
    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "admission_id": self.admission_id,
            "outcome": self.outcome,
            "request_digest": self.request_digest,
            "work_fingerprint": self.work_fingerprint,
            "authority_epoch": self.authority_epoch,
            "config_revision": self.config_revision,
            "policy_revision": self.policy_revision,
        }))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != SWARM_ADMISSION_RECEIPT_SCHEMA
            || self.admission_id.as_uuid().is_nil()
            || !valid_digest(&self.request_digest)
            || !valid_digest(&self.work_fingerprint)
            || self.authority_epoch == 0
            || !valid_text(&self.config_revision)
            || !valid_text(&self.policy_revision)
            || !valid_digest(&self.receipt_digest)
            || self.receipt_digest != self.digest()
        {
            return Err("swarm_admission_receipt_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SwarmAdmissionLedger {
    pub available_budget: u64,
    pub revision: u64,
    idempotency: BTreeMap<String, (String, SwarmAdmissionReceipt)>,
    active_fingerprints: BTreeSet<String>,
    path_locks: BTreeSet<String>,
    data_locks: BTreeSet<String>,
    claims: BTreeSet<String>,
    grants: BTreeSet<String>,
    supervision: BTreeSet<String>,
    intents: BTreeSet<String>,
}

impl SwarmAdmissionLedger {
    pub fn new(available_budget: u64) -> Self {
        Self {
            available_budget,
            revision: 0,
            idempotency: BTreeMap::new(),
            active_fingerprints: BTreeSet::new(),
            path_locks: BTreeSet::new(),
            data_locks: BTreeSet::new(),
            claims: BTreeSet::new(),
            grants: BTreeSet::new(),
            supervision: BTreeSet::new(),
            intents: BTreeSet::new(),
        }
    }

    pub fn admit(
        &mut self,
        request: &SwarmAdmissionRequest,
    ) -> Result<SwarmAdmissionReceipt, &'static str> {
        self.admit_with_fault(request, None)
    }

    /// A fault is checked before any state mutation, which makes partial reservation impossible in
    /// this source contract.  The runtime must still implement the same ordering in its CAS.
    pub fn admit_with_fault(
        &mut self,
        request: &SwarmAdmissionRequest,
        fault: Option<SwarmAdmissionResource>,
    ) -> Result<SwarmAdmissionReceipt, &'static str> {
        request.validate()?;
        let request_digest = request.request_digest();
        if let Some((existing_digest, receipt)) = self.idempotency.get(&request.idempotency_key) {
            if existing_digest == &request_digest {
                let mut replay = receipt.clone();
                replay.outcome = SwarmAdmissionOutcome::Replayed;
                replay.receipt_digest = replay.digest();
                replay.validate()?;
                return Ok(replay);
            }
            return Err("swarm_admission_idempotency_conflict");
        }
        if self.active_fingerprints.contains(&request.work_fingerprint) {
            return Err("swarm_admission_work_fingerprint_active");
        }
        if request.budget_units > self.available_budget {
            return Err("swarm_admission_budget_exceeded");
        }
        if request
            .path_locks
            .iter()
            .any(|key| self.path_locks.contains(key))
        {
            return Err("swarm_admission_path_lock_conflict");
        }
        if request
            .data_locks
            .iter()
            .any(|key| self.data_locks.contains(key))
        {
            return Err("swarm_admission_data_lock_conflict");
        }
        for (resource, occupied) in [
            (SwarmAdmissionResource::Claim, &self.claims),
            (SwarmAdmissionResource::Grant, &self.grants),
            (SwarmAdmissionResource::Supervision, &self.supervision),
            (SwarmAdmissionResource::Intent, &self.intents),
        ] {
            let key = match resource {
                SwarmAdmissionResource::Claim => &request.claim_key,
                SwarmAdmissionResource::Grant => &request.grant_digest,
                SwarmAdmissionResource::Supervision => &request.supervision_key,
                SwarmAdmissionResource::Intent => &request.intent_key,
                _ => unreachable!(),
            };
            if occupied.contains(key) {
                return Err(match resource {
                    SwarmAdmissionResource::Claim => "swarm_admission_claim_conflict",
                    SwarmAdmissionResource::Grant => "swarm_admission_grant_conflict",
                    SwarmAdmissionResource::Supervision => "swarm_admission_supervision_conflict",
                    SwarmAdmissionResource::Intent => "swarm_admission_intent_conflict",
                    _ => "swarm_admission_resource_conflict",
                });
            }
        }
        if fault.is_some() {
            return Err("swarm_admission_atomic_preflight_fault");
        }

        // All checks above precede this single mutation block.
        self.available_budget -= request.budget_units;
        self.path_locks.extend(request.path_locks.iter().cloned());
        self.data_locks.extend(request.data_locks.iter().cloned());
        self.claims.insert(request.claim_key.clone());
        self.grants.insert(request.grant_digest.clone());
        self.supervision.insert(request.supervision_key.clone());
        self.intents.insert(request.intent_key.clone());
        self.active_fingerprints
            .insert(request.work_fingerprint.clone());
        self.revision = self.revision.saturating_add(1);
        let mut receipt = SwarmAdmissionReceipt {
            schema: SWARM_ADMISSION_RECEIPT_SCHEMA.to_owned(),
            admission_id: DispatchIntentId::new(),
            outcome: SwarmAdmissionOutcome::Reserved,
            request_digest: request_digest.clone(),
            work_fingerprint: request.work_fingerprint.clone(),
            authority_epoch: request.authority_epoch,
            config_revision: request.config_revision.clone(),
            policy_revision: request.policy_revision.clone(),
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt.validate()?;
        self.idempotency.insert(
            request.idempotency_key.clone(),
            (request_digest, receipt.clone()),
        );
        Ok(receipt)
    }
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn valid_lock(value: &str) -> bool {
    valid_text(value) && !value.contains(['/', '\\']) && value != "." && value != ".."
}

fn canonical_keys(values: &[String]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}
