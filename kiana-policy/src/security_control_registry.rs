//! SC-34 security control registry: what a control claims, who owns it, and how far the
//! evidence actually reaches.
//!
//! A control in this module is a *claim about a control*, not a certification. The whole point of
//! the file is that three different things are kept apart and none of them may be substituted for
//! another:
//!
//! ```text
//! framework clause  (SEC-09 / GOVERN / LLM01 / INV-S09)  -- what the standard says
//! control           (ctl-...)                            -- what Kiana says it does about it
//! evidence          (ControlEvidence, each with `proves`) -- what has actually been demonstrated
//! ```
//!
//! The failure this exists to refuse is the one named on the SC-34 card: a unit test or a type
//! being described as a regulatory certification. That failure is only preventable if the ceiling
//! a control claims is stored *next to* the evidence it has, and the registry refuses any
//! registration where the ceiling is above the evidence. A control whose strongest evidence is
//! `source` cannot be registered as `durable`, however confident the prose around it is.
//!
//! Four further rules are load-bearing rather than cosmetic:
//!
//! * **A control with no owner cannot exist.** A control nobody owns has nobody who can be asked
//!   for the evidence, so it is not a control; it is a wish.
//! * **Scope only narrows.** A child control derives its scope by intersecting the parent's, so a
//!   child can never claim coverage the parent does not have.
//! * **Unknown stays Unknown.** Missing evidence folds to `ControlStatus::Unknown`, never to
//!   `Effective`. There is no code path from "we did not check" to "this passes".
//! * **Unknown major and unknown clauses fail closed.** A registry that silently accepted a
//!   clause it has never heard of would let a typo look like a mapped control.
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! docs/roadmap/security-threat-register.md   (SC-01: T01..T12)
//! docs/roadmap/security-control-crosswalk.md (SC-34: T/SEC/NIST/OWASP/内部 交叉映射)
//!        ↓ 人/工具准备控制项草案
//! 【本文件：SecurityControlRegistry::register】   ← 纯判定，不碰文件系统与网络
//!        ↓ 接受或给出稳定 reason code
//! SC-35 EvidenceManifest / SC-41 security gate  （记录与放行，尚未接线）
//! ```
//!
//! # ⚠ 当前状态：只读契约，尚未接入产品路径
//!
//! 本文件不写文件、不发网络、不调用任何 port 或 adapter、不追加事件、不新增执行循环。
//! 它能回答的只有一个问题：**给定一份控制项草案，它是否可被登记，以及它到底证明了什么。**
//! 它不能证明控制真的生效了 —— 那是 CI 与运行时证据的事，本文件看不到它们。
//!
//! # 术语
//!
//! - **control**：一条控制项声明，例如"每次外部副作用都要经过精确 permit"。
//! - **framework / clause**：控制项映射到的外部或内部条款。
//! - **proof ceiling**：控制项**声明**要达到的最高证明等级。
//! - **evidence**：控制项**实际**持有的证据，每条证据自己声明它证明了哪个等级。
//! - **owner**：对控制项负责的人或角色。空 owner 一律拒绝。
//! - **fail-closed**：不确定就拒绝，而不是猜一个宽松结论。

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, SchemaVersion, SecretScanChannel,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const SECURITY_CONTROL_SCHEMA: &str = "kiana.security-control.v1";
pub const SECURITY_CONTROL_REGISTRY_SCHEMA: &str = "kiana.security-control-registry.v1";
pub const SECURITY_CONTROL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SECURITY_CONTROL_TEXT: usize = 256;
pub const MAX_SECURITY_CONTROL_MAPPINGS: usize = 32;
pub const MAX_SECURITY_CONTROL_ASSUMPTIONS: usize = 16;
pub const MAX_SECURITY_CONTROL_EVIDENCE: usize = 32;
pub const MAX_SECURITY_CONTROL_REGISTRY_ENTRIES: usize = 512;

