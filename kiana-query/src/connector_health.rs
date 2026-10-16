//! 本文件把连接器绑定事实和健康事实投影成可供查询/展示使用的最新健康摘要。
//!
//! 在系统中的位置：Daemon 的连接器处理器从连接器事件流读取历史，并在健康探测事件提交后
//! 调用 `project_connector_health()`；注册表列表路径也会用同一历史重新计算投影。Query crate
//! 导出该函数，但它只消费传入的已提交 `RuntimeEvent` 切片，不负责探测连接器或写 EventLog。
//!
//! ```text
//! Daemon 连接器事件流
//!   -> connector.binding / connector.health 事件
//!   -> 本文件解码并校验绑定与健康事实
//!   -> 每个 binding 保留切片中最后出现的健康事实
//!   -> 与切片最终绑定 revision 比较，标记 revision drift
//!   -> ConnectorHealthProjection -> Daemon 返回结果或注册表视图
//! ```
//!
//! 该投影的证明等级固定为 `source`：它说明哪些源事件被读到及其字段关系，不表示健康状态
//! 是 live 检测结果，也不能作为权限判断。畸形绑定/健康事件会使整次投影失败；Daemon 健康路径
//! 在生成投影前已追加健康事件，因此若投影失败，事件可能已经持久化而请求随后返回失败。

use kiana_domain::{
    ConnectorBindingSnapshot, ConnectorHealthFact, RuntimeEvent, CONNECTOR_HEALTH_EVENT_KIND,
    CONNECTOR_HEALTH_PROJECTION_SCHEMA,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CONNECTOR_HEALTH_PROJECTION_VERSION: &str = "connector-health.v1";
/// 单个投影最多接受的绑定健康条目数；校验在完整处理输入事件之后执行。
const MAX_HEALTH_ENTRIES: usize = 256;
/// 单个条目及投影级最多保留的限制说明数。
const MAX_HEALTH_LIMITATIONS: usize = 16;

/// 一个连接器绑定对应的最新健康事实及其来源、版本和限制说明。
///
/// 由 projector 从健康事件生成。这里的 `stale` 专指健康事实引用的 binding revision 与输入切片中
/// 最后观察到的 binding revision 不同，不是按 `checked_at_unix_ms` 计算的时间过期判断。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHealthProjectionEntry {
    /// Domain 校验过的健康事实，例如探测状态、检查时间、binding ID 和 revision。
    pub health: ConnectorHealthFact,
    /// 产生本条健康事实的 EventStore stream version；不是全局事件 ID。
    pub source_cursor: u64,
    /// 本 Query 投影合同版本。
    pub projection_version: String,
    /// 健康事实生成时绑定的配置 revision。
    pub binding_revision: u64,
    /// 可选的凭证轮换代次；缺失表示事实没有携带该字段，不等于已验证当前凭证。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_generation: Option<u64>,
    /// 产生健康事件时携带的数据治理 epoch；可能缺失。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_epoch: Option<u64>,
    /// binding revision drift 标记；与健康检查时间新旧是两个不同维度。
    pub stale: bool,
    /// 当前固定为 `source`，不因投影成功自动升级到 `local_behavior` 或 `live`。
    pub proof_level: String,
    /// 源健康事实自带的限制，以及 projector 补充的 revision drift 说明。
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ConnectorHealthProjectionEntry {
    /// 检查单条投影记录保留有效 Domain fact、非零游标、合同版本和有界限制说明。
    ///
    /// 【调用者】外层 `ConnectorHealthProjection::validate()` 对每项调用。
    /// 【失败】Domain 健康事实错误、游标/revision 无效、证明等级不为 source、限制过多或限制文本
    /// 为空/过长/含 NUL 时返回稳定错误；错误会令整份 projection 无效。
    /// 【边界】本方法没有绑定当前连接器外部状态，也无法重新判断 `stale` 是否与完整事件流一致。
    pub fn validate(&self) -> Result<(), String> {
        self.health.validate()?;
        if self.source_cursor == 0
            || self.projection_version != CONNECTOR_HEALTH_PROJECTION_VERSION
            || self.binding_revision == 0
            || self.credential_generation == Some(0)
            || self.proof_level != "source"
            || self.limitations.len() > MAX_HEALTH_LIMITATIONS
            || self
                .limitations
                .iter()
                .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
        {
            return Err("connector_health_projection_entry_invalid".to_owned());
        }
        Ok(())
    }
}

/// 某一 EventStore 历史切片上的连接器健康只读投影。
///
/// `source_cursor` 是输入事件中观察到的最大 stream version；若整个输入没有正游标，当前实现
/// 用 `1` 保持 DTO 的非零合同。无条目时会附带 `connector_health_not_observed`，因此不能把该
/// 占位值解释为真的读过 version 1。此结构不含 EventLog 写入能力，也不做授权判断。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectorHealthProjection {
    /// Domain 定义的稳定投影 schema。
    pub schema: String,
    /// 输入历史切片观察到的最大 stream version，或空/无版本输入时的非零占位值。
    pub source_cursor: u64,
    /// Query projector 的实现合同版本。
    pub projection_version: String,
    /// 每个 binding 至多一条、按 BTreeMap 的 binding ID 顺序生成的结果。
    pub entries: Vec<ConnectorHealthProjectionEntry>,
    /// 整份投影的限制说明；空结果会指出没有观察到健康事实。
    #[serde(default)]
    pub limitations: Vec<String>,
}

