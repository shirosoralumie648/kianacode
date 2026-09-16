//! Versioned identity, credential-reference, configuration and authority snapshot contracts.
//!
//! These values intentionally carry only opaque identifiers, references and digests.  A
//! `SecretRef` is not a secret store and an `AuthoritySnapshot` is not permission by itself; the
//! ControlPlane must still revalidate the snapshot at every effect boundary.
use crate::{
    json_digest, AssignmentId, AuthenticatedPrincipalRef, OrganizationId, PrincipalId, ProjectId,
    ProviderAccountId, ServiceIdentityId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

pub const PRINCIPAL_SCHEMA: &str = "kiana.principal.v1";
pub const MEMBERSHIP_SCHEMA: &str = "kiana.membership.v1";
pub const SECRET_REF_SCHEMA: &str = "kiana.secret-ref.v1";
pub const PROVIDER_ACCOUNT_SCHEMA: &str = "kiana.provider-account.v1";
pub const SERVICE_IDENTITY_SCHEMA: &str = "kiana.service-identity.v1";
pub const CONFIG_SNAPSHOT_SCHEMA: &str = "kiana.config-snapshot.v1";
pub const AUTHORITY_SNAPSHOT_SCHEMA: &str = "kiana.authority-snapshot.v1";

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        return Err(format!("{field}_invalid"));
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

fn list(values: &[String], field: &str, max: usize) -> Result<(), String> {
    if values.len() > max
        || values
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 4_096 || value.contains('\0'))
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    Agent,
    Service,
    Mcp,
    Provider,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalStatus {
    Proposed,
    Active,
    Suspended,
    Revoked,
    Archived,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub schema: String,
    pub principal_id: PrincipalId,
    pub kind: PrincipalKind,
    pub status: PrincipalStatus,
    pub created_at_unix_ms: u64,
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    pub authentication: AuthenticatedPrincipalRef,
    pub principal_digest: String,
}

impl Principal {
    pub fn new(
        principal_id: PrincipalId,
        kind: PrincipalKind,
        authentication: AuthenticatedPrincipalRef,
        created_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut principal = Self {
            schema: PRINCIPAL_SCHEMA.to_owned(),
            principal_id,
            kind,
            status: PrincipalStatus::Active,
            created_at_unix_ms,
            expires_at_unix_ms: None,
            authentication,
            principal_digest: String::new(),
        };
        principal.principal_digest = principal.digest();
        principal.validate()?;
        Ok(principal)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PRINCIPAL_SCHEMA
            || self.created_at_unix_ms == 0
            || self
                .expires_at_unix_ms
                .is_some_and(|expires| expires <= self.created_at_unix_ms)
        {
            return Err("principal_header_invalid".to_owned());
        }
        self.authentication.validate()?;
        if self.authentication.principal_id != self.principal_id.to_string() {
            return Err("principal_identity_mismatch".to_owned());
        }
        required(&self.authentication.principal_id, "principal_id", 256)?;
        digest(&self.principal_digest, "principal_digest")?;
        if self.principal_digest != self.digest() {
            return Err("principal_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        self.status == PrincipalStatus::Active
            && now_unix_ms >= self.created_at_unix_ms
            && self
                .expires_at_unix_ms
                .is_none_or(|expires| now_unix_ms < expires)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("principal_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipStatus {
    Invited,
    Accepted,
    Active,
    Suspended,
    Expired,
    Revoked,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Membership {
    pub schema: String,
    pub membership_id: crate::MembershipId,
    pub principal_id: PrincipalId,
    pub organization_id: OrganizationId,
    pub status: MembershipStatus,
    pub valid_from_unix_ms: u64,
    #[serde(default)]
    pub expires_at_unix_ms: Option<u64>,
    pub authority_epoch: u64,
    pub membership_digest: String,
}

impl Membership {
    pub fn new(
        membership_id: crate::MembershipId,
        principal_id: PrincipalId,
        organization_id: OrganizationId,
        valid_from_unix_ms: u64,
        authority_epoch: u64,
    ) -> Result<Self, String> {
        let mut membership = Self {
            schema: MEMBERSHIP_SCHEMA.to_owned(),
            membership_id,
            principal_id,
            organization_id,
            status: MembershipStatus::Active,
            valid_from_unix_ms,
            expires_at_unix_ms: None,
            authority_epoch,
            membership_digest: String::new(),
        };
        membership.membership_digest = membership.digest();
        membership.validate()?;
        Ok(membership)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != MEMBERSHIP_SCHEMA
            || self.valid_from_unix_ms == 0
            || self.authority_epoch == 0
            || self
                .expires_at_unix_ms
                .is_some_and(|expires| expires <= self.valid_from_unix_ms)
        {
            return Err("membership_header_invalid".to_owned());
        }
        digest(&self.membership_digest, "membership_digest")?;
        if self.membership_digest != self.digest() {
            return Err("membership_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn active_at(&self, now_unix_ms: u64) -> bool {
        self.status == MembershipStatus::Active
            && now_unix_ms >= self.valid_from_unix_ms
            && self
                .expires_at_unix_ms
                .is_none_or(|expires| now_unix_ms < expires)
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("membership_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    pub schema: String,
    pub store: String,
    pub key: String,
    pub purpose: String,
    pub audience: String,
    pub generation: u64,
    pub reference_digest: String,
}

impl SecretRef {
    pub fn new(
        store: impl Into<String>,
        key: impl Into<String>,
        purpose: impl Into<String>,
        audience: impl Into<String>,
        generation: u64,
    ) -> Result<Self, String> {
        let mut reference = Self {
            schema: SECRET_REF_SCHEMA.to_owned(),
            store: store.into(),
            key: key.into(),
            purpose: purpose.into(),
            audience: audience.into(),
            generation,
            reference_digest: String::new(),
        };
        reference.reference_digest = reference.digest();
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECRET_REF_SCHEMA || self.generation == 0 {
            return Err("secret_ref_header_invalid".to_owned());
        }
        for (value, field, max) in [
            (&self.store, "secret_ref_store", 128),
            (&self.key, "secret_ref_key", 1_024),
            (&self.purpose, "secret_ref_purpose", 256),
            (&self.audience, "secret_ref_audience", 256),
        ] {
            required(value, field, max)?;
            if value.contains(['\n', '\r']) {
                return Err(format!("{field}_invalid"));
            }
        }
        digest(&self.reference_digest, "secret_ref_digest")?;
        if self.reference_digest != self.digest() {
            return Err("secret_ref_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "store": self.store,
            "key": self.key,
            "purpose": self.purpose,
            "audience": self.audience,
            "generation": self.generation,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAccountStatus {
    Active,
    Suspended,
    Revoked,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderAccount {
    pub schema: String,
    pub account_id: ProviderAccountId,
    pub provider_id: String,
    #[serde(default)]
    pub external_subject: Option<String>,
    #[serde(default)]
    pub tenant: Option<String>,
    pub data_boundary: String,
    pub status: ProviderAccountStatus,
    pub revision: u64,
    pub account_digest: String,
}

impl ProviderAccount {
    pub fn new(
        account_id: ProviderAccountId,
        provider_id: impl Into<String>,
        data_boundary: impl Into<String>,
    ) -> Result<Self, String> {
        let mut account = Self {
            schema: PROVIDER_ACCOUNT_SCHEMA.to_owned(),
            account_id,
            provider_id: provider_id.into(),
            external_subject: None,
            tenant: None,
            data_boundary: data_boundary.into(),
            status: ProviderAccountStatus::Active,
            revision: 1,
            account_digest: String::new(),
        };
        account.account_digest = account.digest();
        account.validate()?;
        Ok(account)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROVIDER_ACCOUNT_SCHEMA || self.revision == 0 {
            return Err("provider_account_header_invalid".to_owned());
        }
        required(&self.provider_id, "provider_id", 128)?;
        required(&self.data_boundary, "provider_data_boundary", 256)?;
        for (value, field) in [
            (&self.external_subject, "provider_external_subject"),
            (&self.tenant, "provider_tenant"),
        ] {
            if value
                .as_deref()
                .is_some_and(|value| value.trim().is_empty() || value.len() > 512)
            {
                return Err(format!("{field}_invalid"));
            }
        }
        digest(&self.account_digest, "provider_account_digest")?;
        if self.account_digest != self.digest() {
            return Err("provider_account_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("account_digest".to_owned(), Value::String(String::new()));
        }
        json_digest(&value)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceIdentity {
    pub schema: String,
    pub service_identity_id: ServiceIdentityId,
    pub principal_id: PrincipalId,
    pub credential_ref: SecretRef,
    pub capability_scopes: Vec<String>,
    pub rotation_policy: String,
    pub identity_digest: String,
}

impl ServiceIdentity {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SERVICE_IDENTITY_SCHEMA {
            return Err("service_identity_schema_invalid".to_owned());
        }
        self.credential_ref.validate()?;
        list(&self.capability_scopes, "service_capability_scopes", 128)?;
        required(&self.rotation_policy, "service_rotation_policy", 512)?;
        digest(&self.identity_digest, "service_identity_digest")?;
        if self.identity_digest != self.digest() {
            return Err("service_identity_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "service_identity_id": self.service_identity_id,
            "principal_id": self.principal_id,
            "credential_ref": self.credential_ref,
            "capability_scopes": self.capability_scopes,
            "rotation_policy": self.rotation_policy,
        }))
    }
}

fn contains_raw_secret(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            let normalized = key.to_ascii_lowercase();
            let forbidden_key = matches!(
                normalized.as_str(),
                "secret"
                    | "token"
                    | "password"
                    | "api_key"
                    | "access_token"
                    | "refresh_token"
                    | "authorization"
            );
            let reference_key = normalized.ends_with("_ref")
                || normalized.ends_with("_env")
                || normalized.ends_with("_id");
            (forbidden_key && !reference_key) || contains_raw_secret(value)
        }),
        Value::Array(values) => values.iter().any(contains_raw_secret),
        Value::String(text) => {
            let lower = text.to_ascii_lowercase();
            lower.contains("bearer ") || lower.starts_with("sk-")
        }
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigSnapshot {
    pub schema: String,
    pub version: u64,
    pub source_refs: Vec<String>,
    pub effective_non_secret_config: Value,
    pub config_revision: String,
    pub project_trust_revision: String,
    pub snapshot_digest: String,
}

impl ConfigSnapshot {
    pub fn new(
        source_refs: Vec<String>,
        effective_non_secret_config: Value,
        config_revision: impl Into<String>,
        project_trust_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: CONFIG_SNAPSHOT_SCHEMA.to_owned(),
            version: 1,
            source_refs,
            effective_non_secret_config,
            config_revision: config_revision.into(),
            project_trust_revision: project_trust_revision.into(),
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONFIG_SNAPSHOT_SCHEMA
            || self.version == 0
            || serde_json::to_vec(&self.effective_non_secret_config)
                .map_or(true, |bytes| bytes.len() > 64 * 1024)
            || contains_raw_secret(&self.effective_non_secret_config)
        {
            return Err("config_snapshot_secret_or_size_invalid".to_owned());
        }
        list(&self.source_refs, "config_source_refs", 128)?;
        digest(&self.config_revision, "config_revision")?;
        digest(&self.project_trust_revision, "project_trust_revision")?;
        digest(&self.snapshot_digest, "config_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("config_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "source_refs": self.source_refs,
            "effective_non_secret_config": self.effective_non_secret_config,
            "config_revision": self.config_revision,
            "project_trust_revision": self.project_trust_revision,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritySnapshot {
    pub schema: String,
    pub principal_id: PrincipalId,
    pub organization_id: OrganizationId,
    pub project_id: ProjectId,
    pub session_owner: String,
    pub role_id: String,
    pub department_id: String,
    pub policy_profile: String,
    pub data_boundary: String,
    pub authority_epoch: u64,
    pub assignment_ids: Vec<AssignmentId>,
    pub trust_revision: String,
    pub snapshot_digest: String,
}

impl AuthoritySnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        principal_id: PrincipalId,
        organization_id: OrganizationId,
        project_id: ProjectId,
        session_owner: impl Into<String>,
        role_id: impl Into<String>,
        department_id: impl Into<String>,
        policy_profile: impl Into<String>,
        data_boundary: impl Into<String>,
        authority_epoch: u64,
        assignment_ids: Vec<AssignmentId>,
        trust_revision: impl Into<String>,
    ) -> Result<Self, String> {
        let mut snapshot = Self {
            schema: AUTHORITY_SNAPSHOT_SCHEMA.to_owned(),
            principal_id,
            organization_id,
            project_id,
            session_owner: session_owner.into(),
            role_id: role_id.into(),
            department_id: department_id.into(),
            policy_profile: policy_profile.into(),
            data_boundary: data_boundary.into(),
            authority_epoch,
            assignment_ids,
            trust_revision: trust_revision.into(),
            snapshot_digest: String::new(),
        };
        snapshot.snapshot_digest = snapshot.digest();
        snapshot.validate()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHORITY_SNAPSHOT_SCHEMA
            || self.authority_epoch == 0
            || self.assignment_ids.is_empty()
        {
            return Err("authority_snapshot_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.session_owner, "authority_session_owner"),
            (&self.role_id, "authority_role"),
            (&self.department_id, "authority_department"),
            (&self.policy_profile, "authority_policy_profile"),
            (&self.data_boundary, "authority_data_boundary"),
        ] {
            required(value, field, 512)?;
        }
        if self
            .assignment_ids
            .windows(2)
            .any(|pair| pair[0].as_uuid() >= pair[1].as_uuid())
        {
            return Err("authority_assignment_ids_noncanonical".to_owned());
        }
        digest(&self.trust_revision, "authority_trust_revision")?;
        digest(&self.snapshot_digest, "authority_snapshot_digest")?;
        if self.snapshot_digest != self.digest() {
            return Err("authority_snapshot_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_current_epoch(&self, current_epoch: u64) -> Result<(), String> {
        self.validate()?;
        if current_epoch < self.authority_epoch {
            return Err("authority_epoch_rollback".to_owned());
        }
        if current_epoch != self.authority_epoch {
            return Err("authority_epoch_stale".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "principal_id": self.principal_id,
            "organization_id": self.organization_id,
            "project_id": self.project_id,
            "session_owner": self.session_owner,
            "role_id": self.role_id,
            "department_id": self.department_id,
            "policy_profile": self.policy_profile,
            "data_boundary": self.data_boundary,
            "authority_epoch": self.authority_epoch,
            "assignment_ids": self.assignment_ids,
            "trust_revision": self.trust_revision,
        }))
    }
}

pub fn validate_assignment_id_order(ids: &[AssignmentId]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    if ids.iter().any(|id| !seen.insert(id.as_uuid())) {
        return Err("assignment_ids_duplicate".to_owned());
    }
    if ids
        .windows(2)
        .any(|pair| pair[0].as_uuid() >= pair[1].as_uuid())
    {
        return Err("assignment_ids_noncanonical".to_owned());
    }
    Ok(())
}
