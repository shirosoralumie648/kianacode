//! EQ-41 immutable `QualityGate` configuration and append-only decision separation.
//!
//! A gate separates an immutable configuration ([`QualityGateConfig`]: identity, version, suite
//! digest, named thresholds and declared blocking rules) from an immutable decision
//! ([`QualityGateDecision`]: the exact config version, suite and candidate digests, verdict,
//! score delta and evidence lists).  Published configuration versions are append-only: a new
//! version must share the gate identity and carry a strictly higher version, and a version already
//! published can never be rewritten (`gate_config_version_rewritten`).  Because the version is
//! part of the digested content, two published versions can never share a digest.
//!
//! Decisions are append-only as well: a decision binds the untampered config digest it was made
//! under, the same `decision_id` can never be rewritten (`gate_decision_rewritten`), a candidate
//! may only be decided once per config version (`gate_decision_candidate_redecided`), the config
//! version, evidence cursor and decision time cannot regress
//! (`gate_decision_log_version_regressed`), and a Pass that carries blocking findings is
//! structurally impossible (`gate_decision_verdict_inconsistent`).
//!
//! This is a pure value contract.  It does not persist a gate, does not change a route, policy,
//! grant or receipt, does not execute a target, does not decide any authority and never turns a
//! verdict into an effect.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Versioned wire schema for one immutable gate configuration version.
pub const GATE_CONFIG_SCHEMA: &str = "kiana.quality-gate-config.v2";
/// Versioned wire schema for one immutable gate decision.
pub const GATE_DECISION_SCHEMA: &str = "kiana.quality-gate-decision.v2";
/// Versioned wire schema for one append-only configuration history.
pub const GATE_CONFIG_HISTORY_SCHEMA: &str = "kiana.quality-gate-config-history.v1";
/// Versioned wire schema for one append-only decision log.
pub const GATE_DECISION_LOG_SCHEMA: &str = "kiana.quality-gate-decision-log.v1";
/// Versioned envelope for one config/decision separation check.
pub const GATE_INPUT_SCHEMA: &str = "kiana.quality-gate-input.v2";
/// Versioned envelope for one configuration-history/decision-log check.
pub const GATE_LOG_INPUT_SCHEMA: &str = "kiana.quality-gate-log-input.v1";
/// Stable registry identifier for the config/decision separation evaluator.
pub const GATE_EVALUATOR_ID: &str = "quality-gate";
/// Stable registry identifier for the append-only history/log evaluator.
pub const GATE_LOG_EVALUATOR_ID: &str = "quality-gate-log";
/// Maximum bytes accepted in one bounded identity, rule or evidence token.
pub const MAX_GATE_TEXT_BYTES: usize = 512;
/// Maximum named thresholds in one configuration version.
pub const MAX_GATE_THRESHOLDS: usize = 16;
/// Maximum declared blocking rules in one configuration version.
pub const MAX_GATE_BLOCKING_RULES: usize = 16;
/// Maximum blocking findings or known regressions in one decision.
pub const MAX_GATE_EVIDENCE_REFS: usize = 64;
/// Maximum published configuration versions in one history.
pub const MAX_GATE_CONFIG_VERSIONS: usize = 64;
/// Maximum decisions in one append-only log.
pub const MAX_GATE_DECISIONS: usize = 256;
/// Upper bound for one per-mille threshold or score delta.
pub const MAX_GATE_SCORE_MILLI: u64 = 1_000_000;
/// Closed threshold vocabulary; an unknown dimension fails closed.
pub const GATE_THRESHOLD_DIMENSIONS: [&str; 8] = [
    "context",
    "cost",
    "evidence_completeness",
    "functional",
    "latency",
    "policy_safety",
    "recovery_correctness",
    "replay_correctness",
];
/// Closed blocking-rule vocabulary; an unknown rule fails closed.
pub const GATE_BLOCKING_RULES: [&str; 6] = [
    "evidence",
    "fixture_integrity",
    "forbidden_effect",
    "infra",
    "replay",
    "safety",
];

const GATE_EVIDENCE: &str = "gate:input";
const LOG_EVIDENCE: &str = "gate:log";

/// Verdict recorded by one gate decision.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    Pass,
    Reject,
    NeedsShadow,
    Rollback,
}

impl GateVerdict {
    /// Every verdict accepted by this contract, in stable rank order.
    pub const ALL: [Self; 4] = [Self::Pass, Self::Reject, Self::NeedsShadow, Self::Rollback];