/// 证据的证明等级，从弱到强。
///
/// 顺序即强度：[`ProofLevel::Source`] 最弱，[`ProofLevel::Physical`] 最强。声明顺序决定了
/// `Ord`，所以 `ceiling > demonstrated` 表达的就是"声称的比证据强"。
///
/// 名称直接沿用 AGENTS.md 的 `proof_level` 词表，不另造一套等级名 —— 一旦这里出现
/// "基本可信""大致可用"这类词，就等于给"把单测当认证"留了后门。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProofLevel {
    /// 只有源码、类型和文档可见。
    Source,
    /// 有本地可重复的运行证据。
    LocalBehavior,
    /// 跨进程可重建。
    Durable,
    /// 打到真实外部边界并有逐连接证据。
    Live,
    /// 物理世界回执。
    Physical,
}

impl ProofLevel {
    pub const ALL: [Self; 5] = [
        Self::Source,
        Self::LocalBehavior,
        Self::Durable,
        Self::Live,
        Self::Physical,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::LocalBehavior => "local_behavior",
            Self::Durable => "durable",
            Self::Live => "live",
            Self::Physical => "physical",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "source" => Ok(Self::Source),
            "local_behavior" => Ok(Self::LocalBehavior),
            "durable" => Ok(Self::Durable),
            "live" => Ok(Self::Live),
            "physical" => Ok(Self::Physical),
            _ => Err("security_control_proof_level_invalid".to_owned()),
        }
    }
}

/// 控制项可以映射到的四个框架。
///
/// 集合是封闭的：多一个"其他框架"就等于多一条可以把任意条款塞进登记表的路。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityFramework {
    /// 公司安全宪法 SEC-01 至 SEC-12。
    Sec,
    /// NIST AI RMF 四大功能。
    Nist,
    /// OWASP LLM Top 10。
    Owasp,
    /// Kiana 内部控制（INV-S* 不变量与 roadmap 卡片）。
    Internal,
}

impl SecurityFramework {
    pub const ALL: [Self; 4] = [Self::Sec, Self::Nist, Self::Owasp, Self::Internal];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sec => "sec",
            Self::Nist => "nist",
            Self::Owasp => "owasp",
            Self::Internal => "internal",
        }
    }

    /// 该框架当前被登记册接受的 major 版本。
    ///
    /// 写入控制项的 major 与此不一致一律拒绝（fail-closed）：宁可不认，也不要拿一份
    /// 结构已经变过的条款表去对照旧控制项。
    pub const fn supported_major(self) -> u32 {
        1
    }

    /// 该框架下被承认的条款。
    ///
    /// 这是一张**封闭白名单**：不在表里的字符串一律 `security_control_framework_clause_unknown`。
    /// 表外的拼写错误、编号漂移和"看起来像"的自造条款都在这里被挡住。
    pub fn known_clauses(self) -> &'static [&'static str] {
        match self {
            Self::Sec => &[
                "SEC-01", "SEC-02", "SEC-03", "SEC-04", "SEC-05", "SEC-06", "SEC-07", "SEC-08",
                "SEC-09", "SEC-10", "SEC-11", "SEC-12",
            ],
            Self::Nist => &["GOVERN", "MAP", "MEASURE", "MANAGE"],
            Self::Owasp => &[
                "LLM01", "LLM02", "LLM03", "LLM04", "LLM05", "LLM06", "LLM07", "LLM08", "LLM09",
                "LLM10",
            ],
            Self::Internal => &[
                "INV-S01", "INV-S02", "INV-S03", "INV-S04", "INV-S05", "INV-S06", "INV-S07",
                "INV-S08", "INV-S09", "INV-S10", "INV-S11", "INV-S12", "SC-34",
            ],
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "sec" => Ok(Self::Sec),
            "nist" => Ok(Self::Nist),
            "owasp" => Ok(Self::Owasp),
            "internal" => Ok(Self::Internal),
            _ => Err("security_control_framework_unknown".to_owned()),
        }
    }
}

/// 一条"控制项 → 框架条款"的映射。
///
/// 单独成为结构体的原因：映射是 SC-34 卡片里"把控制宣称为法规条款"这件事发生的**唯一位置**。
/// 每条映射都必须自己带 `major_version` 并自己过白名单，登记册不能替它默认一个。
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkClause {
    pub framework: SecurityFramework,
    /// 登记时声明的框架 major 版本，必须等于 [`SecurityFramework::supported_major`]。
    pub major_version: u32,
    /// 条款标识，必须命中 [`SecurityFramework::known_clauses`]。
    pub clause: String,
}

