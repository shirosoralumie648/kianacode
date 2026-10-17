//! Artifact-first compaction commit journal：提交顺序、来源链和失效传播。
//!
//! 【顺序合同】先把摘要 artifact 完整保存并按保存字节重算 hash，再以源事件 cursor / 上下文
//! revision 做 CAS 提交（`ContextCheckpoint`），确认提交后才切换有效视图；提交前崩溃只会留下
//! 未被引用的 artifact，交给正常 GC 回收，旧视图与旧事实仍可查询。
//!
//! 【边界】这不是 ArtifactStore/EventLog 实现，也不是跨存储的分布式两阶段提交：保存字节、
//! 提交引用、切换视图是三个可分别崩溃的步骤。真实字节持久化仍由 Core 的 content-addressed
//! artifact store port 负责；本模块只固定顺序与拒绝条件，不读写文件系统、不起进程、不调用模型。
//!
//! 【失效传播】源数据撤销/删除先让派生摘要事实失效（`invalidate_from_plan`），并阻止同一摘要
//! 再次保存或提交；`data_epoch` 前进使已计划好的提交 fail closed，而不是让晚到摘要覆盖新输入。

use crate::{
    journal_sha256, json_digest, CapabilitySupport, CompactionArtifact, CompactionCommit,
    ContextCheckpoint, ContextCheckpointState, ContextInvalidationPlan, ContextInvalidationTarget,
    ContextResumeView, ModelRoute, RunId, SchemaVersion,
};
use serde::{Deserialize, Serialize};

pub const COMPACTION_STAGED_ARTIFACT_SCHEMA: &str = "kiana.compaction-staged-artifact.v1";
pub const COMPACTION_APPLIED_SCHEMA: &str = "kiana.compaction-applied.v1";
pub const COMPACTION_JOURNAL_SCHEMA: &str = "kiana.compaction-commit-journal.v1";
pub const NATIVE_COMPACTION_BINDING_SCHEMA: &str = "kiana.native-compaction-binding.v1";
pub const COMPACTION_JOURNAL_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 保存进 journal 的摘要载荷上限；超过时在保存前拒绝，不产生半保存状态。
pub const MAX_STAGED_SUMMARY_BYTES: usize = 1 << 20;

fn required_digest(value: &str, field: &str) -> Result<(), String> {
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// 归一化调用方声明的摘要：`sha256:` 前缀可有可无，但本体必须恰好 64 位 hex。
fn declared_digest(value: &str, field: &str) -> Result<String, String> {
    required_digest(value, field)?;
    let hex = value.strip_prefix("sha256:").unwrap_or(value);
    Ok(format!("sha256:{hex}"))
}

/// 已完整保存并校验过 hash、但还没有被任何提交引用的摘要 artifact。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedCompactionArtifact {
    pub schema: String,
    pub version: SchemaVersion,
    pub artifact: CompactionArtifact,
    /// 服务器按保存字节重算的裸 sha256；调用方声明值必须与它一致。
    pub content_hash: String,
    /// 已保存的摘要载荷字节；重启后重新校验，并可据此重建视图。
    pub payload: Vec<u8>,
    /// 只有提交成功后才为 true；此前它是未引用 artifact，属于 GC 候选。
    pub committed: bool,
}

impl StagedCompactionArtifact {
    /// 按保存字节重新校验 artifact：长度、hash 与摘要声明都必须一致。
    pub fn verify(&self) -> Result<(), String> {
        if self.schema != COMPACTION_STAGED_ARTIFACT_SCHEMA
            || !self.version.is_compatible_with(&COMPACTION_JOURNAL_VERSION)
        {
            return Err("compaction_staged_artifact_header_invalid".to_owned());
        }
        self.artifact.validate()?;
        if self.payload.len() as u64 != self.artifact.content_bytes {
            return Err("compaction_staged_artifact_size_changed".to_owned());
        }
        if self.content_hash != journal_sha256(&self.payload) {
            return Err("compaction_summary_digest_mismatch".to_owned());
        }
        let declared = self
            .artifact
            .summary_digest
            .strip_prefix("sha256:")
            .unwrap_or(self.artifact.summary_digest.as_str());
        if declared != self.content_hash {
            return Err("compaction_summary_digest_mismatch".to_owned());
        }
        Ok(())
    }
}