    /// The stable wire key for this verdict.
    pub fn key(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Reject => "reject",
            Self::NeedsShadow => "needs_shadow",
            Self::Rollback => "rollback",
        }
    }
}

/// One immutable QualityGate configuration version.
///
/// The configuration carries no verdict, candidate, approver or decision field: those belong to
/// [`QualityGateDecision`].  `config_digest` binds every other field, so editing a published
/// version produces a different identity that cannot reuse the earlier digest.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityGateConfig {
    /// Wire schema; must equal [`GATE_CONFIG_SCHEMA`].
    pub schema: String,
    /// Stable gate identity shared by every version of one gate.
    pub gate_id: String,
    /// Monotonically increasing version inside one gate, starting at 1.
    pub version: u64,
    /// Digest of the suite version this configuration was written for.
    pub suite_digest: String,
    /// Named per-mille thresholds, restricted to [`GATE_THRESHOLD_DIMENSIONS`].
    pub thresholds: BTreeMap<String, u64>,
    /// Declared blocking rules, sorted and unique, restricted to [`GATE_BLOCKING_RULES`].
    pub blocking_rules: Vec<String>,
    /// Canonical digest binding every other field.
    pub config_digest: String,
}

impl QualityGateConfig {
    /// Build one configuration version and bind its canonical digest.
    pub fn new(
        gate_id: impl Into<String>,
        version: u64,
        suite_digest: impl Into<String>,
        thresholds: BTreeMap<String, u64>,
        blocking_rules: Vec<String>,
    ) -> Result<Self, &'static str> {
        let mut config = Self {
            schema: GATE_CONFIG_SCHEMA.to_owned(),
            gate_id: gate_id.into(),
            version,
            suite_digest: suite_digest.into(),
            thresholds,
            blocking_rules,
            config_digest: String::new(),
        };
        config.config_digest = config.digest();
        config.validate()?;
        Ok(config)
    }

    /// Validate the schema, bounded identity, threshold/rule vocabulary and digest binding.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != GATE_CONFIG_SCHEMA
            || !valid_text(&self.gate_id)
            || self.version == 0
            || !valid_digest(&self.suite_digest)
            || !valid_digest(&self.config_digest)
            || self.thresholds.is_empty()
            || self.thresholds.len() > MAX_GATE_THRESHOLDS
            || self.thresholds.iter().any(|(dimension, value)| {
                !valid_text(dimension) || *value == 0 || *value > MAX_GATE_SCORE_MILLI
            })
            || self.blocking_rules.is_empty()
            || self.blocking_rules.len() > MAX_GATE_BLOCKING_RULES
            || !bounded_sorted_texts(&self.blocking_rules)
        {
            return Err("gate_config_header_invalid");
        }
        if self
            .thresholds
            .keys()
            .any(|dimension| !GATE_THRESHOLD_DIMENSIONS.contains(&dimension.as_str()))
        {
            return Err("gate_config_threshold_unknown");
        }
        if self
            .blocking_rules
            .iter()
            .any(|rule| !GATE_BLOCKING_RULES.contains(&rule.as_str()))
        {
            return Err("gate_config_blocking_rule_unknown");
        }
        if self.config_digest != self.digest() {
            return Err("gate_config_digest_mismatch");
        }
        Ok(())
    }

    /// Canonical digest binding every configured field.
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "gate_id": self.gate_id,
            "version": self.version,
            "suite_digest": self.suite_digest,
            "thresholds": self.thresholds,
            "blocking_rules": self.blocking_rules,
        }))
    }

    /// Canonical digest of the named thresholds of one configuration version.
    pub fn thresholds_digest(&self) -> String {
        json_digest(&serde_json::json!({
            "gate_id": self.gate_id,
            "version": self.version,
            "thresholds": self.thresholds,
        }))
    }
}

/// The configuration-update law: a replacement version must share the gate identity and be
/// strictly higher than the current version.  It never mutates `current`, and because the version
/// is part of the digested content a valid replacement can never reuse the current digest.
pub fn quality_gate_config_update(
    current: &QualityGateConfig,
    updated: &QualityGateConfig,
) -> Result<(), &'static str> {
    current
        .validate()
        .map_err(|_| "gate_config_update_current_invalid")?;
    updated.validate()?;
    if updated.gate_id != current.gate_id {
        return Err("gate_config_update_identity_mismatch");
    }
    if updated.version <= current.version {
        return Err("gate_config_update_version_not_incremented");
    }
    Ok(())
}