impl FrameworkClause {
    /// 构造并校验一条映射。
    ///
    /// 校验顺序有意从粗到细：先框架，再 major，最后条款。先报"框架不认识"，
    /// 调用方才知道要去修框架而不是修条款号。
    pub fn new(
        framework: SecurityFramework,
        major_version: u32,
        clause: impl Into<String>,
    ) -> Result<Self, String> {
        let mapping = Self {
            framework,
            major_version,
            clause: clause.into(),
        };
        mapping.validate()?;
        Ok(mapping)
    }

    pub fn validate(&self) -> Result<(), String> {
        if major_is_unknown(self.framework, self.major_version) {
            return Err("security_control_framework_major_unknown".to_owned());
        }
        if !self
            .framework
            .known_clauses()
            .iter()
            .any(|known| known.eq_ignore_ascii_case(self.clause.trim()))
        {
            return Err("security_control_framework_clause_unknown".to_owned());
        }
        Ok(())
    }
}

/// 内部控制覆盖的面向。
///
/// 面向取自安全宪法的不变量清单，而不是随手起的分类名：这样"范围只能收窄"才有一个
/// 稳定、可比较的维度，而不是靠字符串前缀猜。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlScopeFacet {
    Identity,
    Authorization,
    ScopeLimit,
    CellResource,
    ExternalEffect,
    Secret,
    Loopback,
    Filesystem,
    Cancellation,
    UnknownState,
    Audit,
    UntrustedInput,
    ResourceExhaustion,
}

impl ControlScopeFacet {
    pub const ALL: [Self; 13] = [
        Self::Identity,
        Self::Authorization,
        Self::ScopeLimit,
        Self::CellResource,
        Self::ExternalEffect,
        Self::Secret,
        Self::Loopback,
        Self::Filesystem,
        Self::Cancellation,
        Self::UnknownState,
        Self::Audit,
        Self::UntrustedInput,
        Self::ResourceExhaustion,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Identity => "identity",
            Self::Authorization => "authorization",
            Self::ScopeLimit => "scope_limit",
            Self::CellResource => "cell_resource",
            Self::ExternalEffect => "external_effect",
            Self::Secret => "secret",
            Self::Loopback => "loopback",
            Self::Filesystem => "filesystem",
            Self::Cancellation => "cancellation",
            Self::UnknownState => "unknown_state",
            Self::Audit => "audit",
            Self::UntrustedInput => "untrusted_input",
            Self::ResourceExhaustion => "resource_exhaustion",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        ControlScopeFacet::ALL
            .into_iter()
            .find(|facet| facet.as_str() == value.trim())
            .ok_or_else(|| "security_control_scope_facet_unknown".to_owned())
    }
}

/// 一条控制项的覆盖范围：一组面向的集合。
///
/// 之所以是集合而不是一段自由文本：交集运算只有在集合上才是可比较的。"范围只能收窄"
/// 必须是**可计算的**，不能靠评审者读完两段散文后凭感觉判断。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ControlScope {
    pub facets: BTreeSet<ControlScopeFacet>,
}

impl ControlScope {
    pub fn new(facets: impl IntoIterator<Item = ControlScopeFacet>) -> Self {
        Self {
            facets: facets.into_iter().collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.facets.is_empty()
    }

    /// 交集：父与子同时覆盖才算覆盖。
    pub fn intersect(&self, other: &Self) -> Self {
        Self {
            facets: self
                .facets
                .intersection(&other.facets)
                .copied()
                .collect::<BTreeSet<ControlScopeFacet>>(),
        }
    }

    /// `true` 当且仅当 `self` 不含 `other` 之外的任何面向。
    pub fn is_subset_of(&self, other: &Self) -> bool {
        self.facets.is_subset(&other.facets)
    }
}

/// 控制项当前的判定。
///
/// 顺序即"通过强度"：`Unknown` 最弱，`Effective` 最强。派生判定只会产出
/// `Unknown` / `Partial` / `Effective`；`NotEffective` 是人给出的显式否定结论。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlStatus {
    NotEffective,
    Unknown,
    Partial,
    Effective,
}

