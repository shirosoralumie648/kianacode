//! 本文件把 Connector quota 的保留、认领、结算和释放事件重建成只读状态视图。
//!
//! 在系统中的位置：Core/Daemon 的 quota 写路径把已提交 `RuntimeEvent` 交给 EventStore；本
//! Query projector 预留了从这类事件重新组合 reservation/settlement 的能力。目前仓内没有找到
//! `project_connector_quota()` 的生产调用，INT-17 Query 文件也只是静态 marker guard。
//!
//! ```text
//! 已提交 connector quota 事件切片
//!   -> 按事件 kind 解码 reservation，settled 事件还解码 settlement
//!   -> 按 reservation ID 留下切片中最后出现的状态
//!   -> Domain 校验 reservation 与 settlement
//!   -> ConnectorQuotaProjection（source 级、可回放描述）
//! ```
//!
//! 投影会让 Unknown 或 released 状态继续可见，不将缺席信息偷换成“还有可用额度”。它不重算容量、
//! 不做准入、不调用外部连接器，也不把重建结果升级为授权；真正能否执行仍由 Core 的 quota gate 决定。

use kiana_domain::{
    ConnectorQuotaReservation, ConnectorQuotaSettlement, RuntimeEvent,
    CONNECTOR_QUOTA_EVENT_CLAIMED, CONNECTOR_QUOTA_EVENT_RELEASED, CONNECTOR_QUOTA_EVENT_RESERVED,
    CONNECTOR_QUOTA_EVENT_SETTLED, CONNECTOR_QUOTA_RESERVATION_SCHEMA,
    CONNECTOR_QUOTA_SETTLEMENT_SCHEMA,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONNECTOR_QUOTA_PROJECTION_SCHEMA: &str = "kiana.connector-quota-projection.v1";
pub const CONNECTOR_QUOTA_PROJECTION_VERSION: &str = "connector-quota.v1";
/// 输出中允许保留的 reservation 数；输入事件本身没有在循环前按数量限制。
const MAX_QUOTA_ENTRIES: usize = 512;

/// 一个 reservation 当前可见的状态及可选结算事实。
///
/// projector 在同一 ID 的 reservation 事件之间按传入切片顺序覆盖；结算对象由 settled 事件携带，
/// 并先与该事件自带的 reservation 快照做精确摘要校验，进入输出后再与最终保留的快照做身份级
/// 校验（同一 reservation ID/command digest/上限），因此「结算后再 release」不会让投影整体失败。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaProjectionEntry {
    /// Domain 定义的 reservation 身份、范围、维度、状态和自摘要。
    pub reservation: ConnectorQuotaReservation,
    /// settled 事件提供的结算结果；没有结算事件时为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settlement: Option<ConnectorQuotaSettlement>,
    /// 最后覆盖该 reservation 状态的事件 cursor；不是所有输入事件的全局最大值。
    pub source_cursor: u64,
    /// 当前 Query projection 合同版本。
    pub projection_version: String,
    /// 当前 builder 固定为 true，表示这些状态可从输入事件重建；不表示已执行新的重放操作。
    pub replayable: bool,
    /// 当前固定为 `source`，表示源事件级别的投影，不是 live 额度检查结果。
    pub proof_level: String,
}

impl ConnectorQuotaProjectionEntry {
    /// 校验单条投影的领域事实、cursor、合同版本和证明标签。
    ///
    /// 【调用者】`ConnectorQuotaProjection::validate()` 对每个 entry 调用。
    /// 【失败】reservation 无效、settlement 与 reservation 不匹配、cursor 为零、版本不符、
    /// `replayable` 为 false 或证明等级不是 source 时返回错误并拒绝整个投影。
    /// 【边界】这里只校验传入事实自洽，不证明对应事件已提交或当前时间窗的额度仍可用。
    pub fn validate(&self) -> Result<(), String> {
        self.reservation.validate()?;
        if let Some(settlement) = &self.settlement {
            // The latest snapshot may already be Released/Unknown after a later transition; the
            // settlement therefore binds the reservation identity and command digest rather than
            // the latest snapshot digest. The write path still validates the exact digest against
            // the snapshot carried by the settled event (`validate_against`).
            settlement.validate_identity_against(&self.reservation)?;
        }
        if self.source_cursor == 0
            || self.projection_version != CONNECTOR_QUOTA_PROJECTION_VERSION
            || !self.replayable
            || self.proof_level != "source"
        {
            return Err("connector_quota_projection_entry_invalid".to_owned());
        }
        Ok(())
    }
}

