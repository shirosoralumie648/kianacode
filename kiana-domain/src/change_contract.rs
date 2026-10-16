//! CO-30 基线变更影响与原子发布合同。
//!
//! # 文件职责与数据流
//!
//! 变更请求不能只改一个项目字符串就当作完成。它必须先列出受影响的完整图范围，再发布
//! 一套与新 baseline 绑定的完整 charter/plan/packet/criteria/budget/schedule，并保留哪些旧
//! review/acceptance/delivery 被失效的显式引用：
//!
//! ```text
//! 旧 baseline + ChangeRequest
//!        -> ChangeImpact（影响闭包、旧/新版本、budget/schedule）
//!        -> BaselinePublication（完整新基线和 invalidated_refs）
//!        -> ChangePublicationLedger::publish
//!        -> Company/CAS/EventLog 的真正提交路径
//! ```
//!
//! 本文件是 typed source contract。digest 只是未加密钥的内容一致性指纹，ledger 只是调用方
//! 提供的投影/幂等容器；它们不直接批准变更、停止活跃 Run、替换 EventLog CAS 或证明 live/
//! physical 业务结果。旧的 Company DecideChange 兼容路径仍由其原有 authority 负责。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

pub const CHANGE_IMPACT_SCHEMA: &str = "kiana.change-impact.v1";
pub const BASELINE_PUBLICATION_SCHEMA: &str = "kiana.baseline-publication.v1";

/// 检查引用字段的非空、长度和 NUL 边界。
fn required(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value.trim().is_empty() || value.len() > 16_384 || value.contains('\0') {
        Err(field)
    } else {
        Ok(())
    }
}

/// 检查未加密钥 `sha256:` 摘要的文本形状；不提供签名或来源认证。
fn digest(value: &str, field: &'static str) -> Result<(), &'static str> {
    if value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        Ok(())
    } else {
        Err(field)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次变更对现有 Company 图和权威记录的完整影响集合。
///
/// 旧/新 baseline 版本、受影响 milestone/packet/run、失效 review/acceptance/delivery 以及
/// budget/schedule 引用必须一起出现。只列出 scope 字符串或只列出一个 packet 都不能形成
/// 可发布的 impact fact。
pub struct ChangeImpact {
    /// 固定影响合同 schema。
    pub schema: String,
    /// 变更请求 ID，也是 publication ledger 的幂等关联键。
    pub change_id: String,
    /// 变更所属项目 ID。
    pub project_id: String,
    /// 被替换的旧 baseline 版本，必须非零。
    pub old_baseline_version: u64,
    /// 新 baseline 版本，必须严格大于旧版本。
    pub new_baseline_version: u64,
    /// 受影响的里程碑 ID 集合。
    pub affected_milestones: Vec<String>,
    /// 受影响的 packet ID 集合；新 publication 必须完整携带它们。
    pub affected_packets: Vec<String>,
    /// 受影响的运行引用集合。
    pub affected_runs: Vec<String>,
    /// 变更后失效的 review 引用。
    pub invalidated_reviews: Vec<String>,
    /// 变更后失效的 acceptance 引用。
    pub invalidated_acceptances: Vec<String>,
    /// 变更后失效的 delivery 引用。
    pub invalidated_deliveries: Vec<String>,
    /// 受影响预算定义的引用。
    pub affected_budget_ref: String,
    /// 受影响排期定义的引用。
    pub affected_schedule_ref: String,
    /// 对影响集合字段计算的 canonical digest。
    pub impact_digest: String,
}

impl ChangeImpact {
    /// 校验影响集合的版本推进、必要范围、引用去重和 digest。
    ///
    /// 失败返回稳定错误码，不会自动推导遗漏范围、清理旧审批或写入 publication。上层必须
    /// 把它当作发布前的 deny-first 检查。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.schema != CHANGE_IMPACT_SCHEMA
            || self.old_baseline_version == 0
            || self.new_baseline_version <= self.old_baseline_version
        {
            return Err("change_impact_version_invalid");
        }
        for (value, field) in [
            (&self.change_id, "change_impact_id_required"),
            (&self.project_id, "change_impact_project_required"),
            (&self.affected_budget_ref, "change_impact_budget_required"),
            (
                &self.affected_schedule_ref,
                "change_impact_schedule_required",
            ),
        ] {
            required(value, field)?;
        }
        if self.affected_milestones.is_empty() || self.affected_packets.is_empty() {
            return Err("change_impact_scope_required");
        }
        for list in [
            &self.affected_milestones,
            &self.affected_packets,
            &self.affected_runs,
            &self.invalidated_reviews,
            &self.invalidated_acceptances,
            &self.invalidated_deliveries,
        ] {
            if list.iter().any(|value| value.trim().is_empty())
                || list.iter().collect::<BTreeSet<_>>().len() != list.len()
            {
                return Err("change_impact_identity_invalid");
            }
        }
        digest(&self.impact_digest, "change_impact_digest_invalid")?;
        if self.impact_digest != self.canonical_digest() {
            return Err("change_impact_digest_mismatch");
        }
        Ok(())
    }

    /// 计算不包含自身 `impact_digest` 的影响集合 canonical digest。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema, "change_id": self.change_id, "project_id": self.project_id,
            "old_baseline_version": self.old_baseline_version, "new_baseline_version": self.new_baseline_version,
            "affected_milestones": self.affected_milestones, "affected_packets": self.affected_packets,
            "affected_runs": self.affected_runs, "invalidated_reviews": self.invalidated_reviews,
            "invalidated_acceptances": self.invalidated_acceptances, "invalidated_deliveries": self.invalidated_deliveries,
            "affected_budget_ref": self.affected_budget_ref, "affected_schedule_ref": self.affected_schedule_ref,
        }))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 与一条 `ChangeImpact` 精确绑定的新 baseline 发布事实。