impl ControlStatus {
    pub const ALL: [Self; 4] = [
        Self::NotEffective,
        Self::Unknown,
        Self::Partial,
        Self::Effective,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NotEffective => "not_effective",
            Self::Unknown => "unknown",
            Self::Partial => "partial",
            Self::Effective => "effective",
        }
    }

    /// 是否属于"声称通过"的两档。
    ///
    /// 这两档是"把单测当认证"真正会落进去的格子；拒绝语义只针对它们，
    /// 因为 `Unknown` 和 `NotEffective` 都不是通过，不构成夸大。
    pub const fn claims_pass(self) -> bool {
        matches!(self, Self::Partial | Self::Effective)
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "not_effective" => Ok(Self::NotEffective),
            "unknown" => Ok(Self::Unknown),
            "partial" => Ok(Self::Partial),
            "effective" => Ok(Self::Effective),
            _ => Err("security_control_status_invalid".to_owned()),
        }
    }
}

/// 一条已提交的事实，用来支撑控制项。
///
/// 证据是引用加摘要加"它证明了哪一档"，**从不携带载荷**。这条限制的用处和 SC-33 的
/// 事故证据一样：一份声称证明 `durable` 的证据如果需要把内容贴进来才能自证，
/// 它多半是在把秘密或大块数据复制进登记册。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlEvidence {
    pub evidence_ref: String,
    pub source_digest: String,
    /// 这条证据实际证明到的等级。
    pub proves: ProofLevel,
    pub evidence_digest: String,
}

impl ControlEvidence {
    /// 构造一条证据并计算摘要。
    pub fn new(
        evidence_ref: impl Into<String>,
        source_digest: impl Into<String>,
        proves: ProofLevel,
    ) -> Result<Self, String> {
        let mut evidence = Self {
            evidence_ref: evidence_ref.into(),
            source_digest: source_digest.into(),
            proves,
            evidence_digest: String::new(),
        };
        validate_digest(&evidence.source_digest, "security_control_source_digest")?;
        safe_text(&evidence.evidence_ref, "security_control_evidence_ref")?;
        evidence.evidence_digest = evidence.digest();
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), String> {
        safe_text(&self.evidence_ref, "security_control_evidence_ref")?;
        validate_digest(&self.source_digest, "security_control_source_digest")?;
        validate_digest(&self.evidence_digest, "security_control_evidence_digest")?;
        if self.evidence_digest != self.digest() {
            return Err("security_control_evidence_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "evidence_ref": self.evidence_ref,
            "source_digest": self.source_digest,
            "proves": self.proves,
        }))
    }
}

/// 一条控制项声明。
///
/// 字段可以按"身份 / 声称 / 证据 / 范围"四组读：
/// - **身份**：`control_id`、`title`、`owner`；
/// - **声称**：`mappings`（映射到哪些条款）、`proof_ceiling`（声称到哪一档）、`status`；
/// - **证据**：`evidence`（每条自带 `proves`）；
/// - **范围**：`scope`（覆盖哪些面向）、`assumptions`（成立前提）、`parent`（继承来源）。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityControl {
    pub schema: String,
    pub version: SchemaVersion,
    pub control_id: String,
    pub title: String,
    /// 对这条控制项负责的人或角色。**空值一律拒绝。**
    pub owner: String,
    /// 父控制项。存在时 `scope` 必须是父控制项 `scope` 的子集。
    pub parent: Option<String>,
    pub scope: ControlScope,
    pub assumptions: Vec<String>,
    pub mappings: Vec<FrameworkClause>,
    pub evidence: Vec<ControlEvidence>,
    /// 声称达到的最高证明等级。`None` 表示尚未声称任何等级。
    pub proof_ceiling: Option<ProofLevel>,
    pub status: ControlStatus,
    pub control_digest: String,
}

