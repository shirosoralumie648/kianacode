//! PD-23 恢复门（restore gate）：清单校验、事实绑定、就绪门与替换写入围栏的组合入口。
//!
//! 本文件位于 Domain 数据合同、Core 就绪门与恢复激活阶段之间。它做三件事，每一件都委派给
//! 已有权威，而不是在这里重新实现：
//!
//! ```text
//! SnapshotManifest ─┐
//! RestoreVerificationFact ─┤→ verify_restore()          Domain 校验 + manifest 绑定
//!                       │      ↓
//! 已封存 effect receipt ─┤→ verify_restore_evidence()   Core 重新派生 claim 并比对
//!                       │      ↓
//! DEP-23 ActivationState ─┘→ require_restore_ready()     Core 就绪门（Verified + Activated）
//! ```
//!
//! 激活本身不在这里：新 lease/fence 的铸造属于 `kiana-core::restore_activation`（DEP-23），
//! 本文件只消费它给出的 `ActivationState`。重复实现租约/围栏会成为第二个权威。
//!
//! ⚠ `RestoreVerificationFact` 中的 fence、对账和默认暂停字段是调用方声明。本文件验证声明
//! 格式、manifest 绑定与外部 effect 证据（通过重新派生 receipt），但不读取备份根、不复制
//! 文件、不取得实例锁、不围栏活跃进程、不恢复 trigger，也不把恢复根标记为 active。

use kiana_core::{
    require_restore_ready, verify_restore_effect_claims, ActivationState,
    EffectReconciliationReceipt, EffectReconciliationRequest, RestoreEffectClaimError,
};
use kiana_domain::{
    bind_restore_artifact_refs, EffectObservation, RestoreVerificationFact, SnapshotManifest,
};

/// 验证已封存清单（sealed manifest），并确认恢复事实（fact）绑定到该清单的关键身份、epoch
/// 与 artifact 哈希。
///
/// 【调用者】Daemon 的恢复入口与恢复校验 fixture。函数公开 re-export 为
/// `kiana_daemon::verify_restore`。
///
/// 【输入】
/// - `manifest`：备份阶段生成的快照清单，包含旧实例、owner/store、源游标、数据 epoch 与
///   `files`（相对路径 + 内容哈希）。
/// - `fact`：新实例提交的恢复校验事实，其中也记录快照摘要、旧实例、游标、新旧 epoch 和
///   artifact 引用。
///
/// 【输出】两份对象各自合法、artifact 引用覆盖清单文件且哈希一致、绑定字段一致时返回
/// `Ok(())`；这仅表示合同检查通过，不表示磁盘内容已恢复、fence 已物理生效或实例已允许
/// 接收命令（那由 [`require_restore_ready`] 与 [`DaemonHost::restore_gate`] 判定）。
///
/// 【失败情况】manifest 的任意 Domain 校验错误统一映射成 `restore_manifest_invalid`；
/// fact 的局部错误原样返回；artifact 引用缺失/未知/哈希不符返回 `restore_artifact_ref_*`；
/// 两对象关键绑定不一致返回 `restore_manifest_fact_binding_invalid`。
pub fn verify_restore(
    manifest: &SnapshotManifest,
    fact: &RestoreVerificationFact,
) -> Result<(), &'static str> {
    // manifest 的 Domain validator 返回 String 错误；Daemon 在此收敛成稳定的边界错误码，
    // 避免把路径或字段级错误细节作为此 adapter 的公开返回契约。
    manifest
        .validate()
        .map_err(|_| "restore_manifest_invalid")?;
    // fact 的错误码是恢复合同的一部分，保留 Domain 的具体原因，便于上游决定显示或记录。
    fact.validate()?;
    // 自摘要相等只说明 fact 自身没有和摘要脱节；这些比较再把它绑定到当前传入的快照。
    if fact.snapshot_manifest_digest != manifest.manifest_digest
        || fact.owner_scope != manifest.owner_scope
        || fact.store_id != manifest.store_id
        || fact.previous_instance_id.as_deref() != Some(manifest.instance_id.as_str())
        || fact.source_cursor != manifest.source_cursor
        || fact.previous_data_epoch != Some(manifest.data_epoch)
        || fact.data_epoch <= manifest.data_epoch
    {
        return Err("restore_manifest_fact_binding_invalid");
    }
    // artifact 引用不是「非空且唯一」就算数：它必须逐条对应清单文件的内容哈希。
    bind_restore_artifact_refs(manifest, &fact.artifact_refs)?;
    Ok(())
}

/// 组合校验：manifest/fact 绑定 + 外部 effect 证据 + 激活就绪门。
///
/// 【顺序】先结构（`verify_restore`），再证据（Core 重新派生 receipt），最后就绪
/// （`Verified` + `ActivationState::ready()`）。先结构后证据，因为一条引用未知文件的 claim
/// 没有讨论价值；证据在就绪之前，因为 `NeedsReconciliation` 的 fact 即使激活状态已
/// `Activated` 也不得放行。
///
/// 【输入】`activation` 是 DEP-23 的 `ActivationState`（`prepared` 或 `activated`），
/// `evidence` 是每条 claim 对应的 `(request, receipt, observation)` 三元组。
///
/// 【失败】返回稳定错误码：`restore_manifest_invalid` / `restore_verification_fact_invalid` /
/// `restore_manifest_fact_binding_invalid` / `restore_artifact_ref_*` /
/// `restore_effect_claim_*` / `restore_verification_not_verified` /
/// `restore_activation_not_ready` / `restore_activation_state_invalid`。
pub fn verify_restore_ready(
    manifest: &SnapshotManifest,
    fact: &RestoreVerificationFact,
    activation: &ActivationState,
    evidence: &[(
        &EffectReconciliationRequest,
        &EffectReconciliationReceipt,
        &EffectObservation,
    )],
) -> Result<(), String> {
    verify_restore(manifest, fact).map_err(str::to_owned)?;
    verify_restore_effect_claims(fact, evidence)
        .map_err(|error: RestoreEffectClaimError| error.code().to_owned())?;
    require_restore_ready(fact, activation).map_err(str::to_owned)?;
    Ok(())
}

/// 重启时的默认门：没有激活事实时一律保持 `Prepared`，不得凭进程内状态放行。
///
/// 【返回】`Ok(())` 只在 `state.ready()` 为真时成立；`prepared` 状态返回
/// `restore_activation_not_ready`。这是一个**显式**的拒绝，避免调用方把「没有激活记录」
/// 误当成「无需激活」。
pub fn require_activated_after_restart(state: &ActivationState) -> Result<(), &'static str> {
    state
        .validate()
        .map_err(|_| "restore_activation_state_invalid")?;
    if !state.ready() {
        return Err("restore_activation_not_ready");
    }
    Ok(())
}
