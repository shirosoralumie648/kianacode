//! 存储根、项目信任和文件系统能力预检（preflight）的 Domain 合同，属于 `kiana-domain`。
//!
//! 【系统位置与数据流】
//! ```text
//! 服务端 ProjectTrust + 存储适配器（adapter）提供的 StorageRoot/能力/文件身份/容量快照
//!     -> StoragePreflightRequest
//!     -> Core evaluate_storage_preflight()
//!     -> StoragePreflightReport（Ready 或 Blocked + 原因）
//!     -> 后续调用方决定是否进入真实存储适配器（adapter）阶段
//! ```
//!
//! 本模块把调用方给出的声明做确定性校验和归因：复核 project/root 绑定、相对路径形状、文件身份元数据、
//! 平台保护能力声明，以及调用方报告的字节数/索引节点（inode）数是否够用。它不打开路径、不解析真实符号链接/硬链接、
//! 不创建目录、不测量实时剩余空间、不加锁，也不写入预检事实。
//!
//! 当前源码中 Core 门面（facade）只由测试引用，没有发现生产调用者；Daemon 的 `resolve_storage_root()` 是单独的
//! 路径解析流程，并未调用这里。因此这里的 Ready 是“提交给合同的输入没有触发 Blocked 条件”，不是实际目录
//! 已安全、可写或容量充足的证明。缺少或未证明的能力必须留在 report 原因里，不能当作授权。

use crate::{
    enforce_path_containment, json_digest, normalize_role_path, ProjectId, ProjectTrustSnapshot,
    SchemaVersion, StorageBackend, StorageFileIdentity, StorageRoot, StorageSecurityCapabilities,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// 预检请求 wire DTO 使用的 schema 标识。
pub const STORAGE_PREFLIGHT_SCHEMA: &str = "kiana.storage-preflight.v1";
/// 预检结果 wire DTO 使用的 schema 标识。
pub const STORAGE_PREFLIGHT_REPORT_SCHEMA: &str = "kiana.storage-preflight-report.v1";
/// 请求和结果共用的 schema 主次版本。
pub const STORAGE_PREFLIGHT_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 请求的 `requested_paths` 和 `file_identities` 列表各自允许的最大条目数；超出会拒绝整个请求。
pub const MAX_PREFLIGHT_PATHS: usize = 256;
/// Report 最多可记录的不同阻塞原因数。
pub const MAX_PREFLIGHT_REASONS: usize = 32;

/// 预检给出的合同状态；就绪（Ready）/阻塞（Blocked）不是实际文件操作的完成状态。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoragePreflightStatus {
    /// 当前数据归约器（reducer）实际执行的检查没有生成阻塞原因；不代表未覆盖的不变量也已通过。
    /// 仍需后续适配器（adapter）在真实文件操作前现场执行安全检查。
    Ready,
    /// 至少一个信任、路径、身份、平台能力或容量条件不满足。
    Blocked,
}

/// 由上游服务端和存储适配器（adapter）汇总后交给纯 Domain reducer 的预检输入。
///
/// `Serialize`/`Deserialize` 来自 Serde，用于 Rust 值与 wire/JSON 之间转换；
/// `deny_unknown_fields` 会拒绝当前版本不认识的输入字段。转换不认证发送方，digest 也不构成签名。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePreflightRequest {
    /// 固定的请求 schema 名称。
    pub schema: String,
    /// 请求 wire 版本。
    pub version: SchemaVersion,
    /// 调用方解析得到的目标存储根；其 owner scope 可选择绑定项目。
    pub root: StorageRoot,
    /// 服务端提供的项目信任快照；`trusted = false` 是可表示的输入，evaluate 会产出 Blocked。
    pub project_trust: ProjectTrustSnapshot,
    /// 项目根路径的服务端摘要，单独携带以便与 storage root 的摘要做绑定比较。
    pub project_root_digest: String,
    /// adapter 提供的平台能力声明；不是由本模块现场探测得到。
    pub capabilities: StorageSecurityCapabilities,
    /// 本次计划检查的相对路径；构造请求时原样保留，evaluate 后会规范化并去重。
    pub requested_paths: Vec<String>,
    /// adapter 对部分路径提供的文件身份元数据；列表可为空，也不要求覆盖每个请求路径。
    pub file_identities: Vec<StorageFileIdentity>,
    /// 此操作计划需要的字节容量，由调用方估计/测量后提供。
    pub required_bytes: u64,
    /// adapter 报告的可用字节数；不是本函数读取磁盘得到的值。
    pub available_bytes: u64,
    /// 此操作计划需要的 inode 数，由调用方估计/测量后提供。
    pub required_inodes: u64,
    /// adapter 报告的可用 inode 数；不是本函数现场探测结果。
    pub available_inodes: u64,
    /// 除自身以外请求字段的 SHA-256 内容摘要，用于发现意外字段漂移。
    pub request_digest: String,
}