/// One append-only configuration history for a single gate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateConfigHistory {
    /// Wire schema; must equal [`GATE_CONFIG_HISTORY_SCHEMA`].
    pub schema: String,
    /// Stable gate identity shared by every version in this history.
    pub gate_id: String,
    /// Published versions, consecutive from 1 and never rewritten.
    pub versions: Vec<QualityGateConfig>,
    /// Canonical digest binding the identity and every published version.
    pub history_digest: String,
}

impl GateConfigHistory {
    /// Start an empty history for one gate.
    pub fn new(gate_id: impl Into<String>) -> Self {
        let mut history = Self {
            schema: GATE_CONFIG_HISTORY_SCHEMA.to_owned(),
            gate_id: gate_id.into(),
            versions: Vec::new(),
            history_digest: String::new(),
        };
        history.history_digest = history.digest();
        history
    }

    /// The newest published version, if any.
    pub fn latest(&self) -> Option<&QualityGateConfig> {
        self.versions.last()
    }

    /// Look up one published version by number.
    pub fn find(&self, version: u64) -> Option<&QualityGateConfig> {
        self.versions
            .iter()
            .find(|config| config.version == version)
    }

    /// Append one published version.
    ///
    /// Returns `Ok(true)` when the version was appended, `Ok(false)` when the exact same version
    /// and digest was already published (a replay that mutates nothing), and a refusal when the
    /// entry is invalid, the identity differs, a published version would be rewritten, or the
    /// version ordering is not consecutive.
    pub fn append(&mut self, config: QualityGateConfig) -> Result<bool, &'static str> {
        config
            .validate()
            .map_err(|_| "gate_config_history_entry_invalid")?;
        if config.gate_id != self.gate_id {
            return Err("gate_config_history_identity_mismatch");
        }
        if let Some(existing) = self.find(config.version) {
            if existing.config_digest == config.config_digest {
                return Ok(false);
            }
            return Err("gate_config_version_rewritten");
        }
        let expected = self
            .versions
            .last()
            .map(|entry| entry.version + 1)
            .unwrap_or(1);
        if config.version != expected {
            return Err(if config.version < expected {
                "gate_config_version_regressed"
            } else {
                "gate_config_version_gap"
            });
        }
        self.versions.push(config);
        self.history_digest = self.digest();
        Ok(true)
    }

    /// Validate the schema, identity, consecutive version ordering and digest binding.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != GATE_CONFIG_HISTORY_SCHEMA
            || !valid_text(&self.gate_id)
            || self.versions.len() > MAX_GATE_CONFIG_VERSIONS
            || !valid_digest(&self.history_digest)
        {
            return Err("gate_config_history_header_invalid");
        }
        let mut expected = 1;
        for entry in &self.versions {
            entry
                .validate()
                .map_err(|_| "gate_config_history_entry_invalid")?;
            if entry.gate_id != self.gate_id {
                return Err("gate_config_history_identity_mismatch");
            }
            if entry.version != expected {
                return Err(if entry.version < expected {
                    "gate_config_version_regressed"
                } else {
                    "gate_config_version_gap"
                });
            }
            expected += 1;
        }
        if self.history_digest != self.digest() {
            return Err("gate_config_history_digest_mismatch");
        }
        Ok(())
    }

    /// Canonical digest binding the identity and every published version.
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "gate_id": self.gate_id,
            "versions": self.versions,
        }))
    }
}

