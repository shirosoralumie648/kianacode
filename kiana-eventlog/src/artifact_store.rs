//! 产物存储引擎：在内存中管理代码、日志、决策文档等 blob 数据。
//!
//! # 什么是「产物」（artifact）
//!
//! 一次会话、一次规划、一次构建的产出都是产物。包括：
//! - 源代码片段
//! - 构建日志
//! - 决策文档（plan/DECISION.json）
//! - 性能分析报告
//! - 任务清单（packet/TASK.json）
//!
//! 这些东西通常很大（几 MB 到几 GB），不能都放在事件账本里。
//! 取而代之的是，账本里存一个「指针」（ArtifactRef），指向实际的 blob 在哪里。
//! 这个文件就是管理这些 blob 的存储引擎。
//!
//! # 这是 CI 测试用的非持久化实现
//!
//! 真正的生产实现应该把 blob 写到磁盘或对象存储（S3 等）。
//! 这个实现全在内存，进程重启就丢失——但对 CI 测试来说足够了。
//!
//! # 完整性保护：哈希验证
//!
//! 每个产物都有一个内容哈希（SHA256）。在 staging、commit、read 时都会验证。
//! 如果 blob 被篡改，哈希就对不上，系统会拒绝读取或提交。
//!
//! # 三态生命周期
//!
//! - **staged**：刚上传的 blob，还没被确认提交。
//! - **committed**：已被确认为「这个版本的这个产物是最终的」。
//! - **invalidated**：已被撤销或过期，不再允许读取。
//!
//! 【为什么分 staged 和 committed】
//! 上传后提交前可能检查失败，或者发现 blob 有问题。
//! 分开这两个状态，让我们可以「先上传，再决定是否提交」。
//! 这是一个常见的两阶段提交模式。

use async_trait::async_trait;
use kiana_domain::{
    journal_sha256, ArtifactRef, ArtifactVersion, DataPropagationReceipt, DataPropagationState,
    DataPropagationTarget,
};
use kiana_ports::{ArtifactStorePort, PortError};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tokio::sync::Mutex;

/// 产物的键类型：(产物 ID, 版本号)。
/// 同一个产物可以有多个版本，版本号递增。
type ArtifactKey = (kiana_domain::ArtifactId, u64);

/// 内存中的产物状态。非持久化，进程重启丢失。
///
/// # 字段含义
///
/// - `staged`：已上传但未提交的 blob。
///   键是 (产物 ID, 版本号)。
///   值是 (ArtifactVersion 元数据, 内容字节)。
///   这些 blob 在 commit 前不能被读取。
///
/// - `committed`：已提交的产物版本的集合。
///   键是 (产物 ID, 版本号)。
///   这个集合只记录「哪些版本已提交」，不存内容本身
///   （内容在 staged 的值里，因为提交后也不删）。
///
/// - `invalidated`：已被撤销或过期的产物。
///   键是 (产物 ID, 版本号)。
///   值是 (数据纪元, tombstone 摘要)。
///   一旦被 invalidated，再也读不到这个版本的内容。
///   即使数据在 staged 里，也视为不可用。
///
/// 【为什么 staged 和 committed 分开】
/// - 上传时加入 staged
/// - 验证通过后加入 committed
/// - 读取时只接受 committed 里的
/// - 如果读前被 invalidate 了，即使在 committed 里也拒绝
///
/// 【invalidated 为什么存摘要】
/// 防重放攻击：同一个版本不能被 invalidate 两次（用不同的摘要）。
/// 如果试图用不同的摘要 invalidate 同一个版本，拒绝。
#[derive(Clone, Debug, Default)]
struct ArtifactState {
    staged: BTreeMap<ArtifactKey, (ArtifactVersion, Vec<u8>)>,
    committed: BTreeSet<ArtifactKey>,
    invalidated: BTreeMap<ArtifactKey, (u64, String)>,
}

