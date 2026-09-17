//! Foundational quality object identity and lifecycle contracts.
//!
//! EQ-02 intentionally stops at value objects and state transitions. Dataset/case semantics,
//! trace normalization, stores, judges and promotion authority are later roadmap steps.
use crate::{
    canonical_journal_bytes, json_digest, redact_text, EvalCaseId, EvalDatasetId, EvalSuiteId,
    GoldenTraceId, QualityArtifactId, QualityTransitionId, RunId, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const QUALITY_ARTIFACT_SCHEMA: &str = "kiana.quality-artifact.v1";
pub const QUALITY_TRANSITION_SCHEMA: &str = "kiana.quality-transition.v1";
pub const EVAL_DATASET_SCHEMA: &str = "kiana.eval-dataset.v1";
pub const EVAL_SUITE_OBJECT_SCHEMA: &str = "kiana.eval-suite.v1";
pub const EVAL_CASE_OBJECT_SCHEMA: &str = "kiana.eval-case.v1";
pub const GOLDEN_TRACE_SCHEMA: &str = "kiana.golden-trace.v1";
pub const QUALITY_OBJECT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const QUALITY_SCHEMA_VERSION: u32 = 1;
pub const MAX_QUALITY_OBJECT_TYPE: usize = 64;
pub const MAX_QUALITY_OWNER: usize = 256;
pub const MAX_QUALITY_REASON: usize = 2_048;
pub const MAX_EVAL_PROVENANCE: usize = 32;
pub const MAX_EVAL_REFERENCES: usize = 256;
pub const MAX_EVAL_EVENTS: usize = 4_096;
pub const MAX_EVAL_CONFIG_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityArtifactStatus {
    Draft,
    Admitted,
    Running,
    Completed,
    Passed,
    Failed,
    Blocked,
    Cancelled,
    Deprecated,
    RolledBack,
}

impl QualityArtifactStatus {
    pub fn transition(self, next: Self) -> Result<Self, &'static str> {
        if self == next {
            return Err("quality_transition_duplicate");
        }
        let allowed = matches!(
            (self, next),
            (Self::Draft, Self::Admitted | Self::Cancelled)
                | (Self::Admitted, Self::Running | Self::Cancelled)
                | (
                    Self::Running,
                    Self::Completed | Self::Failed | Self::Blocked | Self::Cancelled
                )
                | (Self::Completed, Self::Passed | Self::Failed | Self::Blocked)
                | (Self::Passed, Self::Deprecated | Self::RolledBack)
                | (Self::Failed, Self::Deprecated)
                | (Self::Blocked, Self::Deprecated)
        );
        if allowed {
            Ok(next)
        } else {
            Err("quality_transition_invalid")
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Passed
                | Self::Failed
                | Self::Blocked
                | Self::Cancelled
                | Self::Deprecated
                | Self::RolledBack
        )
    }
}

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_secret_detected"));
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

