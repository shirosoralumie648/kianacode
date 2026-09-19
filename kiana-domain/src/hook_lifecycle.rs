//! Ordered, bounded lifecycle contracts for Skills/Hooks extensions.
//!
//! A binding is a snapshot-scoped description of what a hook may observe or transform.  It is
//! not a capability, grant or execution loop.  Any tool-argument mutation requires a later
//! ControlPlane revalidation; the hook cannot authorize itself.

use crate::{json_digest, ExtensionSnapshot, HookDescriptor, SchemaVersion, SnapshotId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const HOOK_LIFECYCLE_BINDING_SCHEMA: &str = "kiana.hook-lifecycle-binding.v1";
pub const HOOK_LIFECYCLE_PLAN_SCHEMA: &str = "kiana.hook-lifecycle-plan.v1";
pub const HOOK_LIFECYCLE_INPUT_SCHEMA: &str = "kiana.hook-lifecycle-input.v1";
pub const HOOK_LIFECYCLE_RESULT_SCHEMA: &str = "kiana.hook-lifecycle-result.v1";
pub const HOOK_LIFECYCLE_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_HOOK_LIFECYCLE_POINTS: usize = 6;
pub const MAX_HOOK_FIELDS: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookLifecyclePoint {
    InputAccepted,
    BeforeModel,
    BeforeTool,
    AfterTool,
    BeforeCompact,
    BeforeStop,
}

impl HookLifecyclePoint {
    pub const ALL: [Self; MAX_HOOK_LIFECYCLE_POINTS] = [
        Self::InputAccepted,
        Self::BeforeModel,
        Self::BeforeTool,
        Self::AfterTool,
        Self::BeforeCompact,
        Self::BeforeStop,
    ];

    pub const fn order(self) -> u32 {
        match self {
            Self::InputAccepted => 10,
            Self::BeforeModel => 20,
            Self::BeforeTool => 30,
            Self::AfterTool => 40,
            Self::BeforeCompact => 50,
            Self::BeforeStop => 60,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InputAccepted => "input.accepted",
            Self::BeforeModel => "before.model",
            Self::BeforeTool => "before.tool",
            Self::AfterTool => "after.tool",
            Self::BeforeCompact => "before.compact",
            Self::BeforeStop => "before.stop",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookExecutionRole {
    Observer,
    Transformer,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFailurePolicy {
    Block,
    Ignore,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookBudget {
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    pub max_feedback_bytes: usize,
}

impl HookBudget {
    pub fn validate(&self) -> Result<(), String> {
        if self.timeout_ms == 0
            || self.timeout_ms > 10_000
            || self.max_output_bytes == 0
            || self.max_output_bytes > 64 * 1024
            || self.max_feedback_bytes == 0
            || self.max_feedback_bytes > 16 * 1024
        {
            return Err("hook_budget_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookLifecycleBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub hook_id: String,
    pub snapshot_id: SnapshotId,
    pub snapshot_digest: String,
    pub point: HookLifecyclePoint,
    pub role: HookExecutionRole,
    pub sequence: u32,
    pub readable_fields: BTreeSet<String>,
    pub writable_fields: BTreeSet<String>,
    pub revalidate_on_change: bool,
    pub failure_policy: HookFailurePolicy,
    pub budget: HookBudget,
    pub source_digest: String,
    pub binding_digest: String,
}

impl HookLifecycleBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        descriptor: &HookDescriptor,
        snapshot: &ExtensionSnapshot,
        point: HookLifecyclePoint,
        role: HookExecutionRole,
        sequence: u32,
        readable_fields: BTreeSet<String>,
        writable_fields: BTreeSet<String>,
        revalidate_on_change: bool,
        failure_policy: HookFailurePolicy,
        budget: HookBudget,
    ) -> Result<Self, String> {
        snapshot.validate()?;
        descriptor.validate()?;
        if descriptor.snapshot_id != snapshot.snapshot_id {
            return Err("hook_snapshot_identity_mismatch".to_owned());
        }
        let mut binding = Self {
            schema: HOOK_LIFECYCLE_BINDING_SCHEMA.to_owned(),
            version: HOOK_LIFECYCLE_VERSION,
            hook_id: descriptor.hook_id.clone(),
            snapshot_id: snapshot.snapshot_id,
            snapshot_digest: snapshot.snapshot_digest.clone(),
            point,
            role,
            sequence,
            readable_fields,
            writable_fields,
            revalidate_on_change,
            failure_policy,
            budget,
            source_digest: descriptor.source.digest(),
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HOOK_LIFECYCLE_BINDING_SCHEMA
            || self.version != HOOK_LIFECYCLE_VERSION
            || !bounded(&self.hook_id, 256)
            || self.snapshot_id.as_uuid().is_nil()
            || !digest(&self.snapshot_digest)
            || self.sequence == 0
            || self.readable_fields.len() > MAX_HOOK_FIELDS
            || self.writable_fields.len() > MAX_HOOK_FIELDS
            || !digest(&self.source_digest)
            || !digest(&self.binding_digest)
            || self.binding_digest != self.digest()
        {
            return Err("hook_lifecycle_binding_invalid".to_owned());
        }
        if self
            .readable_fields
            .iter()
            .chain(self.writable_fields.iter())
            .any(|field| !allowed_field(field))
        {
            return Err("hook_lifecycle_field_invalid".to_owned());
        }
        if self.role == HookExecutionRole::Observer && !self.writable_fields.is_empty() {
            return Err("hook_observer_write_denied".to_owned());
        }
        if self.writable_fields.contains("tool.arguments") && !self.revalidate_on_change {
            return Err("hook_tool_change_revalidation_required".to_owned());
        }
        self.budget.validate()?;
        Ok(())
    }

    pub fn apply_updates(
        &self,
        input: &HookLifecycleInput,
        updates: BTreeMap<String, Value>,
    ) -> Result<HookLifecycleResult, String> {
        self.validate()?;
        input.validate_against(self)?;
        if self.role == HookExecutionRole::Observer && !updates.is_empty() {
            return Err("hook_observer_write_denied".to_owned());
        }
        if updates.len() > self.writable_fields.len()
            || updates
                .keys()
                .any(|field| !self.writable_fields.contains(field))
        {
            return Err("hook_update_field_denied".to_owned());
        }
        let mut output = input.fields.clone();
        for (field, value) in updates {
            output.insert(field, value);
        }
        let changed_fields = output
            .iter()
            .filter_map(|(field, value)| {
                (input.fields.get(field) != Some(value)).then(|| field.clone())
            })
            .collect::<BTreeSet<_>>();
        let result = HookLifecycleResult {
            schema: HOOK_LIFECYCLE_RESULT_SCHEMA.to_owned(),
            version: HOOK_LIFECYCLE_VERSION,
            binding_digest: self.binding_digest.clone(),
            output_digest: json_digest(&json!(output)),
            changed_fields,
            requires_revalidation: self.revalidate_on_change && output != input.fields,
        };
        result.validate()?;
        Ok(result)
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "hook_id": self.hook_id,
            "snapshot_id": self.snapshot_id,
            "snapshot_digest": self.snapshot_digest,
            "point": self.point,
            "role": self.role,
            "sequence": self.sequence,
            "readable_fields": self.readable_fields,
            "writable_fields": self.writable_fields,
            "revalidate_on_change": self.revalidate_on_change,
            "failure_policy": self.failure_policy,
            "budget": self.budget,
            "source_digest": self.source_digest,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookLifecyclePlan {
    pub schema: String,
    pub version: SchemaVersion,
    pub snapshot_id: SnapshotId,
    pub snapshot_digest: String,
    pub bindings: Vec<HookLifecycleBinding>,
    pub plan_digest: String,
}

impl HookLifecyclePlan {
    pub fn new(
        snapshot: &ExtensionSnapshot,
        mut bindings: Vec<HookLifecycleBinding>,
    ) -> Result<Self, String> {
        snapshot.validate()?;
        bindings.sort_by(|left, right| {
            (left.point.order(), left.sequence, &left.hook_id).cmp(&(
                right.point.order(),
                right.sequence,
                &right.hook_id,
            ))
        });
        let plan = Self {
            schema: HOOK_LIFECYCLE_PLAN_SCHEMA.to_owned(),
            version: HOOK_LIFECYCLE_VERSION,
            snapshot_id: snapshot.snapshot_id,
            snapshot_digest: snapshot.snapshot_digest.clone(),
            bindings,
            plan_digest: String::new(),
        };
        let mut plan = plan;
        plan.plan_digest = plan.digest();
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HOOK_LIFECYCLE_PLAN_SCHEMA
            || self.version != HOOK_LIFECYCLE_VERSION
            || self.snapshot_id.as_uuid().is_nil()
            || !digest(&self.snapshot_digest)
            || self.bindings.len() > MAX_HOOK_FIELDS
            || !digest(&self.plan_digest)
            || self.plan_digest != self.digest()
        {
            return Err("hook_lifecycle_plan_invalid".to_owned());
        }
        let mut identities = BTreeSet::new();
        for binding in &self.bindings {
            binding.validate()?;
            if binding.snapshot_id != self.snapshot_id
                || binding.snapshot_digest != self.snapshot_digest
                || !identities.insert((binding.point, binding.sequence, binding.hook_id.clone()))
            {
                return Err("hook_lifecycle_plan_binding_invalid".to_owned());
            }
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "snapshot_id": self.snapshot_id,
            "snapshot_digest": self.snapshot_digest,
            "bindings": self.bindings,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookLifecycleInput {
    pub schema: String,
    pub version: SchemaVersion,
    pub binding_digest: String,
    pub fields: BTreeMap<String, Value>,
    pub input_digest: String,
}

impl HookLifecycleInput {
    pub fn new(
        binding: &HookLifecycleBinding,
        fields: BTreeMap<String, Value>,
    ) -> Result<Self, String> {
        binding.validate()?;
        let mut input = Self {
            schema: HOOK_LIFECYCLE_INPUT_SCHEMA.to_owned(),
            version: HOOK_LIFECYCLE_VERSION,
            binding_digest: binding.binding_digest.clone(),
            fields,
            input_digest: String::new(),
        };
        input.input_digest = input.digest();
        input.validate_against(binding)?;
        Ok(input)
    }

    pub fn validate_against(&self, binding: &HookLifecycleBinding) -> Result<(), String> {
        if self.schema != HOOK_LIFECYCLE_INPUT_SCHEMA
            || self.version != HOOK_LIFECYCLE_VERSION
            || self.binding_digest != binding.binding_digest
            || self.fields.len() > MAX_HOOK_FIELDS
            || !digest(&self.input_digest)
            || self.input_digest != self.digest()
            || self
                .fields
                .keys()
                .any(|field| !binding.readable_fields.contains(field))
        {
            return Err("hook_lifecycle_input_invalid".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "binding_digest": self.binding_digest,
            "fields": self.fields,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookLifecycleResult {
    pub schema: String,
    pub version: SchemaVersion,
    pub binding_digest: String,
    pub output_digest: String,
    pub changed_fields: BTreeSet<String>,
    pub requires_revalidation: bool,
}

impl HookLifecycleResult {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != HOOK_LIFECYCLE_RESULT_SCHEMA
            || self.version != HOOK_LIFECYCLE_VERSION
            || !digest(&self.binding_digest)
            || !digest(&self.output_digest)
            || self.changed_fields.len() > MAX_HOOK_FIELDS
            || self
                .changed_fields
                .iter()
                .any(|field| !allowed_field(field))
        {
            return Err("hook_lifecycle_result_invalid".to_owned());
        }
        Ok(())
    }
}

fn allowed_field(field: &str) -> bool {
    bounded(field, 128)
        && (field.starts_with("context.")
            || field == "tool.arguments"
            || field == "tool.result"
            || field == "run.metadata")
}

fn bounded(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.contains('\0')
}

fn digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}
