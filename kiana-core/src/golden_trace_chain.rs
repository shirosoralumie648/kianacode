//! BQ-29: the end-to-end chain, and the three ways it lies.
//!
//! ```text
//! model → tool → event → receipt → invoice correction
//! ```
//!
//! A `GoldenTrace` records that chain so it can be replayed. Recording it is the easy half; this
//! module checks that a chain somebody reports actually is that chain. Three failures matter, and
//! none of them is visible in a list of stage names:
//!
//! - **an entrypoint that bypassed DaemonHost.** Every entry is supposed to reach the control
//!   plane through one route. A stage that arrived by another way is refused even when its
//!   contents look right, because the route is the thing that makes the contents trustworthy.
//! - **call counts that disagree with the reservations.** A provider called twice against one
//!   reservation is a double charge wearing a receipt; a handler that ran without a reservation is
//!   an effect nobody accounted for.
//! - **a receipt that writes a runtime success as a business outcome.** The call returned 200. That
//!   is a fact about the transport. Whether the business succeeded is a separate claim needing its
//!   own evidence, and conflating the two is how a failed workflow gets invoiced as a success.
//!
//! Reused rather than forked: the trace type is `kiana_domain::quality::GoldenTrace`, the route is
//! the existing `ENTRYPOINT_ROUTE`, and the business-outcome rule follows the pattern already used
//! by the automation snapshot, where a confirmed outcome requires evidence references.
//!
//! This module verifies. It replays nothing and calls nothing.

use kiana_domain::{json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::entrypoint_parity::ENTRYPOINT_ROUTE;

pub const GOLDEN_TRACE_CHAIN_SCHEMA: &str = "kiana.golden-trace-chain.v1";
pub const GOLDEN_TRACE_CHAIN_REPORT_SCHEMA: &str = "kiana.golden-trace-chain-report.v1";
pub const GOLDEN_TRACE_CHAIN_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// The chain, in the only order it may appear.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainStage {
    ModelCall,
    ToolDispatch,
    EventAppend,
    Receipt,
    InvoiceCorrection,
}

impl ChainStage {
    pub const ALL: [Self; 5] = [
        Self::ModelCall,
        Self::ToolDispatch,
        Self::EventAppend,
        Self::Receipt,
        Self::InvoiceCorrection,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ModelCall => "model_call",
            Self::ToolDispatch => "tool_dispatch",
            Self::EventAppend => "event_append",
            Self::Receipt => "receipt",
            Self::InvoiceCorrection => "invoice_correction",
        }
    }
}

/// One stage as somebody reported it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainStageObservation {
    pub stage: ChainStage,
    /// How this stage was reached. Every stage must name the one route.
    pub route: String,
    pub provider_calls: u32,
    pub handler_calls: u32,
    /// Reservations held when the stage finished.
    pub reservations_held: u32,
    /// The transport succeeded. Says nothing about the business.
    pub runtime_success: bool,
    /// The business outcome somebody is claiming, if any.
    pub business_outcome: Option<String>,
    /// Evidence for that claim. Required whenever a claim is made.
    pub business_evidence_refs: Vec<String>,
    pub receipt_digest: Option<String>,
}

impl ChainStageObservation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stage: ChainStage,
        route: impl Into<String>,
        provider_calls: u32,
        handler_calls: u32,
        reservations_held: u32,
        runtime_success: bool,
        business_outcome: Option<String>,
        business_evidence_refs: Vec<String>,
        receipt_digest: Option<String>,
    ) -> Self {
        Self {
            stage,
            route: route.into(),
            provider_calls,
            handler_calls,
            reservations_held,
            runtime_success,
            business_outcome,
            business_evidence_refs,
            receipt_digest,
        }
    }
}

/// What the chain is supposed to contain.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenTraceChain {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: String,
    /// The `GoldenTrace.trace_digest` this chain claims to reproduce.
    pub golden_trace_digest: String,
    pub expected_provider_calls: u32,
    pub expected_handler_calls: u32,
    pub stages: Vec<ChainStageObservation>,
    pub chain_digest: String,
}

impl GoldenTraceChain {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trace_id: impl Into<String>,
        golden_trace_digest: impl Into<String>,
        expected_provider_calls: u32,
        expected_handler_calls: u32,
        stages: Vec<ChainStageObservation>,
    ) -> Self {
        let mut value = Self {
            schema: GOLDEN_TRACE_CHAIN_SCHEMA.to_owned(),
            version: GOLDEN_TRACE_CHAIN_VERSION,
            trace_id: trace_id.into(),
            golden_trace_digest: golden_trace_digest.into(),
            expected_provider_calls,
            expected_handler_calls,
            stages,
            chain_digest: String::new(),
        };
        value.chain_digest = value.digest();
        value
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "trace_id": self.trace_id,
            "golden_trace_digest": self.golden_trace_digest,
            "expected_provider_calls": self.expected_provider_calls,
            "expected_handler_calls": self.expected_handler_calls,
            "stages": self.stages,
        }))
    }
}

/// The sealed verdict.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoldenTraceChainReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: String,
    pub golden_trace_digest: String,
    pub stages: Vec<ChainStage>,
    pub route: String,
    pub provider_calls: u32,
    pub handler_calls: u32,
    /// Whether the chain claimed a business outcome, and on what evidence.
    pub business_outcome_claimed: bool,
    pub limitations: Vec<String>,
    pub report_digest: String,
}

