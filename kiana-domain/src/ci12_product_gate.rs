//! CI-12 product-chain deny-first/UAT evidence matrix.
//!
//! This contract indexes the existing DaemonHost/ControlPlane/fake-provider spine. It records
//! zero-effect denial cases and keeps live opt-in proof separate; it never executes a provider or
//! capability and never turns a model/UI assertion into authority.

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const CI12_PRODUCT_GATE_SCHEMA: &str = "kiana.ci12-product-gate.v1";

fn required(value: &str, field: &'static str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > 4_096 || value.contains(['\0', '\r', '\n']) {
        Err(field.to_owned())
    } else {
        Ok(())
    }
}

fn digest(value: &str, field: &'static str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(field.to_owned());
    };
    if hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(field.to_owned())
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ci12Scenario {
    MissingAuthentication,
    UntrustedProject,
    CrossProjectScope,
    ExpiredApproval,
    SecretLeak,
    ToctouDrift,
    Replay,
    ResultUnknown,
    FakeProviderSuccess,
    LiveOptIn,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ci12Disposition {
    Denied,
    Completed,
    Unknown,
    Unverified,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ci12Case {
    pub schema: String,
    pub scenario: Ci12Scenario,
    pub disposition: Ci12Disposition,
    pub handler_calls: u32,
    pub provider_calls: u32,
    pub effect_count: u32,
    pub secret_free: bool,
    pub same_spine: bool,
    pub receipt_bound: bool,
    pub operator_approval_ref: Option<String>,
    pub live_provider_evidence: Option<String>,
    pub limitations: Vec<String>,
    pub digest: String,
}

impl Ci12Case {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CI12_PRODUCT_GATE_SCHEMA
            || !self.secret_free
            || !self.same_spine
            || self.limitations.len() > 32
        {
            return Err("ci12_case_header_or_spine_invalid".to_owned());
        }
        for limitation in &self.limitations {
            required(limitation, "ci12_limitation_invalid")?;
        }
        if let Some(reference) = &self.operator_approval_ref {
            if reference.trim() != reference || !reference.starts_with("approval:") {
                return Err("ci12_approval_ref_invalid".to_owned());
            }
        }
        if let Some(evidence) = &self.live_provider_evidence {
            digest(evidence, "ci12_live_provider_evidence_invalid")?;
        }
        match self.scenario {
            Ci12Scenario::MissingAuthentication
            | Ci12Scenario::UntrustedProject
            | Ci12Scenario::CrossProjectScope
            | Ci12Scenario::ExpiredApproval
            | Ci12Scenario::SecretLeak
            | Ci12Scenario::ToctouDrift
            | Ci12Scenario::Replay => {
                if self.disposition != Ci12Disposition::Denied
                    || self.handler_calls != 0
                    || self.provider_calls != 0
                    || self.effect_count != 0
                {
                    return Err("ci12_deny_case_effect_invalid".to_owned());
                }
            }
            Ci12Scenario::ResultUnknown => {
                if self.disposition != Ci12Disposition::Unknown
                    || !self.receipt_bound
                    || self.effect_count == 0
                    || self.limitations.is_empty()
                {
                    return Err("ci12_unknown_case_reconcile_invalid".to_owned());
                }
            }
            Ci12Scenario::FakeProviderSuccess => {
                if self.disposition != Ci12Disposition::Completed
                    || self.provider_calls != 1
                    || self.effect_count != 1
                    || !self.receipt_bound
                    || self.live_provider_evidence.is_some()
                {
                    return Err("ci12_fake_success_evidence_invalid".to_owned());
                }
            }
            Ci12Scenario::LiveOptIn => {
                if self.disposition != Ci12Disposition::Unverified
                    && (self.operator_approval_ref.is_none()
                        || self.live_provider_evidence.is_none())
                {
                    return Err("ci12_live_opt_in_proof_invalid".to_owned());
                }
                if self.disposition == Ci12Disposition::Completed
                    && self.operator_approval_ref.is_none()
                {
                    return Err("ci12_live_approval_required".to_owned());
                }
            }
        }
        digest(&self.digest, "ci12_case_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("ci12_case_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "scenario": self.scenario,
            "disposition": self.disposition,
            "handler_calls": self.handler_calls,
            "provider_calls": self.provider_calls,
            "effect_count": self.effect_count,
            "secret_free": self.secret_free,
            "same_spine": self.same_spine,
            "receipt_bound": self.receipt_bound,
            "operator_approval_ref": self.operator_approval_ref,
            "live_provider_evidence": self.live_provider_evidence,
            "limitations": self.limitations,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ci12ProductGate {
    pub schema: String,
    pub seed: u64,
    pub cases: Vec<Ci12Case>,
    pub digest: String,
}

impl Ci12ProductGate {
    pub fn new(seed: u64, cases: Vec<Ci12Case>) -> Self {
        let mut gate = Self {
            schema: CI12_PRODUCT_GATE_SCHEMA.to_owned(),
            seed,
            cases,
            digest: String::new(),
        };
        gate.digest = gate.canonical_digest();
        gate
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CI12_PRODUCT_GATE_SCHEMA || self.seed == 0 || self.cases.len() != 10 {
            return Err("ci12_gate_header_invalid".to_owned());
        }
        digest(&self.digest, "ci12_gate_digest_invalid")?;
        if self.digest != self.canonical_digest() {
            return Err("ci12_gate_digest_mismatch".to_owned());
        }
        let mut scenarios = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if !scenarios.insert(case.scenario) {
                return Err("ci12_scenario_duplicate".to_owned());
            }
        }
        Ok(())
    }

    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "seed": self.seed,
            "cases": self.cases,
        }))
    }
}