impl StoragePreflightRequest {
    /// 组装有版本的请求（versioned request）、根据当前字段计算自摘要，并立即执行请求结构校验。
    ///
    /// 【调用者】预期由收集了 ProjectTrust、root 和 adapter 观测值的上层调用；当前仓库中实际构造
    /// 主要位于 DEP-07 Domain 测试夹具，尚未发现生产预检路由。
    ///
    /// 【输入】root/trust/project-root 摘要/能力声明、待检查相对路径、可选身份快照，以及调用方给出的
    /// bytes/inodes 需求和可用量。构造器不会读取配置、环境变量、磁盘或系统容量。
    ///
    /// 【输出/失败】成功返回带 `request_digest` 的 DTO；schema/信任快照/摘要/路径数或路径文本无效时
    /// 返回稳定错误字符串。成功不表示项目已可信，`trusted = false` 会留到 evaluate 时产生 Blocked。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        root: StorageRoot,
        project_trust: ProjectTrustSnapshot,
        project_root_digest: impl Into<String>,
        capabilities: StorageSecurityCapabilities,
        requested_paths: Vec<String>,
        file_identities: Vec<StorageFileIdentity>,
        required_bytes: u64,
        available_bytes: u64,
        required_inodes: u64,
        available_inodes: u64,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: STORAGE_PREFLIGHT_SCHEMA.to_owned(),
            version: STORAGE_PREFLIGHT_VERSION,
            root,
            project_trust,
            project_root_digest: project_root_digest.into(),
            capabilities,
            requested_paths,
            file_identities,
            required_bytes,
            available_bytes,
            required_inodes,
            available_inodes,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    /// 校验请求 wire header、信任/能力快照、路径输入上限和请求自摘要；StorageRoot 的业务校验留给 `evaluate()`，
    /// 以便 reducer 尝试把可报告的 root 问题归类为 Blocked 原因。
    ///
    /// 【调用者】`new()` 在返回前调用，`StoragePreflightReport::evaluate()` 在归因前再次调用；外部 adapter
    /// 也可在把反序列化请求送入 Core 前单独调用。
    ///
    /// 【输出/失败】全部合同通过返回 `Ok(())`；schema、嵌套快照、digest 或路径形状不符时返回稳定错误字符串。
    /// 此处只检查路径文本非空、长度和控制字符，不将路径解析为真实文件系统对象；
    /// 也不要求 ProjectTrust 的 `trusted` 必须为 true，因为不可信项目应得到可解释的 Blocked report。
    /// 文件身份元数据只做本模块的 header/path/digest 检查；evaluate 会把当前覆盖到的风险标志变成阻塞原因，
    /// 但不会完整重跑嵌套 `StorageFileIdentity::validate()`，所以不能把请求校验当作身份状态的完整复核。
    /// 失败返回稳定字符串，不修复 DTO、不记录日志、不产生文件或权限副作用。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_PREFLIGHT_SCHEMA || self.version != STORAGE_PREFLIGHT_VERSION {
            return Err("storage_preflight_header_invalid".to_owned());
        }
        self.project_trust.validate()?;
        valid_digest(
            &self.project_root_digest,
            "storage_preflight_project_root_digest",
        )?;
        self.capabilities.validate()?;
        if self.requested_paths.is_empty()
            || self.requested_paths.len() > MAX_PREFLIGHT_PATHS
            || self.file_identities.len() > MAX_PREFLIGHT_PATHS
        {
            return Err("storage_preflight_path_limit".to_owned());
        }
        // 先拒绝过大的单一路径和明显不适合作为路径文本的控制字符，后续才进入共享词法规范化器。
        for path in &self.requested_paths {
            if path.trim().is_empty()
                || path.len() > 4_096
                || path.contains('\\')
                || path.bytes().any(|byte| byte == 0 || byte < 0x20)
            {
                return Err("storage_preflight_path_invalid".to_owned());
            }
        }
        // 这里验证身份快照的 wire 形状和自摘要，不重新 stat 路径，也不证明 adapter 的观测真实。
        for identity in &self.file_identities {
            validate_identity_metadata(identity)?;
        }
        valid_digest(&self.request_digest, "storage_preflight_request_digest")?;
        if self.request_digest != self.digest() {
            return Err("storage_preflight_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对请求中除 `request_digest` 自身以外的字段做确定性 JSON 摘要。
    ///
    /// digest 将信任、根、路径、能力和容量输入绑定在一起以便检测漂移；无密钥摘要不认证数据来源，
    /// 且不会让本模块重新读取这些字段所代表的文件系统事实。`new()` 用它填充字段，`validate()` 用它
    /// 检查解码值是否与摘要一致；它总是返回摘要字符串，不执行 I/O。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "root": self.root,
            "project_trust": self.project_trust,
            "project_root_digest": self.project_root_digest,
            "capabilities": self.capabilities,
            "requested_paths": self.requested_paths,
            "file_identities": self.file_identities,
            "required_bytes": self.required_bytes,
            "available_bytes": self.available_bytes,
            "required_inodes": self.required_inodes,
            "available_inodes": self.available_inodes,
        }))
    }
}

