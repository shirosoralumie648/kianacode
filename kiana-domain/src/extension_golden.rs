//! Offline fake golden trace contracts for Skills, Plugins and Hooks.
//!
//! A golden trace records the inputs and server-owned decisions around an extension path. It does
//! not load a package, run a Hook, invoke the Broker, open a connector or grant a capability. The
//! real ControlPlane/Broker path remains the only authority for an effect.

use crate::{canonical_journal_bytes, json_digest, redact_text, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const EXTENSION_GOLDEN_TRACE_SCHEMA: &str = "kiana.extension-golden-trace.v1";
pub const EXTENSION_GOLDEN_MATRIX_SCHEMA: &str = "kiana.extension-golden-matrix.v1";
pub const EXTENSION_GOLDEN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_GOLDEN_CASES: usize = 32;
const MAX_SURFACES: usize = 5;

fn bounded(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_contains_secret"));
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

fn clear_digest<T: Serialize>(value: &T, field: &str) -> String {
    let mut value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
    if let Some(object) = value.as_object_mut() {
        object.insert(field.to_owned(), serde_json::Value::String(String::new()));
    }
    json_digest(&value)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionGoldenScenario {
    UntrustedProject,
    SkillCollision,
    PathEscape,
    BudgetExceeded,
    HookBlock,
    HookAsk,
    HookUpdate,
    HookTimeout,
    HookCancelled,
    SignedInstall,
    DependencyCycle,
    McpDenied,
    AllowedCapability,
}

impl ExtensionGoldenScenario {
    pub const ALL: [Self; 13] = [
        Self::UntrustedProject,
        Self::SkillCollision,
        Self::PathEscape,
        Self::BudgetExceeded,
        Self::HookBlock,
        Self::HookAsk,
        Self::HookUpdate,
        Self::HookTimeout,
        Self::HookCancelled,
        Self::SignedInstall,
        Self::DependencyCycle,
        Self::McpDenied,
        Self::AllowedCapability,
    ];
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionGoldenOutcome {
    Denied,
    Allowed,
    Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionGoldenEffect {
    None,
    Capability,
    RegistryMutation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionGoldenHookOutcome {
    None,
    Allow,
    Block,
    Ask,
    Update,
    Timeout,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionGoldenSurface {
    Cli,
    Workbench,
    Web,
    Desktop,
    Mcp,
}

impl ExtensionGoldenSurface {
    pub const ALL: [Self; MAX_SURFACES] = [
        Self::Cli,
        Self::Workbench,
        Self::Web,
        Self::Desktop,
        Self::Mcp,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionGoldenTrace {
    pub schema: String,
    pub version: SchemaVersion,
    pub case_id: String,
    pub scenario: ExtensionGoldenScenario,
    pub outcome: ExtensionGoldenOutcome,
    pub effect: ExtensionGoldenEffect,
    pub hook_outcome: ExtensionGoldenHookOutcome,
    pub input_digest: String,
    pub snapshot_digest: String,
    pub policy_digest: String,
    pub gate_digest: String,
    pub final_input_digest: Option<String>,
    pub revalidation_digest: Option<String>,
    pub capability_request_digest: Option<String>,
    pub broker_result_digest: Option<String>,
    pub receipt_digest: Option<String>,
    pub package_digest: Option<String>,
    pub surface_snapshot_digests: BTreeMap<ExtensionGoldenSurface, String>,
    pub effect_count: u32,
    pub network_call_count: u32,
    pub process_call_count: u32,
    pub trace_digest: String,
}

impl ExtensionGoldenTrace {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case_id: impl Into<String>,
        scenario: ExtensionGoldenScenario,
        outcome: ExtensionGoldenOutcome,
        effect: ExtensionGoldenEffect,
        hook_outcome: ExtensionGoldenHookOutcome,
        input_digest: impl Into<String>,
        snapshot_digest: impl Into<String>,
        policy_digest: impl Into<String>,
        gate_digest: impl Into<String>,
        final_input_digest: Option<String>,
        revalidation_digest: Option<String>,
        capability_request_digest: Option<String>,
        broker_result_digest: Option<String>,
        receipt_digest: Option<String>,
        package_digest: Option<String>,
        surface_snapshot_digests: BTreeMap<ExtensionGoldenSurface, String>,
        effect_count: u32,
        network_call_count: u32,
        process_call_count: u32,
    ) -> Result<Self, String> {
        let mut trace = Self {
            schema: EXTENSION_GOLDEN_TRACE_SCHEMA.to_owned(),
            version: EXTENSION_GOLDEN_VERSION,
            case_id: case_id.into(),
            scenario,
            outcome,
            effect,
            hook_outcome,
            input_digest: input_digest.into(),
            snapshot_digest: snapshot_digest.into(),
            policy_digest: policy_digest.into(),
            gate_digest: gate_digest.into(),
            final_input_digest,
            revalidation_digest,
            capability_request_digest,
            broker_result_digest,
            receipt_digest,
            package_digest,
            surface_snapshot_digests,
            effect_count,
            network_call_count,
            process_call_count,
            trace_digest: String::new(),
        };
        trace.trace_digest = trace.digest();
        trace.validate()?;
        Ok(trace)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_GOLDEN_TRACE_SCHEMA
            || self.version != EXTENSION_GOLDEN_VERSION
            || self.effect_count > 1
            || self.network_call_count != 0
            || self.process_call_count != 0
            || self.surface_snapshot_digests.len() != MAX_SURFACES
        {
            return Err("extension_golden_trace_header_invalid".to_owned());
        }
        bounded(&self.case_id, "extension_golden_case_id", 128)?;
        for (value, field) in [
            (&self.input_digest, "extension_golden_input_digest"),
            (&self.snapshot_digest, "extension_golden_snapshot_digest"),
            (&self.policy_digest, "extension_golden_policy_digest"),
            (&self.gate_digest, "extension_golden_gate_digest"),
            (&self.trace_digest, "extension_golden_trace_digest"),
        ] {
            digest(value, field)?;
        }
        for (value, field) in [
            (
                self.final_input_digest.as_deref(),
                "extension_golden_final_input_digest",
            ),
            (
                self.revalidation_digest.as_deref(),
                "extension_golden_revalidation_digest",
            ),
            (
                self.capability_request_digest.as_deref(),
                "extension_golden_capability_request_digest",
            ),
            (
                self.broker_result_digest.as_deref(),
                "extension_golden_broker_result_digest",
            ),
            (
                self.receipt_digest.as_deref(),
                "extension_golden_receipt_digest",
            ),
            (
                self.package_digest.as_deref(),
                "extension_golden_package_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        let mut surfaces = BTreeSet::new();
        for (surface, snapshot_digest) in &self.surface_snapshot_digests {
            digest(snapshot_digest, "extension_golden_surface_snapshot_digest")?;
            if snapshot_digest != &self.snapshot_digest || !surfaces.insert(*surface) {
                return Err("extension_golden_surface_snapshot_drift".to_owned());
            }
        }
        if ExtensionGoldenSurface::ALL
            .iter()
            .any(|surface| !surfaces.contains(surface))
        {
            return Err("extension_golden_surface_coverage_missing".to_owned());
        }
        if self.hook_outcome == ExtensionGoldenHookOutcome::Update
            && (self.final_input_digest.is_none() || self.revalidation_digest.is_none())
        {
            return Err("extension_golden_hook_update_revalidation_missing".to_owned());
        }
        match self.outcome {
            ExtensionGoldenOutcome::Denied | ExtensionGoldenOutcome::Unknown => {
                if self.effect != ExtensionGoldenEffect::None
                    || self.effect_count != 0
                    || self.capability_request_digest.is_some()
                    || self.broker_result_digest.is_some()
                    || self.receipt_digest.is_some()
                {
                    return Err("extension_golden_denied_effect_leak".to_owned());
                }
            }
            ExtensionGoldenOutcome::Allowed => {
                if self.effect_count != 1 {
                    return Err("extension_golden_allowed_effect_count_invalid".to_owned());
                }
                match self.effect {
                    ExtensionGoldenEffect::Capability => {
                        if self.capability_request_digest.is_none()
                            || self.broker_result_digest.is_none()
                            || self.receipt_digest.is_none()
                        {
                            return Err("extension_golden_capability_receipt_missing".to_owned());
                        }
                        if !matches!(
                            self.hook_outcome,
                            ExtensionGoldenHookOutcome::Allow | ExtensionGoldenHookOutcome::Update
                        ) {
                            return Err("extension_golden_capability_hook_denied".to_owned());
                        }
                    }
                    ExtensionGoldenEffect::RegistryMutation => {
                        if self.package_digest.is_none() || self.receipt_digest.is_none() {
                            return Err("extension_golden_registry_receipt_missing".to_owned());
                        }
                        if self.capability_request_digest.is_some()
                            || self.broker_result_digest.is_some()
                        {
                            return Err("extension_golden_registry_capability_conflict".to_owned());
                        }
                    }
                    ExtensionGoldenEffect::None => {
                        return Err("extension_golden_allowed_effect_missing".to_owned())
                    }
                }
            }
        }
        match self.scenario {
            ExtensionGoldenScenario::HookUpdate
                if self.outcome != ExtensionGoldenOutcome::Allowed
                    || self.hook_outcome != ExtensionGoldenHookOutcome::Update =>
            {
                return Err("extension_golden_hook_update_scenario_invalid".to_owned())
            }
            ExtensionGoldenScenario::SignedInstall
                if self.outcome != ExtensionGoldenOutcome::Allowed
                    || self.effect != ExtensionGoldenEffect::RegistryMutation =>
            {
                return Err("extension_golden_signed_install_scenario_invalid".to_owned())
            }
            ExtensionGoldenScenario::AllowedCapability
                if self.outcome != ExtensionGoldenOutcome::Allowed
                    || self.effect != ExtensionGoldenEffect::Capability =>
            {
                return Err("extension_golden_allowed_scenario_invalid".to_owned())
            }
            ExtensionGoldenScenario::HookTimeout
                if self.outcome != ExtensionGoldenOutcome::Unknown
                    || self.hook_outcome != ExtensionGoldenHookOutcome::Timeout =>
            {
                return Err("extension_golden_timeout_scenario_invalid".to_owned())
            }
            ExtensionGoldenScenario::HookBlock
                if self.outcome != ExtensionGoldenOutcome::Denied
                    || self.hook_outcome != ExtensionGoldenHookOutcome::Block =>
            {
                return Err("extension_golden_block_scenario_invalid".to_owned())
            }
            ExtensionGoldenScenario::HookAsk
                if self.outcome != ExtensionGoldenOutcome::Denied
                    || self.hook_outcome != ExtensionGoldenHookOutcome::Ask =>
            {
                return Err("extension_golden_ask_scenario_invalid".to_owned())
            }
            ExtensionGoldenScenario::HookCancelled
                if self.outcome != ExtensionGoldenOutcome::Denied
                    || self.hook_outcome != ExtensionGoldenHookOutcome::Cancelled =>
            {
                return Err("extension_golden_cancel_scenario_invalid".to_owned())
            }
            _ => {}
        }
        if self.trace_digest != self.digest() {
            return Err("extension_golden_trace_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "trace_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtensionGoldenMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub snapshot_digest: String,
    pub cases: Vec<ExtensionGoldenTrace>,
    pub matrix_digest: String,
}

impl ExtensionGoldenMatrix {
    pub fn new(
        snapshot_digest: impl Into<String>,
        cases: Vec<ExtensionGoldenTrace>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: EXTENSION_GOLDEN_MATRIX_SCHEMA.to_owned(),
            version: EXTENSION_GOLDEN_VERSION,
            snapshot_digest: snapshot_digest.into(),
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != EXTENSION_GOLDEN_MATRIX_SCHEMA
            || self.version != EXTENSION_GOLDEN_VERSION
            || self.cases.is_empty()
            || self.cases.len() > MAX_GOLDEN_CASES
        {
            return Err("extension_golden_matrix_header_invalid".to_owned());
        }
        digest(
            &self.snapshot_digest,
            "extension_golden_matrix_snapshot_digest",
        )?;
        let mut scenarios = BTreeSet::new();
        let mut ids = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.snapshot_digest != self.snapshot_digest
                || !scenarios.insert(case.scenario)
                || !ids.insert(&case.case_id)
            {
                return Err("extension_golden_matrix_case_binding_invalid".to_owned());
            }
        }
        if ExtensionGoldenScenario::ALL
            .iter()
            .any(|scenario| !scenarios.contains(scenario))
        {
            return Err("extension_golden_matrix_coverage_missing".to_owned());
        }
        digest(&self.matrix_digest, "extension_golden_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("extension_golden_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn denied_cases(&self) -> Result<Vec<String>, String> {
        self.validate()?;
        Ok(self
            .cases
            .iter()
            .filter(|case| case.outcome == ExtensionGoldenOutcome::Denied)
            .map(|case| case.case_id.clone())
            .collect())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "matrix_digest")
    }
}
