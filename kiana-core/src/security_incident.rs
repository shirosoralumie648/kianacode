//! SC-33 security Incident / Vulnerability / Reconcile workflow contract.
//!
//! An incident in this module is not a ticket. It is the record that a *named* condition -- an
//! unknown external result, a suspected secret leak, a supply-chain drift -- was seen, that a
//! named human owns it, that it has a deadline, and that every transition cites committed evidence.
//! The three conditions are the ones the security constitution makes unforgeable: an unknown must
//! stay a first-class fact, a secret must never be restated in a record, and an extension whose
//! provenance moved must not keep running.
//!
//! The lifecycle is ordered and the ordering is the whole point:
//!
//! ```text
//! contain -> fence -> reconcile -> close
//! ```
//!
//! Each step answers a different question. `contain` stops the bleeding, `fence` proves the old
//! capability can no longer act, `reconcile` resolves what the outside world actually did, and
//! `close` is the only step that may retire the incident. Two consequences follow, and both are
//! enforced here rather than documented:
//!
//! * A step cannot be skipped. Closing an incident that was never reconciled would silently
//!   convert an unknown into a success, which is precisely the failure the constitution forbids.
//! * A step cannot be walked back. `Closed` is terminal, so a closed incident stays traceable as a
//!   closed incident rather than being reopened by whoever is holding the pen next.
//!
//! Closure never deletes the underlying facts. Closing requires a root cause, an impact scope, the
//! containment actions actually taken, a residual risk statement and a reviewer who is not the
//! owner, so "handled" is a claim somebody has to sign for.
//!
//! This module is a **read-only contract over supplied facts**. It appends no event, stores no
//! state, calls no adapter and no port, and it has no side effects of any kind: it cannot open an
//! incident in a running system, fence a real lease, or talk to an external reconciler. It decides
//! whether a *proposed* action is admissible given an incident record and a supplied state, and it
//! re-derives that decision in [`SecurityIncidentReport::validate_against`] so an edited status,
//! state or reason cannot be published as if it had been decided here. The ControlPlane remains the
//! only place a command becomes an effect; a caller that wants one must route through
//! `ControlPlane::handle_command` and let that path record the fact.
//!
//! Every error string in this file is a stable reason code, and the same code is produced whether
//! it is reached through a constructor, through [`SecurityIncident::evaluate`], or through
//! re-derivation in `validate_against`. That is what makes the negative fixtures in
//! `kiana-core/tests/sc33_security_incident.rs` meaningful rather than decorative.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, EventCursor, SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const SECURITY_INCIDENT_SCHEMA: &str = "kiana.security-incident.v1";
pub const SECURITY_INCIDENT_STATE_SCHEMA: &str = "kiana.security-incident-state.v1";
pub const SECURITY_INCIDENT_ACTION_SCHEMA: &str = "kiana.security-incident-action.v1";
pub const SECURITY_INCIDENT_REPORT_SCHEMA: &str = "kiana.security-incident-report.v1";
pub const SECURITY_VULNERABILITY_SCHEMA: &str = "kiana.security-vulnerability.v1";
pub const SECURITY_INCIDENT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SECURITY_INCIDENT_TEXT: usize = 256;
pub const MAX_SECURITY_INCIDENT_EVIDENCE: usize = 32;
pub const MAX_SECURITY_INCIDENT_ACTIONS: usize = 16;

/// The three conditions this workflow exists for.
///
/// The set is closed on purpose. A fourth "something else went wrong" bucket would be a way to open
/// an incident that is not accountable to a severity ladder, a deadline or a reviewer, so the
/// unknown-value direction is represented explicitly by [`SecurityIncidentClass::UnknownOutcome`]
/// instead of by an untyped catch-all.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityIncidentClass {
    /// An external effect whose real result is not known. Never silently retried, never zeroed.
    UnknownOutcome,
    /// A secret may have reached somewhere it must not be. The record cites a reference and a
    /// digest, never the value.
    SecretLeak,
    /// An extension, provider or artifact moved away from its attested provenance.
    SupplyChainDrift,
}

