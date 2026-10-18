//! Provider/model/prompt-pack/MCP route attestation contracts.
//!
//! This module is a metadata boundary.  It binds a provider claim to a server-owned route,
//! prompt provenance, data/use policy and opaque credential/account/audience identities.  It
//! never resolves credentials, contacts a provider, verifies a signature or invokes MCP.  The
//! ControlPlane must compare the attested snapshot with its current snapshot immediately before
//! dispatch; a provider or model supplied claim is never an authority.

use crate::{json_digest, ModelProtocol, ModelRoute, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;

pub const ROUTE_ATTESTATION_SCHEMA: &str = "kiana.route-attestation.v1";
pub const ROUTE_ATTESTATION_CONTEXT_SCHEMA: &str = "kiana.route-attestation-context.v1";
pub const ROUTE_ATTESTATION_PROMPT_SCHEMA: &str = "kiana.route-attestation-prompt.v1";
pub const ROUTE_ATTESTATION_MCP_SCHEMA: &str = "kiana.route-attestation-mcp.v1";
pub const ROUTE_ATTESTATION_POLICY_SCHEMA: &str = "kiana.route-attestation-policy.v1";
pub const ROUTE_ATTESTATION_REPORT_SCHEMA: &str = "kiana.route-attestation-report.v1";
pub const ROUTE_ATTESTATION_DOWNSTREAM_TOKEN_SCHEMA: &str =
    "kiana.route-attestation-downstream-token.v1";
pub const ROUTE_ATTESTATION_USE_DECISION_SCHEMA: &str = "kiana.route-attestation-use-decision.v1";
pub const ROUTE_ATTESTATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_ROUTE_ATTESTATION_TEXT_BYTES: usize = 512;
pub const MAX_ROUTE_ATTESTATION_CLASSES: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptPackTrust {
    Product,
    Signed,
    Untrusted,
    Unknown,
    Revoked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpRouteTransport {
    Stdio,
    Http,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderRouteClaim {
    pub provider_id: String,
    pub protocol: ModelProtocol,
    pub connection_id: String,
    pub model_id: String,
    pub profile: String,
    pub configuration_revision: String,
    pub streaming: bool,
    pub route_digest: String,
}

impl ProviderRouteClaim {
    pub fn from_route(route: &ModelRoute) -> Self {
        Self {
            provider_id: route.provider_id.clone(),
            protocol: route.protocol,
            connection_id: route.connection_id.clone(),
            model_id: route.model_id.clone(),
            profile: route.profile.clone(),
            configuration_revision: route.configuration_revision.clone(),
            streaming: route.streaming,
            route_digest: route.digest(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        bounded(&self.provider_id, "route_claim_provider")?;
        bounded(&self.connection_id, "route_claim_connection")?;
        bounded(&self.model_id, "route_claim_model")?;
        bounded(&self.profile, "route_claim_profile")?;
        bounded(
            &self.configuration_revision,
            "route_claim_configuration_revision",
        )?;
        digest(&self.route_digest, "route_claim_digest")?;
        let route = self.to_route();
        if self.route_digest != route.digest() {
            return Err("route_claim_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn to_route(&self) -> ModelRoute {
        ModelRoute {
            provider_id: self.provider_id.clone(),
            protocol: self.protocol,
            connection_id: self.connection_id.clone(),
            model_id: self.model_id.clone(),
            profile: self.profile.clone(),
            configuration_revision: self.configuration_revision.clone(),
            streaming: self.streaming,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptPackAttestation {
    pub schema: String,
    pub version: SchemaVersion,
    pub pack_id: String,
    pub pack_version: String,
    pub content_digest: String,
    pub provenance_digest: String,
    pub trust: PromptPackTrust,
}

impl PromptPackAttestation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pack_id: impl Into<String>,
        pack_version: impl Into<String>,
        content_digest: impl Into<String>,
        provenance_digest: impl Into<String>,
        trust: PromptPackTrust,
    ) -> Result<Self, String> {
        let attestation = Self {
            schema: ROUTE_ATTESTATION_PROMPT_SCHEMA.to_owned(),
            version: ROUTE_ATTESTATION_VERSION,
            pack_id: pack_id.into(),
            pack_version: pack_version.into(),
            content_digest: content_digest.into(),
            provenance_digest: provenance_digest.into(),
            trust,
        };
        attestation.validate()?;
        Ok(attestation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROUTE_ATTESTATION_PROMPT_SCHEMA
            || self.version != ROUTE_ATTESTATION_VERSION
        {
            return Err("route_prompt_header_invalid".to_owned());
        }
        bounded(&self.pack_id, "route_prompt_pack_id")?;
        bounded(&self.pack_version, "route_prompt_pack_version")?;
        digest(&self.content_digest, "route_prompt_content_digest")?;
        digest(&self.provenance_digest, "route_prompt_provenance_digest")
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "pack_id": self.pack_id,
            "pack_version": self.pack_version,
            "content_digest": self.content_digest,
            "provenance_digest": self.provenance_digest,
            "trust": self.trust,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpRouteAttestation {
    pub schema: String,
    pub version: SchemaVersion,
    pub server_id: String,
    pub transport: McpRouteTransport,
    pub tool_catalog_digest: String,
    pub credential_audience: String,
    pub network_audience: String,
    pub enabled: bool,
    pub route_digest: String,
}

impl McpRouteAttestation {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        server_id: impl Into<String>,
        transport: McpRouteTransport,
        tool_catalog_digest: impl Into<String>,
        credential_audience: impl Into<String>,
        network_audience: impl Into<String>,
        enabled: bool,
    ) -> Result<Self, String> {
        let mut attestation = Self {
            schema: ROUTE_ATTESTATION_MCP_SCHEMA.to_owned(),
            version: ROUTE_ATTESTATION_VERSION,
            server_id: server_id.into(),
            transport,
            tool_catalog_digest: tool_catalog_digest.into(),
            credential_audience: credential_audience.into(),
            network_audience: network_audience.into(),
            enabled,
            route_digest: String::new(),
        };
        attestation.route_digest = attestation.digest();
        attestation.validate()?;
        Ok(attestation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROUTE_ATTESTATION_MCP_SCHEMA || self.version != ROUTE_ATTESTATION_VERSION
        {
            return Err("route_mcp_header_invalid".to_owned());
        }
        bounded(&self.server_id, "route_mcp_server")?;
        digest(&self.tool_catalog_digest, "route_mcp_tool_catalog_digest")?;
        bounded(&self.credential_audience, "route_mcp_credential_audience")?;
        bounded(&self.network_audience, "route_mcp_network_audience")?;
        digest(&self.route_digest, "route_mcp_digest")?;
        if self.route_digest != self.digest() {
            return Err("route_mcp_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "server_id": self.server_id,
            "transport": self.transport,
            "tool_catalog_digest": self.tool_catalog_digest,
            "credential_audience": self.credential_audience,
            "network_audience": self.network_audience,
            "enabled": self.enabled,
        }))
    }
}

/// Opaque identity of the token minted for the downstream provider or MCP call.
///
/// Only the reference digest and the exact server-owned account/audience/purpose scope are
/// represented: never the token value.  A token minted for another account, audience or
/// purpose is structurally invalid for this context, so a reused downstream token cannot be
/// re-labelled on top of an otherwise valid route snapshot.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DownstreamTokenAttestation {
    pub schema: String,
    pub version: SchemaVersion,
    pub token_ref_digest: String,
    pub account_id: String,
    pub audience: String,
    pub purpose: String,
    pub token_scope_digest: String,
}

impl DownstreamTokenAttestation {
    pub fn new(
        token_ref_digest: impl Into<String>,
        account_id: impl Into<String>,
        audience: impl Into<String>,
        purpose: impl Into<String>,
    ) -> Result<Self, String> {
        let mut token = Self {
            schema: ROUTE_ATTESTATION_DOWNSTREAM_TOKEN_SCHEMA.to_owned(),
            version: ROUTE_ATTESTATION_VERSION,
            token_ref_digest: token_ref_digest.into(),
            account_id: account_id.into(),
            audience: audience.into(),
            purpose: purpose.into(),
            token_scope_digest: String::new(),
        };
        token.token_scope_digest = token.digest();
        token.validate()?;
        Ok(token)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROUTE_ATTESTATION_DOWNSTREAM_TOKEN_SCHEMA
            || self.version != ROUTE_ATTESTATION_VERSION
        {
            return Err("route_downstream_token_header_invalid".to_owned());
        }
        digest(&self.token_ref_digest, "route_downstream_token_ref_digest")?;
        bounded(&self.account_id, "route_downstream_token_account")?;
        bounded(&self.audience, "route_downstream_token_audience")?;
        bounded(&self.purpose, "route_downstream_token_purpose")?;
        digest(
            &self.token_scope_digest,
            "route_downstream_token_scope_digest",
        )?;
        if self.token_scope_digest != self.digest() {
            return Err("route_downstream_token_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "token_ref_digest": self.token_ref_digest,
            "account_id": self.account_id,
            "audience": self.audience,
            "purpose": self.purpose,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDataPolicyBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub policy_digest: String,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub purpose: String,
    pub allowed_classes: BTreeSet<String>,
    pub use_policy_digest: String,
}

impl RouteDataPolicyBinding {
    pub fn new(
        policy_digest: impl Into<String>,
        policy_revision: u64,
        data_epoch: u64,
        purpose: impl Into<String>,
        allowed_classes: BTreeSet<String>,
    ) -> Result<Self, String> {
        let mut binding = Self {
            schema: ROUTE_ATTESTATION_POLICY_SCHEMA.to_owned(),
            version: ROUTE_ATTESTATION_VERSION,
            policy_digest: policy_digest.into(),
            policy_revision,
            data_epoch,
            purpose: purpose.into(),
            allowed_classes,
            use_policy_digest: String::new(),
        };
        binding.use_policy_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROUTE_ATTESTATION_POLICY_SCHEMA
            || self.version != ROUTE_ATTESTATION_VERSION
        {
            return Err("route_policy_header_invalid".to_owned());
        }
        digest(&self.policy_digest, "route_policy_digest")?;
        if self.data_epoch == 0 || self.allowed_classes.len() > MAX_ROUTE_ATTESTATION_CLASSES {
            return Err("route_policy_scope_invalid".to_owned());
        }
        bounded(&self.purpose, "route_policy_purpose")?;
        for class in &self.allowed_classes {
            if !is_data_class(class) {
                return Err("route_policy_data_class_invalid".to_owned());
            }
        }
        digest(&self.use_policy_digest, "route_policy_use_digest")?;
        if self.use_policy_digest != self.digest() {
            return Err("route_policy_use_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "policy_digest": self.policy_digest,
            "policy_revision": self.policy_revision,
            "data_epoch": self.data_epoch,
            "purpose": self.purpose,
            "allowed_classes": self.allowed_classes,
        }))
    }

    /// Decide whether a requested purpose and data-class set stay inside this binding.
    ///
    /// The decision is pure: it never resolves data, contacts a provider or mutates the
    /// binding.  A refusal is a structured, digest-bound report with stable reason codes, so
    /// callers must treat anything other than `allowed` as zero effect.
    pub fn authorize_use(
        &self,
        purpose: &str,
        classes: &BTreeSet<String>,
    ) -> Result<RouteDataUseDecision, String> {
        self.validate()?;
        let reason = if purpose.trim().is_empty() || purpose != self.purpose {
            "route_attestation_purpose_not_permitted"
        } else if classes.is_empty() || classes.len() > MAX_ROUTE_ATTESTATION_CLASSES {
            "route_attestation_data_classes_invalid"
        } else if classes.iter().any(|class| !is_data_class(class)) {
            "route_attestation_data_class_unknown"
        } else if classes
            .iter()
            .any(|class| !self.allowed_classes.contains(class))
        {
            "route_attestation_data_class_not_permitted"
        } else {
            "ok"
        };
        RouteDataUseDecision::new(self, reason, classes)
    }
}

/// Digest-bound outcome of [`RouteDataPolicyBinding::authorize_use`].
///
/// `allowed=false` carries a stable reason and is the only value a caller may treat as a
/// refusal; `allowed=true` always carries `"ok"`.  The decision binds the policy revision,
/// data epoch, requested classes and permitted classes so evidence cannot be re-labelled.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDataUseDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub allowed: bool,
    pub reason: String,
    pub purpose: String,
    pub requested_classes: BTreeSet<String>,
    pub permitted_classes: BTreeSet<String>,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub decision_digest: String,
}

impl RouteDataUseDecision {
    fn new(
        binding: &RouteDataPolicyBinding,
        reason: &str,
        requested_classes: &BTreeSet<String>,
    ) -> Result<Self, String> {
        let mut decision = Self {
            schema: ROUTE_ATTESTATION_USE_DECISION_SCHEMA.to_owned(),
            version: ROUTE_ATTESTATION_VERSION,
            allowed: reason == "ok",
            reason: reason.to_owned(),
            purpose: binding.purpose.clone(),
            requested_classes: requested_classes.clone(),
            permitted_classes: binding.allowed_classes.clone(),
            policy_revision: binding.policy_revision,
            data_epoch: binding.data_epoch,
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate()?;
        Ok(decision)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROUTE_ATTESTATION_USE_DECISION_SCHEMA
            || self.version != ROUTE_ATTESTATION_VERSION
        {
            return Err("route_use_decision_header_invalid".to_owned());
        }
        if self.data_epoch == 0 {
            return Err("route_use_decision_epoch_invalid".to_owned());
        }
        bounded(&self.purpose, "route_use_decision_purpose")?;
        bounded(&self.reason, "route_use_decision_reason")?;
        if self.allowed != (self.reason == "ok") {
            return Err("route_use_decision_state_invalid".to_owned());
        }
        if !self.allowed && !self.reason.starts_with("route_attestation_") {
            return Err("route_use_decision_reason_invalid".to_owned());
        }
        // A refusal records the oversized request it refused (that is what
        // `route_attestation_data_classes_invalid` reports); an allowance is held to the bound.
        if (self.allowed && self.requested_classes.len() > MAX_ROUTE_ATTESTATION_CLASSES)
            || self.permitted_classes.len() > MAX_ROUTE_ATTESTATION_CLASSES
        {
            return Err("route_use_decision_scope_invalid".to_owned());
        }
        // The permitted set comes from a validated policy and is always a known class. A refusal
        // may legitimately echo the requested class it refused (that is how
        // `route_attestation_data_class_unknown` and an oversized set stay reportable), so for a
        // refusal the requested labels only need to be bounded. An allowance may only ever bind
        // known classes.
        for class in &self.permitted_classes {
            if !is_data_class(class) {
                return Err("route_use_decision_data_class_invalid".to_owned());
            }
        }
        for class in &self.requested_classes {
            if self.allowed {
                if !is_data_class(class) {
                    return Err("route_use_decision_data_class_invalid".to_owned());
                }
            } else {
                bounded(class, "route_use_decision_data_class")?;
            }
        }
        digest(&self.decision_digest, "route_use_decision_digest")?;
        if self.decision_digest != self.digest() {
            return Err("route_use_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "allowed": self.allowed,
            "reason": self.reason,
            "purpose": self.purpose,
            "requested_classes": self.requested_classes,
            "permitted_classes": self.permitted_classes,
            "policy_revision": self.policy_revision,
            "data_epoch": self.data_epoch,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteAttestationContext {
    pub schema: String,
    pub version: SchemaVersion,
    pub route: ModelRoute,
    pub route_digest: String,
    pub prompt_pack: PromptPackAttestation,
    #[serde(default)]
    pub mcp_route: Option<McpRouteAttestation>,
    pub data_policy: RouteDataPolicyBinding,
    pub credential_ref_digest: String,
    pub account_id: String,
    pub audience: String,
    #[serde(default)]
    pub downstream_token: Option<DownstreamTokenAttestation>,
    pub release_manifest_digest: String,
    pub release_signature_attestation_digest: String,
}

impl RouteAttestationContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        route: ModelRoute,
        prompt_pack: PromptPackAttestation,
        mcp_route: Option<McpRouteAttestation>,
        data_policy: RouteDataPolicyBinding,
        credential_ref_digest: impl Into<String>,
        account_id: impl Into<String>,
        audience: impl Into<String>,
        release_manifest_digest: impl Into<String>,
        release_signature_attestation_digest: impl Into<String>,
    ) -> Result<Self, String> {
        let context = Self {
            schema: ROUTE_ATTESTATION_CONTEXT_SCHEMA.to_owned(),
            version: ROUTE_ATTESTATION_VERSION,
            route_digest: route.digest(),
            route,
            prompt_pack,
            mcp_route,
            data_policy,
            credential_ref_digest: credential_ref_digest.into(),
            account_id: account_id.into(),
            audience: audience.into(),
            downstream_token: None,
            release_manifest_digest: release_manifest_digest.into(),
            release_signature_attestation_digest: release_signature_attestation_digest.into(),
        };
        context.validate()?;
        Ok(context)
    }

    /// Attach the opaque downstream token identity and re-run the cross-field binding.
    ///
    /// The token must have been minted for this context's exact account, audience and data
    /// policy purpose; a token reused from another route is refused here, before any
    /// provider or MCP dispatch.
    pub fn with_downstream_token(
        mut self,
        downstream_token: DownstreamTokenAttestation,
    ) -> Result<Self, String> {
        self.downstream_token = Some(downstream_token);
        self.validate()?;
        Ok(self)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROUTE_ATTESTATION_CONTEXT_SCHEMA
            || self.version != ROUTE_ATTESTATION_VERSION
        {
            return Err("route_attestation_context_header_invalid".to_owned());
        }
        validate_route(&self.route)?;
        digest(&self.route_digest, "route_attestation_route_digest")?;
        if self.route_digest != self.route.digest() {
            return Err("route_attestation_route_digest_mismatch".to_owned());
        }
        self.prompt_pack.validate()?;
        if let Some(mcp_route) = &self.mcp_route {
            mcp_route.validate()?;
        }
        self.data_policy.validate()?;
        digest(
            &self.credential_ref_digest,
            "route_attestation_credential_digest",
        )?;
        bounded(&self.account_id, "route_attestation_account")?;
        bounded(&self.audience, "route_attestation_audience")?;
        if let Some(token) = &self.downstream_token {
            token.validate()?;
            if token.account_id != self.account_id {
                return Err("route_attestation_downstream_token_account_mismatch".to_owned());
            }
            if token.audience != self.audience {
                return Err("route_attestation_downstream_token_audience_mismatch".to_owned());
            }
            if token.purpose != self.data_policy.purpose {
                return Err("route_attestation_downstream_token_purpose_mismatch".to_owned());
            }
        }
        digest(
            &self.release_manifest_digest,
            "route_attestation_release_manifest_digest",
        )?;
        digest(
            &self.release_signature_attestation_digest,
            "route_attestation_release_signature_digest",
        )
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "route": self.route,
            "route_digest": self.route_digest,
            "prompt_pack": self.prompt_pack,
            "mcp_route": self.mcp_route,
            "data_policy": self.data_policy,
            "credential_ref_digest": self.credential_ref_digest,
            "account_id": self.account_id,
            "audience": self.audience,
            "downstream_token": self.downstream_token,
            "release_manifest_digest": self.release_manifest_digest,
            "release_signature_attestation_digest": self.release_signature_attestation_digest,
        }))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteAttestation {
    pub schema: String,
    pub version: SchemaVersion,
    pub context: RouteAttestationContext,
    pub provider_claim: ProviderRouteClaim,
    pub provider_verified: bool,
    pub attestation_digest: String,
}

impl RouteAttestation {
    pub fn new(
        context: RouteAttestationContext,
        provider_claim: ProviderRouteClaim,
        provider_verified: bool,
    ) -> Result<Self, String> {
        let mut attestation = Self {
            schema: ROUTE_ATTESTATION_SCHEMA.to_owned(),
            version: ROUTE_ATTESTATION_VERSION,
            context,
            provider_claim,
            provider_verified,
            attestation_digest: String::new(),
        };
        attestation.attestation_digest = attestation.digest();
        attestation.validate()?;
        Ok(attestation)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROUTE_ATTESTATION_SCHEMA || self.version != ROUTE_ATTESTATION_VERSION {
            return Err("route_attestation_header_invalid".to_owned());
        }
        self.context.validate()?;
        self.provider_claim.validate()?;
        digest(&self.attestation_digest, "route_attestation_digest")?;
        if self.attestation_digest != self.digest() {
            return Err("route_attestation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "context": self.context,
            "provider_claim": self.provider_claim,
            "provider_verified": self.provider_verified,
        }))
    }

    /// Compare a provider/model claim with the server-owned admission snapshot.
    ///
    /// The comparison order is fixed and deny-first:
    /// route → provider claim → prompt pack equality → prompt trust (untrusted/revoked →
    /// blocked, unknown → unknown) → MCP route equality → MCP transport → MCP enabled →
    /// data policy → credential → account → audience → downstream token (missing, then
    /// mismatch) → release binding → provider verification → `ok`.
    ///
    /// The report is structured so callers can preserve `Unknown` separately from a hard
    /// mismatch.  This function never performs a provider call, resolves a credential or invokes
    /// an MCP server.
    pub fn verify(
        &self,
        expected: &RouteAttestationContext,
    ) -> Result<RouteVerificationReport, String> {
        self.validate()?;
        expected.validate()?;
        let expected_claim = ProviderRouteClaim::from_route(&expected.route);
        let reason = if self.context.route != expected.route
            || self.context.route_digest != expected.route_digest
        {
            "route_attestation_route_mismatch"
        } else if self.provider_claim != expected_claim {
            "route_attestation_provider_claim_mismatch"
        } else if self.context.prompt_pack != expected.prompt_pack {
            "route_attestation_prompt_pack_mismatch"
        } else if matches!(
            self.context.prompt_pack.trust,
            PromptPackTrust::Untrusted | PromptPackTrust::Revoked
        ) {
            "route_attestation_prompt_pack_untrusted"
        } else if self.context.prompt_pack.trust == PromptPackTrust::Unknown {
            "route_attestation_prompt_pack_unknown"
        } else if self.context.mcp_route != expected.mcp_route {
            "route_attestation_mcp_route_mismatch"
        } else if self
            .context
            .mcp_route
            .as_ref()
            .is_some_and(|route| route.transport == McpRouteTransport::Http)
        {
            "route_attestation_mcp_transport_unsupported"
        } else if self
            .context
            .mcp_route
            .as_ref()
            .is_some_and(|route| !route.enabled)
        {
            "route_attestation_mcp_disabled"
        } else if self.context.data_policy != expected.data_policy {
            "route_attestation_data_policy_mismatch"
        } else if self.context.credential_ref_digest != expected.credential_ref_digest {
            "route_attestation_credential_mismatch"
        } else if self.context.account_id != expected.account_id {
            "route_attestation_account_mismatch"
        } else if self.context.audience != expected.audience {
            "route_attestation_audience_mismatch"
        } else if expected.downstream_token.is_some() && self.context.downstream_token.is_none() {
            "route_attestation_downstream_token_missing"
        } else if self.context.downstream_token != expected.downstream_token {
            "route_attestation_downstream_token_mismatch"
        } else if self.context.release_manifest_digest != expected.release_manifest_digest
            || self.context.release_signature_attestation_digest
                != expected.release_signature_attestation_digest
        {
            "route_attestation_release_binding_mismatch"
        } else if !self.provider_verified {
            "route_attestation_provider_unverified"
        } else {
            "ok"
        };

        let status = match reason {
            "ok" => RouteVerificationStatus::Verified,
            "route_attestation_prompt_pack_unknown" | "route_attestation_provider_unverified" => {
                RouteVerificationStatus::Unknown
            }
            _ => RouteVerificationStatus::Blocked,
        };
        RouteVerificationReport::new(
            status,
            self.attestation_digest.clone(),
            self.context.route_digest.clone(),
            self.context.prompt_pack.content_digest.clone(),
            self.context
                .mcp_route
                .as_ref()
                .map(|route| route.route_digest.clone()),
            self.context.data_policy.policy_digest.clone(),
            reason,
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteVerificationStatus {
    Verified,
    Blocked,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteVerificationReport {
    pub schema: String,
    pub version: SchemaVersion,
    pub status: RouteVerificationStatus,
    pub attestation_digest: String,
    pub route_digest: String,
    pub prompt_pack_digest: String,
    #[serde(default)]
    pub mcp_route_digest: Option<String>,
    pub data_policy_digest: String,
    pub reason: String,
    pub report_digest: String,
}

impl RouteVerificationReport {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        status: RouteVerificationStatus,
        attestation_digest: String,
        route_digest: String,
        prompt_pack_digest: String,
        mcp_route_digest: Option<String>,
        data_policy_digest: String,
        reason: impl Into<String>,
    ) -> Result<Self, String> {
        let mut report = Self {
            schema: ROUTE_ATTESTATION_REPORT_SCHEMA.to_owned(),
            version: ROUTE_ATTESTATION_VERSION,
            status,
            attestation_digest,
            route_digest,
            prompt_pack_digest,
            mcp_route_digest,
            data_policy_digest,
            reason: reason.into(),
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != ROUTE_ATTESTATION_REPORT_SCHEMA
            || self.version != ROUTE_ATTESTATION_VERSION
        {
            return Err("route_report_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.attestation_digest, "route_report_attestation_digest"),
            (&self.route_digest, "route_report_route_digest"),
            (&self.prompt_pack_digest, "route_report_prompt_digest"),
            (&self.data_policy_digest, "route_report_policy_digest"),
            (&self.report_digest, "route_report_digest"),
        ] {
            digest(value, field)?;
        }
        if let Some(digest_value) = &self.mcp_route_digest {
            digest(digest_value, "route_report_mcp_digest")?;
        }
        bounded(&self.reason, "route_report_reason")?;
        if self.report_digest != self.digest() {
            return Err("route_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "status": self.status,
            "attestation_digest": self.attestation_digest,
            "route_digest": self.route_digest,
            "prompt_pack_digest": self.prompt_pack_digest,
            "mcp_route_digest": self.mcp_route_digest,
            "data_policy_digest": self.data_policy_digest,
            "reason": self.reason,
        }))
    }
}

fn validate_route(route: &ModelRoute) -> Result<(), String> {
    bounded(&route.provider_id, "route_provider")?;
    bounded(&route.connection_id, "route_connection")?;
    bounded(&route.model_id, "route_model")?;
    bounded(&route.profile, "route_profile")?;
    bounded(
        &route.configuration_revision,
        "route_configuration_revision",
    )
}

fn is_data_class(value: &str) -> bool {
    matches!(value, "public" | "internal" | "confidential" | "restricted")
}

fn bounded(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_ROUTE_ATTESTATION_TEXT_BYTES
        || value.contains(['\0', '\r', '\n'])
    {
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