///
/// publication 必须同时包含新 charter/plan/packet/criteria/budget/schedule、发布者/时间和
/// 明确的 invalidated refs；它不能只更新 project.scope_baseline 而留下旧 graph 节点继续
/// authoritative。
pub struct BaselinePublication {
    /// 固定 publication schema。
    pub schema: String,
    /// 发布所属项目，必须与 impact 一致。
    pub project_id: String,
    /// 对应 change ID，必须与 impact 一致。
    pub change_id: String,
    /// 被替换的旧 baseline 版本。
    pub old_baseline_version: u64,
    /// 发布的新 baseline 版本。
    pub new_baseline_version: u64,
    /// 新 charter 引用。
    pub charter_ref: String,
    /// 新 plan 引用。
    pub plan_ref: String,
    /// 新 packet 引用集合，必须等于 impact 的 affected_packets。
    pub packet_refs: Vec<String>,
    /// 新 criteria 内容摘要。
    pub criteria_digest: String,
    /// 新 budget 引用。
    pub budget_ref: String,
    /// 新 schedule 引用。
    pub schedule_ref: String,
    /// 明确失效的旧 review/acceptance/delivery 等引用。
    pub invalidated_refs: Vec<String>,
    /// 发布者业务身份。
    pub published_by: String,
    /// 发布时刻（Unix 时间，零值无效）。
    pub published_at: u64,
    /// 对 publication 字段计算的 canonical digest。
    pub digest: String,
}

