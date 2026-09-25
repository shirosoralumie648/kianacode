//! Provider capacity, circuit and controlled-fallback contracts.
//!
//! These values describe admission state; they do not grant a model capability, consume a
//! ControlPlane permit or perform a provider request.  A durable quota adapter may persist the
//! same identities, while the in-memory fair queue below is intentionally bounded and disposable.

use crate::{
    json_digest, AttemptId, CapabilitySupport, ModelCapabilities, ModelRoute, QuotaGroupKey, RunId,
    SchemaVersion,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

pub const PROVIDER_CAPACITY_POLICY_SCHEMA: &str = "kiana.provider-capacity-policy.v1";
pub const PROVIDER_CAPACITY_USAGE_SCHEMA: &str = "kiana.provider-capacity-usage.v1";
pub const PROVIDER_CAPACITY_ENTRY_SCHEMA: &str = "kiana.provider-capacity-entry.v1";
pub const PROVIDER_CAPACITY_ADMISSION_SCHEMA: &str = "kiana.provider-capacity-admission.v1";
pub const PROVIDER_CAPACITY_LEASE_SCHEMA: &str = "kiana.provider-capacity-lease.v1";
pub const PROVIDER_CIRCUIT_SCHEMA: &str = "kiana.provider-circuit.v1";
pub const PROVIDER_FALLBACK_SCHEMA: &str = "kiana.provider-fallback.v1";
pub const PROVIDER_CAPACITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

pub const MAX_CAPACITY_QUEUE: u32 = 16_384;
pub const MAX_CAPACITY_CONCURRENCY: u32 = 4_096;
pub const MAX_CAPACITY_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
pub const MAX_ROUTE_ALLOWLIST: usize = 256;
pub const MAX_CAPACITY_OWNER: usize = 256;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains(['\0', '\r', '\n']) {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapacityPolicy {
    pub schema: String,
    pub version: SchemaVersion,
    pub group: QuotaGroupKey,
    pub max_concurrency: u32,
    pub max_queue: u32,
    pub requests_per_minute: u64,
    pub tokens_per_minute: u64,
    pub max_queue_wait_ms: u64,
    pub config_revision: String,
    pub policy_digest: String,
}

impl ProviderCapacityPolicy {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        group: QuotaGroupKey,
        max_concurrency: u32,
        max_queue: u32,
        requests_per_minute: u64,
        tokens_per_minute: u64,
        max_queue_wait_ms: u64,
        config_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let mut policy = Self {
            schema: PROVIDER_CAPACITY_POLICY_SCHEMA.to_owned(),
            version: PROVIDER_CAPACITY_VERSION,
            group,
            max_concurrency,
            max_queue,
            requests_per_minute,
            tokens_per_minute,
            max_queue_wait_ms,
            config_revision: config_revision.into(),
            policy_digest: String::new(),
        };
        policy.policy_digest = policy.digest();
        policy.validate()?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CAPACITY_POLICY_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_CAPACITY_VERSION)
            || self.max_concurrency == 0
            || self.max_concurrency > MAX_CAPACITY_CONCURRENCY
            || self.max_queue > MAX_CAPACITY_QUEUE
            || self.requests_per_minute == 0
            || self.tokens_per_minute == 0
            || self.max_queue_wait_ms == 0
            || self.max_queue_wait_ms > MAX_CAPACITY_WINDOW_MS
            || self.policy_digest != self.digest()
        {
            return Err("provider_capacity_policy_invalid".to_owned());
        }
        self.group.validate()?;
        required(
            &self.config_revision,
            "provider_capacity_config_revision",
            256,
        )?;
        digest(&self.policy_digest, "provider_capacity_policy_digest")
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "group": self.group,
            "max_concurrency": self.max_concurrency,
            "max_queue": self.max_queue,
            "requests_per_minute": self.requests_per_minute,
            "tokens_per_minute": self.tokens_per_minute,
            "max_queue_wait_ms": self.max_queue_wait_ms,
            "config_revision": self.config_revision,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapacityUsage {
    pub schema: String,
    pub version: SchemaVersion,
    pub window_start_unix_ms: u64,
    pub window_end_unix_ms: u64,
    pub admitted_requests: u64,
    pub admitted_tokens: u64,
    pub active_concurrency: u32,
    pub usage_digest: String,
}

impl ProviderCapacityUsage {
    pub fn new(window_start_unix_ms: u64, window_end_unix_ms: u64) -> Result<Self, String> {
        let mut usage = Self {
            schema: PROVIDER_CAPACITY_USAGE_SCHEMA.to_owned(),
            version: PROVIDER_CAPACITY_VERSION,
            window_start_unix_ms,
            window_end_unix_ms,
            admitted_requests: 0,
            admitted_tokens: 0,
            active_concurrency: 0,
            usage_digest: String::new(),
        };
        usage.usage_digest = usage.digest();
        usage.validate()?;
        Ok(usage)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CAPACITY_USAGE_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_CAPACITY_VERSION)
            || self.window_end_unix_ms <= self.window_start_unix_ms
            || self.window_end_unix_ms - self.window_start_unix_ms > MAX_CAPACITY_WINDOW_MS
            || self.usage_digest != self.digest()
        {
            return Err("provider_capacity_usage_invalid".to_owned());
        }
        digest(&self.usage_digest, "provider_capacity_usage_digest")
    }

    pub fn can_admit(
        &self,
        policy: &ProviderCapacityPolicy,
        requested_tokens: u64,
    ) -> Result<(), String> {
        self.validate()?;
        policy.validate()?;
        if requested_tokens == 0 {
            return Err("provider_capacity_tokens_invalid".to_owned());
        }
        if self.admitted_requests >= policy.requests_per_minute
            || self.admitted_tokens > policy.tokens_per_minute
            || requested_tokens
                > policy
                    .tokens_per_minute
                    .saturating_sub(self.admitted_tokens)
            || self.active_concurrency >= policy.max_concurrency
        {
            return Err("provider_capacity_quota_exhausted".to_owned());
        }
        Ok(())
    }

    pub fn reserve(
        &mut self,
        policy: &ProviderCapacityPolicy,
        requested_tokens: u64,
    ) -> Result<(), String> {
        self.can_admit(policy, requested_tokens)?;
        self.admitted_requests = self
            .admitted_requests
            .checked_add(1)
            .ok_or_else(|| "provider_capacity_requests_overflow".to_owned())?;
        self.admitted_tokens = self
            .admitted_tokens
            .checked_add(requested_tokens)
            .ok_or_else(|| "provider_capacity_tokens_overflow".to_owned())?;
        self.active_concurrency = self
            .active_concurrency
            .checked_add(1)
            .ok_or_else(|| "provider_capacity_concurrency_overflow".to_owned())?;
        self.usage_digest = self.digest();
        self.validate()
    }

    pub fn release(&mut self) -> Result<(), String> {
        self.validate()?;
        if self.active_concurrency == 0 {
            return Err("provider_capacity_release_without_permit".to_owned());
        }
        self.active_concurrency -= 1;
        self.usage_digest = self.digest();
        self.validate()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "window_start_unix_ms": self.window_start_unix_ms,
            "window_end_unix_ms": self.window_end_unix_ms,
            "admitted_requests": self.admitted_requests,
            "admitted_tokens": self.admitted_tokens,
            "active_concurrency": self.active_concurrency,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapacityQueueEntry {
    pub schema: String,
    pub version: SchemaVersion,
    pub session_id: String,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub requested_tokens: u64,
    pub enqueued_at_unix_ms: u64,
    pub queue_ticket: u64,
    pub entry_digest: String,
}

impl ProviderCapacityQueueEntry {
    pub fn new(
        session_id: impl Into<String>,
        run_id: RunId,
        attempt_id: AttemptId,
        requested_tokens: u64,
        enqueued_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut entry = Self {
            schema: PROVIDER_CAPACITY_ENTRY_SCHEMA.to_owned(),
            version: PROVIDER_CAPACITY_VERSION,
            session_id: session_id.into(),
            run_id,
            attempt_id,
            requested_tokens,
            enqueued_at_unix_ms,
            queue_ticket: 0,
            entry_digest: String::new(),
        };
        entry.entry_digest = entry.digest();
        entry.validate(false).map_err(|error| error.to_owned())?;
        Ok(entry)
    }

    pub fn validate(&self, enqueued: bool) -> Result<(), &'static str> {
        if self.schema != PROVIDER_CAPACITY_ENTRY_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_CAPACITY_VERSION)
            || self.session_id.trim().is_empty()
            || self.session_id.len() > 256
            || self.session_id.contains(['\0', '\r', '\n'])
            || self.run_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.requested_tokens == 0
            || self.enqueued_at_unix_ms == 0
            || enqueued && self.queue_ticket == 0
            || self.entry_digest != self.digest()
            || digest(&self.entry_digest, "provider_capacity_entry_digest").is_err()
        {
            return Err("provider_capacity_queue_entry_invalid");
        }
        Ok(())
    }

    fn assign_ticket(&mut self, ticket: u64) -> Result<(), String> {
        if ticket == 0 {
            return Err("provider_capacity_queue_ticket_invalid".to_owned());
        }
        self.queue_ticket = ticket;
        self.entry_digest = self.digest();
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "session_id": self.session_id,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "requested_tokens": self.requested_tokens,
            "enqueued_at_unix_ms": self.enqueued_at_unix_ms,
            "queue_ticket": self.queue_ticket,
        }))
    }
}

