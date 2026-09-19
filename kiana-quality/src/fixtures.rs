//! EQ-26 provider-independent core negative trace fixture matrix.

use crate::{
    trace_digest, ArrayPolicy, VolatileEvent, VolatileEventTrace, VOLATILE_NORMALIZATION_VERSION,
};
use kiana_domain::{json_digest, redact_text};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const FIXTURE_SCHEMA: &str = "kiana.quality-core-trace-fixture.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureFamily {
    Runtime,
    Approval,
    Hook,
    Memory,
    Workflow,
    Swarm,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceFixture {
    pub schema: String,
    pub fixture_id: String,
    pub family: FixtureFamily,
    pub trace: VolatileEventTrace,
    pub expected_status: String,
    pub forbidden_effects: Vec<String>,
    pub fixture_digest: String,
}

impl TraceFixture {
    pub fn validate(&self) -> Result<(), FixtureError> {
        if self.schema != FIXTURE_SCHEMA
            || self.fixture_id.trim().is_empty()
            || self.expected_status.trim().is_empty()
            || self.trace.events.is_empty()
            || self.forbidden_effects.is_empty()
            || self
                .forbidden_effects
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
            || self
                .forbidden_effects
                .iter()
                .any(|effect| effect.trim().is_empty() || redact_text(effect) != effect)
        {
            return Err(FixtureError::FixtureInvalid);
        }
        trace_digest(&self.trace).map_err(|error| FixtureError::TraceInvalid(error.to_string()))?;
        if self.fixture_digest != self.digest() {
            return Err(FixtureError::DigestMismatch);
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "fixture_id": self.fixture_id,
            "family": self.family,
            "trace": self.trace,
            "expected_status": self.expected_status,
            "forbidden_effects": self.forbidden_effects,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FixtureError {
    #[error("quality_fixture_invalid")]
    FixtureInvalid,
    #[error("quality_fixture_trace_invalid:{0}")]
    TraceInvalid(String),
    #[error("quality_fixture_digest_mismatch")]
    DigestMismatch,
    #[error("quality_fixture_matrix_incomplete")]
    MatrixIncomplete,
}

fn fixture(family: FixtureFamily, fixture_id: &str, kind: &str) -> TraceFixture {
    let trace = VolatileEventTrace {
        normalization_version: VOLATILE_NORMALIZATION_VERSION.to_owned(),
        source_normalization_version: "eq18.canonical-events.v1".to_owned(),
        array_policy: ArrayPolicy::Ordered,
        source_cursor_start: 1,
        source_cursor_end: 1,
        correlation_id: kiana_domain::RequestId::new(),
        events: vec![VolatileEvent {
            source_cursor: 1,
            kind: kind.to_owned(),
            value: json!({
                "kind": kind,
                "family": family,
                "status": "blocked",
                "effect_started": false,
                "effect_known": false,
                "zero_effect": true,
                "provider_calls": 0,
                "side_effects": false
            }),
        }],
        terminal_event_indexes: vec![0],
        replacement_count: 0,
        replacements: Vec::new(),
    };
    let forbidden_effects = ["desktop", "network", "payment", "publish", "secret"]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let mut result = TraceFixture {
        schema: FIXTURE_SCHEMA.to_owned(),
        fixture_id: fixture_id.to_owned(),
        family,
        trace,
        expected_status: "blocked".to_owned(),
        forbidden_effects,
        fixture_digest: String::new(),
    };
    result.fixture_digest = result.digest();
    result
}

pub fn core_negative_fixture_matrix() -> Vec<TraceFixture> {
    vec![
        fixture(
            FixtureFamily::Runtime,
            "runtime-forbidden-effect",
            "run.result_unknown",
        ),
        fixture(
            FixtureFamily::Approval,
            "approval-expired",
            "approval.expired",
        ),
        fixture(FixtureFamily::Hook, "hook-blocked", "capability.blocked"),
        fixture(
            FixtureFamily::Memory,
            "memory-secret-rejected",
            "memory.candidate_rejected",
        ),
        fixture(
            FixtureFamily::Workflow,
            "workflow-unknown-recovery",
            "workflow.result_unknown",
        ),
        fixture(
            FixtureFamily::Swarm,
            "swarm-scope-denied",
            "delegation.attempt_state",
        ),
    ]
}

pub fn validate_core_negative_fixture_matrix(
    fixtures: &[TraceFixture],
) -> Result<(), FixtureError> {
    let expected = [
        FixtureFamily::Runtime,
        FixtureFamily::Approval,
        FixtureFamily::Hook,
        FixtureFamily::Memory,
        FixtureFamily::Workflow,
        FixtureFamily::Swarm,
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();
    let actual = fixtures
        .iter()
        .map(|fixture| fixture.family)
        .collect::<BTreeSet<_>>();
    if actual != expected || fixtures.len() != expected.len() {
        return Err(FixtureError::MatrixIncomplete);
    }
    for fixture in fixtures {
        fixture.validate()?;
    }
    Ok(())
}
