//! Pure planning/replay. Effects are returned to ControlPlane only after an event CAS.
use kiana_domain::*;
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, &'static str>;
const MAX_WORKFLOW_FAN_OUT: usize = 32;
const MAX_WORKFLOW_DEPTH: u32 = 8;
fn require(ok: bool, reason: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(reason)
    }
}
pub fn workflow_descends_from(state: &AutomationState, instance_id: &str, ancestor: &str) -> bool {
    let mut cursor = state.instances.get(instance_id);
    let mut seen = BTreeSet::new();
    while let Some(instance) = cursor {
        if !seen.insert(instance.instance_id.clone()) {
            return false;
        }
        let Some(parent) = &instance.parent_instance_id else {
            return false;
        };
        if instance.selected_nodes.is_some() {
            return false;
        }
        if parent == ancestor {
            return true;
        }
        cursor = state.instances.get(parent);
    }
    false
}
pub fn workflow_ancestors_active(state: &AutomationState, instance_id: &str) -> bool {
    !state.instances.values().any(|parent| {
        workflow_descends_from(state, instance_id, &parent.instance_id)
            && (parent.status.terminal()
                || matches!(
                    parent.status,
                    WorkflowInstanceStatus::CancelRequested | WorkflowInstanceStatus::Paused
                ))
    })
}
pub fn definition_key(id: &str, version: u64) -> String {
    format!("{id}@{version}")
}

pub fn validate_definition(d: &WorkflowDefinition) -> Result<()> {
    require(
        !d.definition_id.trim().is_empty()
            && d.definition_id.len() <= 128
            && !d.definition_id.contains(['\0', '\r', '\n'])
            && d.version > 0,
        "workflow_definition_identity_invalid",
    )?;
    require(
        !d.nodes.is_empty() && d.nodes.len() <= 128 && (1..=1024).contains(&d.max_steps),
        "workflow_definition_budget_invalid",
    )?;
    require(
        d.max_duration_ms > 0 && d.max_duration_ms <= 86_400_000,
        "workflow_definition_timeout_invalid",
    )?;
    require(
        !d.allowed_roles.is_empty()
            && d.allowed_roles
                .iter()
                .all(|r| RoleSpec::lookup(r).is_some()),
        "workflow_definition_role_invalid",
    )?;
    require(
        d.input_keys.iter().all(|k| !k.trim().is_empty())
            && d.output_keys.iter().all(|k| !k.trim().is_empty()),
        "workflow_schema_key_invalid",
    )?;
    require(
        d.input_keys.len() <= 256
            && d.output_keys.len() <= 256
            && d.input_keys.windows(2).all(|pair| pair[0] < pair[1])
            && d.output_keys.windows(2).all(|pair| pair[0] < pair[1]),
        "workflow_schema_key_noncanonical",
    )?;
    let mut graph = BTreeMap::new();
    for (id, node) in &d.nodes {
        require(
            !id.trim().is_empty() && id.len() <= 128 && !id.contains(['\0', '\r', '\n']),
            "workflow_node_id_invalid",
        )?;
        require(
            node.timeout_ms > 0 && node.timeout_ms <= d.max_duration_ms && node.retry_limit <= 3,
            "workflow_node_policy_invalid",
        )?;
        if let Some(compensation) = &node.compensation {
            require(
                d.nodes.contains_key(compensation)
                    && compensation != id
                    && d.nodes[compensation].dependencies.is_empty(),
                "workflow_compensation_invalid",
            )?;
        }
        match &node.kind {
            WorkflowNodeKind::AgentTask {
                project_id,
                packet_id,
                ..
            } => require(
                !project_id.trim().is_empty()
                    && project_id.len() <= 256
                    && !project_id.contains(['\0', '\r', '\n'])
                    && !packet_id.trim().is_empty()
                    && packet_id.len() <= 256
                    && !packet_id.contains(['\0', '\r', '\n']),
                "workflow_packet_required",
            )?,
            WorkflowNodeKind::Capability { request } => require(
                request.cell_id.is_none()
                    && request.budget_lease_id.is_none()
                    && request.capability_grant_id.is_none()
                    && !request.operation.is_empty(),
                "workflow_capability_authority_injected",
            )?,
            WorkflowNodeKind::Approval { role_id } => require(
                RoleSpec::lookup(role_id).is_some(),
                "workflow_approval_role_invalid",
            )?,
            WorkflowNodeKind::WaitSignal { signal } => {
                require(!signal.trim().is_empty(), "workflow_signal_required")?
            }
            WorkflowNodeKind::SubWorkflow {
                definition_id,
                version,
            } => require(
                definition_id != &d.definition_id
                    && !definition_id.trim().is_empty()
                    && definition_id.len() <= 128
                    && !definition_id.contains(['\0', '\r', '\n'])
                    && *version > 0,
                "workflow_subworkflow_recursive",
            )?,
            _ => {}
        }
        let mut packet = WorkPacket::builder_task(id, id);
        packet.dependencies = node.dependencies.clone();
        graph.insert(id.clone(), packet);
    }
    validate_dependency_dag(&graph).map_err(|_| "workflow_dependency_graph_invalid")?;
    for (id, node) in &d.nodes {
        match &node.kind {
            WorkflowNodeKind::FanOut => {
                let branches = d
                    .nodes
                    .iter()
                    .filter(|(_, candidate)| candidate.dependencies.contains(id))
                    .count();
                require(branches > 0, "workflow_fanout_without_branches")?;
                require(
                    branches <= MAX_WORKFLOW_FAN_OUT,
                    "workflow_fanout_limit_exceeded",
                )?;
            }
            WorkflowNodeKind::FanIn => {
                require(
                    !node.dependencies.is_empty(),
                    "workflow_fanin_dependencies_required",
                )?;
            }
            _ => {}
        }
    }
    let mut artifacts = BTreeMap::new();
    for (id, artifact) in &d.artifacts {
        require(
            !id.trim().is_empty()
                && id.len() <= 128
                && !id.contains(['\0', '\r', '\n'])
                && d.nodes.contains_key(&artifact.generated_by)
                && !artifact.output_key.is_empty()
                && artifact.output_key.len() <= 256
                && !artifact.output_key.contains(['\0', '\r', '\n']),
            "workflow_artifact_generator_invalid",
        )?;
        let mut packet = WorkPacket::builder_task(id, id);
        packet.dependencies = artifact.requires.clone();
        artifacts.insert(id.clone(), packet);
    }
    for artifact in d.artifacts.values() {
        for input in &artifact.requires {
            let source_artifact = d
                .artifacts
                .get(input)
                .ok_or("workflow_artifact_dependency_missing")?;
            let source = &source_artifact.generated_by;
            let mut ancestors = BTreeSet::new();
            let mut pending = d.nodes[&artifact.generated_by].dependencies.clone();
            while let Some(next) = pending.pop() {
                if ancestors.insert(next.clone()) {
                    pending.extend(d.nodes[&next].dependencies.clone());
                }
            }
            require(
                ancestors.contains(source),
                "workflow_artifact_missing_dependency_edge",
            )?;
        }
    }
    validate_dependency_dag(&artifacts).map_err(|_| "workflow_artifact_graph_invalid")?;
    Ok(())
}

