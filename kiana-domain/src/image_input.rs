//! Controlled image input admission for model requests.
//!
//! An image reaches this boundary only after an Artifact adapter has read bytes inside the
//! caller's already-authorized scope.  The provider receives the admitted bytes through this
//! value; it never receives a path, URL, or a capability to open a file.  This type is deliberately
//! not serializable: raw image bytes and a short lived admission must not become a receipt or an
//! event payload.

use crate::{
    journal_sha256, ArtifactRef, CapabilitySupport, DataClass, ModelCapabilities, ModelError,
    ModelProtocol, ModelRoute, ProcessingGrant,
};
use std::fmt;

pub const IMAGE_INPUT_SCHEMA: &str = "kiana.image-input.v1";
pub const MAX_IMAGE_MEDIA_TYPE_BYTES: usize = 128;
pub const MAX_IMAGE_ATTACHMENT_REF_BYTES: usize = 512;
pub const MAX_IMAGE_SOURCE_BYTES: usize = 16 * 1024 * 1024;

/// Limits are applied to the final wire representation as well as the source artifact.
/// `max_encoded_bytes` is intentionally separate so base64 expansion cannot bypass a body limit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImagePayloadLimits {
    pub max_source_bytes: usize,
    pub max_encoded_bytes: usize,
    pub max_request_bytes: usize,
}

impl Default for ImagePayloadLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: MAX_IMAGE_SOURCE_BYTES,
            max_encoded_bytes: 8 * 1024 * 1024,
            max_request_bytes: 8 * 1024 * 1024,
        }
    }
}

impl ImagePayloadLimits {
    pub fn for_request(max_request_bytes: usize) -> Self {
        Self {
            max_request_bytes,
            max_encoded_bytes: max_request_bytes,
            ..Self::default()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.max_source_bytes == 0
            || self.max_source_bytes > MAX_IMAGE_SOURCE_BYTES
            || self.max_encoded_bytes == 0
            || self.max_request_bytes == 0
        {
            return Err("image_payload_limits_invalid".to_owned());
        }
        Ok(())
    }
}

/// Return the canonical digest used by ArtifactRef and ProcessingGrant for image bytes.
pub fn image_payload_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", journal_sha256(bytes))
}

/// Validate a model attachment reference before any provider or filesystem adapter sees it.
/// Remote URLs and arbitrary paths are intentionally different failures from an unknown Artifact.
pub fn validate_image_attachment_ref(value: &str) -> Result<(), String> {
    if value.contains("://") {
        return Err("remote_image_url_is_not_fetched_implicitly".to_owned());
    }
    if !value.starts_with("artifact:") {
        return Err("untrusted_image_path_never_reaches_provider".to_owned());
    }
    if value.len() > MAX_IMAGE_ATTACHMENT_REF_BYTES || value == "artifact:" {
        return Err("model_content_attachment_invalid".to_owned());
    }
    Ok(())
}

/// A short lived, server-created admission for one immutable image Artifact.
///
/// `source_bytes` is private on purpose.  The only way to create this value is to provide bytes
/// together with the Artifact and its ProcessingGrant; a provider cannot turn a source path or a
/// URL into an image by itself.
pub struct ImageInputAdmission {
    pub schema: String,
    pub artifact: ArtifactRef,
    pub grant: ProcessingGrant,
    pub source_path: String,
    pub media_type: String,
    pub policy_digest: String,
    pub policy_revision: u64,
    pub data_epoch: u64,
    pub purpose_id: String,
    pub allowed_data_classes: Vec<DataClass>,
    pub route: ModelRoute,
    pub capabilities: ModelCapabilities,
    pub admitted_at_unix_ms: u64,
    source_bytes: Vec<u8>,
}

impl fmt::Debug for ImageInputAdmission {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ImageInputAdmission")
            .field("schema", &self.schema)
            .field("artifact", &self.artifact)
            .field("grant_id", &self.grant.id)
            .field("source_path", &self.source_path)
            .field("media_type", &self.media_type)
            .field("policy_digest", &self.policy_digest)
            .field("policy_revision", &self.policy_revision)
            .field("data_epoch", &self.data_epoch)
            .field("purpose_id", &self.purpose_id)
            .field("route", &self.route)
            .field("source_size_bytes", &self.source_bytes.len())
            .finish()
    }
}

impl Clone for ImageInputAdmission {
    fn clone(&self) -> Self {
        Self {
            schema: self.schema.clone(),
            artifact: self.artifact.clone(),
            grant: self.grant.clone(),
            source_path: self.source_path.clone(),
            media_type: self.media_type.clone(),
            policy_digest: self.policy_digest.clone(),
            policy_revision: self.policy_revision,
            data_epoch: self.data_epoch,
            purpose_id: self.purpose_id.clone(),
            allowed_data_classes: self.allowed_data_classes.clone(),
            route: self.route.clone(),
            capabilities: self.capabilities.clone(),
            admitted_at_unix_ms: self.admitted_at_unix_ms,
            source_bytes: self.source_bytes.clone(),
        }
    }
}