impl ConnectorHealthProjection {
    /// 检查投影 schema、来源游标、总量边界、每条记录和 binding 唯一性。
    ///
    /// 【调用者】`project_connector_health()` 返回前调用；调用方也可对收到/反序列化的 DTO 调用。
    /// 【失败】任何头部错误、超过 256 条、超过 16 条投影限制、重复 binding 或 entry 无效都会拒绝
    /// 整个投影。它不检查 entry.source_cursor 是否不大于外层 source_cursor，也不重新读取事件流。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CONNECTOR_HEALTH_PROJECTION_SCHEMA
            || self.source_cursor == 0
            || self.projection_version != CONNECTOR_HEALTH_PROJECTION_VERSION
            || self.entries.len() > MAX_HEALTH_ENTRIES
            || self.limitations.len() > MAX_HEALTH_LIMITATIONS
        {
            return Err("connector_health_projection_invalid".to_owned());
        }
        let mut bindings = std::collections::BTreeSet::new();
        for entry in &self.entries {
            entry.validate()?;
            if !bindings.insert(entry.health.binding_id.clone()) {
                return Err("connector_health_projection_duplicate_binding".to_owned());
            }
        }
        if self
            .limitations
            .iter()
            .any(|value| value.trim().is_empty() || value.len() > 256 || value.contains('\0'))
        {
            return Err("connector_health_projection_limitation_invalid".to_owned());
        }
        Ok(())
    }
}