/// One immutable gate decision.
///
/// The decision carries no threshold or rule field: those belong to the configuration version it
/// binds through `gate_id`, `gate_version` and `config_digest`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityGateDecision {
    /// Wire schema; must equal [`GATE_DECISION_SCHEMA`].
    pub schema: String,
    /// Stable decision identity; the same identity can never be rewritten.
    pub decision_id: String,
    /// Gate identity copied from the bound configuration version.
    pub gate_id: String,
    /// Configuration version this decision bound at decision time.
    pub gate_version: u64,
    /// Digest of the exact configuration version this decision bound.
    pub config_digest: String,
    /// Digest of the suite version the decision was made for.
    pub suite_digest: String,
    /// Candidate identity the decision is about.
    pub candidate_id: String,
    /// Digest of the exact candidate version the decision is about.
    pub candidate_digest: String,
    /// Recorded verdict.
    pub verdict: GateVerdict,
    /// Recorded per-mille score delta, if any.
    pub score_delta_milli: i64,
    /// Blocking findings, sorted and unique; a resolved blocker is a terminal reason.
    pub blocking_findings: Vec<String>,
    /// Known regressions, sorted and unique.
    pub known_regressions: Vec<String>,
    /// Bounded reference to the evidence the decision cites.
    pub evidence_ref: String,
    /// Durable evidence cursor the decision was made at; strictly increasing per gate.
    pub source_cursor: u64,
    /// Decision time in Unix milliseconds.
    pub decided_at_unix_ms: u64,
    /// Canonical digest binding every other field.
    pub decision_digest: String,
}

impl QualityGateDecision {
    /// Build one decision from the exact configuration version it binds.
    ///
    /// `gate_id`, `gate_version`, `config_digest` and `suite_digest` are copied from `config`, so a
    /// caller cannot construct a decision that cites a configuration version it did not bind.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        decision_id: impl Into<String>,
        config: &QualityGateConfig,
        candidate_id: impl Into<String>,
        candidate_digest: impl Into<String>,
        verdict: GateVerdict,
        score_delta_milli: i64,
        blocking_findings: Vec<String>,
        known_regressions: Vec<String>,
        evidence_ref: impl Into<String>,
        source_cursor: u64,
        decided_at_unix_ms: u64,
    ) -> Result<Self, &'static str> {
        config.validate()?;
        let mut decision = Self {
            schema: GATE_DECISION_SCHEMA.to_owned(),
            decision_id: decision_id.into(),
            gate_id: config.gate_id.clone(),
            gate_version: config.version,
            config_digest: config.config_digest.clone(),
            suite_digest: config.suite_digest.clone(),
            candidate_id: candidate_id.into(),
            candidate_digest: candidate_digest.into(),
            verdict,
            score_delta_milli,
            blocking_findings,
            known_regressions,
            evidence_ref: evidence_ref.into(),
            source_cursor,
            decided_at_unix_ms,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    /// Validate the header, the verdict/evidence consistency matrix and the digest binding.
    ///
    /// A `Pass` must carry neither blocking findings nor known regressions, `Reject` and
    /// `Rollback` must carry at least one reason, and `NeedsShadow` may not conceal a blocking
    /// finding.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != GATE_DECISION_SCHEMA
            || !valid_text(&self.decision_id)
            || !valid_text(&self.gate_id)
            || self.gate_version == 0
            || !valid_digest(&self.config_digest)
            || !valid_digest(&self.suite_digest)
            || !valid_text(&self.candidate_id)
            || !valid_digest(&self.candidate_digest)
            || !valid_text(&self.evidence_ref)
            || self.source_cursor == 0
            || self.decided_at_unix_ms == 0
            || self.score_delta_milli.unsigned_abs() > MAX_GATE_SCORE_MILLI
            || !bounded_sorted_texts(&self.blocking_findings)
            || !bounded_sorted_texts(&self.known_regressions)
            || !valid_digest(&self.decision_digest)
        {
            return Err("gate_decision_header_invalid");
        }
        let blocked = !self.blocking_findings.is_empty();
        let regressed = !self.known_regressions.is_empty();
        let consistent = match self.verdict {
            GateVerdict::Pass => !blocked && !regressed,
            GateVerdict::Reject | GateVerdict::Rollback => blocked || regressed,
            GateVerdict::NeedsShadow => !blocked,
        };
        if !consistent {
            return Err("gate_decision_verdict_inconsistent");
        }
        if self.decision_digest != self.digest() {
            return Err("gate_decision_digest_mismatch");
        }
        Ok(())
    }

    /// Canonical digest binding every recorded field.
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "decision_id": self.decision_id,
            "gate_id": self.gate_id,
            "gate_version": self.gate_version,
            "config_digest": self.config_digest,
            "suite_digest": self.suite_digest,
            "candidate_id": self.candidate_id,
            "candidate_digest": self.candidate_digest,
            "verdict": self.verdict,
            "score_delta_milli": self.score_delta_milli,
            "blocking_findings": self.blocking_findings,
            "known_regressions": self.known_regressions,
            "evidence_ref": self.evidence_ref,
            "source_cursor": self.source_cursor,
            "decided_at_unix_ms": self.decided_at_unix_ms,
        }))
    }
}