/// 从 quota 事件得到的只读 reservation/settlement 清单。
///
/// 外层 cursor 是输入切片所有事件的最大 stream version，可能包含与 quota 无关的事件；每条
/// entry 的 cursor 则来自对应 reservation 状态事件。若全切片没有正 cursor，builder 以 1 作
/// 非零占位值并通过 limitation 说明无 quota 事实；不能把该值理解成已经观察到真实 stream version 1。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorQuotaProjection {
    /// 固定投影 schema。
    pub schema: String,
    /// 输入事件流中最大的 cursor，或无正 cursor 时的占位值。
    pub source_cursor: u64,
    /// Query 投影实现版本。
    pub projection_version: String,
    /// 按 reservation ID 升序排列的当前状态记录，最多 512 条。
    pub entries: Vec<ConnectorQuotaProjectionEntry>,
    /// 缺少事实或投影非授权等边界说明；builder 会写入至少一条，但 validator 不限制数量。
    pub limitations: Vec<String>,
}

impl ConnectorQuotaProjection {
    /// 校验外层 schema、总记录上限、限制文本和 reservation 身份唯一性。
    ///
    /// 【调用者】projector 返回前调用；调用方也可校验反序列化对象。
    /// 【失败】头字段错误、超过 512 条、限制文本为空/过长/含换行或 NUL、任一 entry 无效、
    /// reservation ID 重复时拒绝整个 projection。
    /// 【边界】限制说明的总条数没有上限；这里也没有要求 entry cursor 不大于外层 cursor，或重建
    /// EventStore 的先后顺序。自洽 DTO 不能证明容量尚可用。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_QUOTA_PROJECTION_SCHEMA
            || self.source_cursor == 0
            || self.projection_version != CONNECTOR_QUOTA_PROJECTION_VERSION
            || self.entries.len() > MAX_QUOTA_ENTRIES
            || self.limitations.iter().any(|value| {
                value.trim().is_empty() || value.len() > 256 || value.contains(['\0', '\r', '\n'])
            })
        {
            return Err("connector_quota_projection_invalid".to_owned());
        }
        let mut ids = std::collections::BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !ids.insert(entry.reservation.reservation_id) {
                return Err("connector_quota_projection_duplicate_reservation".to_owned());
            }
        }
        Ok(())
    }
}