/// 从连接器事件切片投影每个绑定当前可见的最后一条健康事实。
///
/// 【调用者】Daemon `connectors.rs` 的健康结果和连接器列表分支调用；健康结果分支先提交新事件，
/// 再把既有历史与新事件合并后调用。当前切片顺序来自调用方提供的 stream 历史。
/// 【输入】包含 `connector.binding` 与连接器健康事件的 `RuntimeEvent` 切片；其他事件种类被忽略，
/// 但它们携带的 stream version 仍可能提高外层 `source_cursor`。
/// 【输出】成功返回每个 binding 一条的 `ConnectorHealthProjection`；同 ID 的绑定和健康事实都由
/// 输入切片中最后出现的事件覆盖，最终绑定 revision 用来标记健康 fact 是否 stale。
/// 【失败】事件 payload 无法反序列化或 Domain 校验失败、健康事实在此前未观察到 binding、结果超过
/// 限额、健康事实缺少有效 stream cursor 等情况会返回错误，调用方不会得到部分投影。
/// 【副作用】只读传入切片，不访问 adapter/credential，不重试探测，不写事件。错误由 Daemon 转成
/// operation failure；健康路径的 EventStore append 在调用本函数之前已完成。
/// 【易误改点】当前 map 的覆盖依据是切片遍历顺序而不是显式比较 cursor；调用方必须提供按 stream
/// 顺序排列的历史，并把新提交事件追加在末尾。乱序切片可能让低 cursor 的后出现事件覆盖真正较新的事实。
pub fn project_connector_health(
    events: &[RuntimeEvent],
) -> Result<ConnectorHealthProjection, String> {
    let mut bindings: BTreeMap<String, ConnectorBindingSnapshot> = BTreeMap::new();
    let mut latest: BTreeMap<String, (u64, ConnectorHealthFact, Option<u64>)> = BTreeMap::new();
    let mut source_cursor = 0;
    for event in events {
        // 外层 cursor 汇总整个输入流的最大观察版本，不局限于下面识别的健康/绑定事件。
        let cursor = event.stream_version.unwrap_or_default();
        source_cursor = source_cursor.max(cursor);
        match event.kind.as_str() {
            "connector.binding" => {
                // 绑定快照先进入 map，健康 fact 才能证明它引用了本切片中已经观察到的 binding。
                let binding: ConnectorBindingSnapshot =
                    serde_json::from_value(event.data["state"].clone())
                        .map_err(|_| "connector_health_binding_invalid".to_owned())?;
                binding.validate().map_err(str::to_owned)?;
                bindings.insert(binding.binding.binding_id.clone(), binding);
            }
            CONNECTOR_HEALTH_EVENT_KIND => {
                let fact: ConnectorHealthFact =
                    serde_json::from_value(event.data["health"].clone())
                        .map_err(|_| "connector_health_fact_invalid".to_owned())?;
                fact.validate()?;
                if !bindings.contains_key(&fact.binding_id) {
                    return Err("connector_health_binding_missing".to_owned());
                }
                // BTreeMap 提供稳定输出顺序；同一 ID 的值按输入切片顺序覆盖。
                latest.insert(fact.binding_id.clone(), (cursor, fact, event.data_epoch));
            }
            _ => {}
        }
    }
    // 空输入或只有无 stream metadata 的事件没有正 cursor；1 是 DTO 合同占位值，不是新事件。
    if source_cursor == 0 {
        source_cursor = 1;
    }
    let mut entries = Vec::with_capacity(latest.len());
    let mut limitations = Vec::new();
    for (_binding_id, (cursor, fact, data_epoch)) in latest {
        // 比较整段历史最终看到的 binding revision；不根据健康检查时间或 credential generation 推断时效。
        let stale = bindings
            .get(&fact.binding_id)
            .is_none_or(|binding| binding.revision != fact.binding_revision);
        let mut entry_limitations = fact.limitations.clone();
        if stale {
            entry_limitations.truncate(MAX_HEALTH_LIMITATIONS.saturating_sub(1));
            entry_limitations.push("binding_revision_changed".to_owned());
        } else {
            entry_limitations.truncate(MAX_HEALTH_LIMITATIONS);
        }
        entries.push(ConnectorHealthProjectionEntry {
            binding_revision: fact.binding_revision,
            credential_generation: fact.credential_generation,
            health: fact,
            source_cursor: cursor,
            projection_version: CONNECTOR_HEALTH_PROJECTION_VERSION.to_owned(),
            data_epoch,
            stale,
            proof_level: "source".to_owned(),
            limitations: entry_limitations,
        });
    }
    if entries.is_empty() {
        limitations.push("connector_health_not_observed".to_owned());
    }
    limitations.truncate(MAX_HEALTH_LIMITATIONS);
    let projection = ConnectorHealthProjection {
        schema: CONNECTOR_HEALTH_PROJECTION_SCHEMA.to_owned(),
        source_cursor,
        projection_version: CONNECTOR_HEALTH_PROJECTION_VERSION.to_owned(),
        entries,
        limitations,
    };
    projection.validate()?;
    Ok(projection)
}