/// Run the pure planner and expose a bounded intent alongside the next state/effect.  The intent
/// is useful to a queue adapter, but it never authorizes dispatch and contains no I/O or clock
/// lookup beyond the caller-provided `AutomationAuthority` snapshot.
pub fn plan_command_intent(
    state: &AutomationState,
    command: &AutomationCommand,
    authority: &AutomationAuthority,
    proof: &AutomationProof,
) -> Result<(AutomationState, WorkflowPlanIntent, Option<WorkflowEffect>)> {
    let (next, effect) = plan_command(state, command, authority, proof)?;
    let intent = derive_plan_intent(state, &next, command, effect.as_ref(), authority)?;
    Ok((next, intent, effect))
}

pub fn derive_plan_intent(
    state: &AutomationState,
    next: &AutomationState,
    command: &AutomationCommand,
    effect: Option<&WorkflowEffect>,
    authority: &AutomationAuthority,
) -> Result<WorkflowPlanIntent> {
    let (kind, instance_id, node_id) = match effect {
        Some(WorkflowEffect::Dispatch {
            instance_id,
            node_id,
            ..
        }) => (
            WorkflowIntentKind::Dispatch,
            Some(instance_id.clone()),
            Some(node_id.clone()),
        ),
        Some(WorkflowEffect::Cancel { instance_id }) => {
            (WorkflowIntentKind::Cancel, Some(instance_id.clone()), None)
        }
        None => {
            let instance_id = command.instance_id().map(str::to_owned);
            let node_id = instance_id.as_deref().and_then(|instance_id| {
                let before = state.instances.get(instance_id);
                let after = next.instances.get(instance_id)?;
                after
                    .nodes
                    .keys()
                    .find(|node_id| {
                        before.is_none_or(|before| !before.nodes.contains_key(*node_id))
                    })
                    .cloned()
            });
            let next_status = instance_id
                .as_deref()
                .and_then(|id| next.instances.get(id))
                .map(|instance| instance.status);
            let kind = match next_status {
                Some(status) if status.terminal() => WorkflowIntentKind::Terminal,
                Some(
                    WorkflowInstanceStatus::WaitingApproval | WorkflowInstanceStatus::WaitingSignal,
                ) => WorkflowIntentKind::Wait,
                Some(WorkflowInstanceStatus::Ready | WorkflowInstanceStatus::Running) => {
                    if node_id.is_some() {
                        WorkflowIntentKind::Reserve
                    } else {
                        WorkflowIntentKind::Noop
                    }
                }
                _ => WorkflowIntentKind::Noop,
            };
            (kind, instance_id, node_id)
        }
    };
    let definition_digest = instance_id.as_deref().and_then(|id| {
        next.instances.get(id).and_then(|instance| {
            next.definitions
                .get(&definition_key(
                    &instance.definition_id,
                    instance.definition_version,
                ))
                .map(WorkflowDefinition::digest)
        })
    });
    let input_digest = instance_id.as_deref().and_then(|instance_id| {
        node_id.as_deref().and_then(|node_id| {
            next.instances
                .get(instance_id)
                .and_then(|instance| instance.nodes.get(node_id))
                .map(|node| node.input_digest.clone())
        })
    });
    let queue_key = instance_id.as_ref().map(|instance_id| {
        format!(
            "workflow:{instance_id}:{}:{}",
            node_id.as_deref().unwrap_or("none"),
            next.revision
        )
    });
    let authority_digest = json_digest(&serde_json::json!({
        "actor_id": authority.context.actor_id,
        "session_id": authority.context.session_id,
        "role_id": authority.context.role_id,
        "project_root": authority.context.project_root,
        "now_ms": authority.now_ms,
    }));
    let mut intent = WorkflowPlanIntent {
        schema: WORKFLOW_PLAN_INTENT_SCHEMA.to_owned(),
        kind,
        instance_id,
        node_id,
        expected_revision: state.revision,
        next_revision: next.revision,
        definition_digest,
        input_digest,
        queue_key,
        authority_digest,
        intent_digest: String::new(),
    };
    intent.intent_digest = intent.digest();
    intent
        .validate()
        .map_err(|_| "workflow_plan_intent_invalid")?;
    Ok(intent)
}

/// Return all currently ready nodes in stable definition order. The Advance command consumes one
/// entry per committed transition; repeated calls therefore give independent execution IDs rather
/// than smuggling parallel work through one effect.
pub fn ready_node_ids(
    instance: &WorkflowInstance,
    definition: &WorkflowDefinition,
) -> Result<Vec<String>> {
    let mut ready = Vec::new();
    for (id, node) in &definition.nodes {
        if !selected(instance, definition, id) || instance.nodes.contains_key(id) {
            continue;
        }
        for dependency in &node.dependencies {
            require(
                definition.nodes.contains_key(dependency),
                "workflow_dependency_missing",
            )?;
        }
        if node.dependencies.iter().all(|dependency| {
            instance
                .nodes
                .get(dependency)
                .is_some_and(|execution| execution.status == WorkflowNodeStatus::Succeeded)
        }) {
            ready.push(id.clone());
        }
    }
    Ok(ready)
}