/// A disposable, bounded, session-fair queue.  Each dequeue rotates to the next session with a
/// pending entry, so one busy session cannot consume every slot.  It never executes a provider.
#[derive(Debug)]
pub struct ProviderCapacityQueue {
    policy: ProviderCapacityPolicy,
    entries: BTreeMap<String, VecDeque<ProviderCapacityQueueEntry>>,
    rotation: VecDeque<String>,
    queued: usize,
    next_ticket: u64,
}

impl ProviderCapacityQueue {
    pub fn new(policy: ProviderCapacityPolicy) -> Result<Self, String> {
        policy.validate()?;
        Ok(Self {
            policy,
            entries: BTreeMap::new(),
            rotation: VecDeque::new(),
            queued: 0,
            next_ticket: 1,
        })
    }

    pub fn policy(&self) -> &ProviderCapacityPolicy {
        &self.policy
    }

    pub fn len(&self) -> usize {
        self.queued
    }

    pub fn is_empty(&self) -> bool {
        self.queued == 0
    }

    pub fn enqueue(&mut self, mut entry: ProviderCapacityQueueEntry) -> Result<u64, String> {
        entry.validate(false).map_err(|error| error.to_owned())?;
        if self.queued >= self.policy.max_queue as usize {
            return Err("provider_capacity_queue_full".to_owned());
        }
        if self
            .entries
            .values()
            .flatten()
            .any(|queued| queued.attempt_id == entry.attempt_id)
        {
            return Err("provider_capacity_queue_duplicate_attempt".to_owned());
        }
        let ticket = self.next_ticket;
        self.next_ticket = self
            .next_ticket
            .checked_add(1)
            .ok_or_else(|| "provider_capacity_queue_ticket_overflow".to_owned())?;
        entry.assign_ticket(ticket)?;
        entry.validate(true).map_err(|error| error.to_owned())?;
        let session = entry.session_id.clone();
        let queue = self.entries.entry(session.clone()).or_default();
        if queue.is_empty() {
            self.rotation.push_back(session);
        }
        queue.push_back(entry);
        self.queued += 1;
        Ok(ticket)
    }