/// 提交生效事实（`compaction.applied` 的等价记录）：提交引用 + 提交时的视图/Inbox 绑定。
///
/// 事实本身不改变 Invocation/approval/任务状态；被源撤销失效后仍可查询（带 `invalidated_by`），
/// 不做删除，也不把 cache 当权威。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionAppliedFact {
    pub schema: String,
    pub version: SchemaVersion,
    pub commit: CompactionCommit,
    pub active_context_revision: u64,
    pub inbox_digest: String,
    pub inbox_sequence: u64,
    /// 失效来源（`ContextInvalidationPlan::plan_digest`）；`None` 表示仍然有效。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidated_by: Option<String>,
    pub fact_digest: String,
}

impl CompactionAppliedFact {
    fn new(
        commit: CompactionCommit,
        inbox_digest: String,
        inbox_sequence: u64,
    ) -> Result<Self, String> {
        let mut fact = Self {
            schema: COMPACTION_APPLIED_SCHEMA.to_owned(),
            version: COMPACTION_JOURNAL_VERSION,
            active_context_revision: commit.new_context_revision,
            commit,
            inbox_digest,
            inbox_sequence,
            invalidated_by: None,
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPACTION_APPLIED_SCHEMA
            || !self.version.is_compatible_with(&COMPACTION_JOURNAL_VERSION)
            || self.active_context_revision == 0
            || self.inbox_sequence == 0
        {
            return Err("compaction_applied_header_invalid".to_owned());
        }
        self.commit.validate()?;
        if self.active_context_revision != self.commit.new_context_revision {
            return Err("compaction_applied_revision_changed".to_owned());
        }
        required_digest(&self.inbox_digest, "compaction_applied_inbox_digest")?;
        if let Some(value) = &self.invalidated_by {
            required_digest(value, "compaction_applied_invalidation_digest")?;
        }
        required_digest(&self.fact_digest, "compaction_applied_fact_digest")?;
        if self.fact_digest != self.digest() {
            return Err("compaction_applied_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn is_invalidated(&self) -> bool {
        self.invalidated_by.is_some()
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "commit": self.commit,
            "active_context_revision": self.active_context_revision,
            "inbox_digest": self.inbox_digest,
            "inbox_sequence": self.inbox_sequence,
            "invalidated_by": self.invalidated_by,
        }))
    }
}

/// 压缩提交 journal：保存 → 校验 → CAS 提交 → 切换视图的确定性顺序。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionCommitJournal {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    /// 当前视图：Activity 状态表示尚未提交或已开始下一次压缩，Committed 表示该次提交已生效。
    pub checkpoint: ContextCheckpoint,
    pub staged: Vec<StagedCompactionArtifact>,
    pub facts: Vec<CompactionAppliedFact>,
    /// 已被源撤销/删除失效的摘要（`sha256:` 前缀规范化）；不能再次保存或提交。
    #[serde(default)]
    pub invalidated_summaries: Vec<String>,
    /// 之前已经提交生效、被下一次压缩顶替的视图；只增不改，仍可按 revision 查询。
    #[serde(default)]
    pub history: Vec<ContextCheckpoint>,
}

impl CompactionCommitJournal {
    pub fn new(
        run_id: RunId,
        context_revision: u64,
        source_cursor: u64,
        inbox_digest: impl Into<String>,
        inbox_sequence: u64,
    ) -> Result<Self, String> {
        let checkpoint = ContextCheckpoint::new(
            run_id,
            context_revision,
            source_cursor,
            inbox_digest,
            inbox_sequence,
        )?;
        let journal = Self {
            schema: COMPACTION_JOURNAL_SCHEMA.to_owned(),
            version: COMPACTION_JOURNAL_VERSION,
            run_id,
            checkpoint,
            staged: Vec::new(),
            facts: Vec::new(),
            invalidated_summaries: Vec::new(),
            history: Vec::new(),
        };
        journal.validate()?;
        Ok(journal)
    }

