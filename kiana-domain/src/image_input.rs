//! 模型请求图片输入的受控准入合同，属于 `kiana-domain`。
//!
//! 【预期调用链】
//! ```text
//! Artifact/data-governance adapter 在已授权范围内读取 bytes
//!     -> ImageInputAdmission 绑定 ArtifactRef + ProcessingGrant + route/policy/data scope
//!     -> ProviderGateway::prepare_call_with_images()
//!     -> provider request compiler 将 bytes 编码成协议图片块并检查 wire body 预算
//! ```
//! Provider 层只拿 admission 中的 bytes，不拿文件路径、URL 或打开文件的能力；普通请求缺少匹配 admission 时应在网络发送前拒绝。
//! 当前仓内没有发现生产代码调用 `ImageInputAdmission::new()` 或 `prepare_call_with_images()`，该路径由 Domain fixture、Provider 单测和静态 guard 描述。
//! admission 不实现文件读取、授权查询、图片解码或网络发送；它是不可序列化的短期数据，不应进入 Receipt/Event payload。
//! `policy_revision`、`data_epoch` 和摘要仍是调用方 supplied metadata，不会由本合同回读权威策略/数据治理存储。

use crate::{
    journal_sha256, ArtifactRef, CapabilitySupport, DataClass, ModelCapabilities, ModelError,
    ModelProtocol, ModelRoute, ProcessingGrant,
};
use std::fmt;

/// 图片 admission 使用的固定 schema 标签；admission 对象本身不实现序列化。
pub const IMAGE_INPUT_SCHEMA: &str = "kiana.image-input.v1";
/// media type 标签的最大字节数，防止异常长 MIME 文本扩大请求 metadata。
pub const MAX_IMAGE_MEDIA_TYPE_BYTES: usize = 128;
/// AttachmentRef 的最大字节数；引用保留短 Artifact identity，不接受任意长路径/URL。
pub const MAX_IMAGE_ATTACHMENT_REF_BYTES: usize = 512;
/// 单份原始图片 bytes 的绝对上限，避免 caller 自定义预算无限放大内存。
pub const MAX_IMAGE_SOURCE_BYTES: usize = 16 * 1024 * 1024;

/// 原始图片、base64 编码结果和最终请求体分别使用的大小预算。
///
/// 原图大小与 wire 大小不是同一数值：base64 会扩张内容，所以不能只限制 source bytes。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImagePayloadLimits {
    /// 解码前 Artifact bytes 的上限。
    pub max_source_bytes: usize,
    /// base64 编码后图片 payload 的上限。
    pub max_encoded_bytes: usize,
    /// 图片块加入请求 JSON 后的最终 body 上限。
    pub max_request_bytes: usize,
}

impl Default for ImagePayloadLimits {
    /// 使用最多 16 MiB 原图、8 MiB 编码 payload 和 8 MiB 请求体预算。
    /// 编码/请求上限更低，避免大原图经过 base64 后突破默认 HTTP body 预算。
    fn default() -> Self {
        Self {
            max_source_bytes: MAX_IMAGE_SOURCE_BYTES,
            max_encoded_bytes: 8 * 1024 * 1024,
            max_request_bytes: 8 * 1024 * 1024,
        }
    }
}

impl ImagePayloadLimits {
    /// 将 provider 的请求体限制同步应用于编码 payload 与整个 body。
    ///
    /// 保留默认原始 Artifact 硬上限；调用方仍需执行 `validate()` 并在编码后检查实际长度。
    pub fn for_request(max_request_bytes: usize) -> Self {
        Self {
            max_request_bytes,
            max_encoded_bytes: max_request_bytes,
            ..Self::default()
        }
    }

