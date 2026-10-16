//! CAP-30 dynamic tool discovery at the capability-broker boundary.
//!
//! The card's central claim is that a **search result is a candidate, not an authorization**.
//! `tool.search` is an ordinary brokered operation: it returns descriptors, and those descriptors
//! are claims made by an extension package, an MCP server or a catalog generator. Nothing in that
//! result has been through `ControlPlane`, `kiana-policy`, `kiana-gates` or approval. This module
//! owns the single question the broker must answer before any discovered descriptor may be turned
//! into a dispatchable candidate:
//!
//! > Given the *current* server-owned authorization surface, may **this** search result be promoted
//! > into a capability request — right now, under **this** grant?
//!
//! It is deliberately a **read-only contract plus a pure candidate decision**. It spawns no
//! process, loads no package, verifies no signature, opens no socket, appends no event and adds
//! no execution loop. It also adds no sixth model-visible tool: the model surface stays the five
//! frozen entries in `kiana-runner/src/tools.rs`, and nothing here widens it. A catalog entry in
//! [`DiscoverySurface::catalog`] is a *candidate* record, not a model-visible tool schema.
//!
//! ## What the surface owns, and what the result owns
//!
//! The [`DiscoverySurface`] is the parent authority: principal, project, authority epoch, granted
//! capability keys, the [`ScopeSet`] dimensions, the effect ceiling, the bound catalog and the
//! registry of admitted tools. The [`DiscoverySearchOutcome`] is the untrusted side. Every field a
//! candidate supplies is checked against the surface, and the values that end up in
//! [`AdmittedDiscoveryCandidate`] are taken from the surface, never copied from the claim — so a
//! descriptor cannot understate its own `side_effecting` flag or rename itself.
//!
//! ## Fixed decision order
//!
//! The order is part of the contract, not an implementation detail, so the same facts always
//! produce the same report:
//!
//! 1. request shape, then surface/evidence shape (a corrupt input cannot be answered at all);
//! 2. **staleness** — the request is bound to the surface digest it was issued under, so a
//!    revoked, narrowed, re-epoched or re-catalogued grant is caught *before* any per-candidate
//!    question is asked;
//! 3. revocation, then expiry;
//! 4. the search itself — a failed search is [`DiscoveryStatus::Unknown`], never a silent
//!    allow-all or deny-all that a caller could keep executing under;
//! 5. per candidate, structural identity first (is this a real tool at all), then supply-chain
//!    trust, then authorization.
//!
//! A candidate that violates several rules reports the *earliest* one, so a descriptor that both
//! escalates its own effect and asks for an ungranted capability is reported as the effect
//! escalation it is rather than softened into a permission problem.
//!
//! ## Intersection, never union
//!
//! An extension may only ever ask for a subset of what the parent grant already allows. The
//! registered manifest scope and the candidate's own claim are unioned into the *requested* set,
//! then intersected with the grant by [`intersect_extension_scope`]. If the intersection is
//! smaller than what was requested, the request was a superset and the candidate is denied — it is
//! never granted the difference. A denied superset produces no record at all rather than a
//! narrowed one, because a descriptor that asked for too much is not a descriptor that should be
//! trusted with a smaller grant on a retry.
//!
//! ## Honest limits
//!
//! This module proves nothing about any real extension. It is a decision over values a caller
//! supplies: if a caller builds a surface that lists an unsigned package as `PublisherSigned` with
//! a matching digest, this module will admit its candidates. The trust verdict is only as good as
//! the admission path that produced the evidence, which remains
//! `kiana-daemon/src/extensions.rs` signature/package verification plus the existing
//! [`crate::ExtensionAdmission`] recheck. See `docs/roadmap/cap30-dynamic-discovery-baseline.md`.
//!
//! # 中文阅读说明与当前接线状态
//!
//! 这里描述的是“搜索发现出来的工具候选能否符合某个现有授权面”，不是给新工具发权限。
//! 输入由调用方提供：当前授权快照、绑定该快照摘要的搜索请求、扩展准入证据、搜索结果
//! 和当前时间；`DynamicDiscoveryReport::evaluate()` 据此产生 `Admitted`、`Denied` 或
//! `Unknown` 报告。报告中准入候选的能力集合取请求与上级授权的交集，名称、能力和副作用
//! 标志取自服务端目录/准入证据，而不是相信候选自己的声明。
//!
//! ```text
//! 授权/扩展装配方准备 DiscoverySurface + ExtensionAdmissionEvidence
//!     -> 搜索方返回候选列表或“搜索未完成”
//!     -> DynamicDiscoveryRequest 绑定 surface.digest
//!     -> evaluate() 按固定顺序检查并生成带摘要的报告
//!     -> 调用方可读取 promotable()，但后续执行仍必须重新走 ControlPlane 授权链
//! ```
//!
//! 当前源码检索只找到本模块的定义/实现以及 CAP-30 契约测试使用这些类型；没有找到
//! `kiana-core`、`kiana-daemon` 或 Runner 的生产调用点。因此，虽然本模块从 Broker crate
//! 根重导出，它目前仍是同步的值校验与候选判定代码，不能据此声称运行时已接通动态工具
//! 搜索、已加载扩展、已验证签名或已创建可执行操作。测试中的证据对象也是测试构造值，
//! 不构成生产信任链证明。
//!
//! 重要边界：这里的 SHA-256/JSON digest（摘要）用于检测输入/报告内容绑定是否一致，不是
//! 带密钥的签名，也不会证明传入的 surface/evidence 真由可信服务端产生。真实项目信任和
//! 包签名证据应由上游 Daemon 的扩展准入路径建立；此函数只检查调用方给出的值是否彼此
//! 一致。未知搜索结果与“成功搜索但没有候选通过”也必须区分：前者是 `Unknown` 并要求
//! 重试，后者是 `Denied` 且是对当前授权面的确定拒绝。

use kiana_domain::{
    json_digest, redact_text, CapabilityKind, ExtensionAdapterStatus, ExtensionEffect,
    ScopeDimension, ScopeSet,
};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

/// 授权快照 wire 结构版本；字段形状变更时应使用新版本字符串。
pub const DISCOVERY_SURFACE_SCHEMA: &str = "kiana.dynamic-discovery-surface.v1";
/// 搜索请求 wire 结构版本。
pub const DISCOVERY_REQUEST_SCHEMA: &str = "kiana.dynamic-discovery-request.v1";
/// 判定报告 wire 结构版本。
pub const DISCOVERY_REPORT_SCHEMA: &str = "kiana.dynamic-discovery-report.v1";