    /// 第一步：完整保存并校验摘要 artifact 字节。只保存，不提交、不切换视图。
    ///
    /// 返回 staged 下标；调用方仍需通过 [`Self::commit`] 才能让视图前进。保存过的 artifact
    /// 在提交前被丢弃时仍留在 `staged` 里，是正常 GC 候选（`gc_candidates`）。
    pub fn stage(
        &mut self,
        summary_bytes: &[u8],
        declared_summary_digest: &str,
        source_cursor_start: u64,
        source_cursor_end: u64,
    ) -> Result<usize, String> {
        self.validate()?;
        if self.checkpoint.state != ContextCheckpointState::Active {
            return Err("compaction_journal_not_active".to_owned());
        }
        if summary_bytes.is_empty() || summary_bytes.len() > MAX_STAGED_SUMMARY_BYTES {
            return Err("compaction_staged_summary_size_invalid".to_owned());
        }
        let declared = declared_digest(declared_summary_digest, "compaction_summary_digest")?;
        let content_hash = journal_sha256(summary_bytes);
        if declared != format!("sha256:{content_hash}") {
            return Err("compaction_summary_digest_mismatch".to_owned());
        }
        if self.invalidated_summaries.contains(&declared) {
            return Err("compaction_summary_invalidated".to_owned());
        }
        let artifact_digest = json_digest(&serde_json::json!({
            "schema": COMPACTION_STAGED_ARTIFACT_SCHEMA,
            "summary_digest": &declared,
            "content_bytes": summary_bytes.len(),
        }));
        let artifact = CompactionArtifact::new(
            self.run_id,
            declared,
            artifact_digest,
            source_cursor_start,
            source_cursor_end,
            summary_bytes.len() as u64,
        )?;
        self.staged.push(StagedCompactionArtifact {
            schema: COMPACTION_STAGED_ARTIFACT_SCHEMA.to_owned(),
            version: COMPACTION_JOURNAL_VERSION,
            artifact,
            content_hash,
            payload: summary_bytes.to_vec(),
            committed: false,
        });
        Ok(self.staged.len() - 1)
    }

    /// 第二步：在 artifact 已保存并校验后按源 cursor / 上下文 revision / workspace / data epoch
    /// 做 CAS 提交；只有成功时才切换有效视图并写入 applied 事实。
    ///
    /// `current_inbox_*` 在提交时读取：压缩期间到达的新输入被带入新视图，而不是被旧摘要覆盖。
    #[allow(clippy::too_many_arguments)]
    pub fn commit(
        &mut self,
        staged_index: usize,
        commit: &CompactionCommit,
        current_context_revision: u64,
        current_source_cursor: u64,
        current_workspace_revision: u64,
        current_data_epoch: u64,
        current_inbox_digest: impl Into<String>,
        current_inbox_sequence: u64,
    ) -> Result<usize, String> {
        self.validate()?;
        let staged = self
            .staged
            .get(staged_index)
            .ok_or_else(|| "compaction_artifact_not_staged".to_owned())?;
        if staged.committed {
            return Err("compaction_artifact_already_committed".to_owned());
        }
        staged.verify()?;
        if self
            .invalidated_summaries
            .contains(&staged.artifact.summary_digest)
        {
            return Err("compaction_summary_invalidated".to_owned());
        }
        let current_inbox_digest = current_inbox_digest.into();
        let next = self.checkpoint.commit(
            &staged.artifact,
            commit,
            current_context_revision,
            current_source_cursor,
            current_workspace_revision,
            current_data_epoch,
            current_inbox_digest.clone(),
            current_inbox_sequence,
        )?;
        let fact = CompactionAppliedFact::new(
            commit.clone(),
            current_inbox_digest,
            current_inbox_sequence,
        )?;
        let index = self.facts.len();
        self.facts.push(fact);
        self.staged[staged_index].committed = true;
        self.checkpoint = next;
        Ok(index)
    }

    /// 上一次提交完成后开始下一次压缩：把已提交视图归入 `history`，并以它的 revision/cursor/Inbox
    /// 建立新的 Active checkpoint。旧 checkpoint 与旧事实仍可按 revision 查询。
    pub fn begin_next_compaction(&mut self) -> Result<(), String> {
        self.validate()?;
        if self.checkpoint.state != ContextCheckpointState::Committed {
            return Err("compaction_journal_not_committed".to_owned());
        }
        let next = ContextCheckpoint::new(
            self.run_id,
            self.checkpoint.active_context_revision,
            self.checkpoint.source_cursor,
            self.checkpoint.inbox_digest.clone(),
            self.checkpoint.inbox_sequence,
        )?;
        self.history.push(self.checkpoint.clone());
        self.checkpoint = next;
        Ok(())
    }