impl BaselinePublication {
    /// 校验 publication 与 impact 的项目/版本/packet 绑定和完整引用集合。
    ///
    /// 这是“完整新基线是否对应这次变更”的检查，不执行 CAS 或失效旧事实。失败不会部分
    /// 写入；调用方必须在同一上层事务中完成实际提交。
    pub fn validate(&self, impact: &ChangeImpact) -> Result<(), &'static str> {
        if self.schema != BASELINE_PUBLICATION_SCHEMA
            || self.project_id != impact.project_id
            || self.change_id != impact.change_id
            || self.old_baseline_version != impact.old_baseline_version
            || self.new_baseline_version != impact.new_baseline_version
            || self.published_at == 0
        {
            return Err("baseline_publication_binding_invalid");
        }
        for (value, field) in [
            (&self.charter_ref, "baseline_publication_charter_required"),
            (&self.plan_ref, "baseline_publication_plan_required"),
            (&self.budget_ref, "baseline_publication_budget_required"),
            (&self.schedule_ref, "baseline_publication_schedule_required"),
            (&self.published_by, "baseline_publication_author_required"),
        ] {
            required(value, field)?;
        }
        if self.packet_refs != impact.affected_packets
            || self.criteria_digest.trim().is_empty()
            || self.invalidated_refs.is_empty()
        {
            return Err("baseline_publication_complete_set_required");
        }
        if self.invalidated_refs.iter().collect::<BTreeSet<_>>().len()
            != self.invalidated_refs.len()
        {
            return Err("baseline_publication_invalidated_duplicate");
        }
        digest(
            &self.criteria_digest,
            "baseline_publication_criteria_digest_invalid",
        )?;
        if self.digest != self.canonical_digest() {
            return Err("baseline_publication_digest_mismatch");
        }
        Ok(())
    }

    /// 计算不包含自身 `digest` 的 publication canonical digest。
    pub fn canonical_digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema, "project_id": self.project_id, "change_id": self.change_id,
            "old_baseline_version": self.old_baseline_version, "new_baseline_version": self.new_baseline_version,
            "charter_ref": self.charter_ref, "plan_ref": self.plan_ref, "packet_refs": self.packet_refs,
            "criteria_digest": self.criteria_digest, "budget_ref": self.budget_ref, "schedule_ref": self.schedule_ref,
            "invalidated_refs": self.invalidated_refs, "published_by": self.published_by, "published_at": self.published_at,
        }))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
/// 进程内保存历史影响与基线发布的幂等 ledger。
///
/// 它保留旧 impact，防止后来者覆盖历史；但它不是跨进程 EventLog/CAS，也不会自动使旧授权
/// 失效或停止活跃 Run。
pub struct ChangePublicationLedger {
    /// change ID 到已发布 baseline 的索引。
    pub publications: BTreeMap<String, BaselinePublication>,
    /// change ID 到历史影响集合的索引。
    pub historical_impacts: BTreeMap<String, ChangeImpact>,
}

impl ChangePublicationLedger {
    /// 校验并幂等发布一条影响/基线组合。
    ///
    /// 相同 change ID 且 publication digest 相同是幂等成功；同 ID 不同 digest 拒绝 drift；
    /// 任意历史 publication 已经使用不小于新版本的同项目 baseline 时拒绝 version conflict。
    /// 两张 map 只有在所有 Domain 校验通过后才写入，因此本函数本身不会产生部分 ledger 状态。
    /// 真正的 Company/CAS/EventLog durable 提交仍需由上层完成。
    pub fn publish(
        &mut self,
        impact: ChangeImpact,
        publication: BaselinePublication,
    ) -> Result<(), &'static str> {
        impact.validate()?;
        publication.validate(&impact)?;
        if let Some(existing) = self.publications.get(&impact.change_id) {
            if existing.digest == publication.digest {
                return Ok(());
            }
            return Err("baseline_publication_duplicate_digest_mismatch");
        }
        if self.historical_impacts.values().any(|old| {
            old.project_id == impact.project_id
                && old.new_baseline_version >= impact.new_baseline_version
        }) {
            return Err("baseline_publication_version_conflict");
        }
        self.historical_impacts
            .insert(impact.change_id.clone(), impact);
        self.publications
            .insert(publication.change_id.clone(), publication);
        Ok(())
    }
}
