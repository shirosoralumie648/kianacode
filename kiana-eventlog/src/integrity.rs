//! Read-only JSONL integrity scan, recovery report and fail-closed health gate.
//!
//! The scan never repairs, moves or deletes a journal itself: it opens the log through the same
//! loader every reader uses, records what that open had to repair (a torn/incomplete tail), and
//! classifies the store so a caller can refuse to expose facts that are not safe to serve. The
//! loader stays the single authority for framing and tail handling; this module only reports it.

use crate::jsonl::JsonlEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ===== 完整性扫描状态枚举 =====
// 描述事件日志的健康状态。用于 fail-closed 决策：
// - Empty: 日志不存在或为空（全新/已清理），可按空库初始化
// - Ready: 扫描通过，已提交事实完整可用
// - Recovered: 打开时回退了不完整的尾部记录（或补写缺失换行），已提交前缀完整可用
// - Corrupt: 日志中段损坏或校验失败，需隔离并重建
// - Unknown: 记录了当前代码无法识别的必需格式（未知 major/schema/writer-version），需人工介入
//
// 这四类失败/异常状态（Empty/Recovered/Corrupt/Unknown）必须互相可区分：未知格式不能被当成
// 空库，损坏中段不能被跳过，修复过尾部必须留下可观察的恢复记录。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityScanStatus {
    Empty,     // 无数据，可安全初始化
    Ready,     // 扫描通过，数据完整可用
    Recovered, // 打开时修复了不完整尾部，已提交前缀可用
    Corrupt,   // 数据损坏，需隔离和重建
    Unknown,   // 存在无法识别的必需记录，需调查
}

impl IntegrityScanStatus {
    /// 稳定字符串，供报告、日志和守卫测试使用。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Ready => "ready",
            Self::Recovered => "recovered",
            Self::Corrupt => "corrupt",
            Self::Unknown => "unknown",
        }
    }

    /// 是否可以安全地把这个 store 暴露给上层读取/执行。
    ///
    /// `Empty`、`Ready` 与 `Recovered` 允许继续：尾部修复只影响未提交的尾部字节，已提交前缀
    /// 仍由加载器完整校验。`Corrupt`/`Unknown` 一律拒绝。
    pub const fn is_healthy(self) -> bool {
        matches!(self, Self::Empty | Self::Ready | Self::Recovered)
    }
}

// ===== 尾部恢复报告 =====
// 记录加载器为暴露已提交前缀而对文件尾部做的恢复动作。
// - repaired_tail: 是否回退过不完整尾记录或补写过缺失换行
// - repaired_bytes: 受影响的字节数
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrityRecoveryReport {
    pub repaired_tail: bool,
    pub repaired_bytes: u64,
}

// ===== 完整性扫描报告结构 =====
// 记录一次完整性扫描的结果。可序列化为 JSON 存储或传输。
//
// 字段说明：
// - schema: 报告格式版本（用于向后兼容）
// - path: 被扫描的事件日志文件路径
// - status: 扫描结果状态
// - source_cursor: 成功读取的事件数量
// - quarantine_required: 是否需要隔离（Corrupt 时为 true）
// - incident_code: 错误详情（若扫描失败）
// - recovery_action: 推荐的恢复操作（continue / repair_tail_and_continue /
//   pause_and_reconcile / quarantine_and_reconcile）
// - recovery: 打开时实际发生的尾部恢复动作（成功打开时才有意义）
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntegrityScanReport {
    pub schema: String,
    pub path: PathBuf,
    pub status: IntegrityScanStatus,
    pub source_cursor: u64,
    pub quarantine_required: bool,
    pub incident_code: Option<String>,
    pub recovery_action: String,
    #[serde(default)]
    pub recovery: IntegrityRecoveryReport,
}

impl IntegrityScanReport {
    pub const SCHEMA: &'static str = "kiana.eventlog-integrity-report.v2";

    // ===== 完整性健康门 =====
    // 用于控制平面做出 fail-closed 决策。
    // 仅 Empty/Ready/Recovered 通过门，其他状态阻止操作继续。
    pub fn health_gate(&self) -> Result<(), PortError> {
        match self.status {
            // 可安全暴露的状态：空库、完整库、以及仅修复了未提交尾部的库
            IntegrityScanStatus::Empty
            | IntegrityScanStatus::Ready
            | IntegrityScanStatus::Recovered => Ok(()),
            // Corrupt 状态需要隔离处理
            IntegrityScanStatus::Corrupt => Err(PortError::Conflict(
                "eventlog_integrity_quarantine_required".to_owned(),
            )),
            // Unknown 状态需要等待调查，不能被当成空库
            IntegrityScanStatus::Unknown => Err(PortError::Unavailable(
                "eventlog_integrity_unknown".to_owned(),
            )),
        }
    }