/// 从输入事件切片重建每个 quota reservation 的当前可见状态。
///
/// 【调用者】当前仓内没有找到生产调用；Core 负责的 quota admission/提交路径是事件事实的上游，
/// Query crate 对外导出该 projector，INT-17 Query 文件只检查源码 marker。
/// 【输入】已提交的 `RuntimeEvent` 切片；只处理 reserved、claimed、settled、released 四种 quota
/// kind，其他 kind 不解码，但其 cursor 仍计入 projection 的全局 source_cursor。
/// 【输出】成功返回按 reservation ID 排序的 `ConnectorQuotaProjection`，每个 ID 一条最新状态，
/// 并在 settled 事件存在时附上经过 Domain 交叉校验的 settlement。
/// 【失败】reservation/settlement JSON 无法解码、schema 不匹配、Domain 合同不通过、settlement
/// 与 reservation 不匹配、或最终总量/唯一性校验失败时返回稳定错误；不会返回部分投影。
/// 【副作用】纯内存重放，不写 Store、不释放 reservation、不计算剩余额度、不调用连接器。
/// 【易误改点】map 的最后写入者按切片位置决定，而不是显式比较事件 cursor；调用方必须按流顺序
/// 提供事件。若输入乱序，较旧 cursor 但位置靠后的状态会覆盖较新事实。unknown 事件会被忽略，
/// 但不能因此推断它们与 quota 完全无关或授权容量已经释放。
pub fn project_connector_quota(
    events: &[RuntimeEvent],
) -> Result<ConnectorQuotaProjection, String> {
    let mut reservations: BTreeMap<
        kiana_domain::QuotaReservationId,
        (u64, ConnectorQuotaReservation),
    > = BTreeMap::new();
    let mut settlements: BTreeMap<kiana_domain::QuotaReservationId, ConnectorQuotaSettlement> =
        BTreeMap::new();
    let mut source_cursor = 0;
    for event in events {
        // cursor 汇总输入流的最大版本，即使 event kind 不是本 projector 认识的 quota 事件。
        let cursor = event.stream_version.unwrap_or_default();
        source_cursor = source_cursor.max(cursor);
        match event.kind.as_str() {
            CONNECTOR_QUOTA_EVENT_RESERVED
            | CONNECTOR_QUOTA_EVENT_CLAIMED
            | CONNECTOR_QUOTA_EVENT_SETTLED
            | CONNECTOR_QUOTA_EVENT_RELEASED => {
                // 各状态事件都带一份完整 reservation 快照，因此用 ID 覆盖旧快照而非把事件简单计数。
                let reservation: ConnectorQuotaReservation =
                    serde_json::from_value(event.data["reservation"].clone())
                        .map_err(|_| "connector_quota_reservation_invalid".to_owned())?;
                if reservation.schema != CONNECTOR_QUOTA_RESERVATION_SCHEMA {
                    return Err("connector_quota_reservation_schema_invalid".to_owned());
                }
                reservation.validate()?;
                let id = reservation.reservation_id;
                if event.kind == CONNECTOR_QUOTA_EVENT_SETTLED {
                    // 结算需与该事件携带的 reservation 同时校验，避免挂到不同身份/状态的 reservation 上。
                    let settlement: ConnectorQuotaSettlement =
                        serde_json::from_value(event.data["settlement"].clone())
                            .map_err(|_| "connector_quota_settlement_invalid".to_owned())?;
                    if settlement.schema != CONNECTOR_QUOTA_SETTLEMENT_SCHEMA {
                        return Err("connector_quota_settlement_schema_invalid".to_owned());
                    }
                    settlement.validate_against(&reservation)?;
                    settlements.insert(id, settlement);
                }
                reservations.insert(id, (cursor, reservation));
            }
            _ => {}
        }
    }
    // 空流或没有 stream metadata 的输入以 1 满足外层非零字段合同；空结果的 limitations 会说明未观察到事实。
    if source_cursor == 0 {
        source_cursor = 1;
    }
    let mut entries = reservations
        .into_iter()
        .map(
            |(id, (cursor, reservation))| ConnectorQuotaProjectionEntry {
                settlement: settlements.remove(&id),
                reservation,
                source_cursor: cursor,
                projection_version: CONNECTOR_QUOTA_PROJECTION_VERSION.to_owned(),
                replayable: true,
                proof_level: "source".to_owned(),
            },
        )
        .collect::<Vec<_>>();
    // BTreeMap 当前已经按 ID 迭代；显式排序让输出合同不依赖上游 map 的具体实现。
    entries.sort_by_key(|entry| entry.reservation.reservation_id);
    let limitations = if entries.is_empty() {
        vec!["connector_quota_not_observed".to_owned()]
    } else {
        vec!["projection_is_not_authorization".to_owned()]
    };
    let projection = ConnectorQuotaProjection {
        schema: CONNECTOR_QUOTA_PROJECTION_SCHEMA.to_owned(),
        source_cursor,
        projection_version: CONNECTOR_QUOTA_PROJECTION_VERSION.to_owned(),
        entries,
        limitations,
    };
    projection.validate()?;
    Ok(projection)
}
