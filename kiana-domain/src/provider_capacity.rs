//! Provider capacity, circuit and controlled-fallback contracts.
//!
//! These values describe admission state; they do not grant a model capability, consume a
//! ControlPlane permit or perform a provider request.  A durable quota adapter may persist the
//! same identities, while the in-memory fair queue below is intentionally bounded and disposable.

use crate::{json_digest, AttemptId, CapabilitySupport, ModelCapabilities, ModelRoute, QuotaGroupKey,
    RunId, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

pub const PROVIDER_CAPACITY_POLICY_SCHEMA: &str = "kiana.provider-capacity-policy.v1";
pub const PROVIDER_CAPACITY_USAGE_SCHEMA: &str = "kiana.provider-capacity-usage.v1";
pub const PROVIDER_CAPACITY_ENTRY_SCHEMA: &str = "kiana.provider-capacity-entry.v1";
pub const PROVIDER_CIRCUIT_SCHEMA: &str = "kiana.provider-circuit.v1";
pub const PROVIDER_FALLBACK_SCHEMA: &str = "kiana.provider-fallback.v1";
pub const PROVIDER_CAPACITY_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

pub const MAX_CAPACITY_QUEUE: u32 = 16_384;
pub const MAX_CAPACITY_CONCURRENCY: u32 = 4_096;
pub const MAX_CAPACITY_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
pub const MAX_ROUTE_ALLOWLIST: usize = 256;

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
        required(&self.config_revision, "provider_capacity_config_revision", 256)?;
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
        entry.validate(false)
            .map_err(|error| error.to_owned())?;
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
        entry
            .validate(false)
            .map_err(|error| error.to_owned())?;
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
        entry
            .validate(true)
            .map_err(|error| error.to_owned())?;
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
        required(&self.config_revision, "provider_circuit_config_revision", 256)?;
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
        digest(&self.original_permit_digest, "fallback_original_permit_digest")?;
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
        digest(&self.context_scope_digest, "fallback_candidate_context_scope_digest")?;
        digest(&self.data_boundary_digest, "fallback_candidate_data_boundary_digest")?;
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
        digest(&self.original_route_digest, "fallback_original_route_digest")?;
        digest(&self.fallback_route_digest, "fallback_route_digest")?;
        digest(&self.context_scope_digest, "fallback_plan_context_scope_digest")?;
        digest(&self.data_boundary_digest, "fallback_plan_data_boundary_digest")?;
        digest(&self.budget_digest, "fallback_plan_budget_digest")?;
        digest(&self.credential_revision, "fallback_plan_credential_revision")?;
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
    if !allowlist.iter().any(|route| route == &fallback_route_digest) {
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
    if requirements.require_images && candidate.capabilities.images != CapabilitySupport::Supported {
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
