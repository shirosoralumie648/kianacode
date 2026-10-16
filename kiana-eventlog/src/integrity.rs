//! Read-only JSONL integrity scan and fail-closed health gate.

use crate::JsonlEventLog;
use kiana_ports::{EventStorePort, PortError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ===== 完整性扫描状态枚举 =====
// 描述事件日志的健康状态。用于 fail-closed 决策：
// - Ready: 日志可正常读写
// - Empty: 日志文件为空（新创建或已清理）
// - Corrupt: 日志文件损坏（数据校验失败）
// - Unknown: 扫描失败，原因不明确（需人工介入）
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityScanStatus {
    Empty,   // 无数据，可安全初始化
    Ready,   // 扫描通过，数据完整可用
    Corrupt, // 数据损坏，需隔离和重建
    Unknown, // 状态不确定，需调查
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
// - recovery_action: 推荐的恢复操作（continue / pause_and_reconcile / quarantine_and_reconcile）
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
}

impl IntegrityScanReport {
    pub const SCHEMA: &'static str = "kiana.eventlog-integrity-report.v1";

    // ===== 完整性健康门 =====
    // 用于控制平面做出 fail-closed 决策。
    // 仅 Ready 和 Empty 通过门，其他状态阻止操作继续。
    pub fn health_gate(&self) -> Result<(), PortError> {
        match self.status {
            // 这两个状态安全可用
            IntegrityScanStatus::Ready | IntegrityScanStatus::Empty => Ok(()),
            // Corrupt 状态需要隔离处理
            IntegrityScanStatus::Corrupt => Err(PortError::Conflict(
                "eventlog_integrity_quarantine_required".to_owned(),
            )),
            // Unknown 状态需要等待调查
            IntegrityScanStatus::Unknown => Err(PortError::Unavailable(
                "eventlog_integrity_unknown".to_owned(),
            )),
        }
    }
}

// ===== 错误分类和恢复建议 =====
// 根据错误信息推断数据损坏程度并建议恢复操作。
//
// 分类规则：
// 1. 包含 corrupt/checksum/integrity/frame_invalid 关键字 → 数据损坏
//    恢复建议：隔离并重建（quarantine_and_reconcile）
// 2. 其他错误 → 原因不明
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
        // 错误原因不明：暂停并等待人工调查
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
// 1. 打开 JSONL 文件（失败 → 返回 Unknown）
// 2. 尝试读取所有事件（失败 → 根据错误类型分类）
// 3. 统计事件数量和状态
//
// 防御设计：
// - 文件访问错误也会被捕获并记录
// - 报告包含足够信息供下游决策
// - fail-closed：不确定的状态被归为 Unknown（需人工介入）
pub async fn scan_jsonl(path: impl AsRef<Path>) -> IntegrityScanReport {
    let path = path.as_ref().to_path_buf();

    // 第一步：尝试打开 JSONL 文件
    match JsonlEventLog::open_async(&path).await {
        Ok(store) => {
            // 第二步：尝试读取所有事件
            match store.read_all().await {
                Ok(events) => {
                    // 成功读取，生成成功报告
                    IntegrityScanReport {
                        schema: IntegrityScanReport::SCHEMA.to_owned(),
                        path,
                        status: if events.is_empty() {
                            IntegrityScanStatus::Empty
                        } else {
                            IntegrityScanStatus::Ready
                        },
                        source_cursor: events.len() as u64,
                        quarantine_required: false,
                        incident_code: None,
                        recovery_action: "continue".to_owned(),
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
                    }
                }
            }
        }
        Err(error) => {
            // 打开文件失败，分类错误并生成失败报告
            let (status, action) = classify_error(&error);
            IntegrityScanReport {
                schema: IntegrityScanReport::SCHEMA.to_owned(),
                path,
                status,
                source_cursor: 0,
                quarantine_required: status == IntegrityScanStatus::Corrupt,
                incident_code: Some(error.to_string()),
                recovery_action: action,
            }
        }
    }
}