impl SecurityControl {
    /// 登记册里最常见的一条控制项形状。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        control_id: impl Into<String>,
        title: impl Into<String>,
        owner: impl Into<String>,
        parent: Option<String>,
        scope: ControlScope,
        assumptions: Vec<String>,
        mappings: Vec<FrameworkClause>,
        evidence: Vec<ControlEvidence>,
        proof_ceiling: Option<ProofLevel>,
        status: ControlStatus,
    ) -> Result<Self, String> {
        let mut control = Self {
            schema: SECURITY_CONTROL_SCHEMA.to_owned(),
            version: SECURITY_CONTROL_VERSION,
            control_id: control_id.into(),
            title: title.into(),
            owner: owner.into(),
            parent,
            scope,
            assumptions,
            mappings,
            evidence,
            proof_ceiling,
            status,
            control_digest: String::new(),
        };
        // 【为什么必须先盖章再校验，而不是反过来】
        // `control_digest` 是**派生值**：`digest()` 只对 schema/version/control_id/title/
        // owner/parent/scope/assumptions/mappings/evidence/proof_ceiling/status 求摘要，
        // 并不把 `control_digest` 自身算进去，因此这里不存在循环依赖。
        //
        // 原写法是「先 validate()、再盖章」，而 validate() 的第一步就是
        // `validate_digest(&self.control_digest, ...)`，要求 `sha256:` 前缀加 64 位 hex。
        // 此刻该字段还是 `String::new()`，于是每一次 `SecurityControl::new()` 都必然返回
        // `security_control_digest_invalid` —— 构造器从未能成功过一次，SC-34 的 18 个用例
        // 全部因此失败。改成先盖章再校验：validate() 依旧完整执行（含 digest 与重算值的
        // 一致性检查），没有任何检查被跳过或放宽。
        control.control_digest = control.digest();
        control.validate()?;
        Ok(control)
    }

    /// 证据实际证明到的最强一档。**没有证据就是 `None`，不是 `Source`。**
    ///
    /// 区分 `None` 与 `Some(Source)` 是本文件最重要的一行：`None` 表示"什么都没验过"，
    /// 如果把它折算成 `Source`，一条零证据的控制项就能冒充"至少有源码"。
    pub fn demonstrated_proof(&self) -> Option<ProofLevel> {
        self.evidence
            .iter()
            .map(|item| item.proves)
            .max()
            .filter(|_| !self.evidence.is_empty())
    }

    /// 不看 `status` 字段、只按证据算出来的诚实判定。
    ///
    /// 证据缺席就是 `Unknown`。这里没有"缺省通过"这个分支，也没有把空集合当全集的写法。
    pub fn honest_status(&self) -> ControlStatus {
        let Some(demonstrated) = self.demonstrated_proof() else {
            return ControlStatus::Unknown;
        };
        match self.proof_ceiling {
            // 声称的等级被证据完全覆盖。
            Some(ceiling) if demonstrated >= ceiling => ControlStatus::Effective,
            // 声称高于证据：控制确实存在，但没证明到自己声称的那一档。
            _ => ControlStatus::Partial,
        }
    }

    /// 校验控制项自身，deny-first。
    ///
    /// 顺序即论证顺序：先确认这条控制**存在且有人负责**，再确认它**映射到真实条款**，
    /// 最后才评估它**声称了什么**以及**证据支持到什么程度**。把 owner 检查放在最前面是
    /// 有意的 —— 一条没有责任人的控制项，后面所有关于它证明力的讨论都没有意义。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_CONTROL_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_CONTROL_VERSION)
        {
            return Err("security_control_header_invalid".to_owned());
        }
        safe_text(&self.control_id, "security_control_id")?;
        safe_text(&self.title, "security_control_title")?;
        // 空值先于格式检查：否则 safe_text 会把"没人负责"和"责任人字符串损坏"压成同一个码。
        if self.owner.trim().is_empty() {
            return Err("security_control_owner_required".to_owned());
        }
        safe_text(&self.owner, "security_control_owner")?;
        if let Some(parent) = &self.parent {
            safe_text(parent, "security_control_parent")?;
            if parent == &self.control_id {
                return Err("security_control_parent_self_reference".to_owned());
            }
        }
        if self.scope.is_empty() {
            return Err("security_control_scope_empty".to_owned());
        }
        if self.assumptions.len() > MAX_SECURITY_CONTROL_ASSUMPTIONS {
            return Err("security_control_assumptions_exhausted".to_owned());
        }
        for assumption in &self.assumptions {
            safe_text(assumption, "security_control_assumption")?;
        }
        if self.mappings.is_empty() {
            return Err("security_control_mapping_required".to_owned());
        }
        if self.mappings.len() > MAX_SECURITY_CONTROL_MAPPINGS {
            return Err("security_control_mappings_exhausted".to_owned());
        }
        let mut seen_clauses = BTreeSet::new();
        for mapping in &self.mappings {
            mapping.validate()?;
            if !seen_clauses.insert((
                mapping.framework,
                mapping.major_version,
                mapping.clause.to_ascii_uppercase(),
            )) {
                return Err("security_control_mapping_duplicate".to_owned());
            }
        }
        if self.evidence.len() > MAX_SECURITY_CONTROL_EVIDENCE {
            return Err("security_control_evidence_exhausted".to_owned());
        }
        let mut seen_evidence = BTreeSet::new();
        for item in &self.evidence {
            item.validate()?;
            if !seen_evidence.insert(item.evidence_digest.clone()) {
                return Err("security_control_evidence_duplicate".to_owned());
            }
        }

        // 声称必须被证据托住。这是卡片上"把单测或类型宣称为法规认证"的那条拒绝。
        if let Some(ceiling) = self.proof_ceiling {
            // 声称了等级却一条证据都没有：先报"没有证据"，因为那是更根本的缺口。
            let Some(demonstrated) = self.demonstrated_proof() else {
                return Err("security_control_evidence_required".to_owned());
            };
            if ceiling > demonstrated {
                return Err("security_control_proof_ceiling_exceeds_evidence".to_owned());
            }
        }
        // 没有证据就没有通过可言：证据为空时声称 Partial/Effective 一律拒绝。
        // 写成"先判空证据再判上限"，是为了让"零证据"永远得到证据缺失这个更准的码。
        if self.demonstrated_proof().is_none() && self.status.claims_pass() {
            return Err("security_control_status_not_derivable".to_owned());
        }
        validate_digest(&self.control_digest, "security_control_digest")?;
        if self.control_digest != self.digest() {
            return Err("security_control_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "control_id": self.control_id,
            "title": self.title,
            "owner": self.owner,
            "parent": self.parent,
            "scope": self.scope.facets.iter().map(|facet| facet.as_str()).collect::<Vec<&str>>(),
            "assumptions": self.assumptions,
            "mappings": self.mappings,
            "evidence": self.evidence.iter().map(|item| item.evidence_digest.clone()).collect::<Vec<String>>(),
            "proof_ceiling": self.proof_ceiling,
            "status": self.status,
        }))
    }

    /// 从本控制项派生一个子控制项，范围取交集。
    ///
    /// 这是"范围只能被收窄"的实现点：子控制项的范围永远等于 `parent ∩ requested`，
    /// 请求里多出来的面向被丢弃而不是被接受。交集为空时报错而不是返回空范围，
    /// 因为一个什么都不覆盖的子控制项没有登记价值。
    #[allow(clippy::too_many_arguments)]
    pub fn derive_child(
        &self,
        control_id: impl Into<String>,
        title: impl Into<String>,
        owner: impl Into<String>,
        requested: &ControlScope,
        assumptions: Vec<String>,
        mappings: Vec<FrameworkClause>,
        evidence: Vec<ControlEvidence>,
        proof_ceiling: Option<ProofLevel>,
        status: ControlStatus,
    ) -> Result<Self, String> {
        self.validate()?;
        let narrowed = self.scope.intersect(requested);
        if narrowed.is_empty() {
            return Err("security_control_scope_intersection_empty".to_owned());
        }
        Self::new(
            control_id,
            title,
            owner,
            Some(self.control_id.clone()),
            narrowed,
            assumptions,
            mappings,
            evidence,
            proof_ceiling,
            status,
        )
    }
}

