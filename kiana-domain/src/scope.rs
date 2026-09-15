//! Explicit capability scope values and monotonic intersection helpers.
//!
//! `NotApplicable` is different from an empty `Restricted` set: the former does not constrain a
//! dimension, while the latter represents no permission. Scope values never contain wildcards;
//! callers must expand a server-owned allow-list before constructing a scope.

use crate::{canonical_journal_bytes, json_digest, SchemaVersion};
use serde::{Deserialize, Serialize};

pub const SCOPE_SET_SCHEMA: &str = "kiana.scope-set.v1";
pub const SCOPE_SET_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SCOPE_VALUES: usize = 256;
pub const MAX_SCOPE_VALUE_BYTES: usize = 4_096;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "values", rename_all = "snake_case")]
pub enum ScopeDimension {
    NotApplicable,
    Restricted(Vec<String>),
}

impl Default for ScopeDimension {
    fn default() -> Self {
        Self::NotApplicable
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "value", rename_all = "snake_case")]
pub enum ScopeLimit {
    NotApplicable,
    Restricted(u64),
}

impl Default for ScopeLimit {
    fn default() -> Self {
        Self::NotApplicable
    }
}

fn normalize_dimension(dimension: &ScopeDimension, field: &str) -> Result<ScopeDimension, String> {
    let ScopeDimension::Restricted(values) = dimension else {
        return Ok(ScopeDimension::NotApplicable);
    };
    if values.len() > MAX_SCOPE_VALUES {
        return Err(format!("{field}_limit"));
    }
    let mut normalized = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if value.is_empty() || value.len() > MAX_SCOPE_VALUE_BYTES || value.contains('\0') {
            return Err(format!("{field}_value_invalid"));
        }
        normalized.push(value.to_owned());
    }
    normalized.sort_unstable();
    if normalized.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(format!("{field}_duplicate"));
    }
    Ok(ScopeDimension::Restricted(normalized))
}

fn validate_limit(limit: ScopeLimit) -> Result<(), String> {
    match limit {
        ScopeLimit::NotApplicable | ScopeLimit::Restricted(_) => Ok(()),
    }
}

fn intersect_dimension(left: &ScopeDimension, right: &ScopeDimension) -> ScopeDimension {
    match (left, right) {
        (ScopeDimension::NotApplicable, value) | (value, ScopeDimension::NotApplicable) => {
            value.clone()
        }
        (ScopeDimension::Restricted(left), ScopeDimension::Restricted(right)) => {
            ScopeDimension::Restricted(
                left.iter()
                    .filter(|value| right.binary_search(value).is_ok())
                    .cloned()
                    .collect(),
            )
        }
    }
}

fn intersect_path_dimension(left: &ScopeDimension, right: &ScopeDimension) -> ScopeDimension {
    match (left, right) {
        (ScopeDimension::NotApplicable, value) | (value, ScopeDimension::NotApplicable) => {
            value.clone()
        }
        (ScopeDimension::Restricted(left), ScopeDimension::Restricted(right)) => {
            let mut values = Vec::new();
            for left in left {
                for right in right {
                    if left == right {
                        values.push(left.clone());
                    } else if left == "." || left == "*" || right.starts_with(&format!("{left}/")) {
                        values.push(right.clone());
                    } else if right == "." || right == "*" || left.starts_with(&format!("{right}/"))
                    {
                        values.push(left.clone());
                    }
                }
            }
            values.sort_unstable();
            values.dedup();
            ScopeDimension::Restricted(values)
        }
    }
}

fn intersect_limit(left: ScopeLimit, right: ScopeLimit) -> ScopeLimit {
    match (left, right) {
        (ScopeLimit::NotApplicable, value) | (value, ScopeLimit::NotApplicable) => value,
        (ScopeLimit::Restricted(left), ScopeLimit::Restricted(right)) => {
            ScopeLimit::Restricted(left.min(right))
        }
    }
}

fn dimension_subset(child: &ScopeDimension, parent: &ScopeDimension) -> bool {
    match (child, parent) {
        (_, ScopeDimension::NotApplicable) => true,
        (ScopeDimension::NotApplicable, ScopeDimension::Restricted(_)) => false,
        (ScopeDimension::Restricted(child), ScopeDimension::Restricted(parent)) => child
            .iter()
            .all(|value| parent.binary_search(value).is_ok()),
    }
}

fn path_subset(child: &ScopeDimension, parent: &ScopeDimension) -> bool {
    match (child, parent) {
        (_, ScopeDimension::NotApplicable) => true,
        (ScopeDimension::NotApplicable, ScopeDimension::Restricted(_)) => false,
        (ScopeDimension::Restricted(child), ScopeDimension::Restricted(parent)) => {
            child.iter().all(|child| {
                parent.iter().any(|parent| {
                    parent == "."
                        || parent == "*"
                        || child == parent
                        || child.starts_with(&format!("{parent}/"))
                })
            })
        }
    }
}

