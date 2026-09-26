//! EQ-33 workflow and swarm evaluation over caller-supplied topology evidence.
//!
//! This module validates a bounded DAG, attempt history, fan-out/fan-in counts, child scope
//! containment, merge evidence and compensation facts. It is a diagnostic evaluator only: it
//! does not schedule work, create a child Cell, dispatch a runner, merge writes or authorize
//! compensation.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const WORKFLOW_SWARM_INPUT_SCHEMA: &str = "kiana.quality-workflow-swarm-input.v1";
pub const WORKFLOW_SWARM_EVALUATOR_ID: &str = "workflow-swarm";
const NODE_SCHEMA: &str = "kiana.quality-workflow-node-evidence.v1";
const MAX_NODES: usize = 512;
const MAX_TEXT_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowNodeStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
    Compensating,
    Compensated,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowNodeEvidence {
    pub schema: String,
    pub node_id: String,
    pub parent_ids: Vec<String>,
    pub depth: u32,
    pub attempts: Vec<u32>,
    pub parent_scope: BTreeSet<String>,
    pub child_scope: BTreeSet<String>,
    pub expected_children: u32,
    pub observed_children: u32,
    pub required_parents: u32,
    pub observed_parents: u32,
    pub merge_verified: bool,
    #[serde(default)]
    pub merge_expected_digest: Option<String>,
    #[serde(default)]
    pub merge_observed_digest: Option<String>,
    pub compensation_required: bool,
    pub compensation_recorded: bool,
    pub status: WorkflowNodeStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSwarmInput {
    pub schema: String,
    pub max_fan_out: u32,
    pub max_depth: u32,
    pub nodes: Vec<WorkflowNodeEvidence>,
}

impl WorkflowSwarmInput {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != WORKFLOW_SWARM_INPUT_SCHEMA {
            return Err("workflow_swarm_input_schema_invalid");
        }
        if self.max_fan_out == 0 || self.max_depth == 0 || self.nodes.len() > MAX_NODES {
            return Err("workflow_swarm_input_bounds_invalid");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WorkflowSwarmEvaluator;

impl DeterministicEvaluator for WorkflowSwarmEvaluator {
    fn evaluator_id(&self) -> &str {
        WORKFLOW_SWARM_EVALUATOR_ID
    }

    fn evaluate(&self, input: &serde_json::Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: WorkflowSwarmInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("workflow_swarm"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_workflow_swarm(&decoded)
    }
}

pub fn evaluate_workflow_swarm(input: &WorkflowSwarmInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();
    let mut nodes = BTreeMap::new();
    for node in &input.nodes {
        check_schema(
            &mut findings,
            "workflow.node_schema_invalid",
            &node.schema,
            NODE_SCHEMA,
        )?;
        if !valid_reference(&node.node_id) || nodes.insert(node.node_id.clone(), node).is_some() {
            emit(
                &mut findings,
                "workflow.node_identity_invalid",
                "unique_bounded_node_id",
                "invalid_or_duplicate",
            )?;
        }
    }

    let mut graph = BTreeMap::new();
    for node in &input.nodes {
        let mut parent_ids = BTreeSet::new();
        for parent in &node.parent_ids {
            if !valid_reference(parent) || !parent_ids.insert(parent) {
                emit(
                    &mut findings,
                    "workflow.parent_identity_invalid",
                    "unique_bounded_parent_ids",
                    "invalid_or_duplicate",
                )?;
            }
            if !nodes.contains_key(parent) {
                emit(
                    &mut findings,
                    "workflow.parent_missing",
                    "parent_node_exists",
                    "missing",
                )?;
            }
        }
        graph.insert(node.node_id.clone(), node.parent_ids.clone());

        if node.depth > input.max_depth {
            emit(
                &mut findings,
                "workflow.depth_exceeded",
                "depth_within_bound",
                "exceeded",
            )?;
        }
        if node.attempts.is_empty()
            || node.attempts.iter().any(|attempt| *attempt == 0)
            || node.attempts.windows(2).any(|pair| pair[0] >= pair[1])
        {
            emit(
                &mut findings,
                "workflow.attempt_invalid",
                "positive_monotonic_attempts",
                "invalid",
            )?;
        }
        if node.observed_children > input.max_fan_out {
            emit(
                &mut findings,
                "workflow.fanout_exceeded",
                "children_within_bound",
                "exceeded",
            )?;
        }
        if node.expected_children != node.observed_children {
            emit(
                &mut findings,
                "workflow.fanout_mismatch",
                "expected_children_equals_observed",
                "mismatch",
            )?;
        }
        if node.required_parents != node.parent_ids.len() as u32
            || node.observed_parents != node.required_parents
        {
            emit(
                &mut findings,
                "workflow.fanin_mismatch",
                "parent_counts_match",
                "mismatch",
            )?;
        }
        if !node.child_scope.is_subset(&node.parent_scope) {
            emit(
                &mut findings,
                "workflow.child_scope_expanded",
                "child_scope_subset_of_parent",
                "expanded",
            )?;
        }
        if node.required_parents > 1 {
            if !node.merge_verified {
                emit(
                    &mut findings,
                    "workflow.merge_missing",
                    "verified_fan_in_merge",
                    "missing",
                )?;
            }
            match (&node.merge_expected_digest, &node.merge_observed_digest) {
                (Some(expected), Some(observed))
                    if valid_digest(expected) && valid_digest(observed) && expected == observed => {
                }
                (Some(expected), Some(observed))
                    if valid_digest(expected) && valid_digest(observed) =>
                {
                    emit(
                        &mut findings,
                        "workflow.merge_mismatch",
                        "expected_equals_observed",
                        "mismatch",
                    )?;
                }
                _ => emit(
                    &mut findings,
                    "workflow.merge_invalid",
                    "sha256_merge_digests",
                    "invalid_or_missing",
                )?,
            }
        }
        if node.compensation_required && !node.compensation_recorded {
            emit(
                &mut findings,
                "workflow.compensation_missing",
                "compensation_evidence",
                "missing",
            )?;
        }
        if node.status == WorkflowNodeStatus::Compensated
            && (!node.compensation_required || !node.compensation_recorded)
        {
            emit(
                &mut findings,
                "workflow.compensation_invalid",
                "compensated_node_has_compensation",
                "invalid",
            )?;
        }
    }

    let mut marks = BTreeMap::new();
    let mut cycle_reported = BTreeSet::new();
    for node_id in graph.keys() {
        visit_node(
            node_id,
            &graph,
            &mut marks,
            &mut cycle_reported,
            &mut findings,
        )?;
    }
    Ok(findings)
}

fn visit_node(
    node_id: &str,
    graph: &BTreeMap<String, Vec<String>>,
    marks: &mut BTreeMap<String, u8>,
    cycle_reported: &mut BTreeSet<String>,
    findings: &mut Vec<Finding>,
) -> Result<(), EvaluatorError> {
    match marks.get(node_id).copied().unwrap_or(0) {
        1 => {
            if cycle_reported.insert(node_id.to_owned()) {
                emit(findings, "workflow.dag_cycle", "acyclic_graph", "cycle")?;
            }
            return Ok(());
        }
        2 => return Ok(()),
        _ => {}
    }
    marks.insert(node_id.to_owned(), 1);
    if let Some(parents) = graph.get(node_id) {
        for parent in parents {
            if graph.contains_key(parent) {
                visit_node(parent, graph, marks, cycle_reported, findings)?;
            }
        }
    }
    marks.insert(node_id.to_owned(), 2);
    Ok(())
}

fn check_schema(
    findings: &mut Vec<Finding>,
    code: &'static str,
    actual: &str,
    expected: &'static str,
) -> Result<(), EvaluatorError> {
    if actual != expected {
        emit(findings, code, expected, "invalid")?;
    }
    Ok(())
}

fn valid_reference(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && !value.contains(['\0', '\n', '\r'])
        && !value.chars().any(char::is_whitespace)
}

fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: &'static str,
    actual: &'static str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::new(
            code,
            serde_json::Value::String(expected.to_owned()),
            serde_json::Value::String(actual.to_owned()),
            format!("workflow and swarm finding: {code}"),
            "workflow:input",
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