fn object_type(value: &str) -> bool {
    matches!(
        value,
        "dataset"
            | "suite"
            | "case"
            | "golden_trace"
            | "experiment"
            | "result"
            | "candidate"
            | "gate"
            | "gate_decision"
            | "feedback"
            | "drift_alert"
    )
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityArtifact {
    pub schema: String,
    pub quality_version: u32,
    pub artifact_id: QualityArtifactId,
    pub object_type: String,
    pub owner_id: String,
    pub status: QualityArtifactStatus,
    pub revision: u64,
    pub source_digest: String,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub artifact_digest: String,
}

impl QualityArtifact {
    pub fn new(
        object_type: impl Into<String>,
        owner_id: impl Into<String>,
        source_digest: impl Into<String>,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut artifact = Self {
            schema: QUALITY_ARTIFACT_SCHEMA.to_owned(),
            quality_version: QUALITY_SCHEMA_VERSION,
            artifact_id: QualityArtifactId::new(),
            object_type: object_type.into(),
            owner_id: owner_id.into(),
            status: QualityArtifactStatus::Draft,
            revision: 1,
            source_digest: source_digest.into(),
            created_at_unix_ms,
            updated_at_unix_ms: created_at_unix_ms,
            artifact_digest: String::new(),
        };
        artifact.artifact_digest = artifact.digest();
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_ARTIFACT_SCHEMA
            || self.quality_version != QUALITY_SCHEMA_VERSION
            || self.artifact_id.as_uuid().is_nil()
            || self.revision == 0
            || self.updated_at_unix_ms < self.created_at_unix_ms
            || !object_type(&self.object_type)
        {
            return Err("quality_artifact_header_invalid".to_owned());
        }
        required(
            &self.object_type,
            "quality_object_type",
            MAX_QUALITY_OBJECT_TYPE,
        )?;
        required(&self.owner_id, "quality_owner", MAX_QUALITY_OWNER)?;
        digest(&self.source_digest, "quality_source_digest")?;
        digest(&self.artifact_digest, "quality_artifact_digest")?;
        if self.artifact_digest != self.digest() {
            return Err("quality_artifact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn transition(
        &mut self,
        next: QualityArtifactStatus,
        updated_at_unix_ms: u64,
    ) -> Result<QualityStateTransition, String> {
        self.validate()?;
        if updated_at_unix_ms < self.updated_at_unix_ms {
            return Err("quality_transition_time_regression".to_owned());
        }
        let from = self.status;
        self.status = from.transition(next).map_err(str::to_owned)?;
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| "quality_revision_overflow".to_owned())?;
        self.updated_at_unix_ms = updated_at_unix_ms;
        self.artifact_digest = self.digest();
        QualityStateTransition::new(
            self.artifact_id,
            self.object_type.clone(),
            from,
            next,
            self.revision,
            "quality_state_transition",
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "quality_version": self.quality_version,
            "artifact_id": self.artifact_id,
            "object_type": self.object_type,
            "owner_id": self.owner_id,
            "status": self.status,
            "revision": self.revision,
            "source_digest": self.source_digest,
            "created_at_unix_ms": self.created_at_unix_ms,
            "updated_at_unix_ms": self.updated_at_unix_ms,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityStateTransition {
    pub schema: String,
    pub transition_id: QualityTransitionId,
    pub artifact_id: QualityArtifactId,
    pub object_type: String,
    pub from: QualityArtifactStatus,
    pub to: QualityArtifactStatus,
    pub revision: u64,
    pub reason: String,
    pub transition_digest: String,
}

impl QualityStateTransition {
    pub fn new(
        artifact_id: QualityArtifactId,
        object_type: impl Into<String>,
        from: QualityArtifactStatus,
        to: QualityArtifactStatus,
        revision: u64,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut transition = Self {
            schema: QUALITY_TRANSITION_SCHEMA.to_owned(),
            transition_id: QualityTransitionId::new(),
            artifact_id,
            object_type: object_type.into(),
            from,
            to,
            revision,
            reason: reason.into(),
            transition_digest: String::new(),
        };
        transition.transition_digest = transition.digest();
        transition.validate()?;
        Ok(transition)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != QUALITY_TRANSITION_SCHEMA
            || self.transition_id.as_uuid().is_nil()
            || self.artifact_id.as_uuid().is_nil()
            || self.revision == 0
            || !object_type(&self.object_type)
        {
            return Err("quality_transition_header_invalid".to_owned());
        }
        required(
            &self.object_type,
            "quality_object_type",
            MAX_QUALITY_OBJECT_TYPE,
        )?;
        required(
            &self.reason,
            "quality_transition_reason",
            MAX_QUALITY_REASON,
        )?;
        self.from.transition(self.to).map_err(str::to_owned)?;
        digest(&self.transition_digest, "quality_transition_digest")?;
        if self.transition_digest != self.digest() {
            return Err("quality_transition_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "transition_id": self.transition_id,
            "artifact_id": self.artifact_id,
            "object_type": self.object_type,
            "from": self.from,
            "to": self.to,
            "revision": self.revision,
            "reason": self.reason,
        }))
    }
}

pub fn canonical_quality_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>, String> {
    canonical_journal_bytes(value)
}

fn schema_ok(schema: &str, expected: &str, version: &SchemaVersion) -> bool {
    schema == expected && *version == QUALITY_OBJECT_SCHEMA_VERSION
}

fn required_list(values: &[String], field: &str, max: usize) -> Result<(), String> {
    if values.is_empty() || values.len() > max {
        return Err(format!("{field}_invalid"));
    }
    if values.iter().any(|value| {
        value.trim().is_empty()
            || value.len() > MAX_QUALITY_REASON
            || value.contains('\0')
            || redact_text(value) != value.as_str()
    }) {
        return Err(format!("{field}_invalid"));
    }
    if values.windows(2).any(|pair| pair[0] >= pair[1])
        || values.iter().collect::<BTreeSet<_>>().len() != values.len()
    {
        return Err(format!("{field}_noncanonical"));
    }
    Ok(())
}

fn optional_digest(value: Option<&str>, field: &str) -> Result<(), String> {
    if let Some(value) = value {
        digest(value, field)?;
    }
    Ok(())
}

fn value_is_safe(value: &Value, field: &str) -> Result<(), String> {
    let encoded = serde_json::to_vec(value).map_err(|_| format!("{field}_encode_invalid"))?;
    let text = String::from_utf8_lossy(&encoded);
    if encoded.len() > MAX_EVAL_CONFIG_BYTES || redact_text(&text) != text {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

fn validate_expiry(
    created_at_unix_ms: u64,
    expires_at_unix_ms: Option<u64>,
    field: &str,
) -> Result<(), String> {
    if let Some(expires_at) = expires_at_unix_ms {
        if expires_at <= created_at_unix_ms {
            return Err(format!("{field}_expiry_invalid"));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalSplit {
    Train,
    Validation,
    Regression,
    RedTeam,
    Performance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalDataset {
    pub schema: String,
    pub version: SchemaVersion,
    pub dataset_id: EvalDatasetId,
    pub purpose: String,
    pub owner_id: String,
    pub provenance: Vec<String>,
    pub privacy_class: String,
    pub split: EvalSplit,
    pub case_refs: Vec<EvalCaseId>,
    pub created_at_unix_ms: u64,
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    pub dataset_digest: String,
}

impl EvalDataset {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        purpose: impl Into<String>,
        owner_id: impl Into<String>,
        mut provenance: Vec<String>,
        privacy_class: impl Into<String>,
        split: EvalSplit,
        case_refs: Vec<EvalCaseId>,
        created_at_unix_ms: u64,
        expires_at_unix_ms: Option<u64>,
    ) -> Result<Self, String> {
        provenance.sort();
        let mut dataset = Self {
            schema: EVAL_DATASET_SCHEMA.to_owned(),
            version: QUALITY_OBJECT_SCHEMA_VERSION,
            dataset_id: EvalDatasetId::new(),
            purpose: purpose.into(),
            owner_id: owner_id.into(),
            provenance,
            privacy_class: privacy_class.into(),
            split,
            case_refs,
            created_at_unix_ms,
            expires_at_unix_ms,
            dataset_digest: String::new(),
        };
        dataset.dataset_digest = dataset.digest();
        dataset.validate()?;
        Ok(dataset)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !schema_ok(&self.schema, EVAL_DATASET_SCHEMA, &self.version)
            || self.dataset_id.as_uuid().is_nil()
        {
            return Err("eval_dataset_schema_invalid".to_owned());
        }
        required(&self.purpose, "eval_dataset_purpose", MAX_QUALITY_REASON)?;
        required(&self.owner_id, "eval_dataset_owner", MAX_QUALITY_OWNER)?;
        required(
            &self.privacy_class,
            "eval_dataset_privacy",
            MAX_QUALITY_OBJECT_TYPE,
        )?;
        required_list(
            &self.provenance,
            "eval_dataset_provenance",
            MAX_EVAL_PROVENANCE,
        )?;
        if self.case_refs.is_empty() || self.case_refs.len() > MAX_EVAL_REFERENCES {
            return Err("eval_dataset_cases_invalid".to_owned());
        }
        let refs = self
            .case_refs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if refs.windows(2).any(|pair| pair[0] >= pair[1])
            || refs.iter().collect::<BTreeSet<_>>().len() != refs.len()
            || self.case_refs.iter().any(|value| value.as_uuid().is_nil())
        {
            return Err("eval_dataset_cases_noncanonical".to_owned());
        }
        validate_expiry(
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            "eval_dataset",
        )?;
        digest(&self.dataset_digest, "eval_dataset_digest")?;
        if self.dataset_digest != self.digest() {
            return Err("eval_dataset_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "dataset_id": self.dataset_id,
            "purpose": self.purpose,
            "owner_id": self.owner_id,
            "provenance": self.provenance,
            "privacy_class": self.privacy_class,
            "split": self.split,
            "case_refs": self.case_refs,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvalSuiteStatus {
    Draft,
    Active,
    Deprecated,
}

impl EvalSuiteStatus {
    pub fn transition(self, next: Self) -> Result<Self, &'static str> {
        match (self, next) {
            (Self::Draft, Self::Active | Self::Deprecated) | (Self::Active, Self::Deprecated) => {
                Ok(next)
            }
            _ => Err("eval_suite_transition_invalid"),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalSuite {
    pub schema: String,
    pub version: SchemaVersion,
    pub suite_id: EvalSuiteId,
    pub dataset_id: EvalDatasetId,
    pub workload_class: String,
    pub target_kind: String,
    pub case_refs: Vec<EvalCaseId>,
    pub evaluator_refs: Vec<String>,
    pub scoring_policy_ref: String,
    pub safety_policy_ref: String,
    pub budget_policy_ref: String,
    #[serde(default)]
    pub baseline_ref: Option<GoldenTraceId>,
    pub required_fixture_schema: String,
    pub owner_id: String,
    pub status: EvalSuiteStatus,
    pub suite_digest: String,
}

impl EvalSuite {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        dataset_id: EvalDatasetId,
        workload_class: impl Into<String>,
        target_kind: impl Into<String>,
        mut case_refs: Vec<EvalCaseId>,
        mut evaluator_refs: Vec<String>,
        scoring_policy_ref: impl Into<String>,
        safety_policy_ref: impl Into<String>,
        budget_policy_ref: impl Into<String>,
        baseline_ref: Option<GoldenTraceId>,
        required_fixture_schema: impl Into<String>,
        owner_id: impl Into<String>,
    ) -> Result<Self, String> {
        case_refs.sort_by_key(ToString::to_string);
        evaluator_refs.sort();
        let mut suite = Self {
            schema: EVAL_SUITE_OBJECT_SCHEMA.to_owned(),
            version: QUALITY_OBJECT_SCHEMA_VERSION,
            suite_id: EvalSuiteId::new(),
            dataset_id,
            workload_class: workload_class.into(),
            target_kind: target_kind.into(),
            case_refs,
            evaluator_refs,
            scoring_policy_ref: scoring_policy_ref.into(),
            safety_policy_ref: safety_policy_ref.into(),
            budget_policy_ref: budget_policy_ref.into(),
            baseline_ref,
            required_fixture_schema: required_fixture_schema.into(),
            owner_id: owner_id.into(),
            status: EvalSuiteStatus::Draft,
            suite_digest: String::new(),
        };
        suite.suite_digest = suite.digest();
        suite.validate()?;
        Ok(suite)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !schema_ok(&self.schema, EVAL_SUITE_OBJECT_SCHEMA, &self.version)
            || self.suite_id.as_uuid().is_nil()
            || self.dataset_id.as_uuid().is_nil()
        {
            return Err("eval_suite_schema_invalid".to_owned());
        }
        for (value, field) in [
            (&self.workload_class, "eval_suite_workload"),
            (&self.target_kind, "eval_suite_target"),
            (&self.scoring_policy_ref, "eval_suite_scoring_policy"),
            (&self.safety_policy_ref, "eval_suite_safety_policy"),
            (&self.budget_policy_ref, "eval_suite_budget_policy"),
            (&self.required_fixture_schema, "eval_suite_fixture_schema"),
            (&self.owner_id, "eval_suite_owner"),
        ] {
            required(value, field, MAX_QUALITY_REASON)?;
        }
        let refs = self
            .case_refs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if refs.is_empty()
            || refs.len() > MAX_EVAL_REFERENCES
            || refs.windows(2).any(|pair| pair[0] >= pair[1])
            || refs.iter().collect::<BTreeSet<_>>().len() != refs.len()
            || self.case_refs.iter().any(|value| value.as_uuid().is_nil())
        {
            return Err("eval_suite_cases_invalid".to_owned());
        }
        required_list(
            &self.evaluator_refs,
            "eval_suite_evaluators",
            MAX_EVAL_PROVENANCE,
        )?;
        if self
            .baseline_ref
            .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("eval_suite_baseline_invalid".to_owned());
        }
        digest(&self.suite_digest, "eval_suite_digest")?;
        if self.suite_digest != self.digest() {
            return Err("eval_suite_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "suite_id": self.suite_id,
            "dataset_id": self.dataset_id,
            "workload_class": self.workload_class,
            "target_kind": self.target_kind,
            "case_refs": self.case_refs,
            "evaluator_refs": self.evaluator_refs,
            "scoring_policy_ref": self.scoring_policy_ref,
            "safety_policy_ref": self.safety_policy_ref,
            "budget_policy_ref": self.budget_policy_ref,
            "baseline_ref": self.baseline_ref,
            "required_fixture_schema": self.required_fixture_schema,
            "owner_id": self.owner_id,
            "status": self.status,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalCase {
    pub schema: String,
    pub version: SchemaVersion,
    pub case_id: EvalCaseId,
    pub suite_id: EvalSuiteId,
    pub input_fixture_ref: String,
    #[serde(default)]
    pub initial_state_fixture_ref: Option<String>,
    pub target_config: Value,
    pub expected_events: Vec<String>,
    pub expected_state: String,
    pub expected_artifacts: Vec<String>,
    pub expected_receipt_assertions: Vec<String>,
    pub forbidden_effects: Vec<String>,
    pub assertions: Vec<String>,
    pub privacy_class: String,
    pub case_digest: String,
}

impl EvalCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        suite_id: EvalSuiteId,
        input_fixture_ref: impl Into<String>,
        initial_state_fixture_ref: Option<String>,
        target_config: Value,
        mut expected_events: Vec<String>,
        expected_state: impl Into<String>,
        mut expected_artifacts: Vec<String>,
        mut expected_receipt_assertions: Vec<String>,
        mut forbidden_effects: Vec<String>,
        mut assertions: Vec<String>,
        privacy_class: impl Into<String>,
    ) -> Result<Self, String> {
        expected_events.sort();
        expected_artifacts.sort();
        expected_receipt_assertions.sort();
        forbidden_effects.sort();
        assertions.sort();
        let mut case = Self {
            schema: EVAL_CASE_OBJECT_SCHEMA.to_owned(),
            version: QUALITY_OBJECT_SCHEMA_VERSION,
            case_id: EvalCaseId::new(),
            suite_id,
            input_fixture_ref: input_fixture_ref.into(),
            initial_state_fixture_ref,
            target_config,
            expected_events,
            expected_state: expected_state.into(),
            expected_artifacts,
            expected_receipt_assertions,
            forbidden_effects,
            assertions,
            privacy_class: privacy_class.into(),
            case_digest: String::new(),
        };
        case.case_digest = case.digest();
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !schema_ok(&self.schema, EVAL_CASE_OBJECT_SCHEMA, &self.version)
            || self.case_id.as_uuid().is_nil()
            || self.suite_id.as_uuid().is_nil()
        {
            return Err("eval_case_schema_invalid".to_owned());
        }
        required(
            &self.input_fixture_ref,
            "eval_case_input_fixture",
            MAX_QUALITY_REASON,
        )?;
        if let Some(reference) = self.initial_state_fixture_ref.as_deref() {
            required(reference, "eval_case_initial_fixture", MAX_QUALITY_REASON)?;
        }
        value_is_safe(&self.target_config, "eval_case_target_config")?;
        required(
            &self.expected_state,
            "eval_case_expected_state",
            MAX_QUALITY_REASON,
        )?;
        required(
            &self.privacy_class,
            "eval_case_privacy",
            MAX_QUALITY_OBJECT_TYPE,
        )?;
        for (values, field) in [
            (&self.expected_events, "eval_case_expected_events"),
            (&self.expected_artifacts, "eval_case_expected_artifacts"),
            (
                &self.expected_receipt_assertions,
                "eval_case_receipt_assertions",
            ),
            (&self.forbidden_effects, "eval_case_forbidden_effects"),
            (&self.assertions, "eval_case_assertions"),
        ] {
            if !values.is_empty() {
                required_list(values, field, MAX_EVAL_REFERENCES)?;
            }
        }
        digest(&self.case_digest, "eval_case_digest")?;
        if self.case_digest != self.digest() {
            return Err("eval_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "case_id": self.case_id,
            "suite_id": self.suite_id,
            "input_fixture_ref": self.input_fixture_ref,
            "initial_state_fixture_ref": self.initial_state_fixture_ref,
            "target_config": self.target_config,
            "expected_events": self.expected_events,
            "expected_state": self.expected_state,
            "expected_artifacts": self.expected_artifacts,
            "expected_receipt_assertions": self.expected_receipt_assertions,
            "forbidden_effects": self.forbidden_effects,
            "assertions": self.assertions,
            "privacy_class": self.privacy_class,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenTrace {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: GoldenTraceId,
    pub suite_id: EvalSuiteId,
    pub case_id: EvalCaseId,
    #[serde(default)]
    pub source_run_id: Option<RunId>,
    pub source_snapshot: String,
    pub input_hash: String,
    pub target_versions: BTreeMap<String, String>,
    pub event_cursor_start: u64,
    pub event_cursor_end: u64,
    pub normalized_events: Vec<Value>,
    pub artifact_hashes: Vec<String>,
    #[serde(default)]
    pub receipt_hash: Option<String>,
    pub normalization_version: String,
    #[serde(default)]
    pub human_acceptance: Option<bool>,
    #[serde(default)]
    pub quality_score: Option<f64>,
    pub created_at_unix_ms: u64,
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    pub provenance_ref: String,
    pub trace_digest: String,
}

impl GoldenTrace {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        suite_id: EvalSuiteId,
        case_id: EvalCaseId,
        source_run_id: Option<RunId>,
        source_snapshot: impl Into<String>,
        input_hash: impl Into<String>,
        target_versions: BTreeMap<String, String>,
        event_cursor_start: u64,
        event_cursor_end: u64,
        normalized_events: Vec<Value>,
        mut artifact_hashes: Vec<String>,
        receipt_hash: Option<String>,
        normalization_version: impl Into<String>,
        human_acceptance: Option<bool>,
        quality_score: Option<f64>,
        created_at_unix_ms: u64,
        expires_at_unix_ms: Option<u64>,
        provenance_ref: impl Into<String>,
    ) -> Result<Self, String> {
        artifact_hashes.sort();
        let mut trace = Self {
            schema: GOLDEN_TRACE_SCHEMA.to_owned(),
            version: QUALITY_OBJECT_SCHEMA_VERSION,
            trace_id: GoldenTraceId::new(),
            suite_id,
            case_id,
            source_run_id,
            source_snapshot: source_snapshot.into(),
            input_hash: input_hash.into(),
            target_versions,
            event_cursor_start,
            event_cursor_end,
            normalized_events,
            artifact_hashes,
            receipt_hash,
            normalization_version: normalization_version.into(),
            human_acceptance,
            quality_score,
            created_at_unix_ms,
            expires_at_unix_ms,
            provenance_ref: provenance_ref.into(),
            trace_digest: String::new(),
        };
        trace.trace_digest = trace.digest();
        trace.validate()?;
        Ok(trace)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !schema_ok(&self.schema, GOLDEN_TRACE_SCHEMA, &self.version)
            || self.trace_id.as_uuid().is_nil()
            || self.suite_id.as_uuid().is_nil()
            || self.case_id.as_uuid().is_nil()
            || self.event_cursor_start == 0
            || self.event_cursor_end < self.event_cursor_start
            || self.normalized_events.is_empty()
            || self.normalized_events.len() > MAX_EVAL_EVENTS
        {
            return Err("golden_trace_header_invalid".to_owned());
        }
        if self
            .source_run_id
            .is_some_and(|value| value.as_uuid().is_nil())
        {
            return Err("golden_trace_source_run_invalid".to_owned());
        }
        for (value, field) in [
            (&self.source_snapshot, "golden_trace_source_snapshot"),
            (&self.input_hash, "golden_trace_input_hash"),
            (
                &self.normalization_version,
                "golden_trace_normalization_version",
            ),
            (&self.provenance_ref, "golden_trace_provenance"),
        ] {
            required(value, field, MAX_QUALITY_REASON)?;
        }
        digest(&self.input_hash, "golden_trace_input_hash")?;
        if self
            .artifact_hashes
            .iter()
            .any(|value| digest(value, "golden_trace_artifact_hash").is_err())
            || self
                .artifact_hashes
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err("golden_trace_artifacts_invalid".to_owned());
        }
        optional_digest(self.receipt_hash.as_deref(), "golden_trace_receipt_hash")?;
        if self.target_versions.is_empty()
            || self.target_versions.iter().any(|(key, value)| {
                key.trim().is_empty()
                    || value.trim().is_empty()
                    || key.len() > MAX_QUALITY_OBJECT_TYPE
                    || value.len() > MAX_QUALITY_REASON
                    || redact_text(key) != key.as_str()
                    || redact_text(value) != value.as_str()
            })
        {
            return Err("golden_trace_target_versions_invalid".to_owned());
        }
        for event in &self.normalized_events {
            value_is_safe(event, "golden_trace_event")?;
        }
        if self.quality_score.is_some_and(|score| !score.is_finite()) {
            return Err("golden_trace_quality_score_invalid".to_owned());
        }
        validate_expiry(
            self.created_at_unix_ms,
            self.expires_at_unix_ms,
            "golden_trace",
        )?;
        digest(&self.trace_digest, "golden_trace_digest")?;
        if self.trace_digest != self.digest() {
            return Err("golden_trace_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "trace_id": self.trace_id,
            "suite_id": self.suite_id,
            "case_id": self.case_id,
            "source_run_id": self.source_run_id,
            "source_snapshot": self.source_snapshot,
            "input_hash": self.input_hash,
            "target_versions": self.target_versions,
            "event_cursor_start": self.event_cursor_start,
            "event_cursor_end": self.event_cursor_end,
            "normalized_events": self.normalized_events,
            "artifact_hashes": self.artifact_hashes,
            "receipt_hash": self.receipt_hash,
            "normalization_version": self.normalization_version,
            "human_acceptance": self.human_acceptance,
            "quality_score": self.quality_score,
            "created_at_unix_ms": self.created_at_unix_ms,
            "expires_at_unix_ms": self.expires_at_unix_ms,
            "provenance_ref": self.provenance_ref,
        }))
    }
}
