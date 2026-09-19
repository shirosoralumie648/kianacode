//! Byte-stable ContextPlan and retrieval golden fixture contract.
//!
//! A golden fixture freezes the source/policy/index/model inputs and expected rank output. It is
//! a CI/evaluation artifact only: it does not promote a provider, authorize a read, or tune the
//! retrieval algorithm.

use crate::{canonical_journal_bytes, json_digest, ContextPlan, RetrievalResult, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const GOLDEN_CONTEXT_FIXTURE_SCHEMA: &str = "kiana.golden-context-fixture.v1";
pub const GOLDEN_CONTEXT_CASE_SCHEMA: &str = "kiana.golden-context-case.v1";
pub const GOLDEN_CONTEXT_FIXTURE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const REQUIRED_COVERAGE: [&str; 7] = [
    "english",
    "chinese",
    "cjk",
    "identifier",
    "path",
    "time",
    "acl",
];

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
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

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenContextCase {
    pub schema: String,
    pub case_id: String,
    pub query: String,
    pub scope_digest: String,
    pub coverage: BTreeSet<String>,
    pub expected_ranked_ids: Vec<String>,
    pub denied_candidate_ids: Vec<String>,
}

impl GoldenContextCase {
    pub fn new(
        case_id: impl Into<String>,
        query: impl Into<String>,
        scope_digest: impl Into<String>,
        coverage: BTreeSet<String>,
        expected_ranked_ids: Vec<String>,
        denied_candidate_ids: Vec<String>,
    ) -> Result<Self, String> {
        let case = Self {
            schema: GOLDEN_CONTEXT_CASE_SCHEMA.to_owned(),
            case_id: case_id.into(),
            query: query.into(),
            scope_digest: scope_digest.into(),
            coverage,
            expected_ranked_ids,
            denied_candidate_ids,
        };
        case.validate()?;
        Ok(case)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != GOLDEN_CONTEXT_CASE_SCHEMA
            || self.expected_ranked_ids.is_empty()
            || self.expected_ranked_ids.len() > 128
            || self.denied_candidate_ids.len() > 128
        {
            return Err("golden_context_case_header_invalid".to_owned());
        }
        required(&self.case_id, "golden_context_case_id", 128)?;
        required(&self.query, "golden_context_case_query", 32 * 1024)?;
        digest(&self.scope_digest, "golden_context_case_scope_digest")?;
        if self.coverage.is_empty()
            || self
                .coverage
                .iter()
                .any(|tag| tag.trim().is_empty() || tag.len() > 64)
        {
            return Err("golden_context_case_coverage_invalid".to_owned());
        }
        let mut expected = BTreeSet::new();
        for id in &self.expected_ranked_ids {
            required(id, "golden_context_case_expected_id", 512)?;
            if !expected.insert(id.clone()) {
                return Err("golden_context_case_expected_duplicate".to_owned());
            }
        }
        let denied = self
            .denied_candidate_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if denied.iter().any(|id| expected.contains(*id)) {
            return Err("golden_context_case_acl_overlap".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenContextFixture {
    pub schema: String,
    pub version: SchemaVersion,
    pub fixture_id: String,
    pub source_snapshot_digest: String,
    pub policy_epoch: u64,
    pub data_epoch: u64,
    pub index_generation: u64,
    pub algorithm_digest: String,
    pub embedding_digest: String,
    pub context_plan_digest: String,
    pub retrieval_result_digest: String,
    pub cases: Vec<GoldenContextCase>,
    pub fixture_digest: String,
}

impl GoldenContextFixture {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fixture_id: impl Into<String>,
        source_snapshot_digest: impl Into<String>,
        policy_epoch: u64,
        data_epoch: u64,
        index_generation: u64,
        algorithm_digest: impl Into<String>,
        embedding_digest: impl Into<String>,
        context_plan_digest: impl Into<String>,
        retrieval_result_digest: impl Into<String>,
        cases: Vec<GoldenContextCase>,
    ) -> Result<Self, String> {
        let mut fixture = Self {
            schema: GOLDEN_CONTEXT_FIXTURE_SCHEMA.to_owned(),
            version: GOLDEN_CONTEXT_FIXTURE_VERSION,
            fixture_id: fixture_id.into(),
            source_snapshot_digest: source_snapshot_digest.into(),
            policy_epoch,
            data_epoch,
            index_generation,
            algorithm_digest: algorithm_digest.into(),
            embedding_digest: embedding_digest.into(),
            context_plan_digest: context_plan_digest.into(),
            retrieval_result_digest: retrieval_result_digest.into(),
            cases,
            fixture_digest: String::new(),
        };
        fixture.fixture_digest = fixture.digest();
        fixture.validate()?;
        Ok(fixture)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != GOLDEN_CONTEXT_FIXTURE_SCHEMA
            || self.version != GOLDEN_CONTEXT_FIXTURE_VERSION
            || self.policy_epoch == 0
            || self.data_epoch == 0
            || self.index_generation == 0
            || self.cases.is_empty()
            || self.cases.len() > 64
        {
            return Err("golden_context_fixture_header_invalid".to_owned());
        }
        required(&self.fixture_id, "golden_context_fixture_id", 256)?;
        for (value, field) in [
            (
                &self.source_snapshot_digest,
                "golden_context_source_snapshot_digest",
            ),
            (&self.algorithm_digest, "golden_context_algorithm_digest"),
            (&self.embedding_digest, "golden_context_embedding_digest"),
            (&self.context_plan_digest, "golden_context_plan_digest"),
            (
                &self.retrieval_result_digest,
                "golden_context_retrieval_result_digest",
            ),
        ] {
            digest(value, field)?;
        }
        let mut ids = BTreeSet::new();
        let mut coverage = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !ids.insert(case.case_id.clone()) {
                return Err("golden_context_case_id_duplicate".to_owned());
            }
            coverage.extend(case.coverage.iter().cloned());
        }
        if REQUIRED_COVERAGE
            .iter()
            .any(|required| !coverage.contains(*required))
        {
            return Err("golden_context_coverage_incomplete".to_owned());
        }
        digest(&self.fixture_digest, "golden_context_fixture_digest")?;
        if self.fixture_digest != self.digest() {
            return Err("golden_context_fixture_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_context_plan(&self, plan: &ContextPlan) -> Result<(), String> {
        self.validate()?;
        plan.validate()?;
        if plan.plan_digest != self.context_plan_digest {
            return Err("golden_context_plan_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_retrieval(
        &self,
        case_id: &str,
        result: &RetrievalResult,
    ) -> Result<(), String> {
        self.validate()?;
        result.validate()?;
        let case = self
            .cases
            .iter()
            .find(|case| case.case_id == case_id)
            .ok_or_else(|| "golden_context_case_not_found".to_owned())?;
        if result.result_digest != self.retrieval_result_digest
            || result
                .hits
                .iter()
                .map(|hit| hit.id.clone())
                .collect::<Vec<_>>()
                != case.expected_ranked_ids
        {
            return Err("golden_retrieval_rank_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "fixture_id": self.fixture_id,
            "source_snapshot_digest": self.source_snapshot_digest,
            "policy_epoch": self.policy_epoch,
            "data_epoch": self.data_epoch,
            "index_generation": self.index_generation,
            "algorithm_digest": self.algorithm_digest,
            "embedding_digest": self.embedding_digest,
            "context_plan_digest": self.context_plan_digest,
            "retrieval_result_digest": self.retrieval_result_digest,
            "cases": self.cases,
        }))
    }
}