    /// 健康门是否为放行状态；等价于 `health_gate().is_ok()`。
    pub fn is_healthy(&self) -> bool {
        self.health_gate().is_ok()
    }
}

// ===== 错误分类和恢复建议 =====
// 根据错误信息推断数据损坏程度并建议恢复操作。
//
// 分类规则：
// 1. 包含 corrupt/checksum/integrity/frame_invalid 关键字 → 数据损坏
//    恢复建议：隔离并重建（quarantine_and_reconcile）
// 2. 包含 required/unsupported/unknown major 等“当前代码不认识的必需格式” → 未知格式
//    不能被当成空库；恢复建议：暂停并调查（pause_and_reconcile）
// 3. 其他错误 → 原因不明
//    恢复建议：暂停并调查（pause_and_reconcile）
fn classify_error(error: &PortError) -> (IntegrityScanStatus, String) {
    let text = error.to_string();
    if text.contains("corrupt")
        || text.contains("checksum")
        || text.contains("integrity")
        || text.contains("frame_invalid")
    {
        // 数据损坏迹象明确：需要隔离并重建
        (
            IntegrityScanStatus::Corrupt,
            "quarantine_and_reconcile".to_owned(),
        )
    } else {
        // 错误原因不明（含未知必需格式）：暂停并等待人工调查，绝不退化为空库
        (
            IntegrityScanStatus::Unknown,
            "pause_and_reconcile".to_owned(),
        )
    }
}

// ===== 异步 JSONL 完整性扫描 =====
// 对事件日志文件进行完整性检查，记录扫描结果为报告。
//
// 扫描步骤：
// 1. 通过加载器打开 JSONL 文件并重放（失败 → 返回 Unknown/Corrupt）
// 2. 打开成功时读取加载器报告过的尾部恢复动作
// 3. 尝试读取所有事件（失败 → 根据错误类型分类）
// 4. 统计事件数量和状态：修复过尾部 → Recovered；无事件 → Empty；否则 → Ready
//
// 防御设计：
// - 文件访问错误也会被捕获并记录
// - 报告包含足够信息供下游决策
// - fail-closed：不确定的状态被归为 Unknown（需人工介入）
// - 未知格式 / 损坏中段不会被当成空库或可用事实
pub async fn scan_jsonl(path: impl AsRef<Path>) -> IntegrityScanReport {
    let path = path.as_ref().to_path_buf();

    // 第一步：打开并重放日志，同时取回加载器报告的尾部恢复动作
    match JsonlEventLog::open_with_recovery_async(&path).await {
        Ok((store, journal_recovery)) => {
            let recovery = IntegrityRecoveryReport {
                repaired_tail: journal_recovery.repaired_tail,
                repaired_bytes: journal_recovery.repaired_bytes,
            };
            // 第二步：尝试读取所有事件
            match store.read_all().await {
                Ok(events) => {
                    // 修复尾部优先于“空库”：有过修复说明这不是一个全新空库
                    let status = if recovery.repaired_tail {
                        IntegrityScanStatus::Recovered
                    } else if events.is_empty() {
                        IntegrityScanStatus::Empty
                    } else {
                        IntegrityScanStatus::Ready
                    };
                    let recovery_action = if recovery.repaired_tail {
                        "repair_tail_and_continue"
                    } else {
                        "continue"
                    };
                    IntegrityScanReport {
                        schema: IntegrityScanReport::SCHEMA.to_owned(),
                        path,
                        status,
                        source_cursor: events.len() as u64,
                        quarantine_required: false,
                        incident_code: None,
                        recovery_action: recovery_action.to_owned(),
                        recovery,
                    }
                }
                Err(error) => {
                    // 读取失败，分类错误并生成失败报告
                    let (status, action) = classify_error(&error);
                    IntegrityScanReport {
                        schema: IntegrityScanReport::SCHEMA.to_owned(),
                        path,
                        status,
                        source_cursor: 0,
                        quarantine_required: status == IntegrityScanStatus::Corrupt,
                        incident_code: Some(error.to_string()),
                        recovery_action: action,
                        recovery,
                    }
                }
            }
        }
        Err(error) => {
            // 打开/重放失败：未知格式或中段损坏，分类错误并生成失败报告
            let (status, action) = classify_error(&error);
            IntegrityScanReport {
                schema: IntegrityScanReport::SCHEMA.to_owned(),
                path,
                status,
                source_cursor: 0,
                quarantine_required: status == IntegrityScanStatus::Corrupt,
                incident_code: Some(error.to_string()),
                recovery_action: action,
                recovery: IntegrityRecoveryReport::default(),
            }
        }
    }
}