impl SecurityIncidentClass {
    pub const ALL: [Self; 3] = [
        Self::UnknownOutcome,
        Self::SecretLeak,
        Self::SupplyChainDrift,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnknownOutcome => "unknown_outcome",
            Self::SecretLeak => "secret_leak",
            Self::SupplyChainDrift => "supply_chain_drift",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "unknown_outcome" => Ok(Self::UnknownOutcome),
            "secret_leak" => Ok(Self::SecretLeak),
            "supply_chain_drift" => Ok(Self::SupplyChainDrift),
            _ => Err("security_incident_class_invalid".to_owned()),
        }
    }
}

/// How bad the incident is, and therefore how much clock it gets.
///
/// Ordinal order is response order: a `Critical` incident must be contained before a `Low` one is
/// even triaged. Severity is declared once at admission and is part of the incident digest, so it
/// cannot be quietly downgraded after the fact -- downgrading is a new incident with a new owner.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityIncidentSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl SecurityIncidentSeverity {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Critical];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "critical" => Ok(Self::Critical),
            _ => Err("security_incident_severity_invalid".to_owned()),
        }
    }
}

/// The lifecycle stage an incident has actually reached.
///
/// Every variant except `Closed` still has a next step available. `Closed` has none, which is the
/// mechanical form of "a closed incident cannot be reopened from this workflow".
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityIncidentPhase {
    /// Opened, nothing done yet.
    Open,
    /// Contained: the bleeding stopped, but the old capability may still act.
    Contained,
    /// Fenced: the old capability provably can no longer act.
    Fenced,
    /// Reconciled: what the outside world actually did is known.
    Reconciled,
    /// Closed and terminal. No transition leaves this state.
    Closed,
}

impl SecurityIncidentPhase {
    pub const ALL: [Self; 5] = [
        Self::Open,
        Self::Contained,
        Self::Fenced,
        Self::Reconciled,
        Self::Closed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Contained => "contained",
            Self::Fenced => "fenced",
            Self::Reconciled => "reconciled",
            Self::Closed => "closed",
        }
    }
}

/// One response action proposed against an incident.
///
/// The four actions are the workflow. There is no fifth: `Skip`, `Force` and `Waive` are exactly
/// the escape hatches that let an unknown become a success, so they are not representable.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityIncidentAction {
    Contain,
    Fence,
    Reconcile,
    Close,
}

impl SecurityIncidentAction {
    pub const ALL: [Self; 4] = [Self::Contain, Self::Fence, Self::Reconcile, Self::Close];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Contain => "contain",
            Self::Fence => "fence",
            Self::Reconcile => "reconcile",
            Self::Close => "close",
        }
    }

    /// The action that legally follows this one, if any.
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Contain => Some(Self::Fence),
            Self::Fence => Some(Self::Reconcile),
            Self::Reconcile => Some(Self::Close),
            Self::Close => None,
        }
    }

    /// The state an incident must already be in for this action to be admissible.
    pub const fn required_state(self) -> SecurityIncidentPhase {
        match self {
            Self::Contain => SecurityIncidentPhase::Open,
            Self::Fence => SecurityIncidentPhase::Contained,
            Self::Reconcile => SecurityIncidentPhase::Fenced,
            Self::Close => SecurityIncidentPhase::Reconciled,
        }
    }

    /// The state this action produces when it is admitted.
    pub const fn resulting_state(self) -> SecurityIncidentPhase {
        match self {
            Self::Contain => SecurityIncidentPhase::Contained,
            Self::Fence => SecurityIncidentPhase::Fenced,
            Self::Reconcile => SecurityIncidentPhase::Reconciled,
            Self::Close => SecurityIncidentPhase::Closed,
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "contain" => Ok(Self::Contain),
            "fence" => Ok(Self::Fence),
            "reconcile" => Ok(Self::Reconcile),
            "close" => Ok(Self::Close),
            _ => Err("security_incident_action_invalid".to_owned()),
        }
    }
}

/// A committed fact the incident is allowed to cite.
///
/// Evidence is a reference plus a digest, never a payload. That is what keeps a `SecretLeak`
/// incident record from becoming the second copy of the secret, and it is why every field here is
/// length-bounded and secret-scanned before it can be sealed into a digest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityIncidentEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    /// Opaque reference into a committed fact: an event id, an audit record, a projection head.
    pub evidence_ref: String,
    /// Digest of the referenced fact at the moment it was cited.
    pub source_digest: String,
    /// The EventLog position the reference resolves to. Zero is not a valid position.
    pub source_cursor: EventCursor,
    pub evidence_digest: String,
}