    /// 检查各预算非零，且 source 上限不超过本模块的 16 MiB 硬上限。
    /// encoded/request 两项可由调用方设置，真实编码后还必须比较实际 byte 数。
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

/// 对原始图片 bytes 计算 ArtifactRef/ProcessingGrant 共用的内容摘要。
///
/// 摘要用于确认读出的 bytes 与准入记录一致；它不证明 bytes 是可解码图片或由可信 store 提供。
pub fn image_payload_digest(bytes: &[u8]) -> String {
    format!("sha256:{}", journal_sha256(bytes))
}

/// 在 Artifact adapter 或 Provider 处理引用之前拒绝 URL 和任意路径形式。
///
/// 带 `://` 的 URL 返回 `remote_image_url_is_not_fetched_implicitly`；非 `artifact:` 字符串返回不可信路径错误；
/// 空 artifact id 或引用过长返回 `model_content_attachment_invalid`。本函数只验外形，不解析 ID/version 或访问网络/文件系统。
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

/// 将一份图片 Artifact、处理授权和 Provider route 绑定在一起的短期准入值。
///
/// 构造时必须同时提供 ArtifactRef、ProcessingGrant 和实际 bytes；private `source_bytes` 让 Provider 不能从路径/URL自行打开内容。
/// 本类型刻意不实现 Serialize/Deserialize，Debug 实现只显示 byte 数量，不打印图片正文。
/// ⚠ 其余 metadata 字段是 public 且可变的；构造器只验证创建时的值，跨信任边界使用前仍必须重新校验，不能把此 Rust 值当作不可伪造签名票据。
pub struct ImageInputAdmission {
    /// 固定 admission schema 标签。
    pub schema: String,
    /// 图片 Artifact 的稳定 ID、版本、schema 和内容 hash。
    pub artifact: ArtifactRef,
    /// 允许某 purpose 使用该内容的治理授权快照。
    pub grant: ProcessingGrant,
    /// 授权边界实际读取内容时使用的相对来源路径。
    pub source_path: String,
    /// 调用方声明并与 Artifact image schema 比较的 MIME 类型。
    pub media_type: String,
    /// 数据策略版本的摘要引用；这里只检查格式，不回读策略 store。
    pub policy_digest: String,
    /// 数据策略版本号；这里只要求非零，不比较当前策略版本。
    pub policy_revision: u64,
    /// 数据治理 epoch；这里只要求非零，不查询当前 epoch。
    pub data_epoch: u64,
    /// 允许处理数据的业务 purpose ID，应与 ProcessingGrant 一致。
    pub purpose_id: String,
    /// 此用途允许的 DataClass 集合，必须包含 grant.class。
    pub allowed_data_classes: Vec<DataClass>,
    /// 此 admission 绑定的 provider/model/connection/protocol route。
    pub route: ModelRoute,
    /// 创建 admission 时观察到的模型能力快照；派发校验还会检查调用方传入的当前 capability。
    pub capabilities: ModelCapabilities,
    /// admission 创建时间，Unix epoch 毫秒；验证时由调用方传入 now 进行 retention 比较。
    pub admitted_at_unix_ms: u64,
    /// 实际图片字节，故意私有且不序列化；只有显式 `payload_bytes()` 才能借出。
    source_bytes: Vec<u8>,
}

/// 自定义调试输出：保留排查所需 metadata 和 byte 长度，刻意不打印原始图片 bytes。
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
    /// 复制 admission 的 metadata 与私有图片 bytes；调用方需意识到 clone 会复制最多一份源图片内存。
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
    /// 把 artifact bytes 与治理授权、数据策略和模型 route 组合成短期 admission。
    ///
    /// 【调用者】预期由已经读取 Artifact 并检查授权的 adapter 调用；当前仓内实际构造点主要是 Domain/Provider fixtures。
    /// 【输入】ArtifactRef、ProcessingGrant、来源相对路径、MIME、原始 bytes、route/capabilities、policy revision/data epoch、purpose 与允许分类。
    /// 【输出】只在默认 source 上限和派发合同通过后返回非序列化 admission。
    /// 【失败情况】Artifact/grant 无效、撤销、路径/hash/scope/purpose/route/MIME 不匹配或不支持 image capability 时返回稳定错误。
    /// 【副作用/设计原因】只保存调用方给出的内存 bytes，不读取路径、抓取 URL 或调用 Provider；把授权证据和实际 bytes 一起绑定，
    /// 防止 Provider 仅凭 request 中的任意路径引用加载文件。
    /// ⚠ 构造时将 `admitted_at_unix_ms` 当作 now 校验 retention；真正派发还必须重新传入可信当前时间。
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

