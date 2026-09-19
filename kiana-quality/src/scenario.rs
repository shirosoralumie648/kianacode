//! EQ-24 reference/candidate scenario comparison over scrubbed environments.

use crate::{trace_digest, TraceDiff, TraceDiffClass, VolatileEventTrace};
use kiana_domain::{json_digest, redact_text};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const SCENARIO_SCHEMA: &str = "kiana.quality-scenario.v1";
pub const SCRUBBED_ENVIRONMENT_SCHEMA: &str = "kiana.quality-scrubbed-environment.v1";
pub const CLEANUP_RECEIPT_SCHEMA: &str = "kiana.quality-cleanup-receipt.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrubbedEnvironment {
    pub schema: String,
    pub workspace_token: String,
    pub home_token: String,
    pub variables: BTreeMap<String, String>,
    pub network_profile: String,
}

impl ScrubbedEnvironment {
    pub fn validate(&self) -> Result<(), ScenarioError> {
        if self.schema != SCRUBBED_ENVIRONMENT_SCHEMA
            || self.workspace_token.trim().is_empty()
            || self.home_token.trim().is_empty()
            || self.network_profile.trim().is_empty()
            || self.workspace_token.starts_with('/')
            || self.home_token.starts_with('/')
            || redact_text(&self.workspace_token) != self.workspace_token
            || redact_text(&self.home_token) != self.home_token
            || self
                .variables
                .keys()
                .any(|key| key.trim().is_empty() || key.contains(['\0', '\n', '\r']))
            || self
                .variables
                .values()
                .any(|value| redact_text(value) != value || value.starts_with('/'))
        {
            return Err(ScenarioError::EnvironmentInvalid);
        }
        Ok(())
    }