impl SecurityIncidentEvidence {
    pub fn new(
        evidence_ref: impl Into<String>,
        source_digest: impl Into<String>,
        source_cursor: EventCursor,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: SECURITY_INCIDENT_SCHEMA.to_owned(),
            version: SECURITY_INCIDENT_VERSION,
            evidence_ref: evidence_ref.into(),
            source_digest: source_digest.into(),
            source_cursor,
            evidence_digest: String::new(),
        };
        value.evidence_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_INCIDENT_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_INCIDENT_VERSION)
            || self.source_cursor == 0
        {
            return Err("security_incident_evidence_header_invalid".to_owned());
        }
        safe_text(&self.evidence_ref, "security_incident_evidence_ref")?;
        valid_digest(
            &self.source_digest,
            "security_incident_evidence_source_digest",
        )?;
        valid_digest(&self.evidence_digest, "security_incident_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("security_incident_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "evidence_ref": self.evidence_ref,
            "source_digest": self.source_digest,
            "source_cursor": self.source_cursor,
        }))
    }
}

/// A supply-chain finding an incident can be raised against.
///
/// Vulnerability is a separate record from the incident on purpose: the same advisory usually
/// outlives any one response, while the incident is a single dated run at it. A withdrawn
/// vulnerability still raises an incident -- that is normal -- but it forces a different severity
/// floor, because a known-fixed advisory is not the same risk as an open one.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityVulnerability {
    pub schema: String,
    pub version: SchemaVersion,
    pub advisory_id: String,
    pub severity: SecurityIncidentSeverity,
    /// Lowest severity an incident may carry while this advisory is withdrawn.
    pub withdrawn_floor: SecurityIncidentSeverity,
    pub withdrawn: bool,
    pub summary: String,
    pub vulnerability_digest: String,
}

impl SecurityVulnerability {
    pub fn new(
        advisory_id: impl Into<String>,
        severity: SecurityIncidentSeverity,
        withdrawn: bool,
        summary: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: SECURITY_VULNERABILITY_SCHEMA.to_owned(),
            version: SECURITY_INCIDENT_VERSION,
            advisory_id: advisory_id.into(),
            severity,
            withdrawn_floor: SecurityIncidentSeverity::Medium,
            withdrawn,
            summary: summary.into(),
            vulnerability_digest: String::new(),
        };
        value.vulnerability_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_VULNERABILITY_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_INCIDENT_VERSION)
        {
            return Err("security_vulnerability_header_invalid".to_owned());
        }
        safe_text(&self.advisory_id, "security_vulnerability_advisory_id")?;
        safe_text(&self.summary, "security_vulnerability_summary")?;
        valid_digest(&self.vulnerability_digest, "security_vulnerability_digest")?;
        if self.vulnerability_digest != self.digest() {
            return Err("security_vulnerability_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Whether an incident of `severity` may be raised against this advisory.
    pub fn admits(&self, severity: SecurityIncidentSeverity) -> bool {
        !self.withdrawn || severity >= self.withdrawn_floor
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "advisory_id": self.advisory_id,
            "severity": self.severity,
            "withdrawn_floor": self.withdrawn_floor,
            "withdrawn": self.withdrawn,
            "summary": self.summary,
        }))
    }
}

/// The admitted incident record.
///
/// Admission is where the four card fields are enforced together. An incident with no severity, no
/// owner, no deadline or no evidence is not a degraded incident, it is an unaccountable one, so
/// none of the four is optional and none can be filled in later.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityIncident {
    pub schema: String,
    pub version: SchemaVersion,
    pub incident_id: String,
    pub class: SecurityIncidentClass,
    pub severity: SecurityIncidentSeverity,
    /// The named human accountable for this incident. Never a team, never a role, never empty.
    pub owner: String,
    /// Wall-clock deadline in unix milliseconds. A past deadline is a rejection, not a warning.
    pub deadline_unix_ms: u64,
    pub opened_at_unix_ms: u64,
    /// Set only for `SupplyChainDrift`; a class without one is rejected rather than downgraded.
    pub vulnerability: Option<SecurityVulnerability>,
    pub evidence: Vec<SecurityIncidentEvidence>,
    pub incident_digest: String,
}