/// 内存中的不可变产物存储。这是 CI/源代码语义的测试工具，不是持久化证明。
///
/// # 不可变性的保证
///
/// 一旦某个产物被 commit，它就不能再改。
/// 这对建立信任链很关键：你可以相信这个产物的内容不会被后来改掉。
///
/// # 与持久化的区别
///
/// 真正的产物存储应该使用持久化媒介（文件系统、数据库、对象存储）。
/// 这个实现全在内存，方便 CI 快速运行，不阻塞于磁盘 I/O。
#[derive(Clone, Debug, Default)]
pub struct MemoryArtifactStore {
    state: Arc<Mutex<ArtifactState>>,
}

impl MemoryArtifactStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// 校验内容与元数据的一致性。
    ///
    /// # 检查项目
    ///
    /// - `version.validate()`：元数据本身是否有效（格式、字段完整性）
    /// - `version.size_bytes`：声称的大小与实际大小是否一致
    /// - `journal_sha256(content)`：计算内容的 SHA256，与元数据里的哈希对比
    ///
    /// 【为什么用 journal_sha256】
    /// 它是仓库定义的标准哈希方法，保证了跨不同实现的一致性。
    /// 所有产物都用这个方法计算哈希，不会出现「A 用 SHA256，B 用 MD5」的混乱。
    ///
    /// # 失败情况
    ///
    /// - 元数据验证失败 → PortError::Failed（格式问题）
    /// - 大小不匹配 → PortError::Conflict（数据被改或被截断）
    /// - 哈希不匹配 → PortError::Conflict（数据被篡改）
    fn validate_content(version: &ArtifactVersion, content: &[u8]) -> Result<(), PortError> {
        version
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_version:{error}")))?;
        if version.size_bytes != content.len() as u64
            || journal_sha256(content) != version.content_hash
        {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// 读取一个已 commit 的产物版本。
    ///
    /// # 访问权限检查
    ///
    /// - 产物必须存在
    /// - 产物必须已被 commit（不接受还在 staged 的产物）
    /// - 产物必须未被 invalidate（即使被 commit 也可能后来被标记为过期）
    /// - ArtifactRef 里的字段（内容哈希、scope、schema）必须与元数据一致
    ///
    /// # 返回值
    ///
    /// 成功返回 (产物元数据, 内容字节)。
    /// 这两样一起返回，方便调用者重新验证哈希。
    ///
    /// # 失败情况
    ///
    /// - `artifact_reference` 格式错误 → PortError::Failed
    /// - 产物被 invalidate → PortError::Conflict（「已撤销或过期」）
    /// - 产物未提交 → PortError::Unavailable
    /// - 元数据丢失 → PortError::Failed（内部一致性错误）
    /// - ArtifactRef 与元数据不匹配 → PortError::Conflict（边界错误）
    async fn committed(
        &self,
        reference: &ArtifactRef,
    ) -> Result<(ArtifactVersion, Vec<u8>), PortError> {
        reference
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_reference:{error}")))?;
        let state = self.state.lock().await;
        let key = (reference.artifact_id, reference.version);

        // 【invalidation 检查】如果这个版本已被撤销，不能读。
        if state.invalidated.contains_key(&key) {
            return Err(PortError::Conflict(
                "artifact_data_revoked_or_expired".to_owned(),
            ));
        }

        // 【commit 检查】这个版本必须被标记为已提交。
        if !state.committed.contains(&key) {
            return Err(PortError::Unavailable(
                "artifact_version_not_committed".to_owned(),
            ));
        }

        // 【元数据查找】在 staged 里找到这个版本的元数据和内容。
        let (version, content) = state
            .staged
            .get(&key)
            .cloned()
            .ok_or_else(|| PortError::Failed("artifact_manifest_missing".to_owned()))?;

        // 【边界检查】ArtifactRef 的关键字段必须与元数据匹配。
        if version.content_hash != reference.content_hash
            || version.scope_digest != reference.scope_digest
            || version.artifact_schema != reference.artifact_schema
        {
            return Err(PortError::Conflict(
                "artifact_reference_manifest_mismatch".to_owned(),
            ));
        }
        Ok((version, content))
    }

    /// 撤销一个已 commit 的产物。
    ///
    /// # 撤销是什么
    ///
    /// 当数据被法律删除、审计失败、或违反政策时，
    /// 已提交的产物也需要被撤销。字节不会从内存里消失
    /// （这是 append-only 账本的特性），但系统会拒绝任何对这个版本的读取。
    ///
    /// # 数据纪元的意义
    ///
    /// `data_epoch` 是「这次撤销的时间序号」，`previous_epoch` 是「上一个纪元」。
    /// 新的纪元必须 > 旧的，防止撤销命令被重放或乱序执行。
    ///
    /// # 返回值
    ///
    /// 成功返回一份 DataPropagationReceipt，记录了撤销的事实：
    /// - 什么产物被撤销
    /// - 什么时候撤销的
    /// - 撤销的原因（tombstone_digest）
    /// - 完整性摘要
    ///
    /// 这份收据会被存入事件账本，成为「这个产物被撤销了」的审计证明。
    pub async fn invalidate_artifact(
        &self,
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        reference
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_reference:{error}")))?;

        // 【参数有效性检查】这些参数都有取值限制。
        if previous_epoch == 0
            || data_epoch <= previous_epoch
            || observed_at_ms == 0
            || !Self::valid_digest(tombstone_digest)
        {
            return Err(PortError::Failed(
                "artifact_invalidation_boundary_invalid".to_owned(),
            ));
        }

        let key = (reference.artifact_id, reference.version);
        let mut state = self.state.lock().await;

        // 【存在性检查】这个版本必须已被 commit，才能被撤销。
        if !state.committed.contains(&key) {
            return Err(PortError::Unavailable(
                "artifact_version_not_committed".to_owned(),
            ));
        }

        // 【幂等性与冲突检查】
        if let Some((epoch, digest)) = state.invalidated.get(&key) {
            // 如果已经被撤销过，新的撤销必须内容完全相同（幂等）。
            if *epoch == data_epoch && digest == tombstone_digest {
                return Self::invalidation_receipt(
                    reference,
                    previous_epoch,
                    data_epoch,
                    tombstone_digest,
                    observed_at_ms,
                );
            }
            // 否则有冲突：同一个产物被用两个不同的撤销理由撤销了。
            return Err(PortError::Conflict(
                "artifact_invalidation_conflict".to_owned(),
            ));
        }

        // 【记录撤销】标记这个版本已被撤销，并记录摘要用于后续检查。
        state
            .invalidated
            .insert(key, (data_epoch, tombstone_digest.to_owned()));
        Self::invalidation_receipt(
            reference,
            previous_epoch,
            data_epoch,
            tombstone_digest,
            observed_at_ms,
        )
    }

    /// 检查 tombstone_digest 的格式是否有效。
    ///
    /// # 格式要求
    ///
    /// - 必须以 "sha256:" 开头
    /// - 后面跟 64 个十六进制字符（代表 SHA256 哈希）
    ///
    /// # 为什么要检查
    ///
    /// 防止垃圾数据进入系统。如果有人试图传一个格式错误的摘要
    /// （比如 "md5:123" 或完全不像哈希的字符串），我们不接受。
    /// 这是一个「fail-closed」的做法：格式不对就拒绝。
    fn valid_digest(value: &str) -> bool {
        let Some(hex) = value.strip_prefix("sha256:") else {
            return false;
        };
        hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    /// 生成撤销收据。
    ///
    /// # 收据包含什么
    ///
    /// - 被撤销的产物信息（ID、版本、scope）
    /// - 纪元信息（previous_epoch、data_epoch）
    /// - 撤销时间戳（observed_at_ms）
    /// - 撤销理由摘要（tombstone_digest）
    /// - 收据本身的完整性摘要（receipt_digest）
    ///
    /// # 为什么要生成摘要
    ///
    /// 这份收据会被写入事件账本。为了确保收据在写入后不被篡改，
    /// 我们计算收据内容的哈希，并把它一起写入。
    /// 后来读取时，可以重新计算哈希，验证收据没被改过。
    ///
    /// # 这个方法既可以在 invalidate 时调用，也可以在幂等重试时调用
    ///
    /// 因为哈希计算是确定性的，同样的输入会产生同样的摘要。
    fn invalidation_receipt(
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        let mut receipt = DataPropagationReceipt {
            schema: kiana_domain::DATA_PROPAGATION_RECEIPT_SCHEMA.to_owned(),
            version: kiana_domain::DATA_PROPAGATION_VERSION,
            target: DataPropagationTarget::Artifact,
            state: DataPropagationState::Invalidated,
            project_ref: reference.scope_digest.clone(),
            previous_epoch,
            data_epoch,
            source_cursor: reference.version,
            tombstone_digest: tombstone_digest.to_owned(),
            receipt_digest: None,
            observed_at_ms,
        };

        // 【计算收据摘要】哈希这份收据的所有重要字段。
        receipt.receipt_digest = Some(kiana_domain::json_digest(&json!({
            "artifact_id": reference.artifact_id,
            "version": reference.version,
            "scope_digest": reference.scope_digest,
            "data_epoch": data_epoch,
            "tombstone_digest": tombstone_digest,
        })));

        // 【验证收据有效性】确保生成的收据能通过自身的校验。
        receipt
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_invalidation_receipt:{error}")))?;
        Ok(receipt)
    }
}

#[async_trait]
impl ArtifactStorePort for MemoryArtifactStore {
    /// 暂存一个产物版本。
    ///
    /// # 暂存（staging）是什么
    ///
    /// 用户上传一个产物（比如编译输出的二进制）。
    /// 我们验证它的完整性（大小、哈希），然后把它存到 staged 状态。
    /// 此时，这个产物还没有被"确认"为最终版本，可能因为后续检查失败而被丢弃。
    ///
    /// # 返回值
    ///
    /// 返回一份 ArtifactRef，这是对这个产物的"指针"。
    /// 包含：产物 ID、版本号、内容哈希、scope、schema。
    /// 后续的 commit/read 操作都要用这份引用。
    ///
    /// # 检查项
    ///
    /// - 内容完整性（大小和哈希必须一致）
    /// - 产物尚未被暂存（防止重复暂存不同内容的同一个版本）
    ///
    /// # 冲突检查
    ///
    /// 如果同一个 (产物 ID, 版本号) 已经被暂存过，拒绝新的暂存。
    /// 这防止了「我改了一下内容，再传一次」这样的情况。
    async fn stage_artifact(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactRef, PortError> {
        Self::validate_content(&version, &content)?;
        let key = (version.artifact_id, version.version);
        let mut state = self.state.lock().await;
        if state.staged.contains_key(&key) {
            return Err(PortError::Conflict(
                "artifact_version_already_staged".to_owned(),
            ));
        }
        let reference = version.as_ref();
        state.staged.insert(key, (version, content));
        Ok(reference)
    }

    /// 暂存一个产物，返回完整的元数据（而不只是引用）。
    ///
    /// # 与 stage_artifact 的区别
    ///
    /// `stage_artifact` 返回 ArtifactRef（引用）。
    /// `stage_artifact_version` 返回 ArtifactVersion（完整元数据）。
    ///
    /// 这两个方法的功能是一样的，但返回值类型不同，
    /// 满足不同的调用者需求。
    ///
    /// # 幂等性检查
    ///
    /// 如果同一个版本已经被暂存过且内容完全相同，返回成功。
    /// 如果内容不同，拒绝。
    async fn stage_artifact_version(
        &self,
        version: ArtifactVersion,
        content: Vec<u8>,
    ) -> Result<ArtifactVersion, PortError> {
        Self::validate_content(&version, &content)?;
        let key = (version.artifact_id, version.version);
        let mut state = self.state.lock().await;
        if let Some((existing, existing_content)) = state.staged.get(&key) {
            // 【幂等性】如果元数据和内容都一样，认为这是重复提交，返回成功。
            if existing.as_ref() == version.as_ref() && existing_content == &content {
                return Ok(existing.clone());
            }
            return Err(PortError::Conflict(
                "artifact_version_already_staged".to_owned(),
            ));
        }
        state.staged.insert(key, (version.clone(), content));
        Ok(version)
    }

    /// 提交一个产物版本。
    ///
    /// # 提交的含义
    ///
    /// 从"暂存"转移到"已确认"。一旦 commit，这个版本就被认为是最终的，
    /// 可以被其他系统读取和引用。
    ///
    /// # 前置条件
    ///
    /// - 产物必须已被暂存
    /// - ArtifactRef 的字段必须与暂存时的元数据一致
    /// - 如果提供了 expected_revision，它必须与版本号匹配
    ///
    /// # expected_revision 的作用
    ///
    /// 这是一个"乐观锁"机制。调用者可以说：
    /// "我期望这个版本号是 5，只有版本号是 5 时才 commit"。
    /// 如果版本号不是 5，说明有人改了，拒绝提交。
    async fn commit_artifact(
        &self,
        reference: ArtifactRef,
        expected_revision: Option<u64>,
    ) -> Result<(), PortError> {
        reference
            .validate()
            .map_err(|error| PortError::Failed(format!("artifact_reference:{error}")))?;

        // 【乐观锁检查】如果提供了 expected_revision，必须匹配。
        if expected_revision.is_some_and(|revision| revision != reference.version) {
            return Err(PortError::Conflict(
                "artifact_manifest_revision_conflict".to_owned(),
            ));
        }

        let mut state = self.state.lock().await;
        let key = (reference.artifact_id, reference.version);

        // 【暂存检查】产物必须先被暂存。
        let (version, content) = state
            .staged
            .get(&key)
            .cloned()
            .ok_or_else(|| PortError::Unavailable("artifact_version_not_staged".to_owned()))?;

        // 【内容验证】再验证一次，确保暂存以来内容没被改。
        Self::validate_content(&version, &content)?;

        // 【边界检查】ArtifactRef 必须与暂存时的版本对得上。
        if version.as_ref() != reference {
            return Err(PortError::Conflict(
                "artifact_reference_manifest_mismatch".to_owned(),
            ));
        }

        // 【标记已提交】加入 committed 集合。
        state.committed.insert(key);
        Ok(())
    }

    /// 读取一个产物的内容。
    ///
    /// # 前置条件
    ///
    /// - 产物必须已被 commit
    /// - 产物必须未被 invalidate
    /// - ArtifactRef 必须有效（会在 committed 方法里检查）
    ///
    /// # 返回值
    ///
    /// 返回原始字节。调用者可以根据 ArtifactRef 里的信息
    /// （内容哈希、schema）来解析这些字节。
    ///
    /// # 额外验证
    ///
    /// 返回前，再验证一次内容的哈希是否与元数据一致。
    /// 这是"defense in depth"：即使内存有问题，我们也能检测出来。
    async fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, PortError> {
        let (version, content) = self.committed(reference).await?;
        // 【重新验证】确保返回的内容与元数据一致。
        if journal_sha256(&content) != version.content_hash {
            return Err(PortError::Conflict(
                "artifact_content_hash_mismatch".to_owned(),
            ));
        }
        Ok(content)
    }

    /// 验证一个产物的完整性（不返回内容）。
    ///
    /// # 用途
    ///
    /// 有时候我们只想检查「这个产物存在且完整」，不需要读取整个内容
    /// （可能很大）。这个方法就是用来快速验证的。
    ///
    /// # 检查项
    ///
    /// - 产物已被 commit
    /// - 产物未被 invalidate
    /// - 大小和哈希都正确
    async fn verify_artifact(&self, reference: &ArtifactRef) -> Result<(), PortError> {
        let (version, content) = self.committed(reference).await?;
        Self::validate_content(&version, &content)?;
        Ok(())
    }

    /// 撤销一个产物版本。
    ///
    /// # 与 invalidate_artifact 实例方法的关系
    ///
    /// 这个trait方法直接委托给同名的实例方法。
    /// 这是一个常见的 Rust 模式：trait 方法调用实现方法，方便后者被单独调用。
    async fn invalidate_artifact(
        &self,
        reference: &ArtifactRef,
        previous_epoch: u64,
        data_epoch: u64,
        tombstone_digest: &str,
        observed_at_ms: u64,
    ) -> Result<DataPropagationReceipt, PortError> {
        MemoryArtifactStore::invalidate_artifact(
            self,
            reference,
            previous_epoch,
            data_epoch,
            tombstone_digest,
            observed_at_ms,
        )
        .await
    }
}
