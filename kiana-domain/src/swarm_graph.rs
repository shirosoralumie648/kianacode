//! Explicit Swarm Partition/WorkGraph validation and deterministic readiness projection.
//!
//! This module is planning-only. It validates typed facts and returns a read-only projection;
//! claims, dispatch, leases and external effects remain ControlPlane/EventLog responsibilities.
use crate::{
    json_digest, normalize_role_path, path_locks_conflict, validate_dependency_graph, AttemptId,
    ChildCellId, PartitionId, SwarmPlanId, WorkFingerprint,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const SWARM_PARTITION_SCHEMA: &str = "kiana.swarm-partition.v1";
pub const SWARM_WORK_GRAPH_SCHEMA: &str = "kiana.swarm-work-graph.v1";
pub const MAX_SWARM_PARTITIONS: usize = 64;
pub const MAX_SWARM_DEPTH: u32 = 16;
pub const MAX_SWARM_CONCURRENCY: u32 = 64;
pub const MAX_SWARM_SPAWN_RATE: u32 = 1_024;
pub const MAX_SWARM_TTL_MS: u64 = 86_400_000;
pub const MAX_SWARM_TOKENS: u64 = 1_000_000_000;
pub const MAX_SWARM_MODEL_CALLS: u64 = 1_000_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionStatus {
    Pending,
    Ready,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    ResultUnknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partition {
    pub schema: String,
    pub partition_id: PartitionId,
    pub swarm_plan_id: SwarmPlanId,
    pub ordinal: u32,
    pub input_refs: Vec<String>,
    pub input_digest: String,
    pub data_scope: Vec<String>,
    pub owned_paths: Vec<String>,
    pub output_contract: String,
    pub output_contract_version: u32,
    pub dependency_partition_ids: Vec<PartitionId>,
    pub work_fingerprint: WorkFingerprint,
    pub token_budget: u64,
    pub model_call_budget: u64,
    pub status: PartitionStatus,
    #[serde(default)]
    pub child_cell_id: Option<ChildCellId>,
    #[serde(default)]
    pub active_attempt_id: Option<AttemptId>,
}

impl Partition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        swarm_plan_id: SwarmPlanId,
        ordinal: u32,
        mut input_refs: Vec<String>,
        mut data_scope: Vec<String>,
        mut owned_paths: Vec<String>,
        output_contract: impl Into<String>,
        output_contract_version: u32,
        dependency_partition_ids: Vec<PartitionId>,
        token_budget: u64,
        model_call_budget: u64,
    ) -> Result<Self, String> {
        if input_refs.is_empty() {
            return Err("swarm_partition_input_unbound".to_owned());
        }
        if input_refs.iter().any(|value| value.trim().is_empty())
            || input_refs.iter().any(|value| value.len() > 512)
        {
            return Err("swarm_partition_input_invalid".to_owned());
        }
        if input_refs.iter().collect::<BTreeSet<_>>().len() != input_refs.len() {
            return Err("swarm_partition_input_duplicate".to_owned());
        }
        input_refs = input_refs
            .into_iter()
            .map(|value| value.trim().to_owned())
            .collect();
        input_refs.sort();
        data_scope = canonical_scope(data_scope)?;
        owned_paths = canonical_paths(owned_paths)?;
        let output_contract = output_contract.into().trim().to_owned();
        if output_contract.is_empty() || output_contract.len() > 256 || output_contract_version == 0
        {
            return Err("swarm_partition_output_contract_invalid".to_owned());
        }
        if dependency_partition_ids
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>()
            .len()
            != dependency_partition_ids.len()
        {
            return Err("swarm_partition_dependency_duplicate".to_owned());
        }
        if token_budget == 0 || model_call_budget == 0 {
            return Err("swarm_partition_budget_invalid".to_owned());
        }
        let partition_id = PartitionId::new();
        let input_digest = json_digest(&json!(input_refs));
        let work_fingerprint = fingerprint(
            swarm_plan_id,
            &input_refs,
            &output_contract,
            output_contract_version,
        )?;
        let partition = Self {
            schema: SWARM_PARTITION_SCHEMA.to_owned(),
            partition_id,
            swarm_plan_id,
            ordinal,
            input_refs,
            input_digest,
            data_scope,
            owned_paths,
            output_contract,
            output_contract_version,
            dependency_partition_ids,
            work_fingerprint,
            token_budget,
            model_call_budget,
            status: PartitionStatus::Pending,
            child_cell_id: None,
            active_attempt_id: None,
        };
        partition.validate()?;
        Ok(partition)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SWARM_PARTITION_SCHEMA
            || self.partition_id.as_uuid().is_nil()
            || self.swarm_plan_id.as_uuid().is_nil()
        {
            return Err("swarm_partition_identity_invalid".to_owned());
        }
        if self.input_refs.is_empty()
            || self
                .input_refs
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 512 || value.contains('\0'))
            || self.input_refs.windows(2).any(|pair| pair[0] >= pair[1])
            || self.input_refs.iter().collect::<BTreeSet<_>>().len() != self.input_refs.len()
        {
            return Err("swarm_partition_input_invalid".to_owned());
        }
        if self.input_digest != json_digest(&json!(self.input_refs)) {
            return Err("swarm_partition_input_digest_mismatch".to_owned());
        }
        if self
            .data_scope
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 512 || value.contains('\0'))
            || self.data_scope.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("swarm_partition_data_scope_invalid".to_owned());
        }
        if self
            .owned_paths
            .iter()
            .any(|path| normalize_role_path(path).as_deref() != Some(path))
            || self.owned_paths.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err("swarm_partition_path_invalid".to_owned());
        }
        if self.output_contract.trim().is_empty()
            || self.output_contract.len() > 256
            || self.output_contract_version == 0
        {
            return Err("swarm_partition_output_contract_invalid".to_owned());
        }
        if self
            .dependency_partition_ids
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>()
            .len()
            != self.dependency_partition_ids.len()
        {
            return Err("swarm_partition_dependency_duplicate".to_owned());
        }
        if self.token_budget == 0 || self.model_call_budget == 0 {
            return Err("swarm_partition_budget_invalid".to_owned());
        }
        let expected = fingerprint(
            self.swarm_plan_id,
            &self.input_refs,
            &self.output_contract,
            self.output_contract_version,
        )?;
        if self.work_fingerprint != expected {
            return Err("swarm_partition_fingerprint_mismatch".to_owned());
        }
        if !self.work_fingerprint.as_str().starts_with("fnv1a64:")
            || self.work_fingerprint.as_str().len() != 23
        {
            return Err("swarm_partition_fingerprint_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmWorkGraph {
    pub schema: String,
    pub swarm_plan_id: SwarmPlanId,
    pub partitions: Vec<Partition>,
    pub max_partition_count: u32,
    pub max_depth: u32,
    pub max_concurrency: u32,
    pub spawn_rate_limit: u32,
    pub created_at_unix_ms: u64,
    pub ttl_ms: u64,
    pub max_tokens: u64,
    pub max_model_calls: u64,
    pub merge_strategy: String,
    pub graph_digest: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartitionProjection {
    pub ready: Vec<PartitionId>,
    pub blocked: BTreeMap<String, String>,
    pub failed: Vec<PartitionId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwarmGraphError {
    pub code: String,
    pub partition_id: String,
    pub cycle: Vec<String>,
}

impl std::fmt::Display for SwarmGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.code,
            self.partition_id,
            self.cycle.join("->")
        )
    }
}
impl std::error::Error for SwarmGraphError {}