impl SecurityIncident {
    #[allow(clippy::too_many_arguments)]
    pub fn open(
        incident_id: impl Into<String>,
        class: SecurityIncidentClass,
        severity: SecurityIncidentSeverity,
        owner: impl Into<String>,
        deadline_unix_ms: u64,
        opened_at_unix_ms: u64,
        vulnerability: Option<SecurityVulnerability>,
        evidence: Vec<SecurityIncidentEvidence>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: SECURITY_INCIDENT_SCHEMA.to_owned(),
            version: SECURITY_INCIDENT_VERSION,
            incident_id: incident_id.into(),
            class,
            severity,
            owner: owner.into(),
            deadline_unix_ms,
            opened_at_unix_ms,
            vulnerability,
            evidence,
            incident_digest: String::new(),
        };
        value.incident_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_INCIDENT_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_INCIDENT_VERSION)
        {
            return Err("security_incident_header_invalid".to_owned());
        }
        safe_text(&self.incident_id, "security_incident_id")?;
        // Emptiness is checked before `safe_text` so "nobody owns this" and "this owner string is
        // malformed" stay distinguishable; otherwise `safe_text` would collapse both into one code.
        if self.owner.trim().is_empty() {
            return Err("security_incident_owner_required".to_owned());
        }
        safe_text(&self.owner, "security_incident_owner")?;
        if self.opened_at_unix_ms == 0 {
            return Err("security_incident_opened_at_required".to_owned());
        }
        // A deadline that is unset, or already behind the moment the incident was opened, is not a
        // deadline. An incident nobody is racing has no severity ladder behind it either.
        if self.deadline_unix_ms == 0 {
            return Err("security_incident_deadline_required".to_owned());
        }
        if self.deadline_unix_ms <= self.opened_at_unix_ms {
            return Err("security_incident_deadline_expired".to_owned());
        }
        if self.evidence.is_empty() {
            return Err("security_incident_evidence_required".to_owned());
        }
        if self.evidence.len() > MAX_SECURITY_INCIDENT_EVIDENCE {
            return Err("security_incident_evidence_exhausted".to_owned());
        }
        let mut seen = BTreeSet::new();
        for item in &self.evidence {
            item.validate()?;
            if !seen.insert(item.evidence_digest.clone()) {
                return Err("security_incident_evidence_duplicate".to_owned());
            }
        }
        match (&self.class, &self.vulnerability) {
            (SecurityIncidentClass::SupplyChainDrift, None) => {
                return Err("security_incident_vulnerability_required".to_owned())
            }
            (_, Some(vulnerability)) => {
                vulnerability.validate()?;
                if !vulnerability.admits(self.severity) {
                    return Err("security_incident_severity_below_vulnerability_floor".to_owned());
                }
            }
            _ => {}
        }
        valid_digest(&self.incident_digest, "security_incident_digest")?;
        if self.incident_digest != self.digest() {
            return Err("security_incident_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "incident_id": self.incident_id,
            "class": self.class,
            "severity": self.severity,
            "owner": self.owner,
            "deadline_unix_ms": self.deadline_unix_ms,
            "opened_at_unix_ms": self.opened_at_unix_ms,
            "vulnerability": self.vulnerability.as_ref().map(|item| item.vulnerability_digest.clone()),
            "evidence": self.evidence.iter().map(|item| item.evidence_digest.clone()).collect::<Vec<String>>(),
        }))
    }

    /// Decide one proposed action against the incident's current state.
    ///
    /// Returns the decision rather than mutating anything: the caller re-derives it through
    /// [`SecurityIncidentReport::validate_against`] before treating it as fact.
    pub fn evaluate(
        &self,
        state: &SecurityIncidentState,
        action: &SecurityIncidentActionRequest,
    ) -> Result<SecurityIncidentReport, String> {
        self.validate()?;
        state.validate_against(self)?;
        action.validate_against(self)?;
        let (status, state_after, reason, remediation) = derive(self, state, action);
        let mut report = SecurityIncidentReport {
            schema: SECURITY_INCIDENT_REPORT_SCHEMA.to_owned(),
            version: SECURITY_INCIDENT_VERSION,
            incident_id: self.incident_id.clone(),
            action: action.action,
            status,
            state_before: state.state,
            state_after,
            reason,
            remediation,
            incident_digest: self.incident_digest.clone(),
            action_digest: action.action_digest.clone(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate_against(self, state, action)?;
        Ok(report)
    }
}

/// The current position of one incident in its lifecycle.
///
/// `consumed_evidence` is what makes evidence single-use. Each action may cite evidence exactly
/// once, so the same "the provider timed out" fact cannot justify a contain *and* a close, and an
/// evidence reference lifted from one incident cannot be replayed into another.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityIncidentState {
    pub schema: String,
    pub version: SchemaVersion,
    pub incident_id: String,
    pub state: SecurityIncidentPhase,
    /// Actions already applied, in order. A prefix of [`SecurityIncidentAction::ALL`].
    pub applied: Vec<SecurityIncidentAction>,
    pub consumed_evidence: BTreeSet<String>,
    /// The actor who applied the most recent action. Carried so a report cannot invent one.
    pub last_actor: String,
    pub updated_at_unix_ms: u64,
    pub state_digest: String,
}