    /// 源数据撤销/删除后的失效传播：把已提交事实标记为失效，并阻止同一摘要再次保存或提交。
    /// 返回本次新失效的事实条数；旧事实不会被删除。
    pub fn invalidate_from_plan(
        &mut self,
        plan: &ContextInvalidationPlan,
    ) -> Result<usize, String> {
        self.validate()?;
        plan.validate()?;
        if !plan.invalidates(ContextInvalidationTarget::Summary) {
            return Err("context_invalidation_summary_target_missing".to_owned());
        }
        let mut summaries = Vec::new();
        let mut invalidated = 0;
        for fact in self.facts.iter_mut() {
            if fact.is_invalidated() {
                continue;
            }
            fact.invalidated_by = Some(plan.plan_digest.clone());
            fact.fact_digest = fact.digest();
            summaries.push(fact.commit.summary_digest.clone());
            invalidated += 1;
        }
        for digest in summaries {
            if !self.invalidated_summaries.contains(&digest) {
                self.invalidated_summaries.push(digest);
            }
        }
        self.invalidated_summaries.sort();
        self.invalidated_summaries.dedup();
        self.validate()?;
        Ok(invalidated)
    }

    /// 保存成功但从未被提交引用的 artifact 下标；按正常 GC 回收，不影响已提交事实。
    pub fn gc_candidates(&self) -> Vec<usize> {
        self.staged
            .iter()
            .enumerate()
            .filter(|(_, staged)| !staged.committed)
            .map(|(index, _)| index)
            .collect()
    }

    /// 重启重建：重新校验保存的摘要字节，再用已提交 checkpoint 重建同一恢复视图。
    pub fn rebuild_view(
        &self,
        prepared_request_digest: impl Into<String>,
        data_epoch: u64,
    ) -> Result<ContextResumeView, String> {
        self.validate()?;
        let checkpoint = self
            .effective_committed_checkpoint()
            .ok_or_else(|| "compaction_journal_not_committed".to_owned())?;
        let committed = self
            .staged
            .iter()
            .find(|staged| staged.committed)
            .ok_or_else(|| "compaction_journal_committed_artifact_missing".to_owned())?;
        committed.verify()?;
        ContextResumeView::rebuild_after_restart(checkpoint, prepared_request_digest, data_epoch)
    }

    /// 最近一次提交生效的视图：提交完成后是当前 checkpoint，开始下一次压缩后是 `history` 末项。
    pub fn effective_committed_checkpoint(&self) -> Option<&ContextCheckpoint> {
        if self.checkpoint.state == ContextCheckpointState::Committed {
            Some(&self.checkpoint)
        } else {
            self.history.last()
        }
    }

    pub fn active_checkpoint(&self) -> &ContextCheckpoint {
        &self.checkpoint
    }

    pub fn staged_artifacts(&self) -> &[StagedCompactionArtifact] {
        &self.staged
    }

    pub fn applied_facts(&self) -> &[CompactionAppliedFact] {
        &self.facts
    }

    pub fn invalidated_summaries(&self) -> &[String] {
        &self.invalidated_summaries
    }

    pub fn history(&self) -> &[ContextCheckpoint] {
        &self.history
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != COMPACTION_JOURNAL_SCHEMA
            || !self.version.is_compatible_with(&COMPACTION_JOURNAL_VERSION)
            || self.run_id.as_uuid().is_nil()
        {
            return Err("compaction_journal_header_invalid".to_owned());
        }
        self.checkpoint.validate()?;
        if self.checkpoint.run_id != self.run_id {
            return Err("compaction_journal_run_mismatch".to_owned());
        }
        for staged in &self.staged {
            if staged.artifact.run_id != self.run_id {
                return Err("compaction_journal_run_mismatch".to_owned());
            }
            staged.verify()?;
            if staged.committed
                && !self
                    .facts
                    .iter()
                    .any(|fact| fact.commit.artifact_digest == staged.artifact.artifact_digest)
            {
                return Err("compaction_journal_committed_artifact_unreferenced".to_owned());
            }
        }
        for fact in &self.facts {
            if fact.commit.run_id != self.run_id {
                return Err("compaction_journal_run_mismatch".to_owned());
            }
            fact.validate()?;
            if !self.staged.iter().any(|staged| {
                staged.committed && staged.artifact.artifact_digest == fact.commit.artifact_digest
            }) {
                return Err("compaction_journal_fact_artifact_missing".to_owned());
            }
        }
        for pair in self.facts.windows(2) {
            if pair[0].active_context_revision >= pair[1].active_context_revision {
                return Err("compaction_journal_revision_regressed".to_owned());
            }
        }
        for previous in &self.history {
            previous.validate()?;
            if previous.run_id != self.run_id || previous.state != ContextCheckpointState::Committed
            {
                return Err("compaction_journal_history_invalid".to_owned());
            }
        }
        for pair in self.history.windows(2) {
            if pair[0].active_context_revision >= pair[1].active_context_revision {
                return Err("compaction_journal_history_invalid".to_owned());
            }
        }
        if let Some(previous) = self.history.last() {
            if previous.active_context_revision != self.checkpoint.source_context_revision {
                return Err("compaction_journal_history_mismatch".to_owned());
            }
        }
        for digest in &self.invalidated_summaries {
            required_digest(digest, "compaction_journal_invalidated_summary")?;
        }
        if self
            .invalidated_summaries
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err("compaction_journal_invalidated_summaries_unsorted".to_owned());
        }
        Ok(())
    }
}