/// 控制项登记册。
///
/// 登记册是**唯一**能把"声称"和"证据"放在一起比对的场所，也是本文件里唯一带状态的结构。
/// 它做的是查表与比对：不给谁发权限，不改任何调用者的上下文，也不产生副作用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityControlRegistry {
    pub schema: String,
    pub version: SchemaVersion,
    /// `control_id` → 控制项。BTreeMap 让遍历顺序与摘要无关地确定。
    pub controls: BTreeMap<String, SecurityControl>,
    pub registry_digest: String,
}

impl Default for SecurityControlRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityControlRegistry {
    pub fn new() -> Self {
        Self {
            schema: SECURITY_CONTROL_REGISTRY_SCHEMA.to_owned(),
            version: SECURITY_CONTROL_VERSION,
            controls: BTreeMap::new(),
            registry_digest: String::new(),
        }
    }

    /// 登记一条控制项。
    ///
    /// 顺序：控制项自校验 → 父控制项存在性 → 范围未被放大 → 重复检查 → 落表。
    /// 范围检查放在重复检查之前，是因为"这条子控制比父控制覆盖更多"是一个**安全**结论，
    /// 而"这条已经登记过了"只是一个簿记问题。
    pub fn register(&mut self, control: SecurityControl) -> Result<(), String> {
        control.validate()?;
        if let Some(parent_id) = &control.parent {
            let Some(parent) = self.controls.get(parent_id) else {
                return Err("security_control_parent_unknown".to_owned());
            };
            if !control.scope.is_subset_of(&parent.scope) {
                return Err("security_control_scope_widened".to_owned());
            }
        }
        if self.controls.contains_key(&control.control_id) {
            return Err("security_control_duplicate".to_owned());
        }
        if self.controls.len() >= MAX_SECURITY_CONTROL_REGISTRY_ENTRIES {
            return Err("security_control_registry_full".to_owned());
        }
        self.controls.insert(control.control_id.clone(), control);
        self.registry_digest = self.digest();
        Ok(())
    }