impl SecurityIncidentState {
    pub fn new(
        incident_id: impl Into<String>,
        state: SecurityIncidentPhase,
        applied: Vec<SecurityIncidentAction>,
        consumed_evidence: BTreeSet<String>,
        last_actor: impl Into<String>,
        updated_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: SECURITY_INCIDENT_STATE_SCHEMA.to_owned(),
            version: SECURITY_INCIDENT_VERSION,
            incident_id: incident_id.into(),
            state,
            applied,
            consumed_evidence,
            last_actor: last_actor.into(),
            updated_at_unix_ms,
            state_digest: String::new(),
        };
        value.state_digest = value.digest();
        Ok(value)
    }

    /// The opening state: nothing applied, no evidence consumed, no actor yet.
    pub fn opened(incident_id: &str) -> Result<Self, String> {
        Self::new(
            incident_id,
            SecurityIncidentPhase::Open,
            Vec::new(),
            BTreeSet::new(),
            "",
            0,
        )
    }

    pub fn validate_against(&self, incident: &SecurityIncident) -> Result<(), String> {
        if self.schema != SECURITY_INCIDENT_STATE_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_INCIDENT_VERSION)
            || self.incident_id != incident.incident_id
        {
            return Err("security_incident_state_binding_invalid".to_owned());
        }
        if self.applied.len() > MAX_SECURITY_INCIDENT_ACTIONS {
            return Err("security_incident_state_actions_exhausted".to_owned());
        }
        // `applied` must be an exact prefix of the canonical order. A state that contains `close`
        // without `contain` is not a state this workflow can have produced.
        for (index, action) in self.applied.iter().enumerate() {
            if *action != SecurityIncidentAction::ALL[index] {
                return Err("security_incident_state_order_invalid".to_owned());
            }
        }
        // The recorded state must be exactly what the applied prefix implies -- no skipping ahead,
        // no rewinding after `Closed`.
        let derived = match self.applied.last() {
            Some(action) => action.resulting_state(),
            None => SecurityIncidentPhase::Open,
        };
        if self.state != derived {
            return Err("security_incident_state_not_derived".to_owned());
        }
        // Only cited evidence can have been consumed, and nothing can be consumed twice.
        let known: BTreeSet<String> = incident
            .evidence
            .iter()
            .map(|item| item.evidence_digest.clone())
            .collect();
        for digest in &self.consumed_evidence {
            if !known.contains(digest) {
                return Err("security_incident_state_evidence_unknown".to_owned());
            }
        }
        if !self.last_actor.is_empty() {
            safe_text(&self.last_actor, "security_incident_state_last_actor")?;
        }
        valid_digest(&self.state_digest, "security_incident_state_digest")?;
        if self.state_digest != self.digest() {
            return Err("security_incident_state_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "incident_id": self.incident_id,
            "state": self.state,
            "applied": self.applied.iter().map(|item| item.as_str()).collect::<Vec<&str>>(),
            "consumed_evidence": self.consumed_evidence.iter().cloned().collect::<Vec<String>>(),
            "last_actor": self.last_actor,
            "updated_at_unix_ms": self.updated_at_unix_ms,
        }))
    }
}

/// One proposed action: which incident, which step, who is doing it, and what it cites.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityIncidentActionRequest {
    pub schema: String,
    pub version: SchemaVersion,
    pub incident_id: String,
    pub action: SecurityIncidentAction,
    /// The named human performing the action. Anonymous transitions are not transitions.
    pub actor: String,
    pub acted_at_unix_ms: u64,
    /// Evidence this action cites. Must be evidence the incident actually carries, and none of it
    /// may have been consumed already.
    pub cited_evidence: Vec<SecurityIncidentEvidence>,
    /// Required for `Close`, forbidden for every other action.
    pub closure: Option<SecurityIncidentClosure>,
    pub action_digest: String,
}