impl GoldenTraceChainReport {
    pub fn validate_against(&self, chain: &GoldenTraceChain) -> Result<(), String> {
        if self.schema != GOLDEN_TRACE_CHAIN_REPORT_SCHEMA
            || !self.version.is_compatible_with(&GOLDEN_TRACE_CHAIN_VERSION)
            || self.trace_id != chain.trace_id
            || self.golden_trace_digest != chain.golden_trace_digest
            || self.provider_calls != chain.expected_provider_calls
            || self.handler_calls != chain.expected_handler_calls
        {
            return Err("golden_chain_report_binding_invalid".to_owned());
        }
        if self.report_digest != self.digest() {
            return Err("golden_chain_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "trace_id": self.trace_id,
            "golden_trace_digest": self.golden_trace_digest,
            "stages": self.stages,
            "route": self.route,
            "provider_calls": self.provider_calls,
            "handler_calls": self.handler_calls,
            "business_outcome_claimed": self.business_outcome_claimed,
            "limitations": self.limitations,
        }))
    }
}

/// Verify a reported chain.
///
/// The order of the checks is the argument. The route is checked **per stage and first**, because
/// the route is what makes the rest of the stage trustworthy — a stage that arrived another way is
/// not a slightly worse stage, it is an unverified one. Stage order comes next. Only then are the
/// counts compared, and the business-outcome rule last, because it is the one that needs
/// everything else to already be right before it can mean anything.
pub fn verify_golden_trace_chain(
    chain: &GoldenTraceChain,
) -> Result<GoldenTraceChainReport, String> {
    if chain.schema != GOLDEN_TRACE_CHAIN_SCHEMA
        || !chain.version.is_compatible_with(&GOLDEN_TRACE_CHAIN_VERSION)
    {
        return Err("golden_chain_header_invalid".to_owned());
    }
    if chain.chain_digest != chain.digest() {
        return Err("golden_chain_digest_mismatch".to_owned());
    }
    if chain.trace_id.trim().is_empty() {
        return Err("golden_chain_trace_id_required".to_owned());
    }

    for stage in &chain.stages {
        if stage.route != ENTRYPOINT_ROUTE {
            return Err("golden_chain_stage_route_bypass".to_owned());
        }
    }

    // The chain must contain every stage, in order. A missing stage is a hole in the audit, not a
    // shorter trace.
    let observed: Vec<ChainStage> = chain.stages.iter().map(|stage| stage.stage).collect();
    if observed != ChainStage::ALL.to_vec() {
        return Err("golden_chain_stage_order_invalid".to_owned());
    }

    let provider_calls: u32 = chain.stages.iter().map(|stage| stage.provider_calls).sum();
    let handler_calls: u32 = chain.stages.iter().map(|stage| stage.handler_calls).sum();
    if provider_calls != chain.expected_provider_calls
        || handler_calls != chain.expected_handler_calls
    {
        return Err("golden_chain_call_count_mismatch".to_owned());
    }

    // A reservation that outlives every stage, or a stage that ran with none, are both accounting
    // failures; the first is a leak and the second is an effect nobody charged for.
    let reservations_held = chain
        .stages
        .iter()
        .map(|stage| stage.reservations_held)
        .max()
        .unwrap_or(0);
    if reservations_held > provider_calls {
        return Err("golden_chain_reservation_mismatch".to_owned());
    }
    for stage in &chain.stages {
        if stage.provider_calls > 0 && stage.reservations_held == 0 {
            return Err("golden_chain_reservation_mismatch".to_owned());
        }
    }

    for stage in &chain.stages {
        if stage.stage == ChainStage::Receipt && stage.receipt_digest.is_none() {
            return Err("golden_chain_receipt_missing".to_owned());
        }
        if let Some(outcome) = &stage.business_outcome {
            if outcome.trim().is_empty() {
                return Err("golden_chain_business_outcome_empty".to_owned());
            }
            // The rule the card names: a 200 is a fact about the transport. Turning it into a
            // business outcome needs the business to have succeeded *and* evidence for the claim.
            if !stage.runtime_success {
                return Err("golden_chain_business_outcome_without_runtime".to_owned());
            }
            if stage.business_evidence_refs.is_empty() {
                return Err("golden_chain_business_outcome_evidence_missing".to_owned());
            }
        } else if !stage.business_evidence_refs.is_empty() {
            // Evidence travelling without a claim is the same confusion in reverse.
            return Err("golden_chain_business_evidence_without_outcome".to_owned());
        }
    }

    let business_outcome_claimed = chain
        .stages
        .iter()
        .any(|stage| stage.business_outcome.is_some());

    let mut report = GoldenTraceChainReport {
        schema: GOLDEN_TRACE_CHAIN_REPORT_SCHEMA.to_owned(),
        version: GOLDEN_TRACE_CHAIN_VERSION,
        trace_id: chain.trace_id.clone(),
        golden_trace_digest: chain.golden_trace_digest.clone(),
        stages: observed,
        route: ENTRYPOINT_ROUTE.to_owned(),
        provider_calls,
        handler_calls,
        business_outcome_claimed,
        limitations: vec![
            "no stage in this report was executed; the chain was supplied".to_owned(),
            "the golden trace was not loaded and nothing was replayed".to_owned(),
        ],
        report_digest: String::new(),
    };
    report.report_digest = report.digest();
    report.validate_against(chain)?;
    Ok(report)
}