    pub fn dequeue(&mut self) -> Option<ProviderCapacityQueueEntry> {
        let session = self.rotation.pop_front()?;
        let queue = self.entries.get_mut(&session)?;
        let entry = queue.pop_front()?;
        self.queued = self.queued.saturating_sub(1);
        if queue.is_empty() {
            self.entries.remove(&session);
        } else {
            self.rotation.push_back(session);
        }
        Some(entry)
    }

    pub fn cancel(&mut self, attempt_id: AttemptId) -> bool {
        let mut removed = false;
        let sessions = self.entries.keys().cloned().collect::<Vec<_>>();
        for session in sessions {
            let Some(queue) = self.entries.get_mut(&session) else {
                continue;
            };
            let before = queue.len();
            queue.retain(|entry| entry.attempt_id != attempt_id);
            if queue.len() != before {
                removed = true;
                self.queued = self.queued.saturating_sub(before - queue.len());
            }
            if queue.is_empty() {
                self.entries.remove(&session);
                self.rotation.retain(|queued| queued != &session);
            }
        }
        removed
    }

    /// Remove a queued attempt before it can be handed to a provider.  The returned entry is
    /// retained by the caller so cancellation can append a fact without dispatching anything.
    pub fn cancel_entry(&mut self, attempt_id: AttemptId) -> Option<ProviderCapacityQueueEntry> {
        let sessions = self.entries.keys().cloned().collect::<Vec<_>>();
        for session in sessions {
            let Some(queue) = self.entries.get_mut(&session) else {
                continue;
            };
            let Some(index) = queue
                .iter()
                .position(|entry| entry.attempt_id == attempt_id)
            else {
                continue;
            };
            let entry = queue.remove(index)?;
            self.queued = self.queued.saturating_sub(1);
            if queue.is_empty() {
                self.entries.remove(&session);
                self.rotation.retain(|queued| queued != &session);
            }
            return Some(entry);
        }
        None
    }
}

/// The four typed outcomes at the provider capacity boundary.  They describe admission only;
/// none of them invokes a provider or grants a ControlPlane capability.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCapacityOutcomeKind {
    Accept,
    Queue,
    Delay,
    Reject,
}