/// Provider 原生 opaque compact 的能力与路由绑定。
///
/// 只有 provider 明确声明 `Supported` 时才允许启用原生压缩（`Unknown` 也 fail closed）；原生
/// handle 只在同一 route digest 与同一份已编译上下文里可复用，切换模型/profile 必须重新编译
/// 上下文后再执行压缩。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCompactionBinding {
    pub schema: String,
    pub version: SchemaVersion,
    pub run_id: RunId,
    pub provider_id: String,
    pub model_id: String,
    pub model_profile: String,
    pub route_digest: String,
    pub context_plan_digest: String,
    pub native_handle_digest: String,
    pub binding_digest: String,
}

impl NativeCompactionBinding {
    pub fn new(
        support: CapabilitySupport,
        run_id: RunId,
        route: &ModelRoute,
        context_plan_digest: impl Into<String>,
        native_handle_digest: impl Into<String>,
    ) -> Result<Self, String> {
        if support != CapabilitySupport::Supported {
            return Err("native_compaction_capability_unsupported".to_owned());
        }
        if run_id.as_uuid().is_nil() {
            return Err("native_compaction_run_invalid".to_owned());
        }
        let mut binding = Self {
            schema: NATIVE_COMPACTION_BINDING_SCHEMA.to_owned(),
            version: COMPACTION_JOURNAL_VERSION,
            run_id,
            provider_id: route.provider_id.clone(),
            model_id: route.model_id.clone(),
            model_profile: route.profile.clone(),
            route_digest: route.digest(),
            context_plan_digest: context_plan_digest.into(),
            native_handle_digest: native_handle_digest.into(),
            binding_digest: String::new(),
        };
        binding.binding_digest = binding.digest();
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != NATIVE_COMPACTION_BINDING_SCHEMA
            || !self.version.is_compatible_with(&COMPACTION_JOURNAL_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.provider_id.trim().is_empty()
            || self.model_id.trim().is_empty()
            || self.model_profile.trim().is_empty()
        {
            return Err("native_compaction_binding_header_invalid".to_owned());
        }
        for (value, field) in [
            (&self.route_digest, "native_compaction_route_digest"),
            (
                &self.context_plan_digest,
                "native_compaction_context_plan_digest",
            ),
            (
                &self.native_handle_digest,
                "native_compaction_handle_digest",
            ),
            (&self.binding_digest, "native_compaction_binding_digest"),
        ] {
            required_digest(value, field)?;
        }
        if self.binding_digest != self.digest() {
            return Err("native_compaction_binding_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 复用判定：route 或已编译上下文变化都必须重新编译，不能沿用旧的 opaque handle。
    pub fn reuse_for_route(
        &self,
        route: &ModelRoute,
        context_plan_digest: &str,
    ) -> Result<(), String> {
        self.validate()?;
        if self.route_digest != route.digest() {
            return Err("native_compaction_route_changed".to_owned());
        }
        if self.context_plan_digest != context_plan_digest {
            return Err("native_compaction_context_recompile_required".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "provider_id": self.provider_id,
            "model_id": self.model_id,
            "model_profile": self.model_profile,
            "route_digest": self.route_digest,
            "context_plan_digest": self.context_plan_digest,
            "native_handle_digest": self.native_handle_digest,
        }))
    }
}