fn limit_subset(child: ScopeLimit, parent: ScopeLimit) -> bool {
    match (child, parent) {
        (_, ScopeLimit::NotApplicable) => true,
        (ScopeLimit::NotApplicable, ScopeLimit::Restricted(_)) => false,
        (ScopeLimit::Restricted(child), ScopeLimit::Restricted(parent)) => child <= parent,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeSet {
    pub schema: String,
    pub version: SchemaVersion,
    pub operations: ScopeDimension,
    pub paths: ScopeDimension,
    pub namespaces: ScopeDimension,
    pub network: ScopeDimension,
    pub budget: ScopeLimit,
    pub depth: ScopeLimit,
    pub scope_digest: String,
}

impl ScopeSet {
    pub fn new(
        operations: ScopeDimension,
        paths: ScopeDimension,
        namespaces: ScopeDimension,
        network: ScopeDimension,
        budget: ScopeLimit,
        depth: ScopeLimit,
    ) -> Result<Self, String> {
        let mut scope = Self {
            schema: SCOPE_SET_SCHEMA.to_owned(),
            version: SCOPE_SET_SCHEMA_VERSION,
            operations: normalize_dimension(&operations, "scope_operations")?,
            paths: normalize_dimension(&paths, "scope_paths")?,
            namespaces: normalize_dimension(&namespaces, "scope_namespaces")?,
            network: normalize_dimension(&network, "scope_network")?,
            budget,
            depth,
            scope_digest: String::new(),
        };
        validate_limit(scope.budget)?;
        validate_limit(scope.depth)?;
        scope.scope_digest = scope.digest();
        scope.validate()?;
        Ok(scope)
    }

    pub fn unrestricted() -> Self {
        Self::new(
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeDimension::NotApplicable,
            ScopeLimit::NotApplicable,
            ScopeLimit::NotApplicable,
        )
        .expect("unrestricted scope is valid")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCOPE_SET_SCHEMA
            || !self.version.is_compatible_with(&SCOPE_SET_SCHEMA_VERSION)
        {
            return Err("scope_set_header_invalid".to_owned());
        }
        for (dimension, field) in [
            (&self.operations, "scope_operations"),
            (&self.paths, "scope_paths"),
            (&self.namespaces, "scope_namespaces"),
            (&self.network, "scope_network"),
        ] {
            if normalize_dimension(dimension, field)? != dimension.clone() {
                return Err(format!("{field}_noncanonical"));
            }
        }
        validate_limit(self.budget)?;
        validate_limit(self.depth)?;
        let Some(hex) = self.scope_digest.strip_prefix("sha256:") else {
            return Err("scope_set_digest_invalid".to_owned());
        };
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("scope_set_digest_invalid".to_owned());
        }
        if self.scope_digest != self.digest() {
            return Err("scope_set_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, String> {
        canonical_journal_bytes(self)
    }

    pub fn intersect(&self, other: &Self) -> Result<Self, String> {
        self.validate()?;
        other.validate()?;
        let result = Self::new(
            intersect_dimension(&self.operations, &other.operations),
            intersect_path_dimension(&self.paths, &other.paths),
            intersect_dimension(&self.namespaces, &other.namespaces),
            intersect_dimension(&self.network, &other.network),
            intersect_limit(self.budget, other.budget),
            intersect_limit(self.depth, other.depth),
        )?;
        if [
            &result.operations,
            &result.paths,
            &result.namespaces,
            &result.network,
        ]
        .iter()
        .any(|dimension| matches!(dimension, ScopeDimension::Restricted(values) if values.is_empty()))
        {
            return Err("scope_intersection_empty".to_owned());
        }
        Ok(result)
    }

    pub fn intersect_all(scopes: &[Self]) -> Result<Self, String> {
        let Some((first, rest)) = scopes.split_first() else {
            return Err("scope_layers_required".to_owned());
        };
        rest.iter()
            .try_fold(first.clone(), |current, next| current.intersect(next))
    }

    pub fn is_subset_of(&self, parent: &Self) -> Result<bool, String> {
        self.validate()?;
        parent.validate()?;
        Ok(dimension_subset(&self.operations, &parent.operations)
            && path_subset(&self.paths, &parent.paths)
            && dimension_subset(&self.namespaces, &parent.namespaces)
            && dimension_subset(&self.network, &parent.network)
            && limit_subset(self.budget, parent.budget)
            && limit_subset(self.depth, parent.depth))
    }

    pub fn allows_operation(&self, operation: &str) -> bool {
        matches!(&self.operations, ScopeDimension::NotApplicable)
            || matches!(&self.operations, ScopeDimension::Restricted(values) if values.iter().any(|value| value == operation))
    }

    pub fn allows_path(&self, path: &str) -> bool {
        matches!(&self.paths, ScopeDimension::NotApplicable)
            || matches!(&self.paths, ScopeDimension::Restricted(values) if values.iter().any(|value| value == path || path.starts_with(&format!("{value}/"))))
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "scope_digest".to_owned(),
                serde_json::Value::String(String::new()),
            );
        }
        json_digest(&value)
    }
}