impl SecurityIncidentActionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        incident_id: impl Into<String>,
        action: SecurityIncidentAction,
        actor: impl Into<String>,
        acted_at_unix_ms: u64,
        cited_evidence: Vec<SecurityIncidentEvidence>,
        closure: Option<SecurityIncidentClosure>,
    ) -> Result<Self, String> {
        let mut value = Self {
            schema: SECURITY_INCIDENT_ACTION_SCHEMA.to_owned(),
            version: SECURITY_INCIDENT_VERSION,
            incident_id: incident_id.into(),
            action,
            actor: actor.into(),
            acted_at_unix_ms,
            cited_evidence,
            closure,
            action_digest: String::new(),
        };
        value.action_digest = value.digest();
        value.validate_against_schema()?;
        Ok(value)
    }

    fn validate_against_schema(&self) -> Result<(), String> {
        if self.schema != SECURITY_INCIDENT_ACTION_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_INCIDENT_VERSION)
        {
            return Err("security_incident_action_header_invalid".to_owned());
        }
        safe_text(&self.incident_id, "security_incident_action_incident_id")?;
        if self.actor.trim().is_empty() {
            return Err("security_incident_action_actor_required".to_owned());
        }
        safe_text(&self.actor, "security_incident_action_actor")?;
        if self.acted_at_unix_ms == 0 {
            return Err("security_incident_action_timestamp_required".to_owned());
        }
        if self.cited_evidence.len() > MAX_SECURITY_INCIDENT_EVIDENCE {
            return Err("security_incident_action_evidence_exhausted".to_owned());
        }
        let mut seen = BTreeSet::new();
        for item in &self.cited_evidence {
            item.validate()?;
            if !seen.insert(item.evidence_digest.clone()) {
                return Err("security_incident_action_evidence_duplicate".to_owned());
            }
        }
        // Closure travels with close and only with close, so a contain cannot smuggle in a
        // root cause and a close cannot arrive without one.
        match (&self.closure, self.action) {
            (None, SecurityIncidentAction::Close) => {
                return Err("security_incident_closure_required".to_owned())
            }
            (Some(_), SecurityIncidentAction::Close) => {}
            (Some(_), _) => return Err("security_incident_closure_not_allowed".to_owned()),
            (None, _) => {}
        }
        if let Some(closure) = &self.closure {
            closure.validate()?;
        }
        valid_digest(&self.action_digest, "security_incident_action_digest")?;
        if self.action_digest != self.digest() {
            return Err("security_incident_action_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_against(&self, incident: &SecurityIncident) -> Result<(), String> {
        self.validate_against_schema()?;
        if self.incident_id != incident.incident_id {
            return Err("security_incident_action_binding_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "incident_id": self.incident_id,
            "action": self.action,
            "actor": self.actor,
            "acted_at_unix_ms": self.acted_at_unix_ms,
            "cited_evidence": self.cited_evidence.iter().map(|item| item.evidence_digest.clone()).collect::<Vec<String>>(),
            "closure": self.closure.as_ref().map(|item| item.closure_digest.clone()),
        }))
    }
}

/// What closure has to assert before an incident may be retired.
///
/// A closure is a signed claim, so it carries the six things the constitution requires and refuses
/// to accept "handled" as an answer. `reviewer` must differ from the incident `owner`: the person
/// who ran the response cannot also be the person who signs it off.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityIncidentClosure {
    pub root_cause: String,
    pub impact_scope: String,
    /// The containment actions actually taken, in order. An empty list means nothing was done.
    pub containment_actions: Vec<String>,
    pub residual_risk: String,
    pub reviewer: String,
    pub closure_digest: String,
}