/// 预检报告中的二态结论；具体拒绝原因保存在 `reasons` 中。
///
/// 报告是上游提供事实（supplied facts）的投影，不是一次写入、锁获取或启动操作的结果。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoragePreflightReport {
    /// 固定的报告 schema 名称。
    pub schema: String,
    /// 报告 wire 版本。
    pub version: SchemaVersion,
    /// 报告所针对的 storage root ID。
    pub root_id: crate::StorageRootId,
    /// 输入 storage root 的摘要。
    pub root_digest: String,
    /// 输入声明的后端类型；例如本地或网络文件系统。
    pub backend: StorageBackend,
    /// 由 ProjectTrust 快照声明的项目 ID。
    pub project_id: ProjectId,
    /// 与请求相同的项目 canonical root 摘要。
    pub project_root_digest: String,
    /// 输入 ProjectTrust 快照摘要。
    pub trust_digest: String,
    /// 输入 storage capability 声明摘要。
    pub capability_digest: String,
    /// 请求计划需要的字节容量。
    pub required_bytes: u64,
    /// 请求声称的可用字节容量，不是报告生成过程中的实时测量。
    pub available_bytes: u64,
    /// 请求计划需要的 inode 数量。
    pub required_inodes: u64,
    /// 请求声称的可用 inode 数量，不是报告生成过程中的实时测量。
    pub available_inodes: u64,
    /// `evaluate()` 生成时经过词法规范化、去重和排序的路径集合；独立 `validate()` 不重做路径规范化。
    pub checked_paths: Vec<String>,
    /// 由 `reasons` 是否为空派生的 Ready/Blocked 状态。
    pub status: StoragePreflightStatus,
    /// 稳定排序且去重的阻塞原因代码。
    pub reasons: Vec<String>,
    /// 与原因一一对应的展示/处理提示；当前固定格式为 `remediate:<reason>`。
    pub remediation: Vec<String>,
    /// 对报告其余字段计算的 SHA-256 内容摘要。
    pub report_digest: String,
}

