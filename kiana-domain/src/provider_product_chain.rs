//! Provider-backed product-chain evidence contract.
//!
//! This contract describes a loopback/cassette regression over the existing execution spine. It
//! binds model, capability, event, Receipt, file-observation and four-surface projections without
//! executing any of them. A source/CI fixture must still use DaemonHost -> ControlPlane ->
//! KianaHarness/ProviderGateway; this module is not a second runner or effect authority.

use crate::{canonical_journal_bytes, json_digest, redact_text, SchemaVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PROVIDER_PRODUCT_CHAIN_SCHEMA: &str = "kiana.provider-product-chain.v1";
pub const PROVIDER_PRODUCT_CHAIN_STAGE_SCHEMA: &str = "kiana.provider-product-chain-stage.v1";
pub const PROVIDER_PRODUCT_SURFACE_SCHEMA: &str = "kiana.provider-product-surface.v1";
pub const PROVIDER_PRODUCT_CHAIN_MATRIX_SCHEMA: &str = "kiana.provider-product-chain-matrix.v1";
pub const PROVIDER_PRODUCT_CHAIN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
const MAX_STAGES: usize = 6;
const MAX_SURFACES: usize = 4;
const MAX_CASES: usize = 16;

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
pub enum ProviderProductChainScenario {
    CodingRoundTrip,
    CancelBetweenModelFinishAndDispatch,
    SlowSubscriber,
    RunContinueResume,
    BudgetDenied,
}

impl ProviderProductChainScenario {
    pub const ALL: [Self; 5] = [
        Self::CodingRoundTrip,
        Self::CancelBetweenModelFinishAndDispatch,
        Self::SlowSubscriber,
        Self::RunContinueResume,
        Self::BudgetDenied,
    ];

    fn required_stages(self) -> Vec<ProviderProductChainStage> {
        match self {
            Self::CodingRoundTrip | Self::SlowSubscriber | Self::RunContinueResume => vec![
                ProviderProductChainStage::ModelRequest,
                ProviderProductChainStage::ModelReply,
                ProviderProductChainStage::CapabilityDispatch,
                ProviderProductChainStage::ToolResult,
                ProviderProductChainStage::RuntimeEvent,
                ProviderProductChainStage::Receipt,
            ],
            Self::CancelBetweenModelFinishAndDispatch => vec![
                ProviderProductChainStage::ModelRequest,
                ProviderProductChainStage::ModelReply,
                ProviderProductChainStage::RuntimeEvent,
                ProviderProductChainStage::Receipt,
            ],
            Self::BudgetDenied => vec![
                ProviderProductChainStage::RuntimeEvent,
                ProviderProductChainStage::Receipt,
            ],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProductChainMode {
    LoopbackCassette,
    LiveOptIn,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProductChainTerminal {
    Completed,
    Cancelled,
    BudgetDenied,
    ResultUnknown,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProductChainStage {
    ModelRequest,
    ModelReply,
    CapabilityDispatch,
    ToolResult,
    RuntimeEvent,
    Receipt,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProductSurface {
    Cli,
    Tty,
    Web,
    Desktop,
}

impl ProviderProductSurface {
    pub const ALL: [Self; 4] = [Self::Cli, Self::Tty, Self::Web, Self::Desktop];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProductChainStageEvidence {
    pub schema: String,
    pub stage: ProviderProductChainStage,
    pub sequence: u64,
    pub stage_digest: String,
}

impl ProviderProductChainStageEvidence {
    pub fn new(
        stage: ProviderProductChainStage,
        sequence: u64,
        stage_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let evidence = Self {
            schema: PROVIDER_PRODUCT_CHAIN_STAGE_SCHEMA.to_owned(),
            stage,
            sequence,
            stage_digest: stage_digest.into(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_PRODUCT_CHAIN_STAGE_SCHEMA || self.sequence == 0 {
            return Err("provider_product_chain_stage_header_invalid".to_owned());
        }
        digest(&self.stage_digest, "provider_product_chain_stage_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProductSurfaceSnapshot {
    pub schema: String,
    pub surface: ProviderProductSurface,
    pub terminal: ProviderProductChainTerminal,
    pub receipt_digest: String,
    pub file_effect_digest: Option<String>,
    pub usage_digest: Option<String>,
    pub source_cursor: u64,
    pub result_unknown: bool,
}

impl ProviderProductSurfaceSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        surface: ProviderProductSurface,
        terminal: ProviderProductChainTerminal,
        receipt_digest: impl Into<String>,
        file_effect_digest: Option<String>,
        usage_digest: Option<String>,
        source_cursor: u64,
        result_unknown: bool,
    ) -> Result<Self, String> {
        let snapshot = Self {
            schema: PROVIDER_PRODUCT_SURFACE_SCHEMA.to_owned(),
            surface,
            terminal,
            receipt_digest: receipt_digest.into(),
            file_effect_digest,
            usage_digest,
            source_cursor,
            result_unknown,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_PRODUCT_SURFACE_SCHEMA || self.source_cursor == 0 {
            return Err("provider_product_surface_header_invalid".to_owned());
        }
        digest(
            &self.receipt_digest,
            "provider_product_surface_receipt_digest",
        )?;
        for (value, field) in [
            (
                self.file_effect_digest.as_deref(),
                "provider_product_surface_file_effect_digest",
            ),
            (
                self.usage_digest.as_deref(),
                "provider_product_surface_usage_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        if self.result_unknown && self.terminal != ProviderProductChainTerminal::ResultUnknown {
            return Err("provider_product_surface_unknown_terminal_mismatch".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProductChainEvidence {
    pub schema: String,
    pub version: SchemaVersion,
    pub case_id: String,
    pub scenario: ProviderProductChainScenario,
    pub terminal: ProviderProductChainTerminal,
    pub mode: ProviderProductChainMode,
    pub operator_approved: bool,
    pub run_digest: String,
    pub route_digest: String,
    pub receipt_digest: String,
    pub file_effect_digest: Option<String>,
    pub usage_digest: Option<String>,
    pub model_request_count: u32,
    pub capability_dispatch_count: u32,
    pub tool_result_count: u32,
    pub terminal_event_count: u32,
    pub stages: Vec<ProviderProductChainStageEvidence>,
    pub surfaces: Vec<ProviderProductSurfaceSnapshot>,
    pub cancel_fence_verified: bool,
    pub slow_subscriber_terminal_retained: bool,
    pub late_increment_ignored: bool,
    pub result_unknown: bool,
    pub evidence_digest: String,
}

impl ProviderProductChainEvidence {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        case_id: impl Into<String>,
        scenario: ProviderProductChainScenario,
        terminal: ProviderProductChainTerminal,
        mode: ProviderProductChainMode,
        operator_approved: bool,
        run_digest: impl Into<String>,
        route_digest: impl Into<String>,
        receipt_digest: impl Into<String>,
        file_effect_digest: Option<String>,
        usage_digest: Option<String>,
        model_request_count: u32,
        capability_dispatch_count: u32,
        tool_result_count: u32,
        terminal_event_count: u32,
        stages: Vec<ProviderProductChainStageEvidence>,
        surfaces: Vec<ProviderProductSurfaceSnapshot>,
        cancel_fence_verified: bool,
        slow_subscriber_terminal_retained: bool,
        late_increment_ignored: bool,
        result_unknown: bool,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            schema: PROVIDER_PRODUCT_CHAIN_SCHEMA.to_owned(),
            version: PROVIDER_PRODUCT_CHAIN_VERSION,
            case_id: case_id.into(),
            scenario,
            terminal,
            mode,
            operator_approved,
            run_digest: run_digest.into(),
            route_digest: route_digest.into(),
            receipt_digest: receipt_digest.into(),
            file_effect_digest,
            usage_digest,
            model_request_count,
            capability_dispatch_count,
            tool_result_count,
            terminal_event_count,
            stages,
            surfaces,
            cancel_fence_verified,
            slow_subscriber_terminal_retained,
            late_increment_ignored,
            result_unknown,
            evidence_digest: String::new(),
        };
        evidence.evidence_digest = evidence.digest();
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_PRODUCT_CHAIN_SCHEMA
            || self.version != PROVIDER_PRODUCT_CHAIN_VERSION
            || self.terminal_event_count != 1
            || self.model_request_count > 16
            || self.capability_dispatch_count > 16
            || self.tool_result_count > 16
            || self.stages.is_empty()
            || self.stages.len() > MAX_STAGES
            || self.surfaces.len() != MAX_SURFACES
        {
            return Err("provider_product_chain_header_invalid".to_owned());
        }
        bounded(&self.case_id, "provider_product_chain_case_id", 128)?;
        for (value, field) in [
            (&self.run_digest, "provider_product_chain_run_digest"),
            (&self.route_digest, "provider_product_chain_route_digest"),
            (
                &self.receipt_digest,
                "provider_product_chain_receipt_digest",
            ),
            (
                &self.evidence_digest,
                "provider_product_chain_evidence_digest",
            ),
        ] {
            digest(value, field)?;
        }
        for (value, field) in [
            (
                self.file_effect_digest.as_deref(),
                "provider_product_chain_file_effect_digest",
            ),
            (
                self.usage_digest.as_deref(),
                "provider_product_chain_usage_digest",
            ),
        ] {
            if let Some(value) = value {
                digest(value, field)?;
            }
        }
        match self.mode {
            ProviderProductChainMode::LoopbackCassette if self.operator_approved => {
                return Err("provider_product_chain_loopback_approval_conflict".to_owned())
            }
            ProviderProductChainMode::LiveOptIn if !self.operator_approved => {
                return Err("provider_product_chain_live_approval_missing".to_owned())
            }
            _ => {}
        }
        if self.result_unknown != (self.terminal == ProviderProductChainTerminal::ResultUnknown) {
            return Err("provider_product_chain_unknown_terminal_mismatch".to_owned());
        }
        let required = self.scenario.required_stages();
        let mut stages = BTreeSet::new();
        let mut previous_sequence = 0;
        for stage in &self.stages {
            stage.validate()?;
            if stage.sequence <= previous_sequence || !stages.insert(stage.stage) {
                return Err("provider_product_chain_stage_order_invalid".to_owned());
            }
            previous_sequence = stage.sequence;
        }
        let required = required.into_iter().collect::<BTreeSet<_>>();
        if stages != required {
            return Err("provider_product_chain_stage_coverage_invalid".to_owned());
        }
        for surface in &self.surfaces {
            surface.validate()?;
        }
        let mut surfaces = BTreeSet::new();
        for surface in &self.surfaces {
            if !surfaces.insert(surface.surface) {
                return Err("provider_product_chain_surface_duplicate".to_owned());
            }
            if surface.terminal != self.terminal
                || surface.receipt_digest != self.receipt_digest
                || surface.file_effect_digest != self.file_effect_digest
                || surface.usage_digest != self.usage_digest
                || surface.result_unknown != self.result_unknown
            {
                return Err("provider_product_chain_surface_drift".to_owned());
            }
        }
        if ProviderProductSurface::ALL
            .iter()
            .any(|surface| !surfaces.contains(surface))
        {
            return Err("provider_product_chain_surface_coverage_missing".to_owned());
        }
        match self.scenario {
            ProviderProductChainScenario::CodingRoundTrip
            | ProviderProductChainScenario::SlowSubscriber
            | ProviderProductChainScenario::RunContinueResume => {
                if self.terminal != ProviderProductChainTerminal::Completed
                    || self.model_request_count != 2
                    || self.capability_dispatch_count != 1
                    || self.tool_result_count != 1
                    || self.file_effect_digest.is_none()
                    || self.usage_digest.is_none()
                {
                    return Err("provider_product_chain_coding_counts_invalid".to_owned());
                }
            }
            ProviderProductChainScenario::CancelBetweenModelFinishAndDispatch => {
                if self.terminal != ProviderProductChainTerminal::Cancelled
                    || self.model_request_count != 1
                    || self.capability_dispatch_count != 0
                    || self.tool_result_count != 0
                    || !self.cancel_fence_verified
                    || !self.late_increment_ignored
                {
                    return Err("provider_product_chain_cancel_fence_invalid".to_owned());
                }
            }
            ProviderProductChainScenario::BudgetDenied => {
                if self.terminal != ProviderProductChainTerminal::BudgetDenied
                    || self.model_request_count != 0
                    || self.capability_dispatch_count != 0
                    || self.tool_result_count != 0
                    || self.file_effect_digest.is_some()
                {
                    return Err("provider_product_chain_budget_boundary_invalid".to_owned());
                }
            }
        }
        if self.scenario == ProviderProductChainScenario::SlowSubscriber
            && (!self.slow_subscriber_terminal_retained || !self.late_increment_ignored)
        {
            return Err("provider_product_chain_slow_subscriber_terminal_lost".to_owned());
        }
        if self.evidence_digest != self.digest() {
            return Err("provider_product_chain_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "evidence_digest")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderProductChainMatrix {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_digest: String,
    pub cases: Vec<ProviderProductChainEvidence>,
    pub matrix_digest: String,
}

impl ProviderProductChainMatrix {
    pub fn new(
        run_digest: impl Into<String>,
        cases: Vec<ProviderProductChainEvidence>,
    ) -> Result<Self, String> {
        let mut matrix = Self {
            schema: PROVIDER_PRODUCT_CHAIN_MATRIX_SCHEMA.to_owned(),
            version: PROVIDER_PRODUCT_CHAIN_VERSION,
            run_digest: run_digest.into(),
            cases,
            matrix_digest: String::new(),
        };
        matrix.matrix_digest = matrix.digest();
        matrix.validate()?;
        Ok(matrix)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_PRODUCT_CHAIN_MATRIX_SCHEMA
            || self.version != PROVIDER_PRODUCT_CHAIN_VERSION
            || self.cases.is_empty()
            || self.cases.len() > MAX_CASES
        {
            return Err("provider_product_chain_matrix_header_invalid".to_owned());
        }
        digest(&self.run_digest, "provider_product_chain_matrix_run_digest")?;
        let mut scenarios = BTreeSet::new();
        let mut case_ids = BTreeSet::new();
        for case in &self.cases {
            case.validate()?;
            if case.run_digest != self.run_digest {
                return Err("provider_product_chain_matrix_run_drift".to_owned());
            }
            if !scenarios.insert(case.scenario) || !case_ids.insert(&case.case_id) {
                return Err("provider_product_chain_matrix_duplicate".to_owned());
            }
        }
        if ProviderProductChainScenario::ALL
            .iter()
            .any(|scenario| !scenarios.contains(scenario))
        {
            return Err("provider_product_chain_matrix_coverage_missing".to_owned());
        }
        digest(&self.matrix_digest, "provider_product_chain_matrix_digest")?;
        if self.matrix_digest != self.digest() {
            return Err("provider_product_chain_matrix_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn case(
        &self,
        scenario: ProviderProductChainScenario,
    ) -> Option<&ProviderProductChainEvidence> {
        self.cases.iter().find(|case| case.scenario == scenario)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn digest(&self) -> String {
        clear_digest(self, "matrix_digest")
    }
}