impl SecurityIncidentClosure {
    pub fn new(
        root_cause: impl Into<String>,
        impact_scope: impl Into<String>,
        containment_actions: Vec<String>,
        residual_risk: impl Into<String>,
        reviewer: impl Into<String>,
    ) -> Result<Self, String> {
        let mut value = Self {
            root_cause: root_cause.into(),
            impact_scope: impact_scope.into(),
            containment_actions,
            residual_risk: residual_risk.into(),
            reviewer: reviewer.into(),
            closure_digest: String::new(),
        };
        value.closure_digest = value.digest();
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.containment_actions.is_empty() {
            return Err("security_incident_closure_containment_required".to_owned());
        }
        if self.containment_actions.len() > MAX_SECURITY_INCIDENT_ACTIONS {
            return Err("security_incident_closure_containment_exhausted".to_owned());
        }
        for action in &self.containment_actions {
            safe_text(action, "security_incident_closure_containment")?;
        }
        safe_text(&self.root_cause, "security_incident_closure_root_cause")?;
        safe_text(&self.impact_scope, "security_incident_closure_impact_scope")?;
        safe_text(
            &self.residual_risk,
            "security_incident_closure_residual_risk",
        )?;
        if self.reviewer.trim().is_empty() {
            return Err("security_incident_closure_reviewer_required".to_owned());
        }
        safe_text(&self.reviewer, "security_incident_closure_reviewer")?;
        valid_digest(&self.closure_digest, "security_incident_closure_digest")?;
        if self.closure_digest != self.digest() {
            return Err("security_incident_closure_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "root_cause": self.root_cause,
            "impact_scope": self.impact_scope,
            "containment_actions": self.containment_actions,
            "residual_risk": self.residual_risk,
            "reviewer": self.reviewer,
        }))
    }
}

/// The decision for one proposed action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityIncidentReportStatus {
    /// The action is admissible and moves the incident to `state_after`.
    Admitted,
    /// Not admissible. `reason` is always populated.
    Rejected,
}

impl SecurityIncidentReportStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Rejected => "rejected",
        }
    }
}

/// The sealed decision for one action.
///
/// `validate_against` re-derives every field from the same three inputs, so a hand-edited status,
/// state or reason cannot be published as though the workflow had decided it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityIncidentReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub incident_id: String,
    pub action: SecurityIncidentAction,
    pub status: SecurityIncidentReportStatus,
    pub state_before: SecurityIncidentPhase,
    pub state_after: SecurityIncidentPhase,
    pub reason: String,
    pub remediation: String,
    pub incident_digest: String,
    pub action_digest: String,
    pub report_digest: String,
}

impl SecurityIncidentReport {
    pub fn admitted(&self) -> bool {
        self.status == SecurityIncidentReportStatus::Admitted
    }

    /// Re-derive the decision from the same incident, state and action.
    ///
    /// This is the check that makes the report unpublishable-on-its-own-terms: any field a caller
    /// edits after the fact no longer matches the derivation, and the report is refused.
    pub fn validate_against(
        &self,
        incident: &SecurityIncident,
        state: &SecurityIncidentState,
        action: &SecurityIncidentActionRequest,
    ) -> Result<(), String> {
        incident.validate()?;
        state.validate_against(incident)?;
        action.validate_against(incident)?;
        let (status, state_after, reason, remediation) = derive(incident, state, action);
        if self.schema != SECURITY_INCIDENT_REPORT_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_INCIDENT_VERSION)
            || self.incident_id != incident.incident_id
            || self.action != action.action
            || self.status != status
            || self.state_before != state.state
            || self.state_after != state_after
            || self.reason != reason
            || self.remediation != remediation
            || self.incident_digest != incident.incident_digest
            || self.action_digest != action.action_digest
        {
            return Err("security_incident_report_binding_invalid".to_owned());
        }
        // Only a rejection carries a reason, and every rejection must carry one -- a "rejected"
        // report with an empty reason would hide which rule fired.
        if (self.status == SecurityIncidentReportStatus::Rejected) != !self.reason.is_empty() {
            return Err("security_incident_report_reason_state_mismatch".to_owned());
        }
        if self.status != SecurityIncidentReportStatus::Rejected && !self.remediation.is_empty() {
            return Err("security_incident_report_remediation_with_outcome".to_owned());
        }
        if self.status == SecurityIncidentReportStatus::Admitted
            && self.state_after <= self.state_before
        {
            return Err("security_incident_report_state_not_advanced".to_owned());
        }
        if !self.reason.is_empty() {
            safe_text(&self.reason, "security_incident_report_reason")?;
        }
        if !self.remediation.is_empty() {
            safe_text(&self.remediation, "security_incident_report_remediation")?;
        }
        valid_digest(
            &self.incident_digest,
            "security_incident_report_incident_digest",
        )?;
        valid_digest(
            &self.action_digest,
            "security_incident_report_action_digest",
        )?;
        valid_digest(&self.report_digest, "security_incident_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("security_incident_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "incident_id": self.incident_id,
            "action": self.action,
            "status": self.status,
            "state_before": self.state_before,
            "state_after": self.state_after,
            "reason": self.reason,
            "remediation": self.remediation,
            "incident_digest": self.incident_digest,
            "action_digest": self.action_digest,
        }))
    }
}