impl StoragePreflightReport {
    /// 把有效请求折叠为 Ready 或 Blocked 报告，不执行报告中提到的存储操作。
    ///
    /// 【调用者】Core 的 `evaluate_storage_preflight()` 薄门面转调此函数；全仓库源码检索未发现生产调用点，
    /// 已确认的直接调用位于 Domain fixture。Daemon `resolve_storage_root()` 不调用本 reducer。
    ///
    /// 【输入/输出】输入请求内的 root/trust/path/identity/capability/capacity 声明。成功返回报告：无原因则
    /// Ready，有一个或多个原因则 Blocked；请求结构无效或最终报告自校验失败时返回 `Err`。例如 nil root ID
    /// 或格式错误的 root digest 虽会先映射原因，但报告自身不接受这类 header/digest，最终仍会返回 `Err`。
    ///
    /// 【核心流程】先验证请求；将 root 校验错误归并为稳定原因；比较 root/project/trust；规范化相对路径；
    /// 对已提供的身份快照检查根、路径、链接、文件类型和权限；再检查能力声明和 bytes/inodes 数；最后
    /// 对原因排序、派生 remediation/status、计算报告摘要并验证报告。
    ///
    /// 【副作用与 proof 边界】使用 Rust 有序集合 `BTreeSet`（按键排序并自动去重）得到稳定输出；不访问磁盘、系统时钟、EventStore、
    /// Broker 或日志，不创建目录/锁，也不证明 adapter 的元数据来自现场。缺少某个 requested path 的身份记录
    /// 不会单独阻塞；输入报告的容量数字也不会在此重新测量。
    pub fn evaluate(request: &StoragePreflightRequest) -> Result<Self, String> {
        request.validate()?;
        let mut checked_paths = BTreeSet::new();
        let mut reasons = BTreeSet::new();

        // 可归类的 root 校验错误先映射为 Blocked reason 并继续收集；但若复制进 report 的 root ID/digest
        // 本身不符合 report header/digest 规则，末尾 report.validate() 仍会返回 Err，而不是 Blocked。
        let root_validation = request.root.validate();
        if let Err(error) = root_validation {
            reasons.insert(storage_root_reason(&error).to_owned());
        }
        // StorageOwnerScope.project_id 是 Option：仅声明了 owner project 时要求相等；None 不自动继承 trust 项目。
        // project_root_digest 则必须始终和服务端 trust snapshot 精确一致。
        if request
            .root
            .owner_scope
            .project_id
            .is_some_and(|id| id != request.project_trust.project_id)
            || request.project_trust.canonical_root_digest != request.project_root_digest
        {
            reasons.insert("project_root_mismatch".to_owned());
        }
        // 不可信项目是可报告的拒绝状态，而不是请求 DTO 的反序列化/结构错误。
        if !request.project_trust.trusted {
            reasons.insert("project_untrusted".to_owned());
        }
        // `normalize_role_path` 拒绝绝对路径、父目录跳转等词法逃逸；此处的根允许列表只是相对根 `.`，
        // 不解析 symlink，也不检查路径在磁盘上是否存在或仍位于某个 canonical root 下。
        for path in &request.requested_paths {
            match normalize_role_path(path) {
                Some(normalized) => {
                    checked_paths.insert(normalized.clone());
                    if enforce_path_containment(&[".".to_owned()], &normalized).is_err() {
                        reasons.insert("path_escape".to_owned());
                    }
                }
                None => {
                    reasons.insert("path_escape".to_owned());
                }
            }
        }
        // BTreeSet 使输出路径按字典序稳定且去重；重复请求路径因此只在报告里出现一次。
        let requested = checked_paths.iter().cloned().collect::<BTreeSet<_>>();
        // file_identities 是 adapter 可选提交的观测集合，不保证覆盖 requested_paths 中的每一项。
        // 已提交的观测若跨 root、对应未请求路径或指向危险目标，则显式阻塞。
        // 注意：下面的分支没有覆盖所有 StorageFileIdentity 不变量；例如 exists=false 且 regular_file=true
        // 不会命中这些条件，当前仍可能成为 Ready。这里记录现有合同边界，不代表该组合是安全事实。
        for identity in &request.file_identities {
            if identity.root_digest != request.root.root_digest {
                reasons.insert("file_identity_root_mismatch".to_owned());
            }
            if !requested.contains(&identity.relative_path) {
                reasons.insert("file_identity_unrequested".to_owned());
            }
            if identity.symlink {
                reasons.insert("symlink_target".to_owned());
            }
            if identity.hardlink {
                reasons.insert("hardlink_target".to_owned());
            }
            if identity.exists && !identity.regular_file {
                reasons.insert("non_regular_target".to_owned());
            }
            // Unix mode 的八进制 0o077 覆盖 group/other 的全部读、写、执行权限位；任意一位非零都按过宽处理。
            if identity.mode.is_some_and(|mode| mode & 0o077 != 0) {
                reasons.insert("permissions_too_broad".to_owned());
            }
        }
        // 能力 bool 表示调用方声明的保障；这里按声明值拒绝 false，但不会现场验证 true 是否真实。
        if !request.capabilities.symlink_guard {
            reasons.insert("symlink_guard_unavailable".to_owned());
        }
        if !request.capabilities.hardlink_guard {
            reasons.insert("hardlink_guard_unavailable".to_owned());
        }
        if !request.capabilities.permission_guard {
            reasons.insert("permission_guard_unavailable".to_owned());
        }
        if !request.capabilities.atomic_replace {
            reasons.insert("atomic_replace_unproven".to_owned());
        }
        // 容量来源同样是输入快照；比较只做整数门槛检查，不会调用 statvfs 或查询 inode。
        if request.available_bytes < request.required_bytes {
            reasons.insert("capacity_bytes_insufficient".to_owned());
        }
        if request.available_inodes < request.required_inodes {
            reasons.insert("capacity_inodes_insufficient".to_owned());
        }
        // 原因码进入 BTreeSet，因此输出稳定排序且天然去重；remediation 目前是同一原因的机械映射。
        let reasons = reasons.into_iter().collect::<Vec<_>>();
        let remediation = reasons
            .iter()
            .map(|reason| format!("remediate:{reason}"))
            .collect::<Vec<_>>();
        // Ready 仅由“当前收集到的原因为空”派生；它不等于真实路径打开/目录创建成功。
        let mut report = Self {
            schema: STORAGE_PREFLIGHT_REPORT_SCHEMA.to_owned(),
            version: STORAGE_PREFLIGHT_VERSION,
            root_id: request.root.root_id,
            root_digest: request.root.root_digest.clone(),
            backend: request.root.backend,
            project_id: request.project_trust.project_id,
            project_root_digest: request.project_root_digest.clone(),
            trust_digest: request.project_trust.trust_digest.clone(),
            capability_digest: request.capabilities.capability_digest.clone(),
            required_bytes: request.required_bytes,
            available_bytes: request.available_bytes,
            required_inodes: request.required_inodes,
            available_inodes: request.available_inodes,
            checked_paths: checked_paths.into_iter().collect(),
            status: if reasons.is_empty() {
                StoragePreflightStatus::Ready
            } else {
                StoragePreflightStatus::Blocked
            },
            reasons,
            remediation,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    /// 校验反序列化或下游传入报告的结构、排序、状态/提示一致性及自摘要。
    ///
    /// 【调用者】`evaluate()` 在构造完报告后调用；Core `validate_storage_preflight_report()` 和下游消费方
    /// 也可对保存/解码后的报告调用。通过返回 `Ok(())`，失败返回稳定的 Domain 错误字符串。
    ///
    /// 不会拿原始 Request 重新运行 evaluate，也不回读 root/trust/capability/容量来源；因此通过只证明
    /// 报告字段符合这里的内部格式合同。失败返回稳定字符串，无日志、写入或自动纠正。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STORAGE_PREFLIGHT_REPORT_SCHEMA
            || self.version != STORAGE_PREFLIGHT_VERSION
            || self.root_id.as_uuid().is_nil()
            || self.project_id.as_uuid().is_nil()
        {
            return Err("storage_preflight_report_header_invalid".to_owned());
        }
        valid_digest(&self.root_digest, "storage_preflight_report_root_digest")?;
        valid_digest(
            &self.project_root_digest,
            "storage_preflight_report_project_root_digest",
        )?;
        valid_digest(&self.trust_digest, "storage_preflight_report_trust_digest")?;
        valid_digest(
            &self.capability_digest,
            "storage_preflight_report_capability_digest",
        )?;
        // evaluate 生成的 Ready 报告至少有一条词法检查通过的路径；这里自身只检查是否为空，Blocked 可保留空列表。
        if self.checked_paths.len() > MAX_PREFLIGHT_PATHS
            || (self.checked_paths.is_empty() && self.status == StoragePreflightStatus::Ready)
        {
            return Err("storage_preflight_report_path_limit".to_owned());
        }
        // evaluate 按 BTreeSet 输出；这里严格递增只拒绝未排序和重复项，不重新检查每个字符串是否为安全相对路径。
        // 因此不能把单独通过 report.validate() 的反序列化 checked_paths 当作文件访问授权。
        if self.checked_paths.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err("storage_preflight_report_paths_noncanonical".to_owned());
        }
        // 原因列表也必须排序去重，且 remediation 必须逐项按固定模板由 reasons 派生。
        if self.reasons.len() > MAX_PREFLIGHT_REASONS
            || self.reasons.windows(2).any(|pair| pair[0] >= pair[1])
            || self.remediation
                != self
                    .reasons
                    .iter()
                    .map(|reason| format!("remediate:{reason}"))
                    .collect::<Vec<_>>()
        {
            return Err("storage_preflight_report_reasons_invalid".to_owned());
        }
        // 不接受调用方单独篡改 status：它必须和原因列表是否为空一致。
        let expected_status = if self.reasons.is_empty() {
            StoragePreflightStatus::Ready
        } else {
            StoragePreflightStatus::Blocked
        };
        if self.status != expected_status {
            return Err("storage_preflight_report_status_mismatch".to_owned());
        }
        // 自摘要只校验报告字段一致性，不是来源签名或远端存储提交回执。
        valid_digest(&self.report_digest, "storage_preflight_report_digest")?;
        if self.report_digest != self.digest() {
            return Err("storage_preflight_report_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算除 `report_digest` 自身以外所有报告字段的确定性内容摘要。
    ///
    /// 包含状态、路径、原因、容量和引用摘要；它不把原请求整体或外部文件内容嵌入报告，
    /// 也不能证明这些输入观测是真实的。`evaluate()` 用它填充 `report_digest`，`validate()` 用它检查一致性；
    /// 方法本身只返回摘要字符串。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "root_id": self.root_id,
            "root_digest": self.root_digest,
            "backend": self.backend,
            "project_id": self.project_id,
            "project_root_digest": self.project_root_digest,
            "trust_digest": self.trust_digest,
            "capability_digest": self.capability_digest,
            "required_bytes": self.required_bytes,
            "available_bytes": self.available_bytes,
            "required_inodes": self.required_inodes,
            "available_inodes": self.available_inodes,
            "checked_paths": self.checked_paths,
            "status": self.status,
            "reasons": self.reasons,
            "remediation": self.remediation,
        }))
    }
}