impl ImageInputAdmission {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        artifact: ArtifactRef,
        grant: ProcessingGrant,
        source_path: impl Into<String>,
        media_type: impl Into<String>,
        source_bytes: Vec<u8>,
        route: ModelRoute,
        capabilities: ModelCapabilities,
        policy_digest: impl Into<String>,
        policy_revision: u64,
        data_epoch: u64,
        purpose_id: impl Into<String>,
        allowed_data_classes: Vec<DataClass>,
        admitted_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let admission = Self {
            schema: IMAGE_INPUT_SCHEMA.to_owned(),
            artifact,
            grant,
            source_path: source_path.into(),
            media_type: media_type.into(),
            policy_digest: policy_digest.into(),
            policy_revision,
            data_epoch,
            purpose_id: purpose_id.into(),
            allowed_data_classes,
            route,
            capabilities,
            admitted_at_unix_ms,
            source_bytes,
        };
        admission.validate_for_dispatch(
            &admission.route,
            &admission.capabilities,
            admission.admitted_at_unix_ms,
            &ImagePayloadLimits::default(),
        )?;
        Ok(admission)
    }

    pub fn payload_bytes(&self) -> &[u8] {
        &self.source_bytes
    }

    pub fn source_size_bytes(&self) -> usize {
        self.source_bytes.len()
    }

    pub fn source_digest(&self) -> String {
        image_payload_digest(&self.source_bytes)
    }

    pub fn attachment_ref(&self) -> String {
        format!("artifact:{}:{}", self.artifact.artifact_id, self.artifact.version)
    }

    pub fn matches_attachment(
        &self,
        artifact_ref: &str,
        media_type: &str,
        digest: &str,
    ) -> bool {
        (artifact_ref == self.attachment_ref()
            || artifact_ref == format!("artifact:{}", self.artifact.artifact_id))
            && media_type == self.media_type
            && digest == self.artifact.content_hash
    }

    pub fn validate_for_dispatch(
        &self,
        route: &ModelRoute,
        capabilities: &ModelCapabilities,
        now_unix_ms: u64,
        limits: &ImagePayloadLimits,
    ) -> Result<(), String> {
        limits.validate()?;
        self.artifact.validate()?;
        self.grant.validate()?;
        if self.schema != IMAGE_INPUT_SCHEMA {
            return Err("image_input_schema_invalid".to_owned());
        }
        validate_image_attachment_ref(&self.attachment_ref())?;
        if self.source_path != self.grant.source_path
            || crate::normalize_role_path(&self.source_path).as_deref()
                != Some(self.source_path.as_str())
            || self.source_path.contains("://")
        {
            return Err("untrusted_image_path_never_reaches_provider".to_owned());
        }
        if self.grant.revoked {
            return Err("revoked_artifact_grant_blocks_send".to_owned());
        }
        if self.grant.content_hash != self.artifact.content_hash
            || self.source_digest() != self.artifact.content_hash
        {
            return Err("image_artifact_hash_mismatch".to_owned());
        }
        if self.source_bytes.is_empty() || self.source_bytes.len() > limits.max_source_bytes {
            return Err("image_source_payload_limit".to_owned());
        }
        if self.artifact.artifact_schema.starts_with("image/")
            && self.artifact.artifact_schema != self.media_type
        {
            return Err("image_media_type_mismatch".to_owned());
        }
        if self.media_type.len() > MAX_IMAGE_MEDIA_TYPE_BYTES {
            return Err("image_media_type_unsupported".to_owned());
        }
        if !matches!(
            self.media_type.as_str(),
            "image/png" | "image/jpeg" | "image/webp" | "image/gif"
        ) {
            return Err(match self.media_type.as_str() {
                "application/pdf" | "text/plain" | "application/msword"
                | "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                    "unsupported_document_upload"
                }
                value if value.starts_with("audio/") || value.starts_with("video/") => {
                    "unsupported_audio_video_input"
                }
                _ => "image_media_type_unsupported",
            }
            .to_owned());
        }
        if self.policy_revision == 0 || self.data_epoch == 0 || self.admitted_at_unix_ms == 0 {
            return Err("image_data_policy_binding_invalid".to_owned());
        }
        if !self.policy_digest.starts_with("sha256:")
            || self.policy_digest.len() != 71
            || !self.policy_digest[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("image_data_policy_binding_invalid".to_owned());
        }
        if self.grant.retention.expires_at_ms.is_some_and(|expiry| {
            expiry <= now_unix_ms
        }) {
            return Err("image_processing_grant_expired".to_owned());
        }
        if self.purpose_id != self.grant.purpose.id
            || !self
                .allowed_data_classes
                .iter()
                .any(|class| *class == self.grant.class)
        {
            return Err("image_connection_data_scope_denied".to_owned());
        }
        if route != &self.route || route.protocol == ModelProtocol::Legacy {
            return Err("image_route_scope_mismatch".to_owned());
        }
        if capabilities.images != CapabilitySupport::Supported {
            return Err("model_image_capability_unsupported".to_owned());
        }
        Ok(())
    }

    pub fn validate_for_route(
        &self,
        route: &ModelRoute,
        capabilities: &ModelCapabilities,
        now_unix_ms: u64,
        limits: &ImagePayloadLimits,
    ) -> Result<(), ModelError> {
        self.validate_for_dispatch(route, capabilities, now_unix_ms, limits)
            .map_err(ModelError::invalid)
    }
}
