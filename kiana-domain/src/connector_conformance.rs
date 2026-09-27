//! INT-30 connector adapter/registry/protocol conformance evidence.
//!
//! The matrix describes CI fixtures for local fake, stdio MCP fake and HTTP fake adapters. It is
//! not an adapter runner and cannot promote a fixture to live/physical proof.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CONNECTOR_CONFORMANCE_SCHEMA: &str = "kiana.connector-conformance.v1";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorConformanceAdapter {
    LocalFixture,
    StdioMcpFake,
    HttpFake,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorConformanceScenario {
    Deny,
    Schema,
    Replay,
    Unknown,
    Toctou,
    Success,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectorConformanceStatus {
    Verified,
    NotApplicable,
    NotImplemented,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorConformanceCase {
    pub adapter: ConnectorConformanceAdapter,
    pub scenario: ConnectorConformanceScenario,
    pub status: ConnectorConformanceStatus,
    pub effect_count: u32,
    pub unknown_preserved: bool,
    pub dedupe_verified: bool,
    pub scope_fenced: bool,
    pub evidence_digest: Option<String>,
    pub reason: Option<String>,
    pub case_digest: String,
}

impl ConnectorConformanceCase {
    pub fn validate(&self) -> Result<(), String> {
        if !valid_digest(&self.case_digest)
            || self.case_digest != self.digest()
            || self.status == ConnectorConformanceStatus::Verified
                && self
                    .evidence_digest
                    .as_deref()
                    .is_none_or(|value| !valid_digest(value))
            || self.status != ConnectorConformanceStatus::Verified
                && self
                    .reason
                    .as_deref()
                    .is_none_or(|value| !valid_text(value))
        {
            return Err("connector_conformance_case_invalid".to_owned());
        }
        match self.scenario {
            ConnectorConformanceScenario::Deny | ConnectorConformanceScenario::Schema => {
                if self.status == ConnectorConformanceStatus::Verified && self.effect_count != 0 {
                    return Err("connector_conformance_deny_effect_nonzero".to_owned());
                }
            }
            ConnectorConformanceScenario::Replay => {
                if self.status == ConnectorConformanceStatus::Verified
                    && (!self.dedupe_verified || self.effect_count > 1)
                {
                    return Err("connector_conformance_replay_fence_missing".to_owned());
                }
            }
            ConnectorConformanceScenario::Unknown => {
                if self.status == ConnectorConformanceStatus::Verified
                    && (!self.unknown_preserved || self.effect_count > 1)
                {
                    return Err("connector_conformance_unknown_not_preserved".to_owned());
                }
            }
            ConnectorConformanceScenario::Toctou => {
                if self.status == ConnectorConformanceStatus::Verified && !self.scope_fenced {
                    return Err("connector_conformance_toctou_fence_missing".to_owned());
                }
            }
            ConnectorConformanceScenario::Success => {}
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "adapter": self.adapter,
            "scenario": self.scenario,
            "status": self.status,
            "effect_count": self.effect_count,
            "unknown_preserved": self.unknown_preserved,
            "dedupe_verified": self.dedupe_verified,
            "scope_fenced": self.scope_fenced,
            "evidence_digest": self.evidence_digest,
            "reason": self.reason,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorConformanceReport {
    pub schema: String,
    pub cases: Vec<ConnectorConformanceCase>,
    pub verified_count: u32,
    pub not_applicable_count: u32,
    pub not_implemented_count: u32,
    pub blocked_count: u32,
    pub status: ConnectorConformanceStatus,
    pub blocking_reasons: Vec<String>,
    pub report_digest: String,
}

impl ConnectorConformanceReport {
    pub fn evaluate(cases: Vec<ConnectorConformanceCase>) -> Result<Self, String> {
        if cases.is_empty() || cases.len() > 256 {
            return Err("connector_conformance_case_bounds_invalid".to_owned());
        }
        let mut identities = BTreeSet::new();
        for case in &cases {
            case.validate()?;
            if !identities.insert((case.adapter, case.scenario)) {
                return Err("connector_conformance_case_duplicate".to_owned());
            }
        }
        let verified_count = cases
            .iter()
            .filter(|case| case.status == ConnectorConformanceStatus::Verified)
            .count() as u32;
        let not_applicable_count = cases
            .iter()
            .filter(|case| case.status == ConnectorConformanceStatus::NotApplicable)
            .count() as u32;
        let not_implemented_count = cases
            .iter()
            .filter(|case| case.status == ConnectorConformanceStatus::NotImplemented)
            .count() as u32;
        let blocked_count = cases
            .iter()
            .filter(|case| case.status == ConnectorConformanceStatus::Blocked)
            .count() as u32;
        let status = if blocked_count > 0 {
            ConnectorConformanceStatus::Blocked
        } else if not_implemented_count > 0 {
            ConnectorConformanceStatus::NotImplemented
        } else if verified_count + not_applicable_count == cases.len() as u32 {
            ConnectorConformanceStatus::Verified
        } else {
            ConnectorConformanceStatus::NotImplemented
        };
        let blocking_reasons = cases
            .iter()
            .filter(|case| case.status == ConnectorConformanceStatus::Blocked)
            .filter_map(|case| case.reason.clone())
            .collect::<Vec<_>>();
        let mut report = Self {
            schema: CONNECTOR_CONFORMANCE_SCHEMA.to_owned(),
            cases,
            verified_count,
            not_applicable_count,
            not_implemented_count,
            blocked_count,
            status,
            blocking_reasons,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_CONFORMANCE_SCHEMA
            || self.cases.is_empty()
            || self.cases.len() > 256
            || self.report_digest != self.digest()
        {
            return Err("connector_conformance_report_invalid".to_owned());
        }
        for case in &self.cases {
            case.validate()?;
        }
        if self.verified_count
            != self
                .cases
                .iter()
                .filter(|case| case.status == ConnectorConformanceStatus::Verified)
                .count() as u32
            || self.not_applicable_count
                != self
                    .cases
                    .iter()
                    .filter(|case| case.status == ConnectorConformanceStatus::NotApplicable)
                    .count() as u32
            || self.not_implemented_count
                != self
                    .cases
                    .iter()
                    .filter(|case| case.status == ConnectorConformanceStatus::NotImplemented)
                    .count() as u32
            || self.blocked_count
                != self
                    .cases
                    .iter()
                    .filter(|case| case.status == ConnectorConformanceStatus::Blocked)
                    .count() as u32
        {
            return Err("connector_conformance_counts_mismatch".to_owned());
        }
        let expected_status = if self.blocked_count > 0 {
            ConnectorConformanceStatus::Blocked
        } else if self.not_implemented_count > 0 {
            ConnectorConformanceStatus::NotImplemented
        } else {
            ConnectorConformanceStatus::Verified
        };
        if self.status != expected_status {
            return Err("connector_conformance_status_mismatch".to_owned());
        }
        Ok(())
    }

    fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "cases": self.cases,
            "verified_count": self.verified_count,
            "not_applicable_count": self.not_applicable_count,
            "not_implemented_count": self.not_implemented_count,
            "blocked_count": self.blocked_count,
            "status": self.status,
            "blocking_reasons": self.blocking_reasons,
        }))
    }
}

pub fn validate_connector_conformance(report: &ConnectorConformanceReport) -> Result<(), String> {
    report.validate()
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 512 && !value.contains(['\0', '\r', '\n'])
}

fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