/// 校验预检请求中一条 adapter 文件身份记录的 wire header、相对路径和自摘要。
///
/// 它没有调用 `StorageFileIdentity::validate()`：身份自身的 `exists` 与类型标志等完整不变量并未在这里复核。
/// evaluate 只处理它显式检查的那部分属性，因此畸形但摘要自洽的组合仍可能漏过并得到 Ready；这是当前合同缺口，
/// 注释不把它解释为安全。此 helper 不访问文件系统，也不能证明身份采集与后续打开/写入之间没有检查后使用竞态（TOCTOU）。
/// 此处实际检查失败时返回稳定字符串并使整个请求无效。
fn validate_identity_metadata(identity: &StorageFileIdentity) -> Result<(), String> {
    if identity.schema != crate::STORAGE_FILE_IDENTITY_SCHEMA
        || identity.version != crate::STORAGE_SECURITY_SCHEMA_VERSION
    {
        return Err("storage_preflight_identity_header_invalid".to_owned());
    }
    valid_digest(
        &identity.root_digest,
        "storage_preflight_identity_root_digest",
    )?;
    if normalize_role_path(&identity.relative_path).as_deref()
        != Some(identity.relative_path.as_str())
    {
        return Err("storage_preflight_identity_path_invalid".to_owned());
    }
    valid_digest(
        &identity.identity_digest,
        "storage_preflight_identity_digest",
    )?;
    if identity.identity_digest != identity.digest() {
        return Err("storage_preflight_identity_digest_mismatch".to_owned());
    }
    Ok(())
}

/// 把 `StorageRoot::validate()` 的底层错误归并为预检报告使用的稳定原因族。
///
/// 采用字符串包含关系是因为 `StorageRoot::validate()` 暴露的是稳定错误字符串；未命中专类时统一映射为
/// `storage_root_invalid`，原始错误细节不会进入 report。
fn storage_root_reason(error: &str) -> &str {
    if error.contains("network_filesystem") {
        "remote_filesystem_unsupported"
    } else if error.contains("namespace") {
        "storage_namespace_invalid"
    } else if error.contains("owner") || error.contains("project") {
        "storage_owner_mismatch"
    } else {
        "storage_root_invalid"
    }
}

/// 检查输入是否为 `sha256:` 前缀加 64 位十六进制文本；不重新计算其指向对象的内容。
///
/// 大小写十六进制都接受，错误码由字段名前缀组成。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
