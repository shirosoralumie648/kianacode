//! 启动存储完整性扫描、恢复报告与 fail-closed 健康门。
//!
//! 本模块给 daemon 提供一个**只读准入**包装：它在组合根暴露 ControlPlane 之前，用
//! `kiana_eventlog::scan_jsonl` 检查事实日志，把“空库 / 完整 / 修复过尾部 / 损坏 / 未知格式”
//! 分类出来，并在 store 不健康时拒绝放行。
//!
//! # 边界（为什么这不是第二条执行路径）
//!
//! - 本模块不打开第二份事实来源：它复用的仍是 `JsonlEventLog` 加载器，`scan_jsonl` 只是读。
//! - 它不移动、不删除、不重建任何日志；隔离仍是显式的运维/恢复动作（`quarantine_required`
//!   只是报告标记）。
//! - 它不调用 Broker、不创建 Runner，也不产生任何业务副作用；返回的只是只读报告和准入决定。
//! - 未知格式不会被当成空库，损坏中段不会被跳过：这两类都让 `admit_execution()` 失败。
//!
//! 生命周期/迁移/投影调度仍由 `DaemonHost::StorageCoordinator` 一类的组合根负责；本模块只回答
//! “现在这个事实日志能不能安全暴露”。

use kiana_eventlog::{
    scan_jsonl, IntegrityRecoveryReport, IntegrityScanReport, IntegrityScanStatus,
};
use kiana_ports::PortError;
use std::path::Path;

/// 本模块产出/消费的准入报告 schema。
pub const STORAGE_INTEGRITY_GATE_SCHEMA: &str = "kiana.daemon-storage-integrity-gate.v1";

/// 健康门对「是否允许恢复执行」的二值判定。
///
/// 这是只读结论，不是授权：`Allowed` 只表示事实日志本身没有阻塞执行的完整性问题，真正能不能
/// 执行仍由 ControlPlane 的授权/策略/关卡决定。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageIntegrityAdmission {
    Allowed,
    Blocked,
}

/// 启动完整性健康门：持有一次只读扫描的报告。
#[derive(Clone, Debug)]
pub struct StorageIntegrityGate {
    report: IntegrityScanReport,
}

impl StorageIntegrityGate {
    /// 用一份已有的扫描报告构造健康门；不重新读盘。
    pub fn from_report(report: IntegrityScanReport) -> Self {
        Self { report }
    }

    /// 只读扫描指定的事实日志文件。
    ///
    /// 复用 `scan_jsonl`：它通过同一个 JSONL 加载器打开/重放日志，因此不会引入第二套帧解析。
    pub async fn inspect(path: impl AsRef<Path>) -> Self {
        Self {
            report: scan_jsonl(path).await,
        }
    }

    /// 扫描默认会话事实日志（`KIANA_HOME`/`$HOME` 解析出的路径）。
    ///
    /// 路径本身非法（例如相对 `KIANA_HOME`）时返回错误，而不是悄悄退回另一个文件。
    pub async fn inspect_default() -> Result<Self, PortError> {
        let path = kiana_eventlog::default_sessions_log_path()?;
        Ok(Self::inspect(path).await)
    }

    /// 底层的只读扫描报告。
    pub fn report(&self) -> &IntegrityScanReport {
        &self.report
    }

    /// 打开时实际发生的尾部恢复动作（未修复时为默认值）。
    pub fn recovery(&self) -> &IntegrityRecoveryReport {
        &self.report.recovery
    }

    /// 分类结果。
    pub fn status(&self) -> IntegrityScanStatus {
        self.report.status
    }

    /// 是否需要显式隔离（仅 `Corrupt` 为 true）。
    pub fn quarantine_required(&self) -> bool {
        self.report.quarantine_required
    }

    /// 是否允许恢复执行。
    pub fn admission(&self) -> StorageIntegrityAdmission {
        if self.report.status.is_healthy() {
            StorageIntegrityAdmission::Allowed
        } else {
            StorageIntegrityAdmission::Blocked
        }
    }

    /// fail-closed 健康门：`Corrupt` 返回 `Conflict`，`Unknown` 返回 `Unavailable`。
    ///
    /// 错误码沿用 `kiana-eventlog` 的规范词表，保证 CLI/HTTP/Receipt 映射到同一套策略。
    pub fn admit_execution(&self) -> Result<(), PortError> {
        self.report.health_gate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(status: IntegrityScanStatus) -> IntegrityScanReport {
        IntegrityScanReport {
            schema: IntegrityScanReport::SCHEMA.to_owned(),
            path: std::path::PathBuf::from("/tmp/pd08-integrity-gate-unit.jsonl"),
            status,
            source_cursor: 0,
            quarantine_required: status == IntegrityScanStatus::Corrupt,
            incident_code: None,
            recovery_action: "continue".to_owned(),
            recovery: IntegrityRecoveryReport::default(),
        }
    }

    #[test]
    fn corrupt_and_unknown_block_execution_while_healthy_statuses_allow_it() {
        for status in [
            IntegrityScanStatus::Empty,
            IntegrityScanStatus::Ready,
            IntegrityScanStatus::Recovered,
        ] {
            let gate = StorageIntegrityGate::from_report(report(status));
            assert_eq!(gate.admission(), StorageIntegrityAdmission::Allowed);
            assert!(gate.admit_execution().is_ok(), "{status:?}");
        }

        let corrupt = StorageIntegrityGate::from_report(report(IntegrityScanStatus::Corrupt));
        assert_eq!(corrupt.admission(), StorageIntegrityAdmission::Blocked);
        assert!(corrupt.quarantine_required());
        assert_eq!(
            corrupt.admit_execution().unwrap_err(),
            PortError::Conflict("eventlog_integrity_quarantine_required".to_owned())
        );

        let unknown = StorageIntegrityGate::from_report(report(IntegrityScanStatus::Unknown));
        assert_eq!(unknown.admission(), StorageIntegrityAdmission::Blocked);
        assert!(!unknown.quarantine_required());
        assert_eq!(
            unknown.admit_execution().unwrap_err(),
            PortError::Unavailable("eventlog_integrity_unknown".to_owned())
        );
    }
}