/// The decision-binding law: a decision must cite the exact identity, version, config digest and
/// suite digest of the configuration version it was made under.  An empty result is the only
/// valid binding.
pub fn quality_gate_decision_binding(
    config: &QualityGateConfig,
    decision: &QualityGateDecision,
) -> Result<(), &'static str> {
    if decision.gate_id != config.gate_id {
        return Err("gate_decision_config_identity_mismatch");
    }
    if decision.gate_version != config.version {
        return Err("gate_decision_gate_version_mismatch");
    }
    if decision.config_digest != config.config_digest {
        return Err("gate_decision_config_digest_mismatch");
    }
    if decision.suite_digest != config.suite_digest {
        return Err("gate_decision_suite_digest_mismatch");
    }
    Ok(())
}

/// One append-only decision log for a single gate.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityGateDecisionLog {
    /// Wire schema; must equal [`GATE_DECISION_LOG_SCHEMA`].
    pub schema: String,
    /// Stable gate identity shared by every decision in this log.
    pub gate_id: String,
    /// Decisions in append order; an existing decision is never rewritten.
    pub decisions: Vec<QualityGateDecision>,
    /// Canonical digest binding the identity and every decision.
    pub log_digest: String,
}

impl QualityGateDecisionLog {
    /// Start an empty log for one gate.
    pub fn new(gate_id: impl Into<String>) -> Self {
        let mut log = Self {
            schema: GATE_DECISION_LOG_SCHEMA.to_owned(),
            gate_id: gate_id.into(),
            decisions: Vec::new(),
            log_digest: String::new(),
        };
        log.log_digest = log.digest();
        log
    }

    /// The newest decision, if any.
    pub fn latest(&self) -> Option<&QualityGateDecision> {
        self.decisions.last()
    }

    /// Look up one decision by identity.
    pub fn find(&self, decision_id: &str) -> Option<&QualityGateDecision> {
        self.decisions
            .iter()
            .find(|decision| decision.decision_id == decision_id)
    }

    /// Append one decision.
    ///
    /// Returns `Ok(true)` when the decision was appended, `Ok(false)` when an identical decision
    /// (same `decision_id` and digest) was replayed, and a refusal when the entry is invalid, the
    /// gate identity differs, a decision would be rewritten, a candidate would be re-decided
    /// under the same config version, or the version, cursor or time ordering would regress.
    pub fn append(&mut self, decision: QualityGateDecision) -> Result<bool, &'static str> {
        decision
            .validate()
            .map_err(|_| "gate_decision_log_entry_invalid")?;
        if decision.gate_id != self.gate_id {
            return Err("gate_decision_log_identity_mismatch");
        }
        if let Some(existing) = self.find(&decision.decision_id) {
            if existing.decision_digest == decision.decision_digest {
                return Ok(false);
            }
            return Err("gate_decision_rewritten");
        }
        if let Some(latest) = self.latest() {
            if decision.gate_version < latest.gate_version {
                return Err("gate_decision_log_version_regressed");
            }
            if decision.source_cursor <= latest.source_cursor {
                return Err("gate_decision_log_cursor_regressed");
            }
            if decision.decided_at_unix_ms < latest.decided_at_unix_ms {
                return Err("gate_decision_log_time_regressed");
            }
        }
        if self.decisions.iter().any(|existing| {
            existing.candidate_id == decision.candidate_id
                && existing.config_digest == decision.config_digest
        }) {
            return Err("gate_decision_candidate_redecided");
        }
        self.decisions.push(decision);
        self.log_digest = self.digest();
        Ok(true)
    }

    /// Validate the schema, identity, append-only ordering, uniqueness and digest binding.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != GATE_DECISION_LOG_SCHEMA
            || !valid_text(&self.gate_id)
            || self.decisions.len() > MAX_GATE_DECISIONS
            || !valid_digest(&self.log_digest)
        {
            return Err("gate_decision_log_header_invalid");
        }
        let mut decision_ids = BTreeSet::new();
        let mut candidate_bindings = BTreeSet::new();
        let mut previous: Option<&QualityGateDecision> = None;
        for decision in &self.decisions {
            decision
                .validate()
                .map_err(|_| "gate_decision_log_entry_invalid")?;
            if decision.gate_id != self.gate_id {
                return Err("gate_decision_log_identity_mismatch");
            }
            if !decision_ids.insert(decision.decision_id.as_str()) {
                return Err("gate_decision_rewritten");
            }
            if !candidate_bindings.insert((
                decision.candidate_id.as_str(),
                decision.config_digest.as_str(),
            )) {
                return Err("gate_decision_candidate_redecided");
            }
            if let Some(previous) = previous {
                if decision.gate_version < previous.gate_version {
                    return Err("gate_decision_log_version_regressed");
                }
                if decision.source_cursor <= previous.source_cursor {
                    return Err("gate_decision_log_cursor_regressed");
                }
                if decision.decided_at_unix_ms < previous.decided_at_unix_ms {
                    return Err("gate_decision_log_time_regressed");
                }
            }
            previous = Some(decision);
        }
        if self.log_digest != self.digest() {
            return Err("gate_decision_log_digest_mismatch");
        }
        Ok(())
    }

    /// Canonical digest binding the identity and every decision.
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "gate_id": self.gate_id,
            "decisions": self.decisions,
        }))
    }
}