impl SwarmWorkGraph {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        swarm_plan_id: SwarmPlanId,
        partitions: Vec<Partition>,
        max_partition_count: u32,
        max_depth: u32,
        max_concurrency: u32,
        spawn_rate_limit: u32,
        created_at_unix_ms: u64,
        ttl_ms: u64,
        max_tokens: u64,
        max_model_calls: u64,
        merge_strategy: impl Into<String>,
    ) -> Result<Self, SwarmGraphError> {
        let mut graph = Self {
            schema: SWARM_WORK_GRAPH_SCHEMA.to_owned(),
            swarm_plan_id,
            partitions,
            max_partition_count,
            max_depth,
            max_concurrency,
            spawn_rate_limit,
            created_at_unix_ms,
            ttl_ms,
            max_tokens,
            max_model_calls,
            merge_strategy: merge_strategy.into(),
            graph_digest: String::new(),
        };
        graph.graph_digest = graph.digest();
        graph.validate()?;
        Ok(graph)
    }

    pub fn validate(&self) -> Result<(), SwarmGraphError> {
        fn invalid(code: &str, partition_id: impl Into<String>) -> SwarmGraphError {
            SwarmGraphError {
                code: code.to_owned(),
                partition_id: partition_id.into(),
                cycle: Vec::new(),
            }
        }
        if self.schema != SWARM_WORK_GRAPH_SCHEMA || self.swarm_plan_id.as_uuid().is_nil() {
            return Err(invalid("swarm_graph_identity_invalid", ""));
        }
        if self.partitions.is_empty() {
            return Err(invalid("swarm_partition_count_invalid", ""));
        }
        if self.max_partition_count == 0
            || self.max_partition_count as usize > MAX_SWARM_PARTITIONS
            || self.partitions.len() > self.max_partition_count as usize
        {
            return Err(invalid("swarm_partition_count_exceeded", ""));
        }
        if self.max_depth == 0 || self.max_depth > MAX_SWARM_DEPTH {
            return Err(invalid("swarm_depth_limit_exceeded", ""));
        }
        if self.max_concurrency == 0 || self.max_concurrency > MAX_SWARM_CONCURRENCY {
            return Err(invalid("swarm_concurrency_limit_exceeded", ""));
        }
        if self.spawn_rate_limit == 0 || self.spawn_rate_limit > MAX_SWARM_SPAWN_RATE {
            return Err(invalid("swarm_spawn_rate_limit_exceeded", ""));
        }
        if self.ttl_ms == 0 || self.ttl_ms > MAX_SWARM_TTL_MS {
            return Err(invalid("swarm_ttl_limit_exceeded", ""));
        }
        if self.max_tokens == 0 || self.max_tokens > MAX_SWARM_TOKENS {
            return Err(invalid("swarm_token_budget_exceeded", ""));
        }
        if self.max_model_calls == 0 || self.max_model_calls > MAX_SWARM_MODEL_CALLS {
            return Err(invalid("swarm_model_call_budget_exceeded", ""));
        }
        if self.merge_strategy == "first_success" {
            return Err(invalid("swarm_first_success_unsupported", ""));
        }
        if !matches!(self.merge_strategy.as_str(), "all_success" | "all_settled") {
            return Err(invalid("swarm_merge_strategy_invalid", ""));
        }
        let mut keys = BTreeMap::new();
        let mut ordinals = BTreeSet::new();
        let mut fingerprints = BTreeSet::new();
        let mut total_tokens = 0u64;
        let mut total_calls = 0u64;
        for partition in &self.partitions {
            partition
                .validate()
                .map_err(|code| invalid(&code, partition.partition_id.to_string()))?;
            if partition.swarm_plan_id != self.swarm_plan_id {
                return Err(invalid(
                    "swarm_partition_swarm_mismatch",
                    partition.partition_id.to_string(),
                ));
            }
            if !keys
                .insert(partition.partition_id.to_string(), partition)
                .is_none()
            {
                return Err(invalid(
                    "swarm_partition_key_duplicate",
                    partition.partition_id.to_string(),
                ));
            }
            if !ordinals.insert(partition.ordinal) {
                return Err(invalid(
                    "swarm_partition_ordinal_duplicate",
                    partition.partition_id.to_string(),
                ));
            }
            if !fingerprints.insert(partition.work_fingerprint.as_str().to_owned()) {
                return Err(invalid(
                    "swarm_duplicate_fingerprint",
                    partition.partition_id.to_string(),
                ));
            }
            total_tokens = total_tokens
                .checked_add(partition.token_budget)
                .ok_or_else(|| invalid("swarm_token_budget_exceeded", ""))?;
            total_calls = total_calls
                .checked_add(partition.model_call_budget)
                .ok_or_else(|| invalid("swarm_model_call_budget_exceeded", ""))?;
            if total_tokens > self.max_tokens {
                return Err(invalid(
                    "swarm_token_budget_exceeded",
                    partition.partition_id.to_string(),
                ));
            }
            if total_calls > self.max_model_calls {
                return Err(invalid(
                    "swarm_model_call_budget_exceeded",
                    partition.partition_id.to_string(),
                ));
            }
        }
        let mut dependencies = BTreeMap::new();
        for partition in &self.partitions {
            let id = partition.partition_id.to_string();
            dependencies.insert(
                id,
                partition
                    .dependency_partition_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            );
            for dependency in &partition.dependency_partition_ids {
                if !keys.contains_key(&dependency.to_string()) {
                    return Err(invalid(
                        "swarm_partition_dependency_missing",
                        dependency.to_string(),
                    ));
                }
            }
        }
        let order = validate_dependency_graph(&dependencies).map_err(|error| SwarmGraphError {
            code: match error.code.as_str() {
                "packet_dependency_cycle" => "swarm_partition_dependency_cycle",
                "packet_dependency_duplicate" => "swarm_partition_dependency_duplicate",
                "packet_dependency_missing" => "swarm_partition_dependency_missing",
                other => other,
            }
            .to_owned(),
            partition_id: error.packet_id,
            cycle: error.cycle,
        })?;
        let mut depths = BTreeMap::new();
        for id in order {
            let depth = dependencies[&id]
                .iter()
                .map(|dependency| depths.get(dependency).copied().unwrap_or(0) + 1)
                .max()
                .unwrap_or(1);
            if depth > self.max_depth {
                return Err(invalid("swarm_depth_limit_exceeded", id));
            }
            depths.insert(id, depth);
        }
        for (left_index, left) in self.partitions.iter().enumerate() {
            for right in self.partitions.iter().skip(left_index + 1) {
                if left
                    .owned_paths
                    .iter()
                    .any(|a| right.owned_paths.iter().any(|b| path_locks_conflict(a, b)))
                {
                    return Err(invalid(
                        "swarm_partition_overlap",
                        left.partition_id.to_string(),
                    ));
                }
                if left
                    .data_scope
                    .iter()
                    .any(|a| right.data_scope.iter().any(|b| scope_conflict(a, b)))
                {
                    return Err(invalid(
                        "swarm_data_scope_overlap",
                        left.partition_id.to_string(),
                    ));
                }
            }
        }
        if self.graph_digest != self.digest() {
            return Err(invalid("swarm_graph_digest_mismatch", ""));
        }
        Ok(())
    }

    pub fn projection(&self, now_unix_ms: u64) -> Result<PartitionProjection, SwarmGraphError> {
        self.validate()?;
        let mut projection = PartitionProjection::default();
        let mut dependencies = BTreeMap::new();
        for partition in &self.partitions {
            dependencies.insert(
                partition.partition_id.to_string(),
                partition
                    .dependency_partition_ids
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
            );
        }
        let order = validate_dependency_graph(&dependencies).map_err(|error| SwarmGraphError {
            code: "swarm_partition_dependency_invalid".to_owned(),
            partition_id: error.packet_id,
            cycle: error.cycle,
        })?;
        let by_id: BTreeMap<_, _> = self
            .partitions
            .iter()
            .map(|partition| (partition.partition_id.to_string(), partition))
            .collect();
        let expired = now_unix_ms >= self.created_at_unix_ms.saturating_add(self.ttl_ms);
        let mut failed = BTreeSet::new();
        for id in order {
            let partition = by_id[&id];
            if matches!(partition.status, PartitionStatus::Succeeded) {
                continue;
            }
            if expired
                || matches!(
                    partition.status,
                    PartitionStatus::Failed
                        | PartitionStatus::Cancelled
                        | PartitionStatus::ResultUnknown
                )
            {
                failed.insert(id.clone());
                projection.failed.push(partition.partition_id);
                if expired {
                    projection
                        .blocked
                        .insert(id, "swarm_ttl_expired".to_owned());
                }
                continue;
            }
            if matches!(partition.status, PartitionStatus::Running) {
                projection
                    .blocked
                    .insert(id, "partition_running".to_owned());
                continue;
            }
            if let Some(dependency) = partition
                .dependency_partition_ids
                .iter()
                .map(ToString::to_string)
                .find(|dependency| failed.contains(dependency))
            {
                failed.insert(id.clone());
                projection.failed.push(partition.partition_id);
                projection
                    .blocked
                    .insert(id, format!("dependency_failed:{dependency}"));
            } else if partition.dependency_partition_ids.iter().any(|dependency| {
                !matches!(
                    by_id[&dependency.to_string()].status,
                    PartitionStatus::Succeeded
                )
            }) {
                projection
                    .blocked
                    .insert(id, "dependency_incomplete".to_owned());
            } else {
                projection.ready.push(partition.partition_id);
            }
        }
        projection.ready.sort_by_key(ToString::to_string);
        projection.failed.sort_by_key(ToString::to_string);
        Ok(projection)
    }

    pub fn digest(&self) -> String {
        let mut partitions = self.partitions.iter().collect::<Vec<_>>();
        partitions.sort_by_key(|partition| partition.partition_id.to_string());
        json_digest(&json!({
            "schema": self.schema,
            "swarm_plan_id": self.swarm_plan_id,
            "partitions": partitions,
            "max_partition_count": self.max_partition_count,
            "max_depth": self.max_depth,
            "max_concurrency": self.max_concurrency,
            "spawn_rate_limit": self.spawn_rate_limit,
            "created_at_unix_ms": self.created_at_unix_ms,
            "ttl_ms": self.ttl_ms,
            "max_tokens": self.max_tokens,
            "max_model_calls": self.max_model_calls,
            "merge_strategy": self.merge_strategy,
        }))
    }
}