    /// 借出原始图片 bytes，供已验证的 Provider encoder 生成协议 payload。
    ///
    /// 返回的切片仍是敏感内容；调用方不应写入日志、Receipt 或 Event payload。
    pub fn payload_bytes(&self) -> &[u8] {
        &self.source_bytes
    }

    /// 返回原始 Artifact bytes 的字节数，尚未计算 base64 扩张。
    pub fn source_size_bytes(&self) -> usize {
        self.source_bytes.len()
    }

    /// 重新对私有 bytes 计算 Artifact content digest，用于派发前的 hash 绑定检查。
    pub fn source_digest(&self) -> String {
        image_payload_digest(&self.source_bytes)
    }

    /// 为 ModelContent::AttachmentRef 生成包含 Artifact ID 和版本的 canonical 引用文本。
    pub fn attachment_ref(&self) -> String {
        format!(
            "artifact:{}:{}",
            self.artifact.artifact_id, self.artifact.version
        )
    }

    /// 检查请求附件引用是否指向同一 Artifact、MIME 和内容 hash。
    ///
    /// 兼容接受 `artifact:{id}`（不带版本）和 `artifact:{id}:{version}` 两种形式；其余字段仍须精确匹配。
    pub fn matches_attachment(&self, artifact_ref: &str, media_type: &str, digest: &str) -> bool {
        (artifact_ref == self.attachment_ref()
            || artifact_ref == format!("artifact:{}", self.artifact.artifact_id))
            && media_type == self.media_type
            && digest == self.artifact.content_hash
    }

    /// 在请求发送前重验 admission 与指定 route/capability/资源预算的绑定。
    ///
    /// 【调用者】Domain 构造器和 Provider request compiler；后者应提供实际 dispatch 时刻、当前连接 route/capabilities 和 body 限额。
    /// 【输入/输出】route/capabilities/now/limits 全部由调用方提供；合法返回 `Ok(())`，拒绝返回稳定字符串错误。
    /// 【核心流程】先校验预算、Artifact/Grant 和来源路径；再检查 revoked、Artifact/Grant/bytes hash、source 大小、MIME allowlist；
    /// 随后验证 policy/data epoch/purpose/class/retention，最后比对 route 与 vision capability。
    /// 【副作用】纯校验，不重新读 store、文件、系统时钟或策略来源，不编码图片，也不发网络请求。
    /// ⚠ 这里的 retention 比较只使用 `now_unix_ms` 参数；不检查当前策略 revision/data epoch，也不要求 now 晚于 admission 时间。
    /// 当前 Provider request 调用点把 `admitted_at_unix_ms` 再作为 now 传入，因此已过期的 admission 可能仍按创建时状态通过该比较。
    /// 此处也只接受 MIME 标签并校验 allowlist/schema 一致性，不解码文件头确认实际字节格式。
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
                "application/pdf"
                | "text/plain"
                | "application/msword"
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
        // revision/epoch/timestamp 必须有值，但当前合同不持有 authority source，无法验证它们是否仍是最新值。
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
        if self
            .grant
            .retention
            .expires_at_ms
            .is_some_and(|expiry| expiry <= now_unix_ms)
        {
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

    /// 调用 `validate_for_dispatch()` 并把领域字符串拒绝包装为稳定的 ModelError。
    ///
    /// 错误交给 Provider 编译/入口处理；此函数不执行 retry，也不自动撤销 grant。
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