/// Envelope for one config/decision separation check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateInput {
    /// Wire schema; must equal [`GATE_INPUT_SCHEMA`].
    pub schema: String,
    /// The configuration version the decision was made under.
    pub config: QualityGateConfig,
    /// The decision under separation check.
    pub decision: QualityGateDecision,
    /// Optional newer configuration version that must not mutate the decision.
    pub updated_config: Option<QualityGateConfig>,
}

impl GateInput {
    /// Build a separation envelope without a configuration update.
    pub fn new(config: QualityGateConfig, decision: QualityGateDecision) -> Self {
        Self {
            schema: GATE_INPUT_SCHEMA.to_owned(),
            config,
            decision,
            updated_config: None,
        }
    }

    /// Attach the candidate replacement configuration version.
    pub fn with_update(mut self, updated_config: QualityGateConfig) -> Self {
        self.updated_config = Some(updated_config);
        self
    }

    /// Validate the envelope schema.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != GATE_INPUT_SCHEMA {
            return Err("gate_input_schema_invalid");
        }
        Ok(())
    }

    /// Serialize the envelope into its wire value.
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("gate input is serializable")
    }
}

/// Envelope for one configuration-history/decision-log check.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateLogInput {
    /// Wire schema; must equal [`GATE_LOG_INPUT_SCHEMA`].
    pub schema: String,
    /// The append-only configuration history the decisions must bind.
    pub config_history: GateConfigHistory,
    /// The append-only decision log under check.
    pub log: QualityGateDecisionLog,
}

impl GateLogInput {
    /// Build the history/log envelope.
    pub fn new(config_history: GateConfigHistory, log: QualityGateDecisionLog) -> Self {
        Self {
            schema: GATE_LOG_INPUT_SCHEMA.to_owned(),
            config_history,
            log,
        }
    }

    /// Validate the envelope schema.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != GATE_LOG_INPUT_SCHEMA {
            return Err("gate_log_input_schema_invalid");
        }
        Ok(())
    }

    /// Serialize the envelope into its wire value.
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("gate log input is serializable")
    }
}

/// Config/decision separation evaluator.
#[derive(Clone, Copy, Debug, Default)]
pub struct GateEvaluator;