    pub fn get(&self, control_id: &str) -> Option<&SecurityControl> {
        self.controls.get(control_id)
    }

    /// 重算整册：逐条自校验、父链范围单调性与摘要自洽。
    ///
    /// 父链单调性在这里再查一遍，而不是只信 `register` 时的检查 —— 登记册是可以被
    /// 序列化后重新载入的，绕过 `register` 的路径必须也被挡住。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SECURITY_CONTROL_REGISTRY_SCHEMA
            || !self.version.is_compatible_with(&SECURITY_CONTROL_VERSION)
        {
            return Err("security_control_registry_header_invalid".to_owned());
        }
        if self.controls.len() > MAX_SECURITY_CONTROL_REGISTRY_ENTRIES {
            return Err("security_control_registry_full".to_owned());
        }
        for control in self.controls.values() {
            control.validate()?;
            if let Some(parent_id) = &control.parent {
                let Some(parent) = self.controls.get(parent_id) else {
                    return Err("security_control_parent_unknown".to_owned());
                };
                if !control.scope.is_subset_of(&parent.scope) {
                    return Err("security_control_scope_widened".to_owned());
                }
            }
        }
        validate_digest(&self.registry_digest, "security_control_registry_digest")?;
        if self.registry_digest != self.digest() {
            return Err("security_control_registry_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "controls": self.controls.keys().collect::<Vec<&String>>(),
            "control_digests": self
                .controls
                .values()
                .map(|control| control.control_digest.clone())
                .collect::<Vec<String>>(),
        }))
    }
}

fn major_is_unknown(framework: SecurityFramework, major_version: u32) -> bool {
    major_version != framework.supported_major()
}

/// 基础文本检查：非空、不超长、无控制字符、未被脱敏器改写、不含 secret 哨兵。
///
/// 最后两步与 SC-33 事故记录共用同一套域内助手。控制项的 title/owner/assumption 会被
/// 投影给人看和审计日志，所以它们和事故字段适用同一条边界。
fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_SECURITY_CONTROL_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Event, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn validate_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
