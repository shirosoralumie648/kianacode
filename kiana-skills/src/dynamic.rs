use crate::disclosure::{activate_skill, DisclosureError, SkillActivation, SkillActivationRequest};
use crate::types::Command;
use ignore::gitignore::GitignoreBuilder;
use kiana_domain::json_digest;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::RwLock;
use thiserror::Error;

pub const DYNAMIC_SKILL_SCHEMA: &str = "kiana.dynamic-skill.v1";
pub const PATH_GLOB_AST_SCHEMA: &str = "kiana.path-glob-ast.v1";
pub const DYNAMIC_ACTIVATION_RECEIPT_SCHEMA: &str = "kiana.dynamic-activation-receipt.v1";
pub const SKILL_INVOCATION_SCHEMA: &str = "kiana.skill-invocation.v1";
const MAX_PATH_PATTERNS: usize = 64;
const MAX_PATTERN_BYTES: usize = 512;
const MAX_ARGS: usize = 32;
const MAX_ARG_BYTES: usize = 4 * 1024;
const MAX_TOTAL_ARG_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicSkillScope {
    pub session_id: String,
    pub snapshot_generation: u64,
}

impl DynamicSkillScope {
    pub fn new(
        session_id: impl Into<String>,
        snapshot_generation: u64,
    ) -> Result<Self, DynamicSkillError> {
        let scope = Self {
            session_id: session_id.into(),
            snapshot_generation,
        };
        if scope.session_id.trim().is_empty()
            || scope.session_id.len() > 256
            || scope.session_id.contains('\0')
            || scope.snapshot_generation == 0
        {
            return Err(DynamicSkillError::ScopeInvalid);
        }
        Ok(scope)
    }