    fn digest(&self) -> Result<String, ScenarioError> {
        self.validate()?;
        Ok(json_digest(&json!(self)))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopePredicate {
    AllowAll,
    FieldPrefixes(Vec<String>),
}

impl ScopePredicate {
    fn validate(&self) -> Result<(), ScenarioError> {
        if let Self::FieldPrefixes(prefixes) = self {
            if prefixes.is_empty()
                || prefixes.iter().any(|prefix| {
                    prefix.trim().is_empty() || prefix.len() > 512 || prefix.contains(['\0', '\n'])
                })
            {
                return Err(ScenarioError::ScopePredicateInvalid);
            }
        }
        Ok(())
    }

    fn allows(&self, field_path: &str) -> bool {
        match self {
            Self::AllowAll => true,
            Self::FieldPrefixes(prefixes) => prefixes.iter().any(|prefix| {
                field_path == prefix
                    || field_path.starts_with(&format!("{prefix}."))
                    || field_path.starts_with(&format!("{prefix}["))
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSpec {
    pub schema: String,
    pub scenario_id: String,
    pub scope: ScopePredicate,
    pub cleanup_required: bool,
}

impl ScenarioSpec {
    pub fn validate(&self) -> Result<(), ScenarioError> {
        if self.schema != SCENARIO_SCHEMA
            || self.scenario_id.trim().is_empty()
            || self.scenario_id.len() > 256
            || self.scenario_id.contains(['\0', '\n', '\r'])
        {
            return Err(ScenarioError::SpecInvalid);
        }
        self.scope.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioArtifact {
    pub scenario_id: String,
    pub source_ref: String,
    pub trace: VolatileEventTrace,
    pub environment: ScrubbedEnvironment,
}

impl ScenarioArtifact {
    fn validate(&self) -> Result<(), ScenarioError> {
        if self.scenario_id.trim().is_empty()
            || self.source_ref.trim().is_empty()
            || self.source_ref.len() > 512
            || self.source_ref.contains(['\0', '\n', '\r'])
        {
            return Err(ScenarioError::ArtifactInvalid);
        }
        self.environment.validate()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CleanupReceipt {
    pub schema: String,
    pub workspace_ref: String,
    pub home_ref: String,
    pub completed: bool,
    pub orphan_count: u32,
}

impl CleanupReceipt {
    pub fn validate(&self) -> Result<(), ScenarioError> {
        if self.schema != CLEANUP_RECEIPT_SCHEMA
            || self.workspace_ref.trim().is_empty()
            || self.home_ref.trim().is_empty()
            || self.workspace_ref.starts_with('/')
            || self.home_ref.starts_with('/')
            || self.orphan_count > 0 && self.completed
        {
            return Err(ScenarioError::CleanupReceiptInvalid);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioOutcome {
    InScope,
    OutOfScope,
    NotComparable,
    CleanupRequired,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioReport {
    pub schema: String,
    pub scenario_id: String,
    pub outcome: ScenarioOutcome,
    pub reference_trace_digest: String,
    pub candidate_trace_digest: String,
    pub reference_environment_digest: String,
    pub candidate_environment_digest: String,
    pub environment_match: bool,
    pub diff: TraceDiff,
    pub scope_reason: String,
    pub cleanup: CleanupReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ScenarioError {
    #[error("scenario_spec_invalid")]
    SpecInvalid,
    #[error("scenario_scope_predicate_invalid")]
    ScopePredicateInvalid,
    #[error("scenario_artifact_invalid")]
    ArtifactInvalid,
    #[error("scenario_environment_invalid")]
    EnvironmentInvalid,
    #[error("scenario_cleanup_receipt_invalid")]
    CleanupReceiptInvalid,
    #[error("scenario_id_mismatch")]
    ScenarioIdMismatch,
    #[error("scenario_digest_invalid:{0}")]
    DigestInvalid(String),
}

pub struct ScenarioRunner;

impl ScenarioRunner {
    pub fn compare(
        spec: &ScenarioSpec,
        reference: &ScenarioArtifact,
        candidate: &ScenarioArtifact,
        cleanup: CleanupReceipt,
    ) -> Result<ScenarioReport, ScenarioError> {
        spec.validate()?;
        reference.validate()?;
        candidate.validate()?;
        cleanup.validate()?;
        if reference.scenario_id != spec.scenario_id || candidate.scenario_id != spec.scenario_id {
            return Err(ScenarioError::ScenarioIdMismatch);
        }
        let reference_environment_digest = reference.environment.digest()?;
        let candidate_environment_digest = candidate.environment.digest()?;
        let environment_match = reference_environment_digest == candidate_environment_digest;
        let diff = TraceDiff::compare(&reference.trace, &candidate.trace);
        let reference_trace_digest = trace_digest(&reference.trace)
            .map_err(|error| ScenarioError::DigestInvalid(error.to_string()))?
            .digest;
        let candidate_trace_digest = trace_digest(&candidate.trace)
            .map_err(|error| ScenarioError::DigestInvalid(error.to_string()))?
            .digest;

        let (outcome, scope_reason) = if !environment_match {
            (
                ScenarioOutcome::NotComparable,
                "scrubbed_environment_mismatch".to_owned(),
            )
        } else if spec.cleanup_required && !cleanup.completed {
            (
                ScenarioOutcome::CleanupRequired,
                "cleanup_receipt_incomplete".to_owned(),
            )
        } else if diff.classification == TraceDiffClass::NotComparable {
            (
                ScenarioOutcome::NotComparable,
                "trace_not_comparable".to_owned(),
            )
        } else if diff.is_identical() {
            (ScenarioOutcome::InScope, "no_trace_divergence".to_owned())
        } else if diff
            .first_divergence
            .as_ref()
            .is_some_and(|divergence| spec.scope.allows(&divergence.field_path))
        {
            (
                ScenarioOutcome::InScope,
                "first_divergence_in_scope".to_owned(),
            )
        } else {
            (
                ScenarioOutcome::OutOfScope,
                "first_divergence_out_of_scope".to_owned(),
            )
        };

        Ok(ScenarioReport {
            schema: SCENARIO_SCHEMA.to_owned(),
            scenario_id: spec.scenario_id.clone(),
            outcome,
            reference_trace_digest,
            candidate_trace_digest,
            reference_environment_digest,
            candidate_environment_digest,
            environment_match,
            diff,
            scope_reason,
            cleanup,
        })
    }
}