impl DeterministicEvaluator for GateEvaluator {
    fn evaluator_id(&self) -> &str {
        GATE_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: GateInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("gate"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_gate(&decoded)
    }
}

/// Append-only history/log evaluator.
#[derive(Clone, Copy, Debug, Default)]
pub struct GateLogEvaluator;

impl DeterministicEvaluator for GateLogEvaluator {
    fn evaluator_id(&self) -> &str {
        GATE_LOG_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: GateLogInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("gate_log"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_gate_log(&decoded)
    }
}

/// Check that one decision stays separated from and bound to one configuration version, and that
/// an optional replacement configuration is a strictly newer distinct version.  An empty finding
/// list is the only accepted result.
pub fn evaluate_gate(input: &GateInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let config_valid = match input.config.validate() {
        Ok(()) => true,
        Err(reason) => {
            emit(
                &mut findings,
                "gate.config_invalid",
                "validated_immutable_config",
                reason,
                GATE_EVIDENCE,
            )?;
            false
        }
    };
    let decision_valid = match input.decision.validate() {
        Ok(()) => true,
        Err(reason) => {
            let code = if reason == "gate_decision_verdict_inconsistent"
                && input.decision.verdict == GateVerdict::Pass
                && !input.decision.blocking_findings.is_empty()
            {
                "gate.pass_with_blocking_findings"
            } else {
                "gate.decision_invalid"
            };
            emit(
                &mut findings,
                code,
                "validated_immutable_decision",
                reason,
                GATE_EVIDENCE,
            )?;
            false
        }
    };
    if config_valid && decision_valid {
        if let Err(reason) = quality_gate_decision_binding(&input.config, &input.decision) {
            emit(
                &mut findings,
                "gate.decision_config_mismatch",
                "decision_binds_exact_config_version",
                reason,
                GATE_EVIDENCE,
            )?;
        }
    }
    if let Some(updated) = &input.updated_config {
        let updated_valid = match updated.validate() {
            Ok(()) => true,
            Err(reason) => {
                emit(
                    &mut findings,
                    "gate.updated_config_invalid",
                    "validated_new_config_version",
                    reason,
                    GATE_EVIDENCE,
                )?;
                false
            }
        };
        if updated_valid && config_valid {
            if let Err(reason) = quality_gate_config_update(&input.config, updated) {
                emit(
                    &mut findings,
                    "gate.config_update_invalid",
                    "new_higher_config_version",
                    reason,
                    GATE_EVIDENCE,
                )?;
            }
        }
    }
    Ok(findings)
}

/// Check that a configuration history and a decision log stay append-only and mutually bound.
///
/// The history may only publish consecutive versions that are never rewritten, every decision must
/// cite exactly the published configuration version it names, and the log may never rewrite a
/// decision, re-decide a candidate under the same config version, or regress the config version,
/// cursor or decision time.
pub fn evaluate_gate_log(input: &GateLogInput) -> Result<Vec<Finding>, EvaluatorError> {
    input.validate().map_err(EvaluatorError::InputInvalid)?;
    let mut findings = Vec::new();
    let history_valid = match input.config_history.validate() {
        Ok(()) => true,
        Err(reason) => {
            emit(
                &mut findings,
                "gate.config_history_invalid",
                "append_only_config_history",
                reason,
                LOG_EVIDENCE,
            )?;
            false
        }
    };
    let log_valid = match input.log.validate() {
        Ok(()) => true,
        Err(reason) => {
            let code = match reason {
                "gate_decision_rewritten" => "gate.decision_rewritten",
                "gate_decision_candidate_redecided" => "gate.candidate_redecided",
                "gate_decision_log_version_regressed" => "gate.config_version_regressed",
                "gate_decision_log_cursor_regressed" | "gate_decision_log_time_regressed" => {
                    "gate.log_order_invalid"
                }
                _ => "gate.log_invalid",
            };
            emit(
                &mut findings,
                code,
                "append_only_decision_log",
                reason,
                LOG_EVIDENCE,
            )?;
            false
        }
    };
    if !history_valid || !log_valid {
        return Ok(findings);
    }
    if input.config_history.gate_id != input.log.gate_id {
        emit(
            &mut findings,
            "gate.log_identity_mismatch",
            "one_gate_identity",
            "gate_id",
            LOG_EVIDENCE,
        )?;
        return Ok(findings);
    }
    for decision in &input.log.decisions {
        let Some(config) = input.config_history.find(decision.gate_version) else {
            emit(
                &mut findings,
                "gate.log_entry_version_ahead",
                "decision_binds_published_config_version",
                "version",
                LOG_EVIDENCE,
            )?;
            continue;
        };
        if let Err(reason) = quality_gate_decision_binding(config, decision) {
            emit(
                &mut findings,
                "gate.log_entry_config_mismatch",
                "decision_binds_published_config_version",
                reason,
                LOG_EVIDENCE,
            )?;
        }
    }
    Ok(findings)
}

fn bounded_sorted_texts(values: &[String]) -> bool {
    values.len() <= MAX_GATE_EVIDENCE_REFS
        && values.iter().all(|value| valid_text(value))
        && values.windows(2).all(|pair| pair[0] < pair[1])
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_GATE_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: &'static str,
    actual: &'static str,
    evidence_ref: &'static str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::new(
            code,
            Value::String(expected.to_owned()),
            Value::String(actual.to_owned()),
            format!("quality gate finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