/// 单次搜索最多接收的候选条数，用于限制每次纯判定需要处理的输入规模。
pub const MAX_DISCOVERY_CANDIDATES: usize = 128;
/// 授权快照中的服务端工具目录最多容纳的条目数。
pub const MAX_DISCOVERY_CATALOG: usize = 512;
/// 一条扩展证据/候选声明最多列出的能力键数量。
pub const MAX_DISCOVERY_CAPABILITIES: usize = 64;
/// 身份类字符串的最大 UTF-8 字节数；`String::len()` 统计字节，不是中文字符数量。
pub const MAX_DISCOVERY_IDENT_BYTES: usize = 256;
/// 搜索文本的最大 UTF-8 字节数；它限制输入体积，不是模型 token 数。
pub const MAX_DISCOVERY_QUERY_BYTES: usize = 512;
/// 请求者可以要求返回的最大候选数，不能通过请求把处理上限放大。
pub const MAX_DISCOVERY_RESULTS: usize = 128;

/// The project-trust verdict for a project-local source. Mirrors the three-state trust the rest of
/// the system uses; only [`ProjectTrustVerdict::Trusted`] may load a project-local extension.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProjectTrustVerdict {
    /// 上游准入流程已判定该项目本地资源可信。
    Trusted,
    /// 上游已判定不可信，候选必须拒绝。
    Untrusted,
    /// 没有足够证据作出判断；按拒绝处理，不能当作 Trusted。
    Unknown,
}

impl ProjectTrustVerdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Untrusted => "untrusted",
            Self::Unknown => "unknown",
        }
    }
}

/// Where an extension's bytes came from. `SelfAsserted` and `Unknown` are refused outright: a
/// descriptor that describes its own provenance is not provenance.
///
/// 这是证据来源分类，不是本模块验证签名的结果；本模块仅允许服务端可核验的来源类别参与
/// 候选判定。若上游错误地把自述来源填成已签名，这里的纯函数无法自行发现该错误。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiscoveryProvenance {
    /// A publisher signature verified against a configured trust root.
    PublisherSigned,
    /// Shipped inside the repository itself.
    RepositoryBundled,
    /// The package says so about itself.
    SelfAsserted,
    /// No provenance was recorded.
    Unknown,
}

impl DiscoveryProvenance {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PublisherSigned => "publisher_signed",
            Self::RepositoryBundled => "repository_bundled",
            Self::SelfAsserted => "self_asserted",
            Self::Unknown => "unknown",
        }
    }

    /// Only the two server-verifiable roots may contribute a candidate.
    pub const fn is_verified(self) -> bool {
        matches!(self, Self::PublisherSigned | Self::RepositoryBundled)
    }
}

/// One tool the server has already decided may be *offered* for discovery. This is the registry
/// half of the surface: it is the only place an operation name, namespace, schema digest, effect
/// or `side_effecting` flag may come from. It grants nothing — `granted_capabilities` and
/// [`ScopeSet`] still have to permit it.
///
/// 此目录记录的是“服务端允许拿出来比较的工具定义”，不是直接给模型展示的工具 schema，
/// 更不是执行许可。候选提交的同名字段必须与该目录项一致。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegisteredDiscoveryTool {
    /// 目录中登记的展示/工具名；不能由候选自行替换。
    pub tool_name: String,
    /// 工具所属命名空间；必须符合授权快照的命名空间范围。
    pub namespace: String,
    /// 服务端登记的参数 schema 摘要，用于拒绝未登记或已漂移的 schema。
    pub schema_digest: String,
    /// 目录将该操作绑定到的能力种类。
    pub capability: CapabilityKind,
    /// 服务端登记的副作用级别，候选不能自行降级。
    pub effect: ExtensionEffect,
    /// 服务端登记的副作用标志；输出值取此字段而不信任候选声明。
    pub side_effecting: bool,
}

/// The parent authority for one discovery pass, as the broker sees it. Everything here is
/// server-owned; the search result is compared against it and never merged into it.
///
/// 本结构是一次判定所依据的“父级授权面”快照。构造/校验只保证字段形状和摘要自洽；
/// 它本身不会从 ControlPlane 读取当前 grant，也不会证明调用方有权伪造这个快照。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoverySurface {
    /// 必须与当前 `DISCOVERY_SURFACE_SCHEMA` 完全匹配。
    pub schema: String,
    /// 本次授权面对应的主体标识，来源由上游提供。
    pub principal_id: String,
    /// 本次授权面所属项目标识；候选证据必须与它一致。
    pub project_id: String,
    /// 上游授权代次；报告会保留它，便于调用方识别使用的是哪一代授权快照。
    pub authority_epoch: u64,
    /// Capability keys (`capability_key`) this grant may exercise. Empty is fail-closed: a
    /// discovery pass with no granted capability admits nothing.
    pub granted_capabilities: BTreeSet<String>,
    /// 能力范围、命名空间、操作等维度的上级限制；候选的请求范围只能是其子集。
    pub scope: ScopeSet,
    /// The strongest effect this pass may reach. `ReadOnly` refuses every read-write candidate.
    pub effect_ceiling: ExtensionEffect,
    /// Registry of offerable tools, keyed by exact operation name.
    pub catalog: BTreeMap<String, RegisteredDiscoveryTool>,
    /// Digest of the tool catalog this surface was built against.
    pub catalog_digest: String,
    /// Set by the authority when the grant is withdrawn. A revoked surface admits nothing even if
    /// a caller re-seals its digest around the revocation.
    pub revoked: bool,
    /// 授权面到期的 Unix 毫秒时间点；evaluate 使用调用方给出的当前时间比较。
    pub expires_at_unix_ms: u64,
    /// Sealed over every field above, so a mutated surface cannot present a stale binding.
    pub digest: String,
}