    fn legacy() -> Self {
        Self {
            session_id: "legacy-process-adapter".to_owned(),
            snapshot_generation: 1,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathGlobAst {
    pub schema: String,
    pub patterns: Vec<String>,
    pub pattern_digest: String,
}

impl PathGlobAst {
    pub fn compile(patterns: &[String]) -> Result<Self, DynamicSkillError> {
        if patterns.is_empty() || patterns.len() > MAX_PATH_PATTERNS {
            return Err(DynamicSkillError::PathPatternInvalid);
        }
        let mut normalized = patterns
            .iter()
            .map(|pattern| pattern.trim().to_owned())
            .collect::<Vec<_>>();
        if normalized.iter().any(|pattern| {
            pattern.is_empty()
                || pattern.len() > MAX_PATTERN_BYTES
                || pattern.contains('\0')
                || pattern.contains('\\')
                || pattern.starts_with('/')
                || pattern.split('/').any(|part| part == "..")
        }) {
            return Err(DynamicSkillError::PathPatternInvalid);
        }
        normalized.sort();
        normalized.dedup();
        let mut builder = GitignoreBuilder::new(".");
        for pattern in &normalized {
            builder
                .add_line(None, pattern)
                .map_err(|_| DynamicSkillError::PathPatternInvalid)?;
        }
        builder
            .build()
            .map_err(|_| DynamicSkillError::PathPatternInvalid)?;
        let mut ast = Self {
            schema: PATH_GLOB_AST_SCHEMA.to_owned(),
            patterns: normalized,
            pattern_digest: String::new(),
        };
        ast.pattern_digest = ast.digest();
        Ok(ast)
    }

    pub fn matches(&self, cwd: &Path, file_paths: &[impl AsRef<Path>]) -> Vec<String> {
        let mut builder = GitignoreBuilder::new(cwd);
        for pattern in &self.patterns {
            let _ = builder.add_line(None, pattern);
        }
        let Ok(matcher) = builder.build() else {
            return Vec::new();
        };
        let mut matched = BTreeSet::new();
        for file_path in file_paths {
            let path = file_path.as_ref();
            let relative = if path.is_absolute() {
                let Ok(relative) = path.strip_prefix(cwd) else {
                    continue;
                };
                relative.to_path_buf()
            } else {
                path.to_path_buf()
            };
            if matcher.matched(&relative, false).is_ignore() {
                matched.insert(normalize_path(&relative));
            }
        }
        matched.into_iter().collect()
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "patterns": self.patterns,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicActivationStatus {
    Activated,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicActivationReceipt {
    pub schema: String,
    pub scope: DynamicSkillScope,
    pub skill_name: String,
    pub status: DynamicActivationStatus,
    pub reason: String,
    pub trigger_paths: Vec<String>,
    pub trigger_digest: String,
    pub activation: Option<SkillActivation>,
    pub receipt_digest: String,
}

impl DynamicActivationReceipt {
    fn new(
        scope: DynamicSkillScope,
        skill_name: String,
        status: DynamicActivationStatus,
        reason: String,
        trigger_paths: Vec<String>,
        activation: Option<SkillActivation>,
    ) -> Self {
        let trigger_digest = json_digest(&json!({ "paths": &trigger_paths }));
        let mut receipt = Self {
            schema: DYNAMIC_ACTIVATION_RECEIPT_SCHEMA.to_owned(),
            scope,
            skill_name,
            status,
            reason,
            trigger_paths,
            trigger_digest,
            activation,
            receipt_digest: String::new(),
        };
        receipt.receipt_digest = receipt.digest();
        receipt
    }

    fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "scope": self.scope,
            "skill_name": self.skill_name,
            "status": self.status,
            "reason": self.reason,
            "trigger_paths": self.trigger_paths,
            "trigger_digest": self.trigger_digest,
            "activation": self.activation,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillInvocationRequest {
    pub scope: DynamicSkillScope,
    pub skill_name: String,
    pub argv: Vec<String>,
    pub now_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillInvocation {
    pub schema: String,
    pub scope: DynamicSkillScope,
    pub skill_name: String,
    pub argv: Vec<String>,
    pub args_digest: String,
    pub activation_digest: String,
}

#[derive(Debug, Error)]
pub enum DynamicSkillError {
    #[error("dynamic_scope_invalid")]
    ScopeInvalid,
    #[error("dynamic_skill_missing")]
    SkillMissing,
    #[error("dynamic_skill_not_conditional")]
    SkillNotConditional,
    #[error("dynamic_skill_path_pattern_invalid")]
    PathPatternInvalid,
    #[error("dynamic_skill_arguments_invalid:{0}")]
    ArgumentsInvalid(&'static str),
    #[error("dynamic_skill_activation:{0}")]
    Activation(String),
}

impl From<DisclosureError> for DynamicSkillError {
    fn from(error: DisclosureError) -> Self {
        Self::Activation(error.to_string())
    }
}

#[derive(Clone)]
struct ConditionalSkill {
    skill: Command,
    paths: PathGlobAst,
}

#[derive(Clone)]
struct ActiveSkill {
    skill: Command,
    activation: SkillActivation,
}

#[derive(Default)]
struct ScopeState {
    conditional: BTreeMap<String, ConditionalSkill>,
    active: BTreeMap<String, ActiveSkill>,
}

pub struct DynamicSkillStore {
    scopes: RwLock<BTreeMap<DynamicSkillScope, ScopeState>>,
}

impl Default for DynamicSkillStore {
    fn default() -> Self {
        Self {
            scopes: RwLock::new(BTreeMap::new()),
        }
    }
}

impl DynamicSkillStore {
    pub fn add_dynamic_skill_for_scope(
        &self,
        scope: &DynamicSkillScope,
        skill: Command,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
        reason: impl Into<String>,
    ) -> Result<DynamicActivationReceipt, DynamicSkillError> {
        let activation = activate_skill(
            &skill,
            &SkillActivationRequest {
                snapshot_generation: scope.snapshot_generation,
                now_unix_ms,
                expires_at_unix_ms,
                reason: reason.into(),
            },
        )?;
        let receipt = DynamicActivationReceipt::new(
            scope.clone(),
            skill.name.clone(),
            DynamicActivationStatus::Activated,
            "explicit_activation".to_owned(),
            Vec::new(),
            Some(activation.clone()),
        );
        self.scopes
            .write()
            .map_err(|_| DynamicSkillError::ScopeInvalid)?
            .entry(scope.clone())
            .or_default()
            .active
            .insert(skill.name.clone(), ActiveSkill { skill, activation });
        crate::invalidate_extension_snapshots("dynamic_skill_added");
        Ok(receipt)
    }

    pub fn store_conditional_skill_for_scope(
        &self,
        scope: &DynamicSkillScope,
        skill: Command,
    ) -> Result<PathGlobAst, DynamicSkillError> {
        let patterns = skill
            .paths
            .clone()
            .ok_or(DynamicSkillError::SkillNotConditional)?;
        let ast = PathGlobAst::compile(&patterns)?;
        self.scopes
            .write()
            .map_err(|_| DynamicSkillError::ScopeInvalid)?
            .entry(scope.clone())
            .or_default()
            .conditional
            .insert(
                skill.name.clone(),
                ConditionalSkill {
                    skill,
                    paths: ast.clone(),
                },
            );
        crate::invalidate_extension_snapshots("conditional_skill_stored");
        Ok(ast)
    }

    pub fn get_dynamic_skills_for_scope(&self, scope: &DynamicSkillScope) -> Vec<Command> {
        self.scopes
            .read()
            .ok()
            .and_then(|scopes| {
                scopes.get(scope).map(|state| {
                    state
                        .active
                        .values()
                        .map(|entry| entry.skill.clone())
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default()
    }

    pub fn activate_conditional_skills_for_paths_for_scope(
        &self,
        scope: &DynamicSkillScope,
        file_paths: &[impl AsRef<Path>],
        cwd: impl AsRef<Path>,
        now_unix_ms: u64,
        expires_at_unix_ms: u64,
    ) -> Result<Vec<DynamicActivationReceipt>, DynamicSkillError> {
        let cwd = cwd.as_ref();
        let candidates = self
            .scopes
            .read()
            .map_err(|_| DynamicSkillError::ScopeInvalid)?
            .get(scope)
            .map(|state| {
                state
                    .conditional
                    .iter()
                    .map(|(name, skill)| (name.clone(), skill.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut receipts = Vec::new();
        for (name, conditional) in candidates {
            let trigger_paths = conditional.paths.matches(cwd, file_paths);
            if trigger_paths.is_empty() {
                continue;
            }
            let activation = activate_skill(
                &conditional.skill,
                &SkillActivationRequest {
                    snapshot_generation: scope.snapshot_generation,
                    now_unix_ms,
                    expires_at_unix_ms,
                    reason: "path_trigger".to_owned(),
                },
            )?;
            let receipt = DynamicActivationReceipt::new(
                scope.clone(),
                name.clone(),
                DynamicActivationStatus::Activated,
                "path_trigger".to_owned(),
                trigger_paths,
                Some(activation.clone()),
            );
            let mut scopes = self
                .scopes
                .write()
                .map_err(|_| DynamicSkillError::ScopeInvalid)?;
            let state = scopes.entry(scope.clone()).or_default();
            state.conditional.remove(&name);
            state.active.insert(
                name,
                ActiveSkill {
                    skill: conditional.skill,
                    activation,
                },
            );
            receipts.push(receipt);
        }
        if !receipts.is_empty() {
            crate::invalidate_extension_snapshots("conditional_skill_activated");
        }
        Ok(receipts)
    }

    pub fn prepare_invocation(
        &self,
        request: &SkillInvocationRequest,
    ) -> Result<SkillInvocation, DynamicSkillError> {
        let active = self
            .scopes
            .read()
            .map_err(|_| DynamicSkillError::ScopeInvalid)?
            .get(&request.scope)
            .and_then(|state| state.active.get(&request.skill_name).cloned())
            .ok_or(DynamicSkillError::SkillMissing)?;
        active
            .activation
            .validate_for(&active.skill, request.now_unix_ms)
            .map_err(DynamicSkillError::from)?;
        validate_skill_argv(&active.skill, &request.argv)?;
        Ok(SkillInvocation {
            schema: SKILL_INVOCATION_SCHEMA.to_owned(),
            scope: request.scope.clone(),
            skill_name: request.skill_name.clone(),
            argv: request.argv.clone(),
            args_digest: json_digest(&json!({ "argv": &request.argv })),
            activation_digest: active.activation.activation_digest,
        })
    }

    pub fn revoke_skill_for_scope(
        &self,
        scope: &DynamicSkillScope,
        skill_name: &str,
        reason: impl Into<String>,
    ) -> Result<DynamicActivationReceipt, DynamicSkillError> {
        let active = self
            .scopes
            .write()
            .map_err(|_| DynamicSkillError::ScopeInvalid)?
            .get_mut(scope)
            .and_then(|state| state.active.remove(skill_name))
            .ok_or(DynamicSkillError::SkillMissing)?;
        let receipt = DynamicActivationReceipt::new(
            scope.clone(),
            skill_name.to_owned(),
            DynamicActivationStatus::Revoked,
            reason.into(),
            Vec::new(),
            Some(active.activation),
        );
        crate::invalidate_extension_snapshots("dynamic_skill_revoked");
        Ok(receipt)
    }

    pub fn clear_scope(&self, scope: &DynamicSkillScope) {
        if let Ok(mut scopes) = self.scopes.write() {
            scopes.remove(scope);
        }
        crate::invalidate_extension_snapshots("dynamic_skill_scope_cleared");
    }
}

static LEGACY_SCOPE_STORE: Lazy<DynamicSkillStore> = Lazy::new(DynamicSkillStore::default);

pub fn add_dynamic_skill(skill: Command) {
    let _ = LEGACY_SCOPE_STORE.add_dynamic_skill_for_scope(
        &DynamicSkillScope::legacy(),
        skill,
        1,
        u64::MAX - 1,
        "legacy_dynamic_adapter",
    );
}

pub fn get_dynamic_skills() -> Vec<Command> {
    LEGACY_SCOPE_STORE.get_dynamic_skills_for_scope(&DynamicSkillScope::legacy())
}

pub fn store_conditional_skill(skill: Command) {
    let _ =
        LEGACY_SCOPE_STORE.store_conditional_skill_for_scope(&DynamicSkillScope::legacy(), skill);
}

pub fn activate_conditional_skills_for_paths(
    file_paths: &[impl AsRef<Path>],
    cwd: impl AsRef<Path>,
) -> Vec<String> {
    LEGACY_SCOPE_STORE
        .activate_conditional_skills_for_paths_for_scope(
            &DynamicSkillScope::legacy(),
            file_paths,
            cwd,
            1,
            u64::MAX - 1,
        )
        .unwrap_or_default()
        .into_iter()
        .map(|receipt| receipt.skill_name)
        .collect()
}

pub fn clear_dynamic_skills() {
    LEGACY_SCOPE_STORE.clear_scope(&DynamicSkillScope::legacy());
}

fn validate_skill_argv(skill: &Command, argv: &[String]) -> Result<(), DynamicSkillError> {
    if argv.len() > MAX_ARGS {
        return Err(DynamicSkillError::ArgumentsInvalid("argument_count"));
    }
    let total_bytes = argv.iter().try_fold(0usize, |total, arg| {
        if arg.is_empty() || arg.len() > MAX_ARG_BYTES || arg.contains('\0') {
            return Err(DynamicSkillError::ArgumentsInvalid("argument_value"));
        }
        total
            .checked_add(arg.len())
            .filter(|bytes| *bytes <= MAX_TOTAL_ARG_BYTES)
            .ok_or(DynamicSkillError::ArgumentsInvalid("argument_total_bytes"))
    })?;
    let _ = total_bytes;
    let declared = declared_arguments(skill.argument_hint.as_deref());
    let positional_limit = declared_positional_count(skill.argument_hint.as_deref());
    let positional_count = argv.iter().filter(|arg| !arg.starts_with('-')).count();
    if positional_count > positional_limit {
        return Err(DynamicSkillError::ArgumentsInvalid("unknown_argument"));
    }
    for arg in argv.iter().filter(|arg| arg.starts_with('-')) {
        let name = arg
            .trim_start_matches('-')
            .split('=')
            .next()
            .filter(|name| !name.is_empty())
            .ok_or(DynamicSkillError::ArgumentsInvalid("argument_name"))?;
        if !declared.contains(name) {
            return Err(DynamicSkillError::ArgumentsInvalid("unknown_argument"));
        }
    }
    if declared.is_empty() && !argv.is_empty() {
        return Err(DynamicSkillError::ArgumentsInvalid(
            "arguments_not_declared",
        ));
    }
    Ok(())
}

fn declared_arguments(argument_hint: Option<&str>) -> BTreeSet<String> {
    argument_hint
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|token| {
            let token = token.trim_matches(|character| matches!(character, '<' | '>' | '[' | ']'));
            let token = token.strip_prefix("--").unwrap_or(token);
            let token = token.split('=').next().unwrap_or(token);
            (!token.is_empty()
                && token.chars().all(|character| {
                    character.is_ascii_alphanumeric() || character == '-' || character == '_'
                }))
            .then(|| token.to_owned())
        })
        .collect()
}

fn declared_positional_count(argument_hint: Option<&str>) -> usize {
    argument_hint
        .unwrap_or_default()
        .split_whitespace()
        .filter(|token| !token.starts_with('-'))
        .count()
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
