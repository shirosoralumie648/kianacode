//! PD-23 恢复清单与替换实例写入围栏事实（replacement-fence fact）的 Daemon 绑定校验器。
//!
//! 本文件位于 Domain 数据合同与恢复激活阶段之间：它先让 Domain 分别验证已封存清单
//! （sealed manifest）和 `RestoreVerificationFact` 的内部结构，再检查两者是否指向同一 owner、store、旧实例、
//! 源游标和旧 data epoch。可确认的数据流是：
//!
//! ```text
//! SnapshotManifest ─┐
//!                   ├→ verify_restore() → 成功/稳定错误码
//! RestoreVerificationFact ┘
//! ```
//!
//! 当前代码只由 `kiana-domain`/`kiana-daemon` 测试调用；虽然函数从 crate 公开导出，尚未找到
//! 生产恢复入口调用它。它不读取备份根、不复制文件、不取得实例锁、不围栏活跃进程、不
//! 恢复 trigger，也不把恢复根标记为 active。这些是后续 adapter/ControlPlane 激活路径的职责。
//!
//! ⚠ `RestoreVerificationFact` 中的 fence、对账和默认暂停字段是调用方声明。此函数验证
//! 声明格式及其与 manifest 的绑定，不证明实际租约/外部 effect 状态，也不单独要求
//! `fact.state == Verified`；调用者在将结果用于激活前必须执行对应的状态/就绪门检查。

use kiana_domain::{RestoreVerificationFact, SnapshotManifest};

/// 验证已封存清单（sealed manifest），并确认恢复事实（fact）绑定到该清单的关键身份与 epoch。
///
/// 【调用者】当前由恢复校验 fixture 直接调用；生产代码搜索尚未发现上游入口。函数公开
/// re-export 为 `kiana_daemon::verify_restore`，供未来恢复协调流程复用。
///
/// 【输入】
/// - `manifest`：备份阶段生成的快照清单，包含旧实例、owner/store、源游标和数据 epoch。
/// - `fact`：新实例提交的恢复校验事实，其中也记录快照摘要、旧实例、游标和新旧 epoch。
///
/// 【输出】两份对象各自合法且绑定字段一致时返回 `Ok(())`；这仅表示合同检查通过，不表示
/// 磁盘内容已恢复、fence 已物理生效或实例已允许接收命令。
///
/// 【失败情况】manifest 的任意 Domain 校验错误统一映射成 `restore_manifest_invalid`；
/// fact 的局部错误原样返回（例如 `restore_verification_fact_invalid`、
/// `restore_verification_not_ready`）；两对象关键绑定不一致时返回
/// `restore_manifest_fact_binding_invalid`。这里不发起 retry 或写 EventLog，因为失败来自
/// 同步输入校验，而不是需要等待后重试的 I/O 操作。
///
/// 【核心流程】先校验 manifest，再校验 fact，最后比较 manifest digest、owner、store、旧
/// instance、source cursor、previous data epoch 和严格递增的新 data epoch。
///
/// 【为什么单独做绑定】两个 DTO 各自自洽仍可能属于不同项目、store 或备份；逐项比较可以
/// 拒绝把一份有效 fact 错误套在另一份 manifest 上。激活、文件操作和真实 fence 由后续层处理。
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
    // `fact.validate()` 会检查 fence 声明和状态局部条件，但不会在这里证明外部事实真实。
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
    Ok(())
}