impl DiscoverySurface {
    /// 用服务端快照字段创建并校验一次候选判定的授权面。
    ///
    /// 【输入】主体/项目/授权代次、授予能力、ScopeSet、效果上限、登记目录摘要、撤销标志
    /// 和到期时间。字段来自调用方；本函数不查询授权存储或验证来源身份。
    ///
    /// 【输出/失败】成功返回字段摘要已计算且 `validate()` 通过的快照；schema、文本、目录、
    /// 能力数量或摘要形状不符合约束时返回稳定原因字符串。
    ///
    /// 【易误改边界】摘要是在对象中写入 `digest` 后自我校验的内容指纹，不是签名或授权票据。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        principal_id: impl Into<String>,
        project_id: impl Into<String>,
        authority_epoch: u64,
        granted_capabilities: BTreeSet<String>,
        scope: ScopeSet,
        effect_ceiling: ExtensionEffect,
        catalog: BTreeMap<String, RegisteredDiscoveryTool>,
        catalog_digest: impl Into<String>,
        revoked: bool,
        expires_at_unix_ms: u64,
    ) -> Result<Self, String> {
        let mut surface = Self {
            schema: DISCOVERY_SURFACE_SCHEMA.to_owned(),
            principal_id: principal_id.into(),
            project_id: project_id.into(),
            authority_epoch,
            granted_capabilities,
            scope,
            effect_ceiling,
            catalog,
            catalog_digest: catalog_digest.into(),
            revoked,
            expires_at_unix_ms,
            digest: String::new(),
        };
        surface.digest = surface.unsigned_digest();
        surface.validate()?;
        Ok(surface)
    }

    fn unsigned_digest(&self) -> String {
        let granted = self
            .granted_capabilities
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let catalog = self
            .catalog
            .iter()
            .map(|(operation, tool)| {
                json!({
                    "operation": operation,
                    "tool_name": tool.tool_name,
                    "namespace": tool.namespace,
                    "schema_digest": tool.schema_digest,
                    "capability": capability_key(&tool.capability),
                    "effect": tool.effect,
                    "side_effecting": tool.side_effecting,
                })
            })
            .collect::<Vec<_>>();
        json_digest(&json!({
            "schema": self.schema,
            "principal_id": self.principal_id,
            "project_id": self.project_id,
            "authority_epoch": self.authority_epoch,
            "granted_capabilities": granted,
            "scope": self.scope.digest(),
            "effect_ceiling": self.effect_ceiling,
            "catalog": catalog,
            "catalog_digest": self.catalog_digest,
            "revoked": self.revoked,
            "expires_at_unix_ms": self.expires_at_unix_ms,
        }))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DISCOVERY_SURFACE_SCHEMA {
            return Err("discovery_surface_invalid".to_owned());
        }
        required_text(
            &self.principal_id,
            "discovery_surface_principal",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        required_text(
            &self.project_id,
            "discovery_surface_project",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        if self.authority_epoch == 0 {
            return Err("discovery_surface_epoch_invalid".to_owned());
        }
        if self.expires_at_unix_ms == 0 {
            return Err("discovery_surface_expiry_invalid".to_owned());
        }
        if self.granted_capabilities.len() > MAX_DISCOVERY_CAPABILITIES {
            return Err("discovery_surface_capabilities_exhausted".to_owned());
        }
        for capability in &self.granted_capabilities {
            required_text(
                capability,
                "discovery_surface_capability",
                MAX_DISCOVERY_IDENT_BYTES,
            )?;
        }
        self.scope
            .validate()
            .map_err(|_| "discovery_surface_invalid".to_owned())?;
        if self.catalog.len() > MAX_DISCOVERY_CATALOG {
            return Err("discovery_surface_catalog_exhausted".to_owned());
        }
        for (operation, tool) in &self.catalog {
            required_text(
                operation,
                "discovery_catalog_operation",
                MAX_DISCOVERY_IDENT_BYTES,
            )?;
            required_text(
                &tool.tool_name,
                "discovery_catalog_tool",
                MAX_DISCOVERY_IDENT_BYTES,
            )?;
            required_text(
                &tool.namespace,
                "discovery_catalog_namespace",
                MAX_DISCOVERY_IDENT_BYTES,
            )?;
            if !is_digest(&tool.schema_digest) {
                return Err("discovery_catalog_schema_digest_invalid".to_owned());
            }
        }
        if !is_digest(&self.catalog_digest) {
            return Err("discovery_surface_catalog_digest_invalid".to_owned());
        }
        if self.digest != self.unsigned_digest() {
            return Err("discovery_surface_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Namespace admission is **fail-closed**: an absent allow-list means "no namespace may be
    /// discovered", not "every namespace". This deliberately differs from
    /// [`ScopeSet::allows_operation`], where `NotApplicable` means unrestricted, because an
    /// unrestricted namespace list is exactly the bypass this card forbids. A discovery surface
    /// must name the namespaces it offers.
    pub fn allows_namespace(&self, namespace: &str) -> bool {
        match &self.scope.namespaces {
            ScopeDimension::NotApplicable => false,
            ScopeDimension::Restricted(values) => values.iter().any(|value| value == namespace),
        }
    }
}

/// Server-owned supply-chain evidence for one installed extension component. Built by the daemon
/// admission path, never by the search result.
///
/// 每条记录把候选所说的扩展/组件，绑定到上游登记的项目、manifest 与 package 摘要、信任
/// 来源、状态、效果和能力声明。此处的 `validate()` 只验证字段格式与数量；它不读取包文件、
/// 不重验签名，也不从 Daemon 获取可信来源。生产来源必须由上游负责。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtensionAdmissionEvidence {
    /// 安装扩展的稳定标识。
    pub extension_id: String,
    /// 扩展内部组件的稳定标识。
    pub component_id: String,
    /// 建立该准入证据时绑定的项目标识。
    pub project_id: String,
    /// The manifest digest the registry recorded when the component was admitted.
    pub registered_manifest_digest: String,
    /// The package hash the registry recorded, retained so a swap of the whole package under a
    /// reused manifest digest is still representable as evidence.
    pub registered_package_sha256: String,
    /// 包来源类别；只有允许核验的类别可参与候选判定。
    pub provenance: DiscoveryProvenance,
    /// True when the component's bytes come from the project tree rather than a signed package.
    pub project_local: bool,
    pub project_trust: ProjectTrustVerdict,
    /// 上游 adapter 当前状态；只有 `Available` 可作为可用候选。
    pub status: ExtensionAdapterStatus,
    /// 上游登记的效果级别，候选不得宣称比它更弱。
    pub effect: ExtensionEffect,
    /// Capabilities the verified manifest says the component needs.
    pub required_capabilities: BTreeSet<String>,
    /// 生成这份证据时的登记代次；保留来源版本信息，不能当成这里会自动查询的最新代次。
    pub registry_generation: u64,
}

impl ExtensionAdmissionEvidence {
    /// 检查上游准入证据对象的字段完整性、摘要形状和集合上限。
    ///
    /// 这只能拒绝结构错误的证据，不能证明证据真实、最新或经过签名验证；该信任边界属于
    /// 构造证据的 Daemon/ExtensionAdmission，不属于此纯 Domain-like 判定模块。
    pub fn validate(&self) -> Result<(), String> {
        required_text(
            &self.extension_id,
            "discovery_evidence_extension",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        required_text(
            &self.component_id,
            "discovery_evidence_component",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        required_text(
            &self.project_id,
            "discovery_evidence_project",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        if self.extension_id.contains('#') || self.component_id.contains('#') {
            return Err("discovery_evidence_identifier_invalid".to_owned());
        }
        if !is_digest(&self.registered_manifest_digest) {
            return Err("discovery_evidence_manifest_digest_invalid".to_owned());
        }
        if !is_hex_sha256(&self.registered_package_sha256) {
            return Err("discovery_evidence_package_digest_invalid".to_owned());
        }
        if self.required_capabilities.len() > MAX_DISCOVERY_CAPABILITIES {
            return Err("discovery_evidence_capabilities_exhausted".to_owned());
        }
        for capability in &self.required_capabilities {
            required_text(
                capability,
                "discovery_evidence_capability",
                MAX_DISCOVERY_IDENT_BYTES,
            )?;
        }
        Ok(())
    }
}

/// One entry of a search result. Every field is a *claim*. A candidate that fails any check is
/// dropped whole; it is never partially admitted by clearing individual fields.
///
/// 搜索器提供的整条候选记录都视为不可信声明。任何字段校验或绑定检查失败时，会为该候选
/// 产生拒绝记录；不会替候选删掉超范围能力后再把剩余部分偷偷放行。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveredToolCandidate {
    pub tool_name: String,
    pub namespace: String,
    pub operation: String,
    pub capability: CapabilityKind,
    pub extension_id: String,
    pub component_id: String,
    /// The manifest digest the descriptor reports. Compared against the registry's recorded value.
    pub observed_manifest_digest: String,
    /// Capabilities the descriptor says it needs, unioned with the manifest's own list.
    pub claimed_capabilities: BTreeSet<String>,
    pub schema_digest: String,
    pub effect: ExtensionEffect,
    pub side_effecting: bool,
}

impl DiscoveredToolCandidate {
    /// 校验候选文本、摘要格式和能力集合大小；权限、来源、目录匹配在后续准入阶段检查。
    pub fn validate(&self) -> Result<(), String> {
        required_text(
            &self.tool_name,
            "discovery_candidate_tool",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        required_text(
            &self.namespace,
            "discovery_candidate_namespace",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        required_text(
            &self.operation,
            "discovery_candidate_operation",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        required_text(
            &self.extension_id,
            "discovery_candidate_extension",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        required_text(
            &self.component_id,
            "discovery_candidate_component",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        if !is_digest(&self.observed_manifest_digest) {
            return Err("discovery_candidate_manifest_digest_invalid".to_owned());
        }
        if !is_digest(&self.schema_digest) {
            return Err("discovery_candidate_schema_digest_invalid".to_owned());
        }
        if self.claimed_capabilities.len() > MAX_DISCOVERY_CAPABILITIES {
            return Err("discovery_candidate_capabilities_exhausted".to_owned());
        }
        for capability in &self.claimed_capabilities {
            required_text(
                capability,
                "discovery_candidate_capability",
                MAX_DISCOVERY_IDENT_BYTES,
            )?;
        }
        Ok(())
    }
}

/// The search step's own result. Modelling failure as a variant rather than an empty `Vec` is what
/// makes "the search failed" distinguishable from "the search found nothing", which is the whole
/// point of the `Unknown` status.
///
/// `Completed(vec![])` 表示搜索成功但没有结果；`Failed` 表示搜索没有完成。前者最终是
/// `Denied`，后者必须是 `Unknown` 并要求重试，不能把两者折叠为空列表。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscoverySearchOutcome {
    Completed(Vec<DiscoveredToolCandidate>),
    /// The search itself could not be completed. The reason is carried for logging; it never
    /// changes the decision, which is always `Unknown`.
    Failed {
        reason: String,
    },
}

/// The model-issued search, bound to the surface it was issued under. Binding the digest is what
/// makes a cached result detect its own staleness.
///
/// 此值把搜索文本和最大结果数量绑定到某一授权快照；若授权面被撤销、收窄、更新代次或
/// 更换目录，旧请求中的摘要会与当前快照不同，后续判定会先拒绝过期结果。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicDiscoveryRequest {
    /// 请求 wire schema 版本。
    pub schema: String,
    /// 本次搜索的关联标识，不是授权凭证。
    pub request_id: String,
    /// 搜索关键词；检查空值、最大字节数和敏感内容。
    pub query: String,
    /// 创建请求时所依据的授权面摘要。
    pub surface_digest: String,
    /// 请求产生时间，Unix 毫秒；只做非零形状检查，不在 `validate()` 内比较时效。
    pub issued_at_unix_ms: u64,
    /// 调用方期望的最大返回数，不能为零且不得超过模块硬上限。
    pub max_results: usize,
}

impl DynamicDiscoveryRequest {
    /// 检查请求版本、标识、搜索文本、绑定摘要和结果数边界。
    ///
    /// 此校验不访问当前时钟、不确认 digest 对应的快照仍有效，也不会执行搜索；时效和
    /// staleness 由 `DynamicDiscoveryReport::evaluate()` 收到当前 surface/time 后判断。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != DISCOVERY_REQUEST_SCHEMA {
            return Err("discovery_request_invalid".to_owned());
        }
        required_text(
            &self.request_id,
            "discovery_request_id",
            MAX_DISCOVERY_IDENT_BYTES,
        )?;
        if self.query.trim().is_empty() || self.query.len() > MAX_DISCOVERY_QUERY_BYTES {
            return Err("discovery_request_query_invalid".to_owned());
        }
        if redact_text(&self.query) != self.query {
            return Err("discovery_request_query_invalid".to_owned());
        }
        if !is_digest(&self.surface_digest) {
            return Err("discovery_request_surface_digest_invalid".to_owned());
        }
        if self.issued_at_unix_ms == 0 {
            return Err("discovery_request_issue_time_invalid".to_owned());
        }
        if self.max_results == 0 || self.max_results > MAX_DISCOVERY_RESULTS {
            return Err("discovery_request_limits_invalid".to_owned());
        }
        Ok(())
    }
}

/// Every refusal this module can produce, in the order the reducer evaluates them. The order is
/// fixed so a doubly-violating input always reports the same, most-serious reason.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum DiscoveryDenial {
    // --- request level ---
    /// The request shape is unusable, so no per-candidate question can be answered.
    RequestInvalid,
    /// The surface or one of its evidence records is corrupt.
    SurfaceInvalid,
    /// The request was issued under a different surface digest: the grant was revoked, narrowed,
    /// re-epoched or re-catalogued after the search ran.
    AuthorizationStale,
    /// The grant is withdrawn.
    AuthorizationRevoked,
    /// The grant has expired.
    AuthorizationExpired,
    /// The search itself failed. Reported as `Unknown`, never as allow-all or deny-all.
    SearchFailed,
    // --- candidate level ---
    /// The descriptor's own shape is unusable.
    CandidateInvalid,
    /// A second candidate claims an operation already claimed in this result.
    DuplicateCandidate,
    /// The namespace is outside the surface's namespace allow-list, or disagrees with the
    /// registry entry for that operation.
    UnknownNamespace,
    /// The operation is not in the surface's catalog.
    UnknownTool,
    /// The schema digest is unregistered, or disagrees with the registry entry for that operation.
    SchemaUnregistered,
    /// No server-owned evidence exists for the claimed extension component.
    EvidenceMissing,
    /// A project-local component was offered without a `Trusted` project trust verdict.
    ProjectTrustUnverified,
    /// Provenance is self-asserted or absent.
    ProvenanceUnverified,
    /// The descriptor's manifest digest is not the digest the registry recorded.
    ManifestDigestMismatch,
    /// The registered adapter is not `Available`; `RequiresApproval` is routed through the
    /// existing approval path rather than waved through by a search result.
    AdapterNotAvailable,
    /// The evidence was issued for another project.
    ProjectMismatch,
    /// The descriptor claims a different capability than the registry bound to that operation.
    CapabilityClaimMismatch,
    /// The operation's capability is real but this grant does not include it.
    CapabilityNotGranted,
    /// The operation is not inside the grant's scope operations.
    OperationOutOfScope,
    /// The extension asked for more than the parent grant allows.
    ExtensionScopeSuperset,
    /// The descriptor claims a stronger effect than the registry or the manifest recorded.
    EffectClaimEscalated,
    /// The descriptor's effect is above the surface's effect ceiling.
    EffectCeilingExceeded,
}

impl DiscoveryDenial {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RequestInvalid => "discovery_request_invalid",
            Self::SurfaceInvalid => "discovery_surface_invalid",
            Self::AuthorizationStale => "discovery_authorization_stale",
            Self::AuthorizationRevoked => "discovery_authorization_revoked",
            Self::AuthorizationExpired => "discovery_authorization_expired",
            Self::SearchFailed => "discovery_search_failed",
            Self::CandidateInvalid => "discovery_candidate_invalid",
            Self::DuplicateCandidate => "discovery_duplicate_candidate",
            Self::UnknownNamespace => "discovery_unknown_namespace",
            Self::UnknownTool => "discovery_unknown_tool",
            Self::SchemaUnregistered => "discovery_schema_unregistered",
            Self::EvidenceMissing => "discovery_evidence_missing",
            Self::ProjectTrustUnverified => "discovery_project_trust_unverified",
            Self::ProvenanceUnverified => "discovery_provenance_unverified",
            Self::ManifestDigestMismatch => "extension_manifest_digest_mismatch",
            Self::AdapterNotAvailable => "discovery_adapter_not_available",
            Self::ProjectMismatch => "discovery_project_mismatch",
            Self::CapabilityClaimMismatch => "discovery_capability_claim_mismatch",
            Self::CapabilityNotGranted => "discovery_capability_not_granted",
            Self::OperationOutOfScope => "discovery_operation_out_of_scope",
            Self::ExtensionScopeSuperset => "extension_scope_superset_denied",
            Self::EffectClaimEscalated => "discovery_effect_claim_escalated",
            Self::EffectCeilingExceeded => "discovery_effect_ceiling_exceeded",
        }
    }

    /// Lower runs first. The groups are, in order: the input cannot be answered at all; the answer
    /// is stale before it is a permission question; the search did not happen; the descriptor is
    /// not a real registered tool; the descriptor's supply chain is not trustworthy; the
    /// descriptor asks for something this grant does not have.
    pub const fn precedence(self) -> u8 {
        match self {
            Self::RequestInvalid => 0,
            Self::SurfaceInvalid => 1,
            Self::AuthorizationStale => 2,
            Self::AuthorizationRevoked => 3,
            Self::AuthorizationExpired => 4,
            Self::SearchFailed => 5,
            Self::CandidateInvalid => 6,
            Self::DuplicateCandidate => 7,
            Self::UnknownNamespace => 8,
            Self::UnknownTool => 9,
            Self::SchemaUnregistered => 10,
            Self::EvidenceMissing => 11,
            Self::ProjectTrustUnverified => 12,
            Self::ProvenanceUnverified => 13,
            Self::ManifestDigestMismatch => 14,
            Self::AdapterNotAvailable => 15,
            Self::ProjectMismatch => 16,
            Self::CapabilityClaimMismatch => 17,
            Self::CapabilityNotGranted => 18,
            Self::OperationOutOfScope => 19,
            Self::ExtensionScopeSuperset => 20,
            Self::EffectClaimEscalated => 21,
            Self::EffectCeilingExceeded => 22,
        }
    }

    /// Whether this refusal describes a search that never completed, as opposed to a search that
    /// completed and produced a definite answer.
    pub const fn is_unknown(self) -> bool {
        matches!(self, Self::SearchFailed)
    }
}

/// One denial, tied to the candidate it belongs to. `candidate_ref` is the refused operation (or
/// the tool name when the operation was too malformed to use) and is empty for request-level
/// refusals, so a caller can tell "the whole pass failed" from "these two tools failed".
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryDenialRecord {
    pub candidate_ref: String,
    pub code: &'static str,
}

impl DiscoveryDenialRecord {
    fn request_level(denial: DiscoveryDenial) -> Self {
        Self {
            candidate_ref: String::new(),
            code: denial.as_str(),
        }
    }

    /// The ref is built from the descriptor's own text, so it is bounded and secret-screened by
    /// [`DiscoveredToolCandidate::validate`] before it can be logged. A descriptor that failed
    /// validation has no trustworthy text left, so it is referenced by position instead.
    fn candidate_level(
        candidate: &DiscoveredToolCandidate,
        index: usize,
        denial: DiscoveryDenial,
    ) -> Self {
        let candidate_ref = if candidate.validate().is_ok() && !candidate.operation.is_empty() {
            candidate.operation.clone()
        } else {
            format!("candidate[{index}]")
        };
        Self {
            candidate_ref,
            code: denial.as_str(),
        }
    }
}

/// A candidate that survived every check. Every value here is copied from the surface or computed
/// as an intersection, never copied from the descriptor's claim.
///
/// 这是“可继续被调用方考虑”的结果记录，不是已经授权的执行请求，也不是执行
/// 许可。任何真正的能力调用仍必须重新经过 Core 的授权、scope、permit 和 Broker 路径。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdmittedDiscoveryCandidate {
    /// 来自服务端目录的工具名。
    pub tool_name: String,
    /// 来自服务端目录的命名空间。
    pub namespace: String,
    /// 来自服务端目录的精确操作名。
    pub operation: String,
    /// 与服务端目录绑定的稳定能力键。
    pub capability_key: String,
    /// The intersection of what the manifest plus the descriptor asked for and what the grant
    /// allows. Provenance of this set is the intersection, so it can never exceed the grant.
    pub effective_capabilities: BTreeSet<String>,
    /// 由服务端登记效果和候选声明共同核对后的效果级别。
    pub effect: ExtensionEffect,
    /// Taken from the registry, not from the claim, so a descriptor cannot understate it.
    pub side_effecting: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiscoveryStatus {
    /// At least one candidate was admitted. The only status from which `promotable` yields
    /// anything.
    Admitted,
    /// The search completed and produced no admissible candidate.
    Denied,
    /// The search did not complete. A caller must re-run the search; this report is not an answer.
    Unknown,
}

impl DiscoveryStatus {
    /// 输出给报告/日志的稳定状态名称；不会触发重试或执行。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Admitted => "admitted",
            Self::Denied => "denied",
            Self::Unknown => "unknown",
        }
    }
}

/// The sealed decision. `promotable` is the only way to get candidates out of it, and it is empty
/// unless the status is [`DiscoveryStatus::Admitted`], so an `Unknown` or `Denied` report cannot be
/// degraded into "allow everything" or "deny everything and continue" by a careless caller.
///
/// 报告把请求 ID、授权面/目录摘要、授权代次、通过项、拒绝项和重试要求收拢为一个结果。
/// digest 只能发现内容与封装值不一致；需要用原始输入重新执行同一纯判定的
/// `validate_against()` 才能检查报告是否忠实反映输入。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicDiscoveryReport {
    /// 报告结构版本。
    pub schema: String,
    /// Admitted=至少一个候选通过；Denied=搜索完成但没有候选通过；Unknown=搜索失败。
    pub status: DiscoveryStatus,
    /// 与输入请求相同的关联标识。
    pub request_id: String,
    /// 生成报告所依据的授权快照摘要。
    pub surface_digest: String,
    /// 生成报告时授权快照登记的工具目录摘要。
    pub catalog_digest: String,
    /// 生成报告时的授权代次。
    pub authority_epoch: u64,
    /// 通过全部校验的候选；只有状态为 Admitted 才能由 `promotable()` 暴露。
    pub admitted: Vec<AdmittedDiscoveryCandidate>,
    /// 被拒绝的候选及稳定原因；请求级错误用空 candidate_ref 表示。
    pub denials: Vec<DiscoveryDenialRecord>,
    /// True only for [`DiscoveryStatus::Unknown`]: the search must be re-run before anything may
    /// proceed. A `Denied` report is a final answer for this surface and must not be re-rolled to
    /// fish for a different result.
    pub retry_required: bool,
    pub digest: String,
}

