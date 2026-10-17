//! EQ-33 workflow and swarm evaluation over caller-supplied topology evidence.
//!
//! The evaluator validates a bounded DAG of node evidence together with its attempt history,
//! fan-out/fan-in counts, child scope containment, merge verification and compensation facts.
//! Every section is deny-first: schema drift, missing or self-referencing parents, cycles,
//! non-monotonic attempts, scope expansion, unverified fan-in merges and absent compensation
//! evidence all produce stable findings before any success is considered.
//!
//! This is a pure diagnostic over the evidence it is given: it never schedules a node, creates a
//! child Cell, dispatches a runner, merges a workspace, runs a swarm, sends messages between
//! agents or authorizes compensation.  Nodes are ordered by a stable total key before evaluation,
//! so the finding set does not depend on the caller's array order while duplicate evidence stays
//! visible.  Every caller-supplied value passes through [`Finding::redacted`], and each finding
//! cites the evidence section it was derived from.

use crate::{DeterministicEvaluator, EvaluatorError, Finding, MAX_FINDINGS};
use kiana_domain::redact_text;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Versioned envelope for one workflow/swarm evaluation input.
pub const WORKFLOW_SWARM_INPUT_SCHEMA: &str = "kiana.quality-workflow-swarm-input.v1";
/// Stable registry identifier for the workflow/swarm evaluator.
pub const WORKFLOW_SWARM_EVALUATOR_ID: &str = "workflow-swarm";
const NODE_SCHEMA: &str = "kiana.quality-workflow-node-evidence.v1";
const MAX_NODES: usize = 512;
const MAX_ATTEMPTS_PER_NODE: usize = 64;
const MAX_FAN_IN: u32 = 64;
const MAX_SCOPE_ENTRIES: usize = 64;
const MAX_TEXT_BYTES: usize = 512;
const INPUT_EVIDENCE: &str = "workflow:input";
const DAG_EVIDENCE: &str = "workflow:dag";
const NODE_EVIDENCE: &str = "workflow:node:";

/// Terminal-ish status observed for one workflow node.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
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

/// One node of the observed workflow/swarm graph.
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
    /// Bounded reference to the recorded compensation receipt.  Required when
    /// [`Self::compensation_recorded`] is set, and left empty otherwise.
    pub compensation_receipt_ref: String,
    pub status: WorkflowNodeStatus,
}

/// Versioned input envelope for [`WorkflowSwarmEvaluator`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowSwarmInput {
    pub schema: String,
    pub max_fan_out: u32,
    pub max_depth: u32,
    pub nodes: Vec<WorkflowNodeEvidence>,
}

impl WorkflowSwarmInput {
    /// Hard input bounds.  Structural drift fails closed before any finding is produced; entry or
    /// bound declaration problems remain findings so the report explains *why* the evidence is
    /// unusable.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != WORKFLOW_SWARM_INPUT_SCHEMA {
            return Err("workflow_swarm_input_schema_invalid");
        }
        if self.nodes.len() > MAX_NODES {
            return Err("workflow_swarm_node_limit_exceeded");
        }
        Ok(())
    }

    pub fn as_value(&self) -> Value {
        serde_json::to_value(self).expect("workflow swarm input is serializable")
    }
}

/// Pure workflow/swarm evaluator.  All authority-bearing actions remain outside this type.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorkflowSwarmEvaluator;

impl DeterministicEvaluator for WorkflowSwarmEvaluator {
    fn evaluator_id(&self) -> &str {
        WORKFLOW_SWARM_EVALUATOR_ID
    }

    fn evaluate(&self, input: &Value) -> Result<Vec<Finding>, EvaluatorError> {
        let decoded: WorkflowSwarmInput = serde_json::from_value(input.clone())
            .map_err(|_| EvaluatorError::InputInvalid("workflow_swarm"))?;
        decoded.validate().map_err(EvaluatorError::InputInvalid)?;
        evaluate_workflow_swarm(&decoded)
    }
}