/// The whole decision, in one place.
///
/// Order matters here. Each branch returns a *complete* outcome, so there is exactly one reason a
/// caller can be given, and the reasons are ordered from "this is addressed to the wrong incident"
/// down to "you cited evidence you have already spent".
fn derive(
    incident: &SecurityIncident,
    state: &SecurityIncidentState,
    action: &SecurityIncidentActionRequest,
) -> (
    SecurityIncidentReportStatus,
    SecurityIncidentPhase,
    String,
    String,
) {
    let held = state.state;
    let rejected = |reason: &str, remediation: String| {
        (
            SecurityIncidentReportStatus::Rejected,
            held,
            reason.to_owned(),
            remediation,
        )
    };

    // A closed incident is terminal. This is checked before the ordering rules so that "reopen a
    // closed incident" reports the real obstacle -- irreversibility -- not a generic bad step.
    if held == SecurityIncidentPhase::Closed {
        return rejected(
            "security_incident_state_irreversible",
            "a closed incident is terminal: raise a new incident that cites the closed one as evidence"
                .to_owned(),
        );
    }
    // The incident the action names is not the one being decided.
    if state.incident_id != incident.incident_id {
        return rejected(
            "security_incident_state_binding_invalid",
            "evaluate the action against the state that belongs to the same incident".to_owned(),
        );
    }
    // An action performed after its own deadline is a late action, not an on-time one.
    if action.acted_at_unix_ms > incident.deadline_unix_ms {
        return rejected(
            "security_incident_deadline_exceeded",
            format!(
                "the incident deadline passed at {}; escalate instead of acting late",
                incident.deadline_unix_ms
            ),
        );
    }
    // Evidence must belong to this incident. A reference lifted from a different incident is not
    // evidence about this one, and accepting it would let one response justify another.
    let known: BTreeSet<String> = incident
        .evidence
        .iter()
        .map(|item| item.evidence_digest.clone())
        .collect();
    for item in &action.cited_evidence {
        if !known.contains(&item.evidence_digest) {
            return rejected(
                "security_incident_evidence_not_in_incident",
                "cite evidence this incident carries, not a reference copied from elsewhere"
                    .to_owned(),
            );
        }
    }
    // Evidence is single-use, across every action of this incident. Re-citing a spent fact is how
    // one timeout ends up justifying a contain, a reconcile and a close.
    for item in &action.cited_evidence {
        if state.consumed_evidence.contains(&item.evidence_digest) {
            return rejected(
                "security_incident_evidence_replay",
                "this evidence was already consumed by an earlier action; cite a new committed fact"
                    .to_owned(),
            );
        }
    }
    // An action that cites nothing proves nothing.
    if action.cited_evidence.is_empty() {
        return rejected(
            "security_incident_evidence_required",
            "every action cites at least one committed fact".to_owned(),
        );
    }

    // The ordering rule. `reconcile` is the step that turns an unknown into a known, so it is the
    // one that can never be skipped: a close without it would launder an unknown into a success.
    if action.action == SecurityIncidentAction::Close && held != SecurityIncidentPhase::Reconciled {
        return rejected(
            "security_incident_reconcile_required",
            "reconcile the external result before closing: an unreconciled unknown is not a success"
                .to_owned(),
        );
    }
    if action.action.required_state() != held {
        return rejected(
            "security_incident_action_out_of_order",
            format!(
                "the incident is {}; the next admissible step is {}",
                held.as_str(),
                SecurityIncidentAction::ALL[held as usize].as_str()
            ),
        );
    }
    // Closure is where separation of duties is enforced: the responder cannot sign off.
    if let Some(closure) = &action.closure {
        if closure.reviewer == incident.owner {
            return rejected(
                "security_incident_closure_reviewer_conflict",
                "the incident owner cannot be the closure reviewer".to_owned(),
            );
        }
    }
    (
        SecurityIncidentReportStatus::Admitted,
        action.action.resulting_state(),
        String::new(),
        String::new(),
    )
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_SECURITY_INCIDENT_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Event, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