impl DynamicDiscoveryReport {
    /// The only accessor for admitted candidates. Non-`Admitted` reports yield nothing.
    pub fn promotable(&self) -> &[AdmittedDiscoveryCandidate] {
        if self.status == DiscoveryStatus::Admitted {
            &self.admitted
        } else {
            &[]
        }
    }

    /// The stable refusal codes in decision order, for logs, receipts and the guard fixture.
    pub fn denial_codes(&self) -> Vec<&'static str> {
        self.denials.iter().map(|record| record.code).collect()
    }

    /// The seal over this report's own contents. Exposed so a fixture can construct a
    /// *structurally* invalid but correctly sealed report and prove that
    /// [`DynamicDiscoveryReport::validate_against`] refuses it for the right reason.
    ///
    /// Re-sealing is **not** a way to make a report valid: `validate_against` re-derives the whole
    /// decision from the inputs and compares every field, so a re-sealed forgery still fails with
    /// `discovery_report_binding_invalid`. The seal catches casual mutation; the re-derivation is
    /// the actual defense.
    pub fn sealed_digest(&self) -> String {
        self.unsigned_digest()
    }

    fn unsigned_digest(&self) -> String {
        let admitted = self
            .admitted
            .iter()
            .map(|candidate| {
                json!({
                    "tool_name": candidate.tool_name,
                    "namespace": candidate.namespace,
                    "operation": candidate.operation,
                    "capability_key": candidate.capability_key,
                    "effective_capabilities": candidate
                        .effective_capabilities
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>(),
                    "effect": candidate.effect,
                    "side_effecting": candidate.side_effecting,
                })
            })
            .collect::<Vec<_>>();
        let denials = self
            .denials
            .iter()
            .map(|record| json!({"candidate_ref": record.candidate_ref, "code": record.code}))
            .collect::<Vec<_>>();
        json_digest(&json!({
            "schema": self.schema,
            "status": self.status.as_str(),
            "request_id": self.request_id,
            "surface_digest": self.surface_digest,
            "catalog_digest": self.catalog_digest,
            "authority_epoch": self.authority_epoch,
            "admitted": admitted,
            "denials": denials,
            "retry_required": self.retry_required,
        }))
    }

    fn seal(mut self) -> Self {
        self.digest = self.unsigned_digest();
        self
    }

    /// 依固定验证顺序把一组输入归约为单个不可变的候选准入报告。
    ///
    /// 【调用者/当前使用情况】设计上由搜索流程在拿到授权快照、服务端扩展证据和搜索返回值
    /// 后调用；当前仓库源码中没有找到 Core/Daemon/Runner 的生产调用点，现有调用位于
    /// CAP-30 契约测试。
    ///
    /// 【输入】绑定授权面摘要的请求、当前授权快照、服务端准入证据、搜索结果以及调用方
    /// 提供的当前 Unix 毫秒时间。
    ///
    /// 【输出与状态】始终返回带内容摘要的报告，不返回 `Result`。请求/授权面无效或过期时
    /// 返回 `Denied`；搜索失败时返回 `Unknown` 且 `retry_required=true`；搜索完成后至少
    /// 一个候选通过才是 `Admitted`，否则是 `Denied`。候选级拒绝可以和通过项同存。
    ///
    /// 【副作用与设计原因】这是纯同步计算，不读文件/网络、不写事件、不启动 adapter，也
    /// 不校验真实签名。先检查请求、授权面摘要、撤销和过期，再检查搜索结果，最后逐候选
    /// 按固定优先级检查，保证过期授权不会继续参与候选决策，且多项违规得到稳定原因。
    ///
    /// Reduce one discovery pass to a decision. Pure: it creates no catalog, starts no adapter,
    /// reads no file, calls no port and appends no event.
    pub fn evaluate(
        request: &DynamicDiscoveryRequest,
        surface: &DiscoverySurface,
        evidence: &BTreeMap<String, ExtensionAdmissionEvidence>,
        outcome: &DiscoverySearchOutcome,
        now_unix_ms: u64,
    ) -> Self {
        let denied = |denial: DiscoveryDenial| {
            Self::new(
                DiscoveryStatus::Denied,
                request,
                surface,
                Vec::new(),
                vec![DiscoveryDenialRecord::request_level(denial)],
                false,
            )
        };

        if request.validate().is_err() {
            return denied(DiscoveryDenial::RequestInvalid);
        }
        if surface.validate().is_err() || evidence.values().any(|record| record.validate().is_err())
        {
            return denied(DiscoveryDenial::SurfaceInvalid);
        }
        // Staleness first: once the grant has moved, every per-candidate question would be
        // answered against a surface that no longer exists.
        if request.surface_digest != surface.digest {
            return denied(DiscoveryDenial::AuthorizationStale);
        }
        if surface.revoked {
            return denied(DiscoveryDenial::AuthorizationRevoked);
        }
        if now_unix_ms >= surface.expires_at_unix_ms {
            return denied(DiscoveryDenial::AuthorizationExpired);
        }

        let DiscoverySearchOutcome::Completed(candidates) = outcome else {
            return Self::new(
                DiscoveryStatus::Unknown,
                request,
                surface,
                Vec::new(),
                vec![DiscoveryDenialRecord::request_level(
                    DiscoveryDenial::SearchFailed,
                )],
                true,
            );
        };
        if candidates.len() > MAX_DISCOVERY_CANDIDATES {
            return denied(DiscoveryDenial::RequestInvalid);
        }

        let mut admitted = Vec::new();
        let mut denials = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, candidate) in candidates.iter().enumerate() {
            match admit_candidate(candidate, surface, evidence, &mut seen) {
                Ok(entry) => admitted.push(entry),
                Err(denial) => denials.push(DiscoveryDenialRecord::candidate_level(
                    candidate, index, denial,
                )),
            }
        }

        let status = if admitted.is_empty() {
            DiscoveryStatus::Denied
        } else {
            DiscoveryStatus::Admitted
        };
        Self::new(status, request, surface, admitted, denials, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        status: DiscoveryStatus,
        request: &DynamicDiscoveryRequest,
        surface: &DiscoverySurface,
        admitted: Vec<AdmittedDiscoveryCandidate>,
        denials: Vec<DiscoveryDenialRecord>,
        retry_required: bool,
    ) -> Self {
        Self {
            schema: DISCOVERY_REPORT_SCHEMA.to_owned(),
            status,
            request_id: request.request_id.clone(),
            surface_digest: surface.digest.clone(),
            catalog_digest: surface.catalog_digest.clone(),
            authority_epoch: surface.authority_epoch,
            admitted,
            denials,
            retry_required,
            digest: String::new(),
        }
        .seal()
    }

    /// Re-derive the whole decision from the same inputs and refuse any report that disagrees, so
    /// a hand-edited status or an appended candidate is caught rather than believed. A report whose
    /// own digest does not cover its contents is refused too.
    pub fn validate_against(
        report: &DynamicDiscoveryReport,
        request: &DynamicDiscoveryRequest,
        surface: &DiscoverySurface,
        evidence: &BTreeMap<String, ExtensionAdmissionEvidence>,
        outcome: &DiscoverySearchOutcome,
        now_unix_ms: u64,
    ) -> Result<(), String> {
        if report.schema != DISCOVERY_REPORT_SCHEMA {
            return Err("discovery_report_schema_invalid".to_owned());
        }
        if report.digest != report.unsigned_digest() {
            return Err("discovery_report_digest_mismatch".to_owned());
        }
        // A report may not claim to be usable when its own status says it is not.
        if report.status != DiscoveryStatus::Admitted && !report.admitted.is_empty() {
            return Err("discovery_report_admission_without_status".to_owned());
        }
        if report.status != DiscoveryStatus::Unknown && report.retry_required {
            return Err("discovery_report_retry_without_unknown".to_owned());
        }
        if report.status == DiscoveryStatus::Unknown
            && report.denial_codes() != vec![DiscoveryDenial::SearchFailed.as_str()]
        {
            return Err("discovery_report_unknown_reason_invalid".to_owned());
        }
        let expected = Self::evaluate(request, surface, evidence, outcome, now_unix_ms);
        if report.status != expected.status
            || report.admitted != expected.admitted
            || report.denial_codes() != expected.denial_codes()
            || report.surface_digest != expected.surface_digest
            || report.catalog_digest != expected.catalog_digest
            || report.authority_epoch != expected.authority_epoch
            || report.retry_required != expected.retry_required
            || report.digest != expected.digest
        {
            return Err("discovery_report_binding_invalid".to_owned());
        }
        Ok(())
    }
}