fn canonical_paths(paths: Vec<String>) -> Result<Vec<String>, String> {
    let mut normalized = Vec::with_capacity(paths.len());
    for path in paths {
        let path = normalize_role_path(&path).ok_or("swarm_partition_path_invalid")?;
        normalized.push(path);
    }
    normalized.sort();
    if normalized.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("swarm_partition_path_duplicate".to_owned());
    }
    Ok(normalized)
}

fn canonical_scope(scope: Vec<String>) -> Result<Vec<String>, String> {
    let mut normalized = scope
        .into_iter()
        .map(|value| value.trim().to_owned())
        .collect::<Vec<_>>();
    if normalized
        .iter()
        .any(|value| value.is_empty() || value.len() > 512 || value.contains('\0'))
    {
        return Err("swarm_partition_data_scope_invalid".to_owned());
    }
    normalized.sort();
    if normalized.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("swarm_partition_data_scope_duplicate".to_owned());
    }
    Ok(normalized)
}

fn scope_conflict(left: &str, right: &str) -> bool {
    left == "*"
        || right == "*"
        || left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

fn fingerprint(
    swarm_plan_id: SwarmPlanId,
    input_refs: &[String],
    output_contract: &str,
    output_contract_version: u32,
) -> Result<WorkFingerprint, String> {
    WorkFingerprint::from_parts(
        &format!("swarm:{swarm_plan_id}"),
        input_refs,
        "partition-work",
        &format!("{output_contract}:v{output_contract_version}"),
        SWARM_WORK_GRAPH_SCHEMA,
    )
    .map_err(|error| error.to_owned())
}