/// Evaluate the deny-first workflow/swarm matrix without reading any store.
///
/// Check order is deliberate: declared bounds, then per node the schema, identity, parent edges and
/// depth order, attempts, fan-out/fan-in counts, scope containment, merge evidence and compensation
/// facts, and finally the DAG cycle check.  Nodes are ordered by a stable total key before
/// evaluation so the finding set does not depend on the caller's array order.
pub fn evaluate_workflow_swarm(input: &WorkflowSwarmInput) -> Result<Vec<Finding>, EvaluatorError> {
    let mut findings = Vec::new();

    if input.max_fan_out == 0 {
        emit(
            &mut findings,
            "workflow.bounds_invalid",
            text("positive_max_fan_out"),
            text("zero_max_fan_out"),
            INPUT_EVIDENCE,
        )?;
    }
    if input.max_depth == 0 {
        emit(
            &mut findings,
            "workflow.bounds_invalid",
            text("positive_max_depth"),
            text("zero_max_depth"),
            INPUT_EVIDENCE,
        )?;
    }

    let mut nodes: Vec<&WorkflowNodeEvidence> = input.nodes.iter().collect();
    nodes.sort_by_key(|node| node_sort_key(node));

    let mut node_by_id: BTreeMap<String, &WorkflowNodeEvidence> = BTreeMap::new();
    for node in &nodes {
        node_by_id.entry(node.node_id.clone()).or_insert(*node);
    }

    let mut node_ids: BTreeSet<String> = BTreeSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let node = *node;
        let node_ref = format!("{NODE_EVIDENCE}{index}");
        check_schema(
            &mut findings,
            "workflow.node_schema_invalid",
            &node.schema,
            NODE_SCHEMA,
            &node_ref,
        )?;

        if !is_bounded_reference(&node.node_id) || !node_ids.insert(node.node_id.clone()) {
            emit(
                &mut findings,
                "workflow.node_identity_invalid",
                text("unique_bounded_node_id"),
                node_actual(node),
                &node_ref,
            )?;
        }

        // Parent edges: bounded, unique, never self-referencing, and always resolvable.
        let mut parent_refs: BTreeSet<String> = BTreeSet::new();
        for parent in &node.parent_ids {
            if !is_bounded_reference(parent) || !parent_refs.insert(parent.clone()) {
                emit(
                    &mut findings,
                    "workflow.parent_identity_invalid",
                    text("unique_bounded_parent_ids"),
                    node_actual(node),
                    &node_ref,
                )?;
            }
            if parent == &node.node_id {
                emit(
                    &mut findings,
                    "workflow.parent_self",
                    text("parent_is_not_the_node_itself"),
                    node_actual(node),
                    &node_ref,
                )?;
            } else if !node_by_id.contains_key(parent) {
                emit(
                    &mut findings,
                    "workflow.parent_missing",
                    text("parent_node_exists"),
                    node_actual(node),
                    &node_ref,
                )?;
            }
        }

        // Depth must stay within the declared bound and strictly above every existing parent.
        if node.depth > input.max_depth {
            emit(
                &mut findings,
                "workflow.depth_exceeded",
                text("depth_within_max_depth"),
                node_actual(node),
                &node_ref,
            )?;
        }
        for parent in &node.parent_ids {
            if parent == &node.node_id {
                continue;
            }
            if let Some(parent_node) = node_by_id.get(parent) {
                if node.depth <= parent_node.depth {
                    emit(
                        &mut findings,
                        "workflow.depth_inconsistent",
                        text("depth_above_every_parent_depth"),
                        node_actual(node),
                        &node_ref,
                    )?;
                }
            }
        }

        // Attempt history: non-empty, positive, strictly increasing and within the hard limit.
        if node.attempts.is_empty()
            || node.attempts.contains(&0)
            || node.attempts.windows(2).any(|pair| pair[0] >= pair[1])
        {
            emit(
                &mut findings,
                "workflow.attempt_invalid",
                text("positive_strictly_increasing_attempts"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.attempts.len() > MAX_ATTEMPTS_PER_NODE {
            emit(
                &mut findings,
                "workflow.attempt_unbounded",
                text("attempts_within_limit"),
                node_actual(node),
                &node_ref,
            )?;
        }

        // Fan-out: expected and observed children stay within the bound and agree with each other.
        if node.observed_children > input.max_fan_out {
            emit(
                &mut findings,
                "workflow.fanout_exceeded",
                text("observed_children_within_max_fan_out"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.expected_children > input.max_fan_out {
            emit(
                &mut findings,
                "workflow.fanout_exceeded",
                text("expected_children_within_max_fan_out"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.expected_children != node.observed_children {
            emit(
                &mut findings,
                "workflow.fanout_mismatch",
                text("expected_children_equal_observed_children"),
                node_actual(node),
                &node_ref,
            )?;
        }

        // Fan-in: the parent count stays within the hard limit and the declared/observed counts
        // agree with the recorded parent references.
        let declared_parents = node.parent_ids.len() as u32;
        if node.required_parents > MAX_FAN_IN || declared_parents > MAX_FAN_IN {
            emit(
                &mut findings,
                "workflow.fanin_unbounded",
                text("parents_within_fan_in_limit"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.required_parents != declared_parents {
            emit(
                &mut findings,
                "workflow.fanin_mismatch",
                text("required_parents_equal_declared_parents"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.observed_parents != node.required_parents {
            emit(
                &mut findings,
                "workflow.fanin_mismatch",
                text("observed_parents_equal_required_parents"),
                node_actual(node),
                &node_ref,
            )?;
        }

        // Scope: entries are bounded within a per-set limit, the child scope never widens beyond
        // the node's own parent scope, and the parent scope never exceeds what each parent granted.
        if node.parent_scope.len() > MAX_SCOPE_ENTRIES
            || node.child_scope.len() > MAX_SCOPE_ENTRIES
            || node
                .parent_scope
                .iter()
                .any(|entry| !is_bounded_reference(entry))
            || node
                .child_scope
                .iter()
                .any(|entry| !is_bounded_reference(entry))
        {
            emit(
                &mut findings,
                "workflow.scope_invalid",
                text("bounded_scope_entries_within_limit"),
                scope_actual(node),
                &node_ref,
            )?;
        }
        if !node.child_scope.is_subset(&node.parent_scope) {
            emit(
                &mut findings,
                "workflow.child_scope_expanded",
                text("child_scope_subset_of_parent_scope"),
                scope_actual(node),
                &node_ref,
            )?;
        }
        for parent in &node.parent_ids {
            if parent == &node.node_id {
                continue;
            }
            if let Some(parent_node) = node_by_id.get(parent) {
                if !node.parent_scope.is_subset(&parent_node.child_scope) {
                    emit(
                        &mut findings,
                        "workflow.parent_scope_expanded",
                        text("parent_scope_within_parent_child_scope"),
                        scope_actual(node),
                        &node_ref,
                    )?;
                }
            }
        }

        // Fan-in merges: verification is required above one parent, digests must be a strict
        // `sha256:` pair and equal, and merge evidence below fan-in is unexpected.
        let has_merge_evidence = node.merge_verified
            || node.merge_expected_digest.is_some()
            || node.merge_observed_digest.is_some();
        if node.parent_ids.len() > 1 {
            if !node.merge_verified {
                emit(
                    &mut findings,
                    "workflow.merge_missing",
                    text("verified_fan_in_merge"),
                    node_actual(node),
                    &node_ref,
                )?;
            }
            match (&node.merge_expected_digest, &node.merge_observed_digest) {
                (Some(expected), Some(observed))
                    if is_sha256_digest(expected) && is_sha256_digest(observed) =>
                {
                    if node.merge_verified && expected != observed {
                        emit(
                            &mut findings,
                            "workflow.merge_mismatch",
                            text("merge_expected_equals_observed"),
                            node_actual(node),
                            &node_ref,
                        )?;
                    }
                }
                _ => {
                    if has_merge_evidence {
                        emit(
                            &mut findings,
                            "workflow.merge_invalid",
                            text("sha256_merge_digests"),
                            node_actual(node),
                            &node_ref,
                        )?;
                    }
                }
            }
        } else if has_merge_evidence {
            emit(
                &mut findings,
                "workflow.merge_unexpected",
                text("no_merge_evidence_below_fan_in"),
                node_actual(node),
                &node_ref,
            )?;
        }

        // Compensation: a requirement must be recorded (outside the in-flight compensating
        // status), a record needs a bounded receipt reference, and the status must stay coherent.
        if node.compensation_required
            && !node.compensation_recorded
            && node.status != WorkflowNodeStatus::Compensating
        {
            emit(
                &mut findings,
                "workflow.compensation_missing",
                text("compensation_evidence"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.compensation_recorded && !node.compensation_required {
            emit(
                &mut findings,
                "workflow.compensation_unexpected",
                text("compensation_only_when_required"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.compensation_recorded {
            if !is_bounded_reference(&node.compensation_receipt_ref) {
                emit(
                    &mut findings,
                    "workflow.compensation_receipt_missing",
                    text("bounded_compensation_receipt_ref"),
                    node_actual(node),
                    &node_ref,
                )?;
            }
        } else if !node.compensation_receipt_ref.is_empty() {
            emit(
                &mut findings,
                "workflow.compensation_receipt_unexpected",
                text("no_receipt_without_recorded_compensation"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.status == WorkflowNodeStatus::Compensated
            && (!node.compensation_required || !node.compensation_recorded)
        {
            emit(
                &mut findings,
                "workflow.compensation_invalid",
                text("compensated_requires_required_and_recorded_compensation"),
                node_actual(node),
                &node_ref,
            )?;
        }
        if node.status == WorkflowNodeStatus::Compensating && !node.compensation_required {
            emit(
                &mut findings,
                "workflow.compensation_invalid",
                text("compensating_requires_a_compensation_requirement"),
                node_actual(node),
                &node_ref,
            )?;
        }
    }

    // DAG cycle check over the recorded parent edges; every cycle is visible exactly once per
    // node that first re-enters the in-progress mark.
    let mut graph: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in &nodes {
        graph
            .entry(node.node_id.clone())
            .or_insert_with(|| node.parent_ids.clone());
    }
    let mut marks: BTreeMap<String, u8> = BTreeMap::new();
    let mut cycle_reported: BTreeSet<String> = BTreeSet::new();
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
                emit(
                    findings,
                    "workflow.dag_cycle",
                    text("acyclic_graph"),
                    text("cycle"),
                    DAG_EVIDENCE,
                )?;
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

/// Stable total key: the finding set is independent of the caller's array order, while duplicate
/// evidence stays visible under its distinct `workflow:node:<index>` reference.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct NodeSortKey {
    node_id: String,
    depth: u32,
    parent_ids: Vec<String>,
    attempts: Vec<u32>,
    parent_scope: BTreeSet<String>,
    child_scope: BTreeSet<String>,
    expected_children: u32,
    observed_children: u32,
    required_parents: u32,
    observed_parents: u32,
    merge_verified: bool,
    merge_expected_digest: Option<String>,
    merge_observed_digest: Option<String>,
    compensation_required: bool,
    compensation_recorded: bool,
    compensation_receipt_ref: String,
    status: WorkflowNodeStatus,
}

fn node_sort_key(node: &WorkflowNodeEvidence) -> NodeSortKey {
    NodeSortKey {
        node_id: node.node_id.clone(),
        depth: node.depth,
        parent_ids: node.parent_ids.clone(),
        attempts: node.attempts.clone(),
        parent_scope: node.parent_scope.clone(),
        child_scope: node.child_scope.clone(),
        expected_children: node.expected_children,
        observed_children: node.observed_children,
        required_parents: node.required_parents,
        observed_parents: node.observed_parents,
        merge_verified: node.merge_verified,
        merge_expected_digest: node.merge_expected_digest.clone(),
        merge_observed_digest: node.merge_observed_digest.clone(),
        compensation_required: node.compensation_required,
        compensation_recorded: node.compensation_recorded,
        compensation_receipt_ref: node.compensation_receipt_ref.clone(),
        status: node.status,
    }
}

fn node_actual(node: &WorkflowNodeEvidence) -> Value {
    serde_json::json!({
        "node_id": node.node_id,
        "parent_ids": node.parent_ids,
        "depth": node.depth,
        "attempts": node.attempts,
        "parent_scope": node.parent_scope,
        "child_scope": node.child_scope,
        "expected_children": node.expected_children,
        "observed_children": node.observed_children,
        "required_parents": node.required_parents,
        "observed_parents": node.observed_parents,
        "merge_verified": node.merge_verified,
        "merge_expected_digest": node.merge_expected_digest,
        "merge_observed_digest": node.merge_observed_digest,
        "compensation_required": node.compensation_required,
        "compensation_recorded": node.compensation_recorded,
        "compensation_receipt_ref": node.compensation_receipt_ref,
        "status": node.status,
    })
}

fn scope_actual(node: &WorkflowNodeEvidence) -> Value {
    serde_json::json!({
        "parent_scope": node.parent_scope,
        "child_scope": node.child_scope,
    })
}

fn check_schema(
    findings: &mut Vec<Finding>,
    code: &'static str,
    actual: &str,
    expected: &'static str,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if actual != expected {
        emit(findings, code, text(expected), text(actual), evidence_ref)?;
    }
    Ok(())
}

/// Bounded, single-line, secret-free reference accepted in node/parent identities, scope entries
/// and compensation receipt references.
fn is_bounded_reference(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_TEXT_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'.' | b':' | b'/' | b'#' | b'-' | b'@')
        })
        && redact_text(value) == value
}

fn is_sha256_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// Push one finding.  Caller-supplied values go through [`Finding::redacted`] so a secret-shaped
/// reference cannot escape through the finding itself.
fn emit(
    findings: &mut Vec<Finding>,
    code: &'static str,
    expected: Value,
    actual: Value,
    evidence_ref: &str,
) -> Result<(), EvaluatorError> {
    if findings.len() >= MAX_FINDINGS {
        return Err(EvaluatorError::FindingLimitExceeded);
    }
    findings.push(
        Finding::redacted(
            code,
            expected,
            actual,
            format!("workflow and swarm finding: {code}"),
            evidence_ref,
        )
        .map_err(EvaluatorError::Finding)?,
    );
    Ok(())
}