/// 对单个不可信候选按固定优先级与授权面、服务端目录和扩展证据逐项比较。
///
/// 成功值中名称/操作/能力/效果来自已登记目录，`effective_capabilities` 是声明并集与授予
/// 集合的交集。返回 `Err(denial)` 时整条候选拒绝，不尝试修剪成“勉强可用”的子集；调用者
/// 会把拒绝理由记录到报告中，而不会让候选触发执行。
///
/// Decide one candidate against the surface, in the fixed order documented on
/// [`DiscoveryDenial::precedence`]. At most one refusal is produced, so the report is a function of
/// the inputs rather than of the order in which checks happen to be written.
fn admit_candidate(
    candidate: &DiscoveredToolCandidate,
    surface: &DiscoverySurface,
    evidence: &BTreeMap<String, ExtensionAdmissionEvidence>,
    seen: &mut BTreeSet<(String, String)>,
) -> Result<AdmittedDiscoveryCandidate, DiscoveryDenial> {
    candidate
        .validate()
        .map_err(|_| DiscoveryDenial::CandidateInvalid)?;

    if !seen.insert((candidate.namespace.clone(), candidate.operation.clone())) {
        return Err(DiscoveryDenial::DuplicateCandidate);
    }

    // The registry is the only source of a tool's identity. An operation nobody registered is not
    // a tool, whatever the result calls it.
    let Some(registered) = surface.catalog.get(&candidate.operation) else {
        return Err(DiscoveryDenial::UnknownTool);
    };
    if !surface.allows_namespace(&candidate.namespace)
        || candidate.namespace != registered.namespace
    {
        return Err(DiscoveryDenial::UnknownNamespace);
    }
    if candidate.schema_digest != registered.schema_digest {
        return Err(DiscoveryDenial::SchemaUnregistered);
    }

    let key = evidence_key(&candidate.extension_id, &candidate.component_id);
    let Some(record) = evidence.get(key.as_str()) else {
        return Err(DiscoveryDenial::EvidenceMissing);
    };
    if record.project_local && record.project_trust != ProjectTrustVerdict::Trusted {
        return Err(DiscoveryDenial::ProjectTrustUnverified);
    }
    if !record.provenance.is_verified() {
        return Err(DiscoveryDenial::ProvenanceUnverified);
    }
    if candidate.observed_manifest_digest != record.registered_manifest_digest {
        return Err(DiscoveryDenial::ManifestDigestMismatch);
    }
    if record.status != ExtensionAdapterStatus::Available {
        return Err(DiscoveryDenial::AdapterNotAvailable);
    }
    if record.project_id != surface.project_id {
        return Err(DiscoveryDenial::ProjectMismatch);
    }

    let claimed_key = capability_key(&candidate.capability);
    let registered_key = capability_key(&registered.capability);
    if claimed_key != registered_key {
        return Err(DiscoveryDenial::CapabilityClaimMismatch);
    }
    if !surface.granted_capabilities.contains(&registered_key) {
        return Err(DiscoveryDenial::CapabilityNotGranted);
    }
    if !surface.scope.allows_operation(&candidate.operation) {
        return Err(DiscoveryDenial::OperationOutOfScope);
    }

    // Permission union: union what the manifest needs with what the descriptor claims, then
    // intersect with the grant. Anything left over means the request was a superset.
    let requested = record
        .required_capabilities
        .union(&candidate.claimed_capabilities)
        .cloned()
        .collect::<BTreeSet<_>>();
    let effective = intersect_extension_scope(&requested, &surface.granted_capabilities);
    if effective != requested {
        return Err(DiscoveryDenial::ExtensionScopeSuperset);
    }

    // Effect is a property of the verified manifest and the registry, never of the descriptor.
    let registered_effect = match (registered.effect, record.effect) {
        (ExtensionEffect::ReadWrite, ExtensionEffect::ReadWrite) => ExtensionEffect::ReadWrite,
        _ => ExtensionEffect::ReadOnly,
    };
    if candidate.effect == ExtensionEffect::ReadWrite
        && registered_effect == ExtensionEffect::ReadOnly
    {
        return Err(DiscoveryDenial::EffectClaimEscalated);
    }
    if surface.effect_ceiling == ExtensionEffect::ReadOnly
        && registered_effect == ExtensionEffect::ReadWrite
    {
        return Err(DiscoveryDenial::EffectCeilingExceeded);
    }

    Ok(AdmittedDiscoveryCandidate {
        tool_name: registered.tool_name.clone(),
        namespace: registered.namespace.clone(),
        operation: candidate.operation.clone(),
        capability_key: registered_key,
        effective_capabilities: effective,
        effect: registered_effect,
        side_effecting: registered.side_effecting,
    })
}