/// Fan-in consumes only committed successful outputs in lexicographic dependency order. A node
/// with a missing/stale output digest cannot be treated as an empty value.
pub fn stable_fan_in_outputs(
    instance: &WorkflowInstance,
    definition: &WorkflowDefinition,
    node_id: &str,
) -> Result<Vec<WorkflowValue>> {
    let node = definition
        .nodes
        .get(node_id)
        .ok_or("workflow_node_not_found")?;
    require(
        matches!(&node.kind, WorkflowNodeKind::FanIn),
        "workflow_fanin_node_required",
    )?;
    let mut dependencies = node.dependencies.clone();
    dependencies.sort();
    let mut values = Vec::with_capacity(dependencies.len());
    for dependency in dependencies {
        let execution = instance
            .nodes
            .get(&dependency)
            .ok_or("workflow_fanin_result_missing")?;
        require(
            execution.status == WorkflowNodeStatus::Succeeded
                && execution.output_recorded
                && execution.output_digest == json_digest(&execution.output),
            "workflow_fanin_result_missing",
        )?;
        values.push(execution.output.clone());
    }
    Ok(values)
}

fn reconcile_subworkflow_children(state: &mut AutomationState, parent_id: &str) -> Result<()> {
    let parent = state
        .instances
        .get(parent_id)
        .ok_or("workflow_instance_not_found")?;
    let definition_key = definition_key(&parent.definition_id, parent.definition_version);
    let definition = state
        .definitions
        .get(&definition_key)
        .cloned()
        .ok_or("workflow_definition_not_found")?;
    let children = parent
        .nodes
        .iter()
        .filter_map(|(node_id, node)| {
            let child_id = node.child_instance_id.as_ref()?;
            let WorkflowNodeKind::SubWorkflow {
                definition_id,
                version,
            } = &definition.nodes.get(node_id)?.kind
            else {
                return Some(Err("workflow_child_node_kind_mismatch"));
            };
            Some(Ok((
                node_id.clone(),
                child_id.clone(),
                definition_id.clone(),
                *version,
            )))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut changed = false;
    for (node_id, child_id, expected_definition_id, expected_version) in children {
        let Some(child) = state.instances.get(&child_id).cloned() else {
            return Err("workflow_child_instance_missing");
        };
        require(
            child.definition_id == expected_definition_id
                && child.definition_version == expected_version
                && child.parent_instance_id.as_deref() == Some(parent_id),
            "workflow_child_definition_drift",
        )?;
        let parent = state
            .instances
            .get_mut(parent_id)
            .ok_or("workflow_instance_not_found")?;
        let node = parent
            .nodes
            .get_mut(&node_id)
            .ok_or("workflow_node_not_started")?;
        if node.status.terminal() {
            continue;
        }
        match child.status {
            WorkflowInstanceStatus::Succeeded => {
                node.status = WorkflowNodeStatus::Succeeded;
                node.output = WorkflowValue::Object(child.outputs.into_iter().collect());
                node.output_digest = json_digest(&node.output);
                node.output_recorded = true;
                node.ended_at = Some(child.created_at.max(parent.created_at));
                changed = true;
            }
            WorkflowInstanceStatus::Failed | WorkflowInstanceStatus::Cancelled => {
                node.status = WorkflowNodeStatus::Failed;
                node.error_code = Some("workflow_child_failed".to_owned());
                node.ended_at = Some(child.created_at.max(parent.created_at));
                changed = true;
            }
            WorkflowInstanceStatus::ResultUnknown => {
                node.status = WorkflowNodeStatus::ResultUnknown;
                node.error_code = Some("workflow_child_result_unknown".to_owned());
                node.ended_at = Some(child.created_at.max(parent.created_at));
                changed = true;
            }
            _ => {}
        }
    }
    if changed {
        let parent = state
            .instances
            .get_mut(parent_id)
            .ok_or("workflow_instance_not_found")?;
        refresh(parent, &definition)?;
    }
    Ok(())
}

fn create_instance(
    state: &mut AutomationState,
    id: &str,
    definition_id: &str,
    version: u64,
    inputs: BTreeMap<String, WorkflowValue>,
    a: &AutomationAuthority,
    trigger_id: Option<String>,
    parent_instance_id: Option<String>,
) -> Result<()> {
    require(
        !id.trim().is_empty() && id.len() <= 256 && !state.instances.contains_key(id),
        "workflow_instance_exists_or_invalid",
    )?;
    require(
        state.instances.len() < 4096,
        "workflow_instance_limit_exceeded",
    )?;
    let d = state
        .definitions
        .get(&definition_key(definition_id, version))
        .ok_or("workflow_definition_not_found")?;
    let depth = if let Some(parent_id) = parent_instance_id.as_deref() {
        let parent = state
            .instances
            .get(parent_id)
            .ok_or("workflow_parent_instance_missing")?;
        require(
            parent.owner_id == a.context.actor_id.clone().unwrap_or_default(),
            "workflow_child_owner_mismatch",
        )?;
        let parent_definition = state
            .definitions
            .get(&definition_key(
                &parent.definition_id,
                parent.definition_version,
            ))
            .ok_or("workflow_definition_not_found")?;
        require(
            d.max_steps
                <= parent_definition
                    .max_steps
                    .saturating_sub(parent.steps_used),
            "workflow_child_budget_exceeded",
        )?;
        parent
            .depth
            .checked_add(1)
            .ok_or("workflow_child_depth_overflow")?
    } else {
        0
    };
    require(depth <= MAX_WORKFLOW_DEPTH, "workflow_child_depth_exceeded")?;
    require(
        d.allowed_roles.contains(&a.context.role_id),
        "workflow_role_denied",
    )?;
    require(
        d.input_keys.iter().all(|key| inputs.contains_key(key)),
        "workflow_input_schema_invalid",
    )?;
    state.instances.insert(
        id.into(),
        WorkflowInstance {
            instance_id: id.into(),
            definition_id: definition_id.into(),
            definition_version: version,
            owner_id: a
                .context
                .actor_id
                .clone()
                .ok_or("workflow_actor_required")?,
            role_id: a.context.role_id.clone(),
            created_at: a.now_ms,
            deadline: a.now_ms.saturating_add(d.max_duration_ms),
            inputs,
            outputs: BTreeMap::new(),
            status: WorkflowInstanceStatus::Ready,
            steps_used: 0,
            error_code: None,
            nodes: BTreeMap::new(),
            signals: BTreeMap::new(),
            trigger_id,
            parent_instance_id,
            depth,
            selected_nodes: None,
            retry_counts: BTreeMap::new(),
        },
    );
    Ok(())
}
fn selected(instance: &WorkflowInstance, definition: &WorkflowDefinition, id: &str) -> bool {
    if let Some(nodes) = &instance.selected_nodes {
        return nodes.iter().any(|node| node == id);
    }
    !definition
        .nodes
        .values()
        .any(|node| node.compensation.as_deref() == Some(id))
}
fn refresh(instance: &mut WorkflowInstance, definition: &WorkflowDefinition) -> Result<()> {
    let paused = instance.status == WorkflowInstanceStatus::Paused;
    if instance
        .nodes
        .values()
        .any(|n| n.status == WorkflowNodeStatus::ResultUnknown)
    {
        instance.status = WorkflowInstanceStatus::ResultUnknown;
    } else if instance.status == WorkflowInstanceStatus::CancelRequested {
        if instance.nodes.values().all(|n| n.status.terminal()) {
            instance.status = WorkflowInstanceStatus::Cancelled;
        }
    } else if instance.nodes.values().any(|n| {
        matches!(
            n.status,
            WorkflowNodeStatus::Failed | WorkflowNodeStatus::Cancelled
        )
    }) {
        instance.status = WorkflowInstanceStatus::Failed;
    } else if definition
        .nodes
        .keys()
        .filter(|id| selected(instance, definition, id))
        .all(|id| {
            instance
                .nodes
                .get(id)
                .is_some_and(|n| n.status == WorkflowNodeStatus::Succeeded)
        })
    {
        if instance.selected_nodes.is_none() {
            if !definition
                .output_keys
                .iter()
                .all(|k| instance.outputs.contains_key(k))
            {
                instance.status = WorkflowInstanceStatus::Failed;
                instance.error_code = Some("workflow_output_schema_invalid".into());
                return Ok(());
            }
            if !definition
                .artifacts
                .values()
                .all(|artifact| instance.outputs.contains_key(&artifact.output_key))
            {
                instance.status = WorkflowInstanceStatus::Failed;
                instance.error_code = Some("workflow_artifact_output_missing".into());
                return Ok(());
            }
        }
        instance.status = WorkflowInstanceStatus::Succeeded;
    } else if instance
        .nodes
        .values()
        .any(|n| n.status == WorkflowNodeStatus::Reserved)
    {
        instance.status = WorkflowInstanceStatus::Running;
    } else if instance
        .nodes
        .values()
        .any(|n| n.status == WorkflowNodeStatus::WaitingApproval)
    {
        instance.status = WorkflowInstanceStatus::WaitingApproval;
    } else if instance
        .nodes
        .values()
        .any(|n| n.status == WorkflowNodeStatus::WaitingSignal)
    {
        instance.status = WorkflowInstanceStatus::WaitingSignal;
    } else {
        instance.status = WorkflowInstanceStatus::Ready;
    }
    if paused && !instance.status.terminal() {
        instance.status = WorkflowInstanceStatus::Paused;
    }
    Ok(())
}
fn observe(
    instance: &mut WorkflowInstance,
    definition: &WorkflowDefinition,
    node_id: &str,
    a: &AutomationAuthority,
    p: &AutomationProof,
) -> Result<()> {
    let response = p
        .response
        .as_ref()
        .ok_or("workflow_runtime_evidence_required")?;
    let node = instance
        .nodes
        .get_mut(node_id)
        .ok_or("workflow_node_not_started")?;
    require(
        !node.status.terminal() && response.request_id == node.execution_id,
        "workflow_observation_identity_conflict",
    )?;
    require(
        !p.evidence_refs.is_empty() || response.status == ExecutionStatus::ResultUnknown,
        "workflow_evidence_required",
    )?;
    node.status = match response.status {
        ExecutionStatus::Completed => WorkflowNodeStatus::Succeeded,
        ExecutionStatus::Failed | ExecutionStatus::Blocked | ExecutionStatus::Denied => {
            WorkflowNodeStatus::Failed
        }
        ExecutionStatus::Cancelled => WorkflowNodeStatus::Cancelled,
        ExecutionStatus::ResultUnknown => WorkflowNodeStatus::ResultUnknown,
        ExecutionStatus::AwaitingApproval => WorkflowNodeStatus::WaitingApproval,
        _ => WorkflowNodeStatus::Reserved,
    };
    node.output = response.output.clone();
    node.output_digest = json_digest(&node.output);
    node.output_recorded = true;
    node.error_code = response.error.clone();
    node.evidence_refs = p.evidence_refs.clone();
    if node.status.terminal() {
        node.ended_at = Some(a.now_ms);
    }
    if node.status == WorkflowNodeStatus::Succeeded {
        instance.outputs.insert(node_id.into(), node.output.clone());
    }
    refresh(instance, definition)
}
pub fn plan_command(
    state: &AutomationState,
    command: &AutomationCommand,
    a: &AutomationAuthority,
    p: &AutomationProof,
) -> Result<(AutomationState, Option<WorkflowEffect>)> {
    let mut next = state.clone();
    let mut effect = None;
    require(
        a.context.actor_id.as_ref().is_some_and(|id| !id.is_empty()),
        "workflow_actor_required",
    )?;
    if let Some(id) = command.instance_id() {
        let instance = next
            .instances
            .get(id)
            .ok_or("workflow_instance_not_found")?;
        require(
            Some(instance.owner_id.as_str()) == a.context.actor_id.as_deref(),
            "workflow_owner_mismatch",
        )?;
        if !matches!(
            command,
            AutomationCommand::Reconcile { .. }
                | AutomationCommand::RecordObservation { .. }
                | AutomationCommand::Cancel { .. }
        ) {
            require(
                workflow_ancestors_active(state, id),
                "workflow_parent_inactive",
            )?;
        }
        if !matches!(
            command,
            AutomationCommand::Decide { .. }
                | AutomationCommand::RecordObservation { .. }
                | AutomationCommand::Reconcile { .. }
        ) {
            require(
                instance.role_id == a.context.role_id,
                "workflow_role_denied",
            )?;
        }
    }
    match command {
        AutomationCommand::RegisterDefinition { definition } => {
            require(
                matches!(a.context.role_id.as_str(), "pm" | "sponsor" | "architect"),
                "workflow_definition_role_denied",
            )?;
            validate_definition(definition)?;
            let key = definition_key(&definition.definition_id, definition.version);
            require(
                !next.definitions.contains_key(&key),
                "workflow_definition_version_immutable",
            )?;
            // Require subdefinitions to be frozen already; this also prevents indirect recursion.
            for node in definition.nodes.values() {
                if let WorkflowNodeKind::SubWorkflow {
                    definition_id,
                    version,
                } = &node.kind
                {
                    require(
                        next.definitions
                            .contains_key(&definition_key(definition_id, *version)),
                        "workflow_subdefinition_not_found",
                    )?;
                }
            }
            next.definitions.insert(key, definition.clone());
        }
        AutomationCommand::Start {
            instance_id,
            definition_id,
            version,
            inputs,
        } => create_instance(
            &mut next,
            instance_id,
            definition_id,
            *version,
            inputs.clone(),
            a,
            None,
            None,
        )?,
        AutomationCommand::Advance { instance_id } => {
            reconcile_subworkflow_children(&mut next, instance_id)?;
            let instance = next.instances.get_mut(instance_id).unwrap();
            require(
                !instance.status.terminal()
                    && !matches!(
                        instance.status,
                        WorkflowInstanceStatus::Paused | WorkflowInstanceStatus::CancelRequested
                    ),
                "workflow_not_advanceable",
            )?;
            require(a.now_ms < instance.deadline, "workflow_deadline_expired")?;
            let definition = &next.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            require(
                instance.steps_used < definition.max_steps,
                "workflow_step_budget_exhausted",
            )?;
            require(
                !instance
                    .nodes
                    .values()
                    .any(|n| n.status == WorkflowNodeStatus::Reserved),
                "workflow_execution_in_flight",
            )?;
            let ready = ready_node_ids(instance, definition)?;
            let id = ready.first().ok_or("workflow_no_ready_node")?;
            let node = definition.nodes.get(id).ok_or("workflow_node_not_found")?;
            let (id, node) = (id.clone(), node.clone());
            let mut inputs = instance.inputs.clone();
            inputs.extend(instance.outputs.clone());
            let mut execution = WorkflowNodeExecution {
                execution_id: a.execution_id,
                session_id: a.session_id.clone(),
                attempt: instance.retry_counts.get(&id).copied().unwrap_or(0) + 1,
                status: WorkflowNodeStatus::Succeeded,
                started_at: a.now_ms,
                lease_expires_at: a
                    .now_ms
                    .saturating_add(node.timeout_ms)
                    .min(instance.deadline),
                ended_at: Some(a.now_ms),
                input_digest: workflow_input_digest(&inputs),
                output: WorkflowValue::Null,
                output_digest: json_digest(&WorkflowValue::Null),
                output_recorded: false,
                error_code: None,
                evidence_refs: Vec::new(),
                child_instance_id: None,
            };
            match &node.kind {
                WorkflowNodeKind::Literal { values } => {
                    instance.outputs.extend(values.clone());
                    execution.output = WorkflowValue::Object(values.clone().into_iter().collect());
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::CopyInput { mapping } => {
                    let mut copied = BTreeMap::new();
                    for (output, input) in mapping {
                        let value = inputs.get(input).ok_or("workflow_input_missing")?;
                        instance.outputs.insert(output.clone(), value.clone());
                        copied.insert(output.clone(), value.clone());
                    }
                    execution.output = WorkflowValue::Object(copied.into_iter().collect());
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::Gate { key, equals } => {
                    if inputs.get(key) != Some(equals) {
                        execution.status = WorkflowNodeStatus::Failed;
                        execution.error_code = Some("workflow_gate_denied".into());
                    }
                    execution.output = serde_json::json!({"key": key, "matched": execution.status == WorkflowNodeStatus::Succeeded});
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::FanOut => {
                    let mut branches = definition
                        .nodes
                        .iter()
                        .filter(|(_, candidate)| candidate.dependencies.contains(&id))
                        .map(|(branch_id, _)| branch_id.clone())
                        .collect::<Vec<_>>();
                    branches.sort();
                    require(!branches.is_empty(), "workflow_fanout_without_branches")?;
                    require(
                        branches.len() <= MAX_WORKFLOW_FAN_OUT,
                        "workflow_fanout_limit_exceeded",
                    )?;
                    execution.output = WorkflowValue::Array(
                        branches.into_iter().map(WorkflowValue::String).collect(),
                    );
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::FanIn => {
                    let values = stable_fan_in_outputs(instance, definition, &id)?;
                    execution.output = WorkflowValue::Array(values);
                    execution.output_recorded = true;
                }
                WorkflowNodeKind::Approval { .. } => {
                    execution.status = WorkflowNodeStatus::WaitingApproval;
                    execution.ended_at = None;
                }
                WorkflowNodeKind::WaitSignal { signal } => {
                    if let Some(value) = instance.signals.get(signal) {
                        execution.output = value.clone();
                        instance.outputs.insert(id.clone(), value.clone());
                        execution.output_recorded = true;
                    } else {
                        execution.status = WorkflowNodeStatus::WaitingSignal;
                        execution.ended_at = None;
                    }
                }
                WorkflowNodeKind::AgentTask { .. } | WorkflowNodeKind::Capability { .. } => {
                    execution.status = WorkflowNodeStatus::Reserved;
                    execution.ended_at = None;
                    effect = Some(WorkflowEffect::Dispatch {
                        instance_id: instance_id.clone(),
                        node_id: id.clone(),
                        kind: node.kind.clone(),
                        execution_id: a.execution_id,
                        session_id: a.session_id.clone(),
                    });
                }
                WorkflowNodeKind::SubWorkflow { .. } => {
                    execution.status = WorkflowNodeStatus::Reserved;
                    execution.ended_at = None;
                    execution.child_instance_id =
                        Some(format!("{}:{}:{}", instance_id, id, a.execution_id));
                }
            }
            if execution.output_recorded {
                execution.output_digest = json_digest(&execution.output);
            }
            instance.steps_used += 1;
            instance.nodes.insert(id.clone(), execution.clone());
            refresh(instance, definition)?;
            if let WorkflowNodeKind::SubWorkflow {
                definition_id,
                version,
            } = &node.kind
            {
                let child_id = execution.child_instance_id.unwrap();
                create_instance(
                    &mut next,
                    &child_id,
                    definition_id,
                    *version,
                    inputs,
                    a,
                    None,
                    Some(instance_id.clone()),
                )?;
            }
        }
        AutomationCommand::Reconcile {
            instance_id,
            node_id,
        }
        | AutomationCommand::RecordObservation {
            instance_id,
            node_id,
        } => {
            let instance = next.instances.get_mut(instance_id).unwrap();
            let definition = &next.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            observe(instance, definition, node_id, a, p)?;
        }
        AutomationCommand::Decide {
            instance_id,
            node_id,
            approve,
            evidence_ref,
        } => {
            require(
                p.evidence_refs.contains(evidence_ref),
                "workflow_approval_evidence_required",
            )?;
            let instance = next.instances.get_mut(instance_id).unwrap();
            require(
                !instance.status.terminal() && a.now_ms < instance.deadline,
                "workflow_approval_expired",
            )?;
            let definition = &next.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            let WorkflowNodeKind::Approval { role_id } = &definition
                .nodes
                .get(node_id)
                .ok_or("workflow_node_not_found")?
                .kind
            else {
                return Err("workflow_approval_node_required");
            };
            require(
                role_id == &a.context.role_id,
                "workflow_approval_role_denied",
            )?;
            let node = instance
                .nodes
                .get_mut(node_id)
                .ok_or("workflow_node_not_started")?;
            require(
                node.status == WorkflowNodeStatus::WaitingApproval
                    && a.now_ms < node.lease_expires_at
                    && node.session_id != a.context.session_id,
                "workflow_approval_identity_or_expiry_invalid",
            )?;
            node.status = if *approve {
                WorkflowNodeStatus::Succeeded
            } else {
                WorkflowNodeStatus::Failed
            };
            node.ended_at = Some(a.now_ms);
            node.evidence_refs.push(evidence_ref.clone());
            refresh(instance, definition)?;
        }
        AutomationCommand::Signal {
            instance_id,
            signal,
            value,
            evidence_ref,
        } => {
            require(
                p.evidence_refs.contains(evidence_ref),
                "workflow_signal_evidence_required",
            )?;
            let instance = next.instances.get_mut(instance_id).unwrap();
            require(
                !instance.status.terminal() && a.now_ms < instance.deadline,
                "workflow_signal_expired",
            )?;
            require(
                !instance.signals.contains_key(signal),
                "workflow_signal_already_consumed",
            )?;
            let definition = &next.definitions
                [&definition_key(&instance.definition_id, instance.definition_version)];
            require(definition.nodes.values().any(|n| matches!(&n.kind,WorkflowNodeKind::WaitSignal { signal:key } if key==signal)),"workflow_signal_unknown")?;
            instance.signals.insert(signal.clone(), value.clone());
            for (id, node) in &mut instance.nodes {
                if node.status == WorkflowNodeStatus::WaitingSignal
                    && matches!(&definition.nodes[id].kind,WorkflowNodeKind::WaitSignal { signal:key } if key==signal)
                {
                    require(a.now_ms < node.lease_expires_at, "workflow_signal_expired")?;
                    node.status = WorkflowNodeStatus::Succeeded;
                    node.output = value.clone();
                    node.output_digest = json_digest(&node.output);
                    node.output_recorded = true;
                    node.ended_at = Some(a.now_ms);
                    node.evidence_refs.push(evidence_ref.clone());
                    instance.outputs.insert(id.clone(), value.clone());
                }
            }
            refresh(instance, definition)?;
        }
        AutomationCommand::Pause { instance_id } => {
            let i = next.instances.get_mut(instance_id).unwrap();
            require(!i.status.terminal(), "workflow_terminal")?;
            i.status = WorkflowInstanceStatus::Paused;
        }
        AutomationCommand::Resume { instance_id } => {
            let i = next.instances.get_mut(instance_id).unwrap();
            require(
                i.status == WorkflowInstanceStatus::Paused && a.now_ms < i.deadline,
                "workflow_not_resumable",
            )?;
            refresh(
                i,
                &next.definitions[&definition_key(&i.definition_id, i.definition_version)],
            )?;
        }
        AutomationCommand::Cancel {
            instance_id,
            reason,
        } => {
            require(!reason.trim().is_empty(), "workflow_cancel_reason_required")?;
            let i = next.instances.get_mut(instance_id).unwrap();
            require(!i.status.terminal(), "workflow_terminal")?;
            i.status = WorkflowInstanceStatus::CancelRequested;
            for (id, node) in &mut i.nodes {
                if matches!(
                    next.definitions[&definition_key(&i.definition_id, i.definition_version)].nodes
                        [id]
                        .kind,
                    WorkflowNodeKind::Approval { .. } | WorkflowNodeKind::WaitSignal { .. }
                ) && !node.status.terminal()
                {
                    node.status = WorkflowNodeStatus::Cancelled;
                    node.ended_at = Some(a.now_ms);
                }
            }
            refresh(
                i,
                &next.definitions[&definition_key(&i.definition_id, i.definition_version)],
            )?;
            effect = Some(WorkflowEffect::Cancel {
                instance_id: instance_id.clone(),
            });
        }
        AutomationCommand::Retry {
            instance_id,
            node_id,
        } => {
            let i = next.instances.get_mut(instance_id).unwrap();
            let d = &next.definitions[&definition_key(&i.definition_id, i.definition_version)];
            let n = i.nodes.get(node_id).ok_or("workflow_node_not_started")?;
            require(
                n.status == WorkflowNodeStatus::Failed
                    && n.attempt <= d.nodes[node_id].retry_limit
                    && a.now_ms < i.deadline,
                "workflow_retry_denied",
            )?;
            // Retry only pure nodes; external side effects need a fresh reviewed work packet.
            require(
                matches!(
                    d.nodes[node_id].kind,
                    WorkflowNodeKind::Literal { .. }
                        | WorkflowNodeKind::CopyInput { .. }
                        | WorkflowNodeKind::Gate { .. }
                ),
                "workflow_effect_retry_requires_new_contract",
            )?;
            let retries = i.retry_counts.get(node_id).copied().unwrap_or(0);
            require(
                retries < d.nodes[node_id].retry_limit,
                "workflow_retry_budget_exhausted",
            )?;
            i.retry_counts.insert(node_id.clone(), retries + 1);
            i.nodes.remove(node_id);
            i.status = WorkflowInstanceStatus::Retrying;
        }
        AutomationCommand::Compensate {
            instance_id,
            compensation_instance_id,
        } => {
            let source = next.instances.get(instance_id).unwrap().clone();
            require(
                matches!(
                    source.status,
                    WorkflowInstanceStatus::Failed | WorkflowInstanceStatus::Cancelled
                ),
                "workflow_compensation_requires_known_terminal",
            )?;
            require(
                !next.instances.values().any(|i| {
                    i.parent_instance_id.as_deref() == Some(instance_id)
                        && i.selected_nodes.is_some()
                }),
                "workflow_compensation_already_requested",
            )?;
            let d = &next.definitions
                [&definition_key(&source.definition_id, source.definition_version)];
            let mut nodes = source
                .nodes
                .iter()
                .filter(|(_, n)| n.status == WorkflowNodeStatus::Succeeded)
                .filter_map(|(id, _)| d.nodes[id].compensation.clone())
                .collect::<Vec<_>>();
            nodes.sort();
            nodes.dedup();
            require(!nodes.is_empty(), "workflow_compensation_not_defined")?;
            let mut inputs = source.inputs;
            inputs.extend(source.outputs);
            create_instance(
                &mut next,
                compensation_instance_id,
                &source.definition_id,
                source.definition_version,
                inputs,
                a,
                None,
                Some(instance_id.clone()),
            )?;
            let compensation = next.instances.get_mut(compensation_instance_id).unwrap();
            compensation.selected_nodes = Some(nodes);
            compensation.status = WorkflowInstanceStatus::Compensating;
        }
        AutomationCommand::RegisterTrigger { trigger } => {
            trigger
                .validate_shape(a.now_ms)
                .map_err(|_| "trigger_definition_shape_invalid")?;
            require(
                matches!(a.context.role_id.as_str(), "sponsor" | "pm")
                    && trigger.owner_id == a.context.actor_id.clone().unwrap_or_default(),
                "trigger_owner_or_role_denied",
            )?;
            require(
                p.evidence_refs.contains(&trigger.approval_ref),
                "trigger_approval_required",
            )?;
            require(
                !next.triggers.contains_key(&trigger.trigger_id)
                    && !trigger.trigger_id.is_empty()
                    && trigger.trigger_id.len() <= 128,
                "trigger_identity_invalid",
            )?;
            require(
                trigger.expires_at > a.now_ms
                    && trigger.expires_at - a.now_ms <= 31_536_000_000
                    && (1..=1024).contains(&trigger.max_firings),
                "trigger_budget_or_expiry_invalid",
            )?;
            let d = next
                .definitions
                .get(&definition_key(
                    &trigger.definition_id,
                    trigger.definition_version,
                ))
                .ok_or("workflow_definition_not_found")?;
            require(
                d.allowed_roles.contains(&trigger.role_id)
                    && d.input_keys.iter().all(|k| trigger.inputs.contains_key(k)),
                "trigger_template_invalid",
            )?;
            let next_at = match trigger.schedule {
                TriggerSchedule::Interval { every_ms, first_at } => {
                    require(every_ms >= 1000 && first_at > 0, "trigger_schedule_invalid")?;
                    Some(first_at)
                }
                _ => None,
            };
            next.triggers.insert(
                trigger.trigger_id.clone(),
                DurableTrigger {
                    definition: trigger.clone(),
                    enabled: true,
                    next_at,
                    firings_used: 0,
                    pending_keys: Vec::new(),
                    pending_digests: BTreeMap::new(),
                    fired: BTreeMap::new(),
                    fired_digests: BTreeMap::new(),
                },
            );
        }
        AutomationCommand::DisableTrigger { trigger_id } => {
            let t = next
                .triggers
                .get_mut(trigger_id)
                .ok_or("trigger_not_found")?;
            require(
                matches!(a.context.role_id.as_str(), "sponsor" | "pm")
                    && Some(t.definition.owner_id.as_str()) == a.context.actor_id.as_deref(),
                "trigger_owner_or_role_denied",
            )?;
            t.enabled = false;
            t.pending_keys.clear();
            t.pending_digests.clear();
        }
        AutomationCommand::Tick { trigger_id } | AutomationCommand::Fire { trigger_id, .. } => {
            fire_trigger(&mut next, trigger_id, command, a, p)?
        }
    }
    if let AutomationCommand::Cancel { instance_id, .. } = command {
        let descendants = next
            .instances
            .keys()
            .filter(|id| workflow_descends_from(&next, id, instance_id))
            .cloned()
            .collect::<Vec<_>>();
        for id in descendants {
            let i = next.instances.get_mut(&id).unwrap();
            if i.status.terminal() {
                continue;
            }
            i.status = WorkflowInstanceStatus::CancelRequested;
            let d = &next.definitions[&definition_key(&i.definition_id, i.definition_version)];
            for (node_id, node) in &mut i.nodes {
                if !node.status.terminal()
                    && matches!(
                        d.nodes[node_id].kind,
                        WorkflowNodeKind::Approval { .. } | WorkflowNodeKind::WaitSignal { .. }
                    )
                {
                    node.status = WorkflowNodeStatus::Cancelled;
                    node.ended_at = Some(a.now_ms);
                }
            }
            refresh(i, d)?;
        }
    }
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or("workflow_revision_exhausted")?;
    Ok((next, effect))
}

fn fire_trigger(
    state: &mut AutomationState,
    id: &str,
    command: &AutomationCommand,
    a: &AutomationAuthority,
    p: &AutomationProof,
) -> Result<()> {
    let t = state.triggers.get(id).ok_or("trigger_not_found")?.clone();
    require(
        t.enabled
            && a.now_ms < t.definition.expires_at
            && Some(t.definition.owner_id.as_str()) == a.context.actor_id.as_deref(),
        "trigger_disabled_expired_or_owner_mismatch",
    )?;
    require(
        t.definition.role_id == a.context.role_id,
        "trigger_execution_role_mismatch",
    )?;
    let mut occurrences = Vec::new();
    let mut next_at = t.next_at;
    match command {
        AutomationCommand::Fire {
            firing_key,
            event_ref,
            ..
        } => {
            require(
                !firing_key.is_empty() && firing_key.len() <= 256,
                "trigger_firing_key_invalid",
            )?;
            require(
                !firing_key.contains(['\0', '\r', '\n']),
                "trigger_firing_key_invalid",
            )?;
            match &t.definition.schedule {
                TriggerSchedule::Manual => {
                    require(event_ref.is_none(), "trigger_manual_event_invalid")?
                }
                TriggerSchedule::Event { kind } => require(
                    event_ref
                        .as_ref()
                        .is_some_and(|r| p.evidence_refs.contains(r))
                        && p.event_kind.as_ref() == Some(kind),
                    "trigger_event_evidence_invalid",
                )?,
                _ => return Err("trigger_interval_requires_tick"),
            }
            let digest = trigger_occurrence_digest(
                id,
                firing_key,
                event_ref.as_deref(),
                p.event_kind.as_deref(),
            );
            push_occurrence(&t, &mut occurrences, firing_key.clone(), digest)?;
        }
        AutomationCommand::Tick { .. } => {
            for key in &t.pending_keys {
                let digest = t
                    .pending_digests
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| trigger_occurrence_digest(id, key, None, None));
                push_occurrence(&t, &mut occurrences, key.clone(), digest)?;
            }
            if let TriggerSchedule::Interval { every_ms, .. } = t.definition.schedule {
                let batch = plan_interval_due(
                    t.next_at.ok_or("trigger_schedule_invalid")?,
                    every_ms,
                    a.now_ms,
                    t.definition.missed_schedule,
                )
                .map_err(|_| "trigger_interval_cursor_invalid")?;
                for key in batch.occurrence_keys {
                    let digest = trigger_occurrence_digest(id, &key, None, Some("interval"));
                    push_occurrence(&t, &mut occurrences, key, digest)?;
                }
                next_at = Some(batch.next_at);
            }
        }
        _ => return Err("trigger_command_invalid"),
    }
    let active: Vec<_> = state
        .instances
        .values()
        .filter(|i| i.trigger_id.as_deref() == Some(id) && !i.status.terminal())
        .map(|i| i.instance_id.clone())
        .collect();
    let mut pending = Vec::new();
    let mut started = !active.is_empty();
    for (key, digest) in occurrences {
        if started {
            match t.definition.concurrency {
                TriggerConcurrency::Reject => return Err("trigger_already_running"),
                TriggerConcurrency::Queue => push_pending(&mut pending, key, digest)?,
                TriggerConcurrency::Coalesce => {
                    pending.clear();
                    push_pending(&mut pending, key, digest)?;
                }
                TriggerConcurrency::Replace => {
                    // Replacement never assumes an old side effect stopped. Queue it and require
                    // the existing cancellation/reconciliation path before starting a successor.
                    for active_id in &active {
                        state.instances.get_mut(active_id).unwrap().status =
                            WorkflowInstanceStatus::CancelRequested;
                    }
                    pending.clear();
                    push_pending(&mut pending, key, digest)?;
                }
            }
            continue;
        }
        let count = state.triggers[id].firings_used;
        require(count < t.definition.max_firings, "trigger_budget_exhausted")?;
        let instance_id = format!("trigger:{id}:{}", count + 1);
        create_instance(
            state,
            &instance_id,
            &t.definition.definition_id,
            t.definition.definition_version,
            t.definition.inputs.clone(),
            a,
            Some(id.into()),
            None,
        )?;
        let current = state.triggers.get_mut(id).unwrap();
        current.fired.insert(key.clone(), instance_id);
        current.fired_digests.insert(key, digest);
        current.firings_used += 1;
        started = true;
    }
    let current = state.triggers.get_mut(id).unwrap();
    current.next_at = next_at;
    current.pending_keys = pending.iter().map(|(key, _)| key.clone()).collect();
    current.pending_digests = pending.into_iter().collect();
    Ok(())
}

fn trigger_occurrence_digest(
    trigger_id: &str,
    firing_key: &str,
    event_ref: Option<&str>,
    event_kind: Option<&str>,
) -> String {
    json_digest(&serde_json::json!({
        "trigger_id": trigger_id,
        "firing_key": firing_key,
        "event_ref": event_ref,
        "event_kind": event_kind,
    }))
}

fn push_occurrence(
    trigger: &DurableTrigger,
    occurrences: &mut Vec<(String, String)>,
    key: String,
    digest: String,
) -> Result<()> {
    if let Some(previous) = trigger.fired_digests.get(&key) {
        if previous != &digest {
            return Err("trigger_occurrence_digest_conflict");
        }
        return Ok(());
    }
    if trigger.fired.contains_key(&key) {
        return Ok(());
    }
    if let Some(previous) = trigger.pending_digests.get(&key) {
        if previous != &digest {
            return Err("trigger_occurrence_digest_conflict");
        }
    }
    if let Some((_, previous)) = occurrences.iter().find(|(candidate, _)| candidate == &key) {
        if previous != &digest {
            return Err("trigger_occurrence_digest_conflict");
        }
        return Ok(());
    }
    occurrences.push((key, digest));
    Ok(())
}

fn push_pending(pending: &mut Vec<(String, String)>, key: String, digest: String) -> Result<()> {
    if let Some((_, previous)) = pending.iter().find(|(candidate, _)| candidate == &key) {
        if previous != &digest {
            return Err("trigger_occurrence_digest_conflict");
        }
        return Ok(());
    }
    if pending.len() >= 128 {
        return Err("trigger_queue_full");
    }
    pending.push((key, digest));
    Ok(())
}