impl ProviderCapacityOutcomeKind {
    pub fn is_dispatchable(self) -> bool {
        self == Self::Accept
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapacityRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub session_id: String,
    pub run_id: RunId,
    pub attempt_id: AttemptId,
    pub group: QuotaGroupKey,
    pub window: QuotaWindow,
    pub requested_tokens: u64,
    pub requested_at_unix_ms: u64,
    pub deadline_unix_ms: u64,
    pub request_digest: String,
}

impl ProviderCapacityRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session_id: impl Into<String>,
        run_id: RunId,
        attempt_id: AttemptId,
        group: QuotaGroupKey,
        window: QuotaWindow,
        requested_tokens: u64,
        requested_at_unix_ms: u64,
        deadline_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: PROVIDER_CAPACITY_ADMISSION_SCHEMA.to_owned(),
            version: PROVIDER_CAPACITY_VERSION,
            session_id: session_id.into(),
            run_id,
            attempt_id,
            group,
            window,
            requested_tokens,
            requested_at_unix_ms,
            deadline_unix_ms,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CAPACITY_ADMISSION_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_CAPACITY_VERSION)
            || self.session_id.trim().is_empty()
            || self.session_id.len() > MAX_CAPACITY_OWNER
            || self.session_id.contains(['\0', '\r', '\n'])
            || self.run_id.as_uuid().is_nil()
            || self.attempt_id.as_uuid().is_nil()
            || self.requested_tokens == 0
            || self.requested_at_unix_ms == 0
            || self.deadline_unix_ms <= self.requested_at_unix_ms
            || self.request_digest != self.digest()
            || digest(&self.request_digest, "provider_capacity_request_digest").is_err()
        {
            return Err("provider_capacity_request_invalid".to_owned());
        }
        self.group.validate()?;
        self.window.validate()?;
        if self.requested_at_unix_ms < self.window.start_unix_ms {
            return Err("provider_capacity_clock_rollback".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "session_id": self.session_id,
            "run_id": self.run_id,
            "attempt_id": self.attempt_id,
            "group": self.group,
            "window": self.window,
            "requested_tokens": self.requested_tokens,
            "requested_at_unix_ms": self.requested_at_unix_ms,
            "deadline_unix_ms": self.deadline_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapacityOutcome {
    pub schema: String,
    pub version: SchemaVersion,
    pub kind: ProviderCapacityOutcomeKind,
    pub attempt_id: AttemptId,
    pub group_digest: String,
    pub window_digest: String,
    pub policy_digest: String,
    pub queue_ticket: Option<u64>,
    pub queue_wait_ms: Option<u64>,
    pub retry_after_ms: Option<u64>,
    pub reason: Option<String>,
    pub outcome_digest: String,
}

impl ProviderCapacityOutcome {
    fn new(
        kind: ProviderCapacityOutcomeKind,
        request: &ProviderCapacityRequest,
        policy: &ProviderCapacityPolicy,
        queue_ticket: Option<u64>,
        queue_wait_ms: Option<u64>,
        retry_after_ms: Option<u64>,
        reason: Option<String>,
    ) -> Result<Self, String> {
        let mut outcome = Self {
            schema: PROVIDER_CAPACITY_ADMISSION_SCHEMA.to_owned(),
            version: PROVIDER_CAPACITY_VERSION,
            kind,
            attempt_id: request.attempt_id,
            group_digest: request.group.group_digest.clone(),
            window_digest: request.window.window_digest.clone(),
            policy_digest: policy.policy_digest.clone(),
            queue_ticket,
            queue_wait_ms,
            retry_after_ms,
            reason,
            outcome_digest: String::new(),
        };
        outcome.outcome_digest = outcome.digest();
        outcome.validate()?;
        Ok(outcome)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CAPACITY_ADMISSION_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_CAPACITY_VERSION)
            || self.attempt_id.as_uuid().is_nil()
            || digest(&self.group_digest, "provider_capacity_outcome_group").is_err()
            || digest(&self.window_digest, "provider_capacity_outcome_window").is_err()
            || digest(&self.policy_digest, "provider_capacity_outcome_policy").is_err()
            || self.queue_ticket == Some(0)
            || self.retry_after_ms == Some(0)
            || self.queue_wait_ms == Some(0)
            || self.reason.as_deref().is_some_and(|reason| {
                reason.trim().is_empty()
                    || reason.len() > 256
                    || reason.contains(['\0', '\r', '\n'])
            })
            || self.outcome_digest != self.digest()
            || digest(&self.outcome_digest, "provider_capacity_outcome_digest").is_err()
        {
            return Err("provider_capacity_outcome_invalid".to_owned());
        }
        match self.kind {
            ProviderCapacityOutcomeKind::Accept => {
                if self.queue_ticket.is_some() || self.retry_after_ms.is_some() {
                    return Err("provider_capacity_accept_fields_invalid".to_owned());
                }
            }
            ProviderCapacityOutcomeKind::Queue => {
                if self.queue_ticket.is_none() || self.retry_after_ms.is_some() {
                    return Err("provider_capacity_queue_fields_invalid".to_owned());
                }
            }
            ProviderCapacityOutcomeKind::Delay | ProviderCapacityOutcomeKind::Reject => {
                if self.retry_after_ms.is_none() {
                    return Err("provider_capacity_retry_after_missing".to_owned());
                }
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "kind": self.kind,
            "attempt_id": self.attempt_id,
            "group_digest": self.group_digest,
            "window_digest": self.window_digest,
            "policy_digest": self.policy_digest,
            "queue_ticket": self.queue_ticket,
            "queue_wait_ms": self.queue_wait_ms,
            "retry_after_ms": self.retry_after_ms,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCapacityLease {
    pub schema: String,
    pub version: SchemaVersion,
    pub lease_id: u64,
    pub owner: String,
    pub attempt_id: AttemptId,
    pub group_digest: String,
    pub window_digest: String,
    pub policy_digest: String,
    pub acquired_at_unix_ms: u64,
    pub lease_digest: String,
}

impl ProviderCapacityLease {
    fn new(
        lease_id: u64,
        owner: impl Into<String>,
        request: &ProviderCapacityRequest,
        policy: &ProviderCapacityPolicy,
    ) -> Result<Self, String> {
        let mut lease = Self {
            schema: PROVIDER_CAPACITY_LEASE_SCHEMA.to_owned(),
            version: PROVIDER_CAPACITY_VERSION,
            lease_id,
            owner: owner.into(),
            attempt_id: request.attempt_id,
            group_digest: request.group.group_digest.clone(),
            window_digest: request.window.window_digest.clone(),
            policy_digest: policy.policy_digest.clone(),
            acquired_at_unix_ms: request.requested_at_unix_ms,
            lease_digest: String::new(),
        };
        lease.lease_digest = lease.digest();
        lease.validate()?;
        Ok(lease)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CAPACITY_LEASE_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_CAPACITY_VERSION)
            || self.lease_id == 0
            || self.owner.trim().is_empty()
            || self.owner.len() > MAX_CAPACITY_OWNER
            || self.owner.contains(['\0', '\r', '\n'])
            || self.attempt_id.as_uuid().is_nil()
            || self.acquired_at_unix_ms == 0
            || digest(&self.group_digest, "provider_capacity_lease_group").is_err()
            || digest(&self.window_digest, "provider_capacity_lease_window").is_err()
            || digest(&self.policy_digest, "provider_capacity_lease_policy").is_err()
            || self.lease_digest != self.digest()
            || digest(&self.lease_digest, "provider_capacity_lease_digest").is_err()
        {
            return Err("provider_capacity_lease_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "lease_id": self.lease_id,
            "owner": self.owner,
            "attempt_id": self.attempt_id,
            "group_digest": self.group_digest,
            "window_digest": self.window_digest,
            "policy_digest": self.policy_digest,
            "acquired_at_unix_ms": self.acquired_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderCapacityDecision {
    pub outcome: ProviderCapacityOutcome,
    pub lease: Option<ProviderCapacityLease>,
}

/// Process-local capacity authority used by adapters.  It owns only reservation/queue state;
/// ControlPlane still decides whether the model attempt is authorized and the provider transport
/// remains the sole place that may send a request.
#[derive(Debug)]
pub struct ProviderCapacityController {
    policy: ProviderCapacityPolicy,
    window: QuotaWindow,
    usage: ProviderCapacityUsage,
    queue: ProviderCapacityQueue,
    active: BTreeMap<u64, ProviderCapacityLease>,
    next_lease_id: u64,
}

impl ProviderCapacityController {
    pub fn new(policy: ProviderCapacityPolicy, window: QuotaWindow) -> Result<Self, String> {
        policy.validate()?;
        window.validate()?;
        let usage = ProviderCapacityUsage::new(window.start_unix_ms, window.end_unix_ms)?;
        Ok(Self {
            queue: ProviderCapacityQueue::new(policy.clone())?,
            policy,
            window,
            usage,
            active: BTreeMap::new(),
            next_lease_id: 1,
        })
    }

    pub fn policy(&self) -> &ProviderCapacityPolicy {
        &self.policy
    }

    pub fn window(&self) -> &QuotaWindow {
        &self.window
    }

    pub fn usage(&self) -> &ProviderCapacityUsage {
        &self.usage
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn active_len(&self) -> usize {
        self.active.len()
    }

    pub fn admit(
        &mut self,
        request: ProviderCapacityRequest,
    ) -> Result<ProviderCapacityDecision, String> {
        request.validate()?;
        self.validate_scope(&request)?;
        if request.requested_at_unix_ms >= request.deadline_unix_ms {
            return self.decision(
                ProviderCapacityOutcomeKind::Reject,
                &request,
                None,
                Some(1),
                Some("provider_capacity_deadline_expired".to_owned()),
            );
        }
        if self.active.len() >= self.policy.max_concurrency as usize {
            return self.enqueue_or_reject(request, "provider_capacity_concurrency_full");
        }
        if let Err(error) = self.usage.can_admit(&self.policy, request.requested_tokens) {
            if error == "provider_capacity_quota_exhausted" {
                let retry_after = self.window.retry_after_ms(request.requested_at_unix_ms)?;
                return self.decision(
                    ProviderCapacityOutcomeKind::Delay,
                    &request,
                    None,
                    Some(retry_after.max(1)),
                    Some("provider_capacity_quota_window_exhausted".to_owned()),
                );
            }
            return Err(error);
        }
        self.accept(request)
    }

    fn validate_scope(&self, request: &ProviderCapacityRequest) -> Result<(), String> {
        if request.group.group_digest != self.policy.group.group_digest {
            return Err("provider_capacity_quota_group_mismatch".to_owned());
        }
        if request.window.window_digest != self.window.window_digest {
            return Err("provider_capacity_window_mismatch".to_owned());
        }
        Ok(())
    }

    fn enqueue_or_reject(
        &mut self,
        request: ProviderCapacityRequest,
        reason: &str,
    ) -> Result<ProviderCapacityDecision, String> {
        if self.queue.len() >= self.policy.max_queue as usize {
            let retry_after = self
                .window
                .retry_after_ms(request.requested_at_unix_ms)?
                .max(1);
            return self.decision(
                ProviderCapacityOutcomeKind::Reject,
                &request,
                None,
                Some(retry_after),
                Some("provider_capacity_queue_full".to_owned()),
            );
        }
        let mut entry = ProviderCapacityQueueEntry::new(
            request.session_id.clone(),
            request.run_id,
            request.attempt_id,
            request.requested_tokens,
            request.requested_at_unix_ms,
        )?;
        let ticket = self.queue.enqueue(entry.clone())?;
        entry.queue_ticket = ticket;
        self.decision(
            ProviderCapacityOutcomeKind::Queue,
            &request,
            Some(ticket),
            None,
            Some(reason.to_owned()),
        )
    }

    fn accept(
        &mut self,
        request: ProviderCapacityRequest,
    ) -> Result<ProviderCapacityDecision, String> {
        self.accept_with_queue_wait(request, None)
    }

    fn accept_with_queue_wait(
        &mut self,
        request: ProviderCapacityRequest,
        queue_wait_ms: Option<u64>,
    ) -> Result<ProviderCapacityDecision, String> {
        self.usage.reserve(&self.policy, request.requested_tokens)?;
        let lease_id = self.next_lease_id;
        self.next_lease_id = self
            .next_lease_id
            .checked_add(1)
            .ok_or_else(|| "provider_capacity_lease_overflow".to_owned())?;
        let lease = ProviderCapacityLease::new(
            lease_id,
            request.session_id.clone(),
            &request,
            &self.policy,
        )?;
        self.active.insert(lease_id, lease.clone());
        let outcome = ProviderCapacityOutcome::new(
            ProviderCapacityOutcomeKind::Accept,
            &request,
            &self.policy,
            None,
            queue_wait_ms,
            None,
            None,
        )?;
        Ok(ProviderCapacityDecision {
            outcome,
            lease: Some(lease),
        })
    }

    fn decision(
        &self,
        kind: ProviderCapacityOutcomeKind,
        request: &ProviderCapacityRequest,
        queue_ticket: Option<u64>,
        retry_after_ms: Option<u64>,
        reason: Option<String>,
    ) -> Result<ProviderCapacityDecision, String> {
        Ok(ProviderCapacityDecision {
            outcome: ProviderCapacityOutcome::new(
                kind,
                request,
                &self.policy,
                queue_ticket,
                None,
                retry_after_ms,
                reason,
            )?,
            lease: None,
        })
    }

    /// Dispatch the next fair queued item when a slot is available.  Delay leaves the item in the
    /// queue and never consumes active capacity; an expired/cancelled item is removed first.
    pub fn dispatch_next(
        &mut self,
        now_unix_ms: u64,
    ) -> Result<Option<ProviderCapacityDecision>, String> {
        if self.active.len() >= self.policy.max_concurrency as usize || self.queue.is_empty() {
            return Ok(None);
        }
        let Some(entry) = self.queue.dequeue() else {
            return Ok(None);
        };
        let request = ProviderCapacityRequest::new(
            entry.session_id.clone(),
            entry.run_id,
            entry.attempt_id,
            self.policy.group.clone(),
            self.window.clone(),
            entry.requested_tokens,
            now_unix_ms.max(entry.enqueued_at_unix_ms),
            now_unix_ms.saturating_add(self.policy.max_queue_wait_ms),
        )?;
        if request.requested_at_unix_ms >= request.deadline_unix_ms {
            return self
                .decision(
                    ProviderCapacityOutcomeKind::Reject,
                    &request,
                    Some(entry.queue_ticket),
                    Some(1),
                    Some("provider_capacity_queue_wait_expired".to_owned()),
                )
                .map(Some);
        }
        if self
            .usage
            .can_admit(&self.policy, request.requested_tokens)
            .is_err()
        {
            // Put the entry back in its fair session position.  No active slot or request quota
            // is consumed while waiting for the UTC window to roll.
            let _ = self.queue.enqueue(entry);
            let retry_after = self.window.retry_after_ms(now_unix_ms)?.max(1);
            return self
                .decision(
                    ProviderCapacityOutcomeKind::Delay,
                    &request,
                    None,
                    Some(retry_after),
                    Some("provider_capacity_quota_window_exhausted".to_owned()),
                )
                .map(Some);
        }
        self.accept_with_queue_wait(
            request,
            Some(now_unix_ms.saturating_sub(entry.enqueued_at_unix_ms).max(1)),
        )
        .map(Some)
    }

    pub fn cancel(&mut self, attempt_id: AttemptId) -> bool {
        // Cancellation only removes queued work.  A leased attempt requires the ControlPlane's
        // cancellation/fencing path and is never silently recycled here.
        self.queue.cancel_entry(attempt_id).is_some()
    }

    pub fn release(&mut self, lease_id: u64, owner: &str) -> Result<(), String> {
        let lease = self
            .active
            .get(&lease_id)
            .ok_or_else(|| "provider_capacity_lease_unknown".to_owned())?;
        if lease.owner != owner {
            return Err("provider_capacity_lease_owner_mismatch".to_owned());
        }
        self.active.remove(&lease_id);
        self.usage.release()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCircuitState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CircuitAdmission {
    Allowed,
    HalfOpenProbe,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCircuitBreaker {
    pub schema: String,
    pub version: SchemaVersion,
    pub config_revision: String,
    pub state: ProviderCircuitState,
    pub failure_count: u32,
    pub failure_threshold: u32,
    pub open_duration_ms: u64,
    pub open_until_unix_ms: Option<u64>,
    pub half_open_probe_in_flight: bool,
    pub health_digest: String,
}

impl ProviderCircuitBreaker {
    pub fn new(
        config_revision: impl Into<String>,
        failure_threshold: u32,
        open_duration_ms: u64,
    ) -> Result<Self, String> {
        let mut breaker = Self {
            schema: PROVIDER_CIRCUIT_SCHEMA.to_owned(),
            version: PROVIDER_CAPACITY_VERSION,
            config_revision: config_revision.into(),
            state: ProviderCircuitState::Closed,
            failure_count: 0,
            failure_threshold,
            open_duration_ms,
            open_until_unix_ms: None,
            half_open_probe_in_flight: false,
            health_digest: String::new(),
        };
        breaker.health_digest = breaker.digest();
        breaker.validate()?;
        Ok(breaker)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_CIRCUIT_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_CAPACITY_VERSION)
            || self.failure_threshold == 0
            || self.failure_threshold > 128
            || self.open_duration_ms == 0
            || self.open_duration_ms > MAX_CAPACITY_WINDOW_MS
            || self.state == ProviderCircuitState::Open && self.open_until_unix_ms.is_none()
            || self.state != ProviderCircuitState::Open && self.open_until_unix_ms.is_some()
            || self.state != ProviderCircuitState::HalfOpen && self.half_open_probe_in_flight
            || self.health_digest != self.digest()
        {
            return Err("provider_circuit_invalid".to_owned());
        }
        required(
            &self.config_revision,
            "provider_circuit_config_revision",
            256,
        )?;
        digest(&self.health_digest, "provider_circuit_digest")
    }

    pub fn allow(&mut self, now_unix_ms: u64) -> Result<CircuitAdmission, String> {
        self.validate()?;
        if now_unix_ms == 0 {
            return Err("provider_circuit_clock_invalid".to_owned());
        }
        let result = match self.state {
            ProviderCircuitState::Closed => CircuitAdmission::Allowed,
            ProviderCircuitState::Open => {
                if now_unix_ms < self.open_until_unix_ms.unwrap_or(now_unix_ms) {
                    return Err("provider_circuit_open".to_owned());
                }
                self.state = ProviderCircuitState::HalfOpen;
                self.half_open_probe_in_flight = true;
                CircuitAdmission::HalfOpenProbe
            }
            ProviderCircuitState::HalfOpen => {
                if self.half_open_probe_in_flight {
                    return Err("provider_circuit_probe_busy".to_owned());
                }
                self.half_open_probe_in_flight = true;
                CircuitAdmission::HalfOpenProbe
            }
        };
        self.open_until_unix_ms = None;
        self.health_digest = self.digest();
        self.validate()?;
        Ok(result)
    }

    pub fn observe_success(&mut self) -> Result<(), String> {
        self.validate()?;
        self.state = ProviderCircuitState::Closed;
        self.failure_count = 0;
        self.open_until_unix_ms = None;
        self.half_open_probe_in_flight = false;
        self.health_digest = self.digest();
        self.validate()
    }

    pub fn observe_failure(&mut self, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms == 0 {
            return Err("provider_circuit_clock_invalid".to_owned());
        }
        self.half_open_probe_in_flight = false;
        self.failure_count = self.failure_count.saturating_add(1);
        if self.state == ProviderCircuitState::HalfOpen
            || self.failure_count >= self.failure_threshold
        {
            self.state = ProviderCircuitState::Open;
            self.open_until_unix_ms = Some(
                now_unix_ms
                    .checked_add(self.open_duration_ms)
                    .ok_or_else(|| "provider_circuit_open_until_overflow".to_owned())?,
            );
        }
        self.health_digest = self.digest();
        self.validate()
    }

    /// A half-open probe that is cancelled or otherwise ends without a classifiable observation
    /// must not leave the breaker permanently busy. Reopen it for a fresh cooldown rather than
    /// treating the unobserved request as success.
    pub fn abandon_probe(&mut self, now_unix_ms: u64) -> Result<(), String> {
        self.validate()?;
        if now_unix_ms == 0 {
            return Err("provider_circuit_clock_invalid".to_owned());
        }
        if self.state != ProviderCircuitState::HalfOpen || !self.half_open_probe_in_flight {
            return Err("provider_circuit_probe_not_in_flight".to_owned());
        }
        let open_until_unix_ms = now_unix_ms
            .checked_add(self.open_duration_ms)
            .ok_or_else(|| "provider_circuit_open_until_overflow".to_owned())?;
        self.state = ProviderCircuitState::Open;
        self.open_until_unix_ms = Some(open_until_unix_ms);
        self.half_open_probe_in_flight = false;
        self.health_digest = self.digest();
        self.validate()
    }

    /// Configuration changes reset derived health.  Health never authorizes a request by itself.
    pub fn reset_for_config(&mut self, config_revision: impl Into<String>) -> Result<(), String> {
        let revision = config_revision.into();
        required(&revision, "provider_circuit_config_revision", 256)?;
        self.config_revision = revision;
        self.state = ProviderCircuitState::Closed;
        self.failure_count = 0;
        self.open_until_unix_ms = None;
        self.half_open_probe_in_flight = false;
        self.health_digest = self.digest();
        self.validate()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "config_revision": self.config_revision,
            "state": self.state,
            "failure_count": self.failure_count,
            "failure_threshold": self.failure_threshold,
            "open_duration_ms": self.open_duration_ms,
            "open_until_unix_ms": self.open_until_unix_ms,
            "half_open_probe_in_flight": self.half_open_probe_in_flight,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackRequirements {
    pub context_scope_digest: String,
    pub data_boundary_digest: String,
    pub budget_digest: String,
    pub original_permit_digest: String,
    pub require_tools: bool,
    pub require_images: bool,
    pub require_structured_output: bool,
    pub min_context_window: u64,
    pub min_output_tokens: u64,
}

impl FallbackRequirements {
    pub fn validate(&self) -> Result<(), String> {
        digest(&self.context_scope_digest, "fallback_context_scope_digest")?;
        digest(&self.data_boundary_digest, "fallback_data_boundary_digest")?;
        digest(&self.budget_digest, "fallback_budget_digest")?;
        digest(
            &self.original_permit_digest,
            "fallback_original_permit_digest",
        )?;
        if self.min_context_window == 0 || self.min_output_tokens == 0 {
            return Err("fallback_capability_requirement_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackCandidate {
    pub route: ModelRoute,
    pub capabilities: ModelCapabilities,
    pub credential_revision: Option<String>,
    pub context_scope_digest: String,
    pub data_boundary_digest: String,
    pub budget_digest: String,
    pub permit_digest: String,
}

impl FallbackCandidate {
    pub fn validate(&self) -> Result<(), String> {
        required(&self.route.provider_id, "fallback_provider", 256)?;
        required(&self.route.model_id, "fallback_model", 256)?;
        required(&self.route.profile, "fallback_profile", 256)?;
        required(
            &self.route.configuration_revision,
            "fallback_configuration_revision",
            256,
        )?;
        digest(
            &self.context_scope_digest,
            "fallback_candidate_context_scope_digest",
        )?;
        digest(
            &self.data_boundary_digest,
            "fallback_candidate_data_boundary_digest",
        )?;
        digest(&self.budget_digest, "fallback_candidate_budget_digest")?;
        digest(&self.permit_digest, "fallback_candidate_permit_digest")?;
        let credential = self
            .credential_revision
            .as_deref()
            .ok_or_else(|| "fallback_credential_missing".to_owned())?;
        digest(credential, "fallback_credential_revision")?;
        if self.capabilities.context_window == 0 || self.capabilities.max_output == 0 {
            return Err("fallback_capabilities_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackRoutePlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub original_route_digest: String,
    pub fallback_route_digest: String,
    pub attempt_id: AttemptId,
    pub route_reason: String,
    pub context_scope_digest: String,
    pub data_boundary_digest: String,
    pub budget_digest: String,
    pub credential_revision: String,
    pub permit_digest: String,
    pub plan_digest: String,
}

impl FallbackRoutePlan {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_FALLBACK_SCHEMA
            || !self.version.is_compatible_with(&PROVIDER_CAPACITY_VERSION)
            || self.attempt_id.as_uuid().is_nil()
            || self.route_reason != "allowlisted_fallback"
            || self.original_route_digest == self.fallback_route_digest
            || self.plan_digest != self.digest()
        {
            return Err("fallback_route_plan_invalid".to_owned());
        }
        digest(
            &self.original_route_digest,
            "fallback_original_route_digest",
        )?;
        digest(&self.fallback_route_digest, "fallback_route_digest")?;
        digest(
            &self.context_scope_digest,
            "fallback_plan_context_scope_digest",
        )?;
        digest(
            &self.data_boundary_digest,
            "fallback_plan_data_boundary_digest",
        )?;
        digest(&self.budget_digest, "fallback_plan_budget_digest")?;
        digest(
            &self.credential_revision,
            "fallback_plan_credential_revision",
        )?;
        digest(&self.permit_digest, "fallback_plan_permit_digest")?;
        digest(&self.plan_digest, "fallback_plan_digest")
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "original_route_digest": self.original_route_digest,
            "fallback_route_digest": self.fallback_route_digest,
            "attempt_id": self.attempt_id,
            "route_reason": self.route_reason,
            "context_scope_digest": self.context_scope_digest,
            "data_boundary_digest": self.data_boundary_digest,
            "budget_digest": self.budget_digest,
            "credential_revision": self.credential_revision,
            "permit_digest": self.permit_digest,
        }))
    }
}

/// Re-admit a fallback as a fresh route/attempt.  The whitelist contains route digests, never
/// profile aliases, so two names pointing at one credential cannot bypass policy.
pub fn admit_fallback(
    original_route: &ModelRoute,
    original_attempt_id: AttemptId,
    requirements: &FallbackRequirements,
    candidate: &FallbackCandidate,
    allowlist: &[String],
    new_attempt_id: AttemptId,
) -> Result<FallbackRoutePlan, String> {
    requirements.validate()?;
    candidate.validate()?;
    if original_attempt_id.as_uuid().is_nil()
        || new_attempt_id.as_uuid().is_nil()
        || original_attempt_id == new_attempt_id
    {
        return Err("fallback_attempt_identity_invalid".to_owned());
    }
    if allowlist.is_empty() || allowlist.len() > MAX_ROUTE_ALLOWLIST {
        return Err("fallback_allowlist_invalid".to_owned());
    }
    let original_route_digest = original_route.digest();
    let fallback_route_digest = candidate.route.digest();
    if !allowlist
        .iter()
        .any(|route| route == &fallback_route_digest)
    {
        return Err("fallback_route_not_allowlisted".to_owned());
    }
    if original_route_digest == fallback_route_digest {
        return Err("fallback_route_must_change".to_owned());
    }
    if candidate.context_scope_digest != requirements.context_scope_digest {
        return Err("fallback_cannot_send_private_context_to_new_provider".to_owned());
    }
    if candidate.data_boundary_digest != requirements.data_boundary_digest {
        return Err("fallback_data_boundary_mismatch".to_owned());
    }
    if candidate.budget_digest != requirements.budget_digest {
        return Err("fallback_budget_recheck_required".to_owned());
    }
    if candidate.permit_digest == requirements.original_permit_digest {
        return Err("fallback_cannot_reuse_original_permit".to_owned());
    }
    if requirements.require_tools && candidate.capabilities.tools != CapabilitySupport::Supported {
        return Err("fallback_cannot_reduce_required_capabilities".to_owned());
    }
    if requirements.require_images && candidate.capabilities.images != CapabilitySupport::Supported
    {
        return Err("fallback_cannot_reduce_required_capabilities".to_owned());
    }
    if requirements.require_structured_output
        && candidate.capabilities.structured_output != CapabilitySupport::Supported
    {
        return Err("fallback_cannot_reduce_required_capabilities".to_owned());
    }
    if candidate.capabilities.context_window < requirements.min_context_window
        || candidate.capabilities.max_output < requirements.min_output_tokens
    {
        return Err("fallback_cannot_reduce_required_capabilities".to_owned());
    }
    let credential_revision = candidate
        .credential_revision
        .clone()
        .ok_or_else(|| "fallback_credential_missing".to_owned())?;
    let mut plan = FallbackRoutePlan {
        schema: PROVIDER_FALLBACK_SCHEMA.to_owned(),
        version: PROVIDER_CAPACITY_VERSION,
        original_route_digest,
        fallback_route_digest,
        attempt_id: new_attempt_id,
        route_reason: "allowlisted_fallback".to_owned(),
        context_scope_digest: requirements.context_scope_digest.clone(),
        data_boundary_digest: requirements.data_boundary_digest.clone(),
        budget_digest: requirements.budget_digest.clone(),
        credential_revision,
        permit_digest: candidate.permit_digest.clone(),
        plan_digest: String::new(),
    };
    plan.plan_digest = plan.digest();
    plan.validate()?;
    Ok(plan)
}