/// The evidence map key for one extension component.
pub fn evidence_key(extension_id: &str, component_id: &str) -> String {
    format!("{extension_id}#{component_id}")
}

/// The namespace of an operation: the segment before the first `.`, or `None` when the operation has
/// no namespace at all. Namespaces are derived, never declared — a descriptor cannot invent one.
pub fn operation_namespace(operation: &str) -> Option<&str> {
    let (namespace, rest) = operation.split_once('.')?;
    if namespace.is_empty() || rest.is_empty() {
        None
    } else {
        Some(namespace)
    }
}

/// Canonical capability key for a [`CapabilityKind`], matching the snake_case wire spelling.
/// `Other(name)` becomes `other.<name>` so an unreviewed category cannot collide with a built-in
/// one.
pub fn capability_key(capability: &CapabilityKind) -> String {
    match serde_json::to_value(capability) {
        Ok(serde_json::Value::String(name)) => name,
        Ok(serde_json::Value::Object(mut map)) => match map.remove("other") {
            Some(serde_json::Value::String(name)) => format!("other.{name}"),
            _ => "other".to_owned(),
        },
        _ => "unknown".to_owned(),
    }
}

/// Intersect what an extension asked for with what the parent grant allows. The result is the
/// *only* set an admitted candidate may carry: it can never contain a capability the grant does not
/// already have, which is what makes the permission union non-amplifying.
pub fn intersect_extension_scope(
    requested: &BTreeSet<String>,
    granted: &BTreeSet<String>,
) -> BTreeSet<String> {
    requested.intersection(granted).cloned().collect()
}

fn required_text(value: &str, code: &'static str, max_bytes: usize) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || value != trimmed
        || value.len() > max_bytes
        || value.contains(['\0', '\n', '\r'])
        || redact_text(value) != value
    {
        return Err(code.to_owned());
    }
    Ok(())
}

fn is_hex_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(is_hex_sha256)
}
