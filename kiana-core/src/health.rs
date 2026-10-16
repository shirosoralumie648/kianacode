//! `kiana-core::health` 模块负责控制平面只读健康投影（health projection）。
//!
//! 【文件职责】
//! 本文件把事件存储（EventStore）中的运行时事件（`RuntimeEvent`）和 EventStore 能力
//! 声明，汇总成健康快照（`HealthSnapshot`），供启动探针（startup probe）、存活探针
//!（liveness probe）和就绪探针（readiness probe）使用。它还提供 `ControlPlane` 上的三个
//! 便捷入口，供 `DaemonHost`、HTTP/API 适配层、运维证据（operator evidence）、入口一致性
//! 检查（parity）和重放诊断（replay diagnostics）读取同一份健康事实。
//!
//! 【系统位置与数据流】
//! ```text
//! EventStore 已提交 RuntimeEvent + EventStoreCapabilities 能力声明
//!             |
//!             v
//! project_operational_metrics()
//!             |
//!             v
//! status_for_probe() + component()/limitations
//!             |
//!             v
//! Domain::HealthSnapshot（带 source_cursor、能力位、组件状态、摘要）
//!             |
//!             +--> DaemonHost::health_snapshot() 补充 daemon/telemetry 观察
//!             +--> operator evidence / parity / replay diagnostics 读取摘要
//!             +--> API 层向调用者返回受限的健康结果
//! ```
//!
//! 【上游与下游】
//! - 在 ControlPlane 路径中，上游是 `read_all_events()` 和 EventStore 的能力快照；
//!   operator/parity/replay 等其他投影调用者也可把各自读取的同一事件视图直接传入。
//!   本文件不探测上游模型服务（Provider）、能力代理（Broker）、网络或本地进程。
//! - 下游是 `kiana-daemon` 的组合根以及只读诊断/证据投影。下游可以据此显示
//!   `Ok`、`Degraded` 或 `Unknown`，但不能把这个快照当成授权、执行收据或 Provider
//!   成功证明。
//!
//! 【为什么必须单独做投影】
//! 健康状态是事实的派生视图，不是第二个权威状态机。把事件中的游标、投影进度、
//! 恢复异常和 EventStore 能力集中计算，可以让不同入口看到同一套判断；缺失、陈旧或
//! 不可验证的证据会被明确标为降级/未知，而不是凭空声称系统健康。删除当前实际参与
//! 判定的能力、游标或恢复检查，可能让调用者在事实不足时误以为可以继续接纳请求。
//! 这里提到的持久提交（durable commit）能力是 EventStore 的声明之一；它不是本文件
//! 自己验证磁盘已经完成物理落盘的证明。

use kiana_domain::{
    ComponentHealth, ComponentHealthState, EventId, EventStoreCapabilities, HealthProbeKind,
    HealthSnapshot, MetricSnapshot, RuntimeEvent, SignalStatus,
};
use std::collections::{BTreeMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{project_operational_metrics, ControlPlane, CoreError};

const COMPONENT_VERSION: &str = "health.v1";
// 健康快照只能携带有限数量的限制原因。限制数量有上界，既防止异常事件制造无限大
// 响应，也让下游 UI、日志和摘要的大小保持可预测；达到上限后不会改变核心状态判定。
const MAX_LIMITATIONS: usize = 16;

/// 健康投影在构造输入或校验输出时可能遇到的结构化失败。
///
/// 这些错误在 `ControlPlane::health_snapshot()` 中会被转换为 `PortError`，最终由
/// 调用入口决定如何向用户显示或记录。它们表示“当前无法得到可信快照”，不是一次
/// Provider 请求失败，也不会触发重试、禁用账号或产生执行事件。
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HealthProjectionError {
    /// EventLog 没有任何可作为健康依据的事件。
    #[error("health_source_empty")]
    SourceEmpty,
    /// 观测时间为零或系统时钟无法转换为 Unix 毫秒时间戳。
    #[error("health_observed_at_invalid")]
    ObservedAtInvalid,
    /// operational metrics 无法从事件视图构造，括号内保留底层错误码。
    #[error("health_metrics_unavailable:{0}")]
    MetricsUnavailable(String),
    /// Domain `HealthSnapshot` 的字段、能力数量、限制数量或摘要校验失败。
    #[error("health_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
}

/// 从事件列表中提取有限且去重的源事件 ID。
///
/// 【调用者】`project_health_snapshot()` 用它填充快照的 `source_event_ids`，让下游能
/// 知道这份健康判断引用过哪些事件。
///
/// 【为什么先去重再限长】同一个事件 ID 可能因适配器合并或恢复输入重复出现。先去重
/// 可以避免重复占用快照预算；随后使用 Domain 的统一上限，保证健康响应不会随着事件
/// 历史无限增长。这里仅返回事实引用，不修改输入事件，也不写 EventStore。
fn bounded_source_ids(events: &[RuntimeEvent]) -> Vec<EventId> {
    let mut seen = HashSet::new();
    events
        .iter()
        .filter(|event| seen.insert(event.event_id.to_string()))
        .map(|event| event.event_id)
        .take(kiana_domain::MAX_SOURCE_EVENT_IDS)
        .collect()
}

/// 从 operational metrics 中读取一个名为 `name` 的非负整数指标。
///
/// 指标在事件投影阶段以 `f64` 保存，健康判定只需要计数，因此这里将负数视为零、
/// 再转换为 `u64`。负数不是有效的恢复/失败计数；把它当成零可以防止异常观测反而
/// 让 `unknown_total` 或 `orphan_total` 看起来更小。找不到指标时返回零，调用者会
/// 将“未观察到”与“观察到零”统一用于这些布尔门槛。
fn metric_value(snapshot: &MetricSnapshot, name: &str) -> u64 {
    snapshot
        .points
        .iter()
        .find(|point| point.name == name)
        .map(|point| point.value.max(0.0) as u64)
        .unwrap_or_default()
}

/// 判断一组 metrics 是否已经记录指定的限制原因。
///
/// 这是一个只读的小 helper，用于把 metrics 阶段发现的游标缺口、推断游标等信息
/// 继续传递到健康状态和组件状态；它不重新推断事件，也不删除限制。
fn has_limitation(snapshot: &MetricSnapshot, value: &str) -> bool {
    snapshot
        .limitations
        .iter()
        .any(|limitation| limitation == value)
}

/// 向限制列表加入一个非空、未重复且未超出上限的原因。
///
/// 限制原因是诊断信息而不是新的权威状态。重复项被折叠；列表达到
/// `MAX_LIMITATIONS` 后不再追加新原因，但本 helper 不会裁剪已有列表。状态判定仍由
/// `status_for_probe()` 的明确条件负责。指标投影本身也使用同一上限，因此正常数据流
/// 能留在 Domain `HealthSnapshot` 的容量范围内。
fn add_limitation(limitations: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !value.is_empty()
        && limitations.len() < MAX_LIMITATIONS
        && !limitations.iter().any(|item| item == &value)
    {
        limitations.push(value);
    }
}

/// 用 Domain 校验规则构造一个单独的组件健康记录。
///
/// `HealthSnapshot` 的组件表要求 map key 与 `ComponentHealth.name` 一致，调用者会
/// 在返回后使用 `(name, health)` 插入 map。若 Domain 拒绝组件字段，这里把错误归入
/// `SnapshotInvalid`，从而不会返回一个看似可用但摘要不可信的快照。
fn component(
    name: &'static str,
    state: ComponentHealthState,
    cursor: Option<u64>,
    limitation: Option<&'static str>,
) -> Result<(String, ComponentHealth), HealthProjectionError> {
    let health = ComponentHealth::new(
        name,
        COMPONENT_VERSION,
        state,
        cursor,
        limitation.map(str::to_owned),
    )
    .map_err(HealthProjectionError::SnapshotInvalid)?;
    Ok((name.to_owned(), health))
}

/// 判断 EventStore 是否具备声明“可就绪”的最低能力。
///
/// 就绪意味着系统不仅能读到事件，还能原子提交、持久化提交、产生命令收据并读取
/// 游标。四项能力缺一不可：缺少任一项时，存活信号仍可能有意义，但不能把系统当成
/// 能安全接纳新工作的 Ready。这里读取的是服务端声明，不会主动探测或修复能力。
fn readiness_capable(capabilities: &EventStoreCapabilities) -> bool {
    capabilities.atomic_transitions
        && capabilities.durable_commits
        && capabilities.command_receipts
        && capabilities.cursor_reads
}

/// 按探针类型和已投影证据计算总体 `SignalStatus`。
///
/// - `Readiness` 要求完整 EventStore 能力、投影游标追平源游标，并且没有游标缺口、
///   推断游标、未知副作用或孤儿派发。
/// - `Liveness` 只判断事件游标是否出现明确缺口；它回答“服务仍能形成基本事实视图”，
///   不回答“当前是否适合接纳新请求”。
/// - `Startup` 检查最低能力和游标缺口，表示初始化基础是否可用。
/// - `Drain`/`Maintenance` 被固定标为降级，因为这两种模式的语义就是不接纳新工作，
///   即使底层事件事实本身没有损坏，也不能返回 Ready。
///
/// 这个函数只消费快照，不访问网络、不改变 ControlPlane 状态，也不触发重试。
fn status_for_probe(
    probe: HealthProbeKind,
    metrics: &MetricSnapshot,
    capabilities: &EventStoreCapabilities,
) -> SignalStatus {
    let cursor_gap = has_limitation(metrics, "eventlog_cursor_gap");
    let projector_inferred = has_limitation(metrics, "projector_cursor_inferred");
    let unknown = metric_value(metrics, "kiana.recovery.unknown_total") > 0;
    let orphan = metric_value(metrics, "kiana.recovery.orphan_total") > 0;
    match probe {
        HealthProbeKind::Readiness => {
            if readiness_capable(capabilities)
                && metrics.projector_cursor == metrics.source_cursor
                && !cursor_gap
                && !projector_inferred
                && !unknown
                && !orphan
            {
                SignalStatus::Ok
            } else {
                SignalStatus::Degraded
            }
        }
        HealthProbeKind::Liveness => {
            if cursor_gap {
                SignalStatus::Degraded
            } else {
                SignalStatus::Ok
            }
        }
        HealthProbeKind::Startup => {
            if readiness_capable(capabilities) && !cursor_gap {
                SignalStatus::Ok
            } else {
                SignalStatus::Degraded
            }
        }
        HealthProbeKind::Drain | HealthProbeKind::Maintenance => SignalStatus::Degraded,
    }
}

/// 从调用方提供的 EventStore 事件视图和能力声明构造健康快照。
///
/// 【作用与调用关系】
/// 这是本文件的核心同步投影函数。生产调用者包括 `ControlPlane::health_snapshot()`、
/// `project_operator_evidence()`、入口 parity 投影和 replay diagnostics；它们把同一批
/// EventStore 事件交给这里，避免每种入口各自发明健康判断。`kiana-daemon` 在此结果上
/// 还会补充 DaemonHost 与观测队列自己的观察。
///
/// 【输入】
/// - `events`：调用者从 EventStore 读取的事件视图。此函数假定调用方提供的是要作为
///   健康依据的事实集合；它自身不访问存储，也不验证这些事件是否已经提交。
/// - `capabilities`：EventStore adapter 声明的原子转换、持久提交、命令收据和游标读取
///   能力。它是能力声明，不是本函数现场做出的存储探测或 durability 证明。
/// - `probe`：当前要回答的 startup/readiness/liveness/drain/maintenance 问题；不同探针
///   采用不同门槛，不能把一个探针的 `Ok` 当成其他探针也通过。
/// - `observed_at_ms`：调用方提供的 Unix 毫秒观测时间，必须大于零。
///
/// 【输出与失败】
/// 成功返回带源游标、有限事件引用、能力位、组件状态、限制原因和自校验摘要的
/// Domain `HealthSnapshot`。空事件、零时间、metrics reducer 失败或 Domain 快照校验
/// 失败会返回 `HealthProjectionError`；`ControlPlane` 随后把投影类错误包装成
/// `PortError::Failed`，HTTP health handler 再将错误压成低细节的 `unavailable` 响应。
/// 全量读取不支持则在进入本函数前由 `ControlPlane` 返回 `Unavailable`。
///
/// 【副作用与设计原因】
/// 函数本身是同步、只读的 reducer：不会读写文件、追加 EventLog、请求 Provider/Broker、
/// 更改 admission 或触发重试。先复用 `project_operational_metrics()`，再由同一 metrics
/// 中的游标和 limitation 判断组件，能使健康快照与运维指标基于相同源事实。事件 ID、
/// limitation 和组件数量受 Domain 上限约束，避免输出随历史长度无界增长。
///
/// ⚠ 不要把“快照中存在 provider/broker/daemon 条目”理解为这些系统已被探测：Core 没有
/// 相应独立探针时会明确写成 `Unknown`；也不要把快照当授权票据、执行结果收据或 live
/// Provider 健康证明。
pub fn project_health_snapshot(
    events: &[RuntimeEvent],
    capabilities: &EventStoreCapabilities,
    probe: HealthProbeKind,
    observed_at_ms: u64,
) -> Result<HealthSnapshot, HealthProjectionError> {
    if events.is_empty() {
        return Err(HealthProjectionError::SourceEmpty);
    }
    if observed_at_ms == 0 {
        return Err(HealthProjectionError::ObservedAtInvalid);
    }
    // operational metrics 是健康判断共用的中间结果：它从事件中重建源游标、投影游标、
    // 恢复计数和既有 limitation。若 metrics 自身不能可信地重建，就不能跳过它继续给出
    // 看似健康的快照；保留底层稳定错误文本便于内部诊断。
    let metrics = project_operational_metrics(events, None)
        .map_err(|error| HealthProjectionError::MetricsUnavailable(error.to_string()))?;
    let mut limitations = metrics.limitations.clone();

    // 能力不足先作为可读 limitation 暴露。readiness 是否通过仍由 status_for_probe() 的
    // 明确门槛决定；限制列表用于解释原因，受 MAX_LIMITATIONS 限制，不承担授权功能。
    if !capabilities.durable_commits {
        add_limitation(&mut limitations, "eventlog_durability_unavailable");
    }
    if !capabilities.atomic_transitions {
        add_limitation(&mut limitations, "eventlog_atomic_transitions_unavailable");
    }
    if metric_value(&metrics, "kiana.recovery.unknown_total") > 0 {
        add_limitation(&mut limitations, "effect_unknown_present");
    }
    if metric_value(&metrics, "kiana.recovery.orphan_total") > 0 {
        add_limitation(&mut limitations, "orphan_dispatch_present");
    }
    match probe {
        HealthProbeKind::Drain => add_limitation(&mut limitations, "draining_no_new_admission"),
        HealthProbeKind::Maintenance => {
            add_limitation(&mut limitations, "maintenance_no_new_admission")
        }
        _ => {}
    }

    let status = status_for_probe(probe, &metrics, capabilities);
    let source_cursor = metrics.source_cursor;
    let projector_cursor = metrics.projector_cursor;
    let mut components = BTreeMap::new();

    // EventLog 组件描述 adapter 是否声明了完整的核心读写能力，以及指标里是否发现
    // cursor gap。它不是一次新的 EventStore I/O 健康检查；`cursor_reads=false` 时标成
    // Unavailable，只有能力和游标证据都满足时才标 Healthy。
    let eventlog_state =
        if readiness_capable(capabilities) && !has_limitation(&metrics, "eventlog_cursor_gap") {
            ComponentHealthState::Healthy
        } else if capabilities.cursor_reads {
            ComponentHealthState::Degraded
        } else {
            ComponentHealthState::Unavailable
        };
    let (eventlog_name, eventlog) = component(
        "eventlog",
        eventlog_state,
        Some(source_cursor),
        (eventlog_state != ComponentHealthState::Healthy)
            .then_some("eventlog_capability_or_cursor_limit"),
    )?;
    components.insert(eventlog_name, eventlog);

    // projector 组件对照 reducer 游标与事件源游标。游标落后、存在 cursor gap，或没有
    // 显式 projector cursor 而只能推断时会降级；“推断”表示缺少显式游标证据，不必然
    // 表示数值上落后，因此单独记录其原因，避免把两种情况混为一谈。
    let projector_state = if has_limitation(&metrics, "eventlog_cursor_gap")
        || projector_cursor < source_cursor
        || has_limitation(&metrics, "projector_cursor_inferred")
    {
        ComponentHealthState::Degraded
    } else {
        ComponentHealthState::Healthy
    };
    let projector_limitation = (projector_state != ComponentHealthState::Healthy)
        .then_some("projector_cursor_or_checkpoint_limit");
    let (projector_name, projector) = component(
        "projector",
        projector_state,
        Some(projector_cursor),
        projector_limitation,
    )?;
    components.insert(projector_name, projector);

    // 下面两个 failure_total 来自事件 reducer，而不是实时调用 Receipt/Artifact 服务。
    // 没有失败计数只能说明当前事件视图未记录失败，不能证明外部资源在现实中可访问。
    let receipt_failure = metric_value(&metrics, "kiana.receipts.query_failure_total") > 0;
    let (receipt_name, receipt) = component(
        "receipt",
        if receipt_failure {
            ComponentHealthState::Degraded
        } else {
            ComponentHealthState::Healthy
        },
        Some(source_cursor),
        receipt_failure.then_some("receipt_query_failure"),
    )?;
    components.insert(receipt_name, receipt);

    let artifact_failure = metric_value(&metrics, "kiana.artifacts.read_failure_total") > 0;
    // 此处仅从已读事件判断“是否观察到 artifact 相关活动”：事件类型包含 artifact，或
    // payload 带 artifact_id。没观察到不等于 Artifact Store 已确认不存在，因此状态是
    // Unknown；有读取失败时优先报告 Degraded。
    let artifact_seen = events
        .iter()
        .any(|event| event.kind.contains("artifact") || event.data.get("artifact_id").is_some());
    let (artifact_name, artifact) = component(
        "artifact",
        if artifact_failure {
            ComponentHealthState::Degraded
        } else if artifact_seen {
            ComponentHealthState::Healthy
        } else {
            ComponentHealthState::Unknown
        },
        artifact_seen.then_some(source_cursor),
        if artifact_failure {
            Some("artifact_read_failure")
        } else if !artifact_seen {
            Some("artifact_not_observed")
        } else {
            None
        },
    )?;
    components.insert(artifact_name, artifact);

    // Unknown effect 与 orphan dispatch 都说明副作用结果或派发收敛仍需恢复处理；二者
    // 会让 recovery 组件降级。readiness 也在 status_for_probe() 中单独拦截这两类计数，
    // 防止只看某个组件列表的消费者漏掉 admission 相关门槛。
    let unknown_or_orphan = metric_value(&metrics, "kiana.recovery.unknown_total") > 0
        || metric_value(&metrics, "kiana.recovery.orphan_total") > 0;
    let (recovery_name, recovery) = component(
        "recovery",
        if unknown_or_orphan {
            ComponentHealthState::Degraded
        } else {
            ComponentHealthState::Healthy
        },
        Some(source_cursor),
        unknown_or_orphan.then_some("unknown_or_orphan_present"),
    )?;
    components.insert(recovery_name, recovery);

    // Core 没有 Provider、Broker、Telemetry exporter 或 DaemonHost 的独立现场探针，
    // 因而把它们标为 Unknown。DaemonHost 可以在上层把自身的“成功提供快照”观察补入，
    // 但这不会证明 Provider/Broker 可用，也不会证明 telemetry 已持久导出。
    for (name, reason) in [
        ("provider", "provider_health_not_observed"),
        ("broker", "broker_health_not_observed"),
        ("telemetry", "telemetry_exporter_health_not_observed"),
        ("daemon", "daemon_host_probe_not_observed"),
    ] {
        let (name, health) = component(name, ComponentHealthState::Unknown, None, Some(reason))?;
        components.insert(name, health);
    }

    let mut health = HealthSnapshot::new(
        "control_plane",
        status,
        source_cursor,
        bounded_source_ids(events),
        observed_at_ms,
    )
    .map_err(HealthProjectionError::SnapshotInvalid)?;
    health.probe = probe;
    health.capabilities.insert(
        "eventlog.atomic_transitions".to_owned(),
        capabilities.atomic_transitions,
    );
    health.capabilities.insert(
        "eventlog.durable_commits".to_owned(),
        capabilities.durable_commits,
    );
    health.capabilities.insert(
        "eventlog.command_receipts".to_owned(),
        capabilities.command_receipts,
    );
    health.capabilities.insert(
        "eventlog.cursor_reads".to_owned(),
        capabilities.cursor_reads,
    );
    // 当前 Core health projection 没有自己的持久 projector checkpoint，所以如实记录
    // “checkpoint durability 未获得证据”。
    // 注意：当前 status_for_probe() 没有读取这个 capability；readiness 实际由游标相等、
    // gap/inferred limitation、Unknown 和 orphan 等条件判定。不能仅凭这个 false 值声称
    // 本函数已经把 readiness 降级。
    health
        .capabilities
        .insert("projector.checkpoint_durable".to_owned(), false);
    health.components = components;
    health.limitations = limitations;
    // 摘要绑定最终快照内容。Domain validate 同时检查 schema、游标引用、字段上限和摘要，
    // 防止返回结构内部不一致的数据；摘要用于完整性比对，不是签名，也不证明来源可信。
    health.snapshot_digest = health.digest();
    health
        .validate()
        .map_err(HealthProjectionError::SnapshotInvalid)?;
    Ok(health)
}

/// 读取当前系统墙钟并转成 Unix 毫秒时间戳，作为快照的观测时刻。
///
/// 它仅用于标注“这份投影何时计算”，不是超时计时器或单调时钟。系统时间早于 Unix
/// epoch、转换结果为零时返回 `ObservedAtInvalid`；超出 `u64` 的毫秒值截到可表示上限。
/// 调用者 `ControlPlane::health_snapshot()` 会把该错误包装成端口失败，不重试或修改状态。
fn observed_at_ms() -> Result<u64, HealthProjectionError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HealthProjectionError::ObservedAtInvalid)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .and_then(|value| {
            (value > 0)
                .then_some(value)
                .ok_or(HealthProjectionError::ObservedAtInvalid)
        })
}

impl ControlPlane {
    /// 从 EventStore 的完整事件视图构建一份健康快照。
    ///
    /// 【调用者】生产 HTTP 路径由 `web::health()` 调用 `DaemonHost::liveness()`，再经
    /// `DaemonHost::health_snapshot()` 到达这里。同步投影
    /// `project_health_snapshot()` 还被 operator evidence、parity 和 replay diagnostics
    /// 直接复用。
    ///
    /// 【输入与输出】参数 `probe` 指定本次健康问题；事件由 `read_all_events()` 从
    /// EventStore 读取，能力由当前 EventStore port 声明，观测时间由本机墙钟产生。成功
    /// 返回 Domain `HealthSnapshot`，其中状态、source cursor 与限制原因都绑定到这次读取。
    ///
    /// 【失败与副作用】如果 adapter 明确表示不支持全量读取，返回
    /// `PortError::Unavailable("health_read_all_unsupported")`；实际读失败继续传播，
    /// 不会被伪装成空事件集。零时间或投影/Domain 校验失败会包装为 `PortError::Failed`。
    /// 该入口只读取 EventStore 和能力声明，不调用 Provider/Broker、不写事件、不创建
    /// admission permit，也不因降级而重试。读取完整视图是必要的，因为 readiness 需要
    /// 比较系统源游标、projector 游标和全局未知/孤儿副作用计数，单个 Run stream 无法
    /// 代表系统级健康。
    pub async fn health_snapshot(
        &self,
        probe: HealthProbeKind,
    ) -> Result<HealthSnapshot, CoreError> {
        // `None` 专门表示适配器明确不支持 read_all；真实 I/O 错误在 read_all_events 内
        // 已经作为 CoreError 返回，不能在这里降级为空列表继续生成错误的健康快照。
        let events = self.read_all_events().await?.ok_or_else(|| {
            kiana_ports::PortError::Unavailable("health_read_all_unsupported".to_owned())
        })?;
        let capabilities = self.events.capabilities();
        let observed_at =
            observed_at_ms().map_err(|error| kiana_ports::PortError::Failed(error.to_string()))?;
        project_health_snapshot(&events, &capabilities, probe, observed_at)
            .map_err(|error| kiana_ports::PortError::Failed(error.to_string()).into())
    }

    /// 使用 `Readiness` 门槛检查系统是否具备接收新受控命令所需的证据。
    ///
    /// 调用者通常是 DaemonHost/API 的 ready 探针。它仍只是只读健康投影，不会自己打开
    /// 或关闭 admission；上层是否据此接收流量由宿主策略决定。
    pub async fn readiness(&self) -> Result<HealthSnapshot, CoreError> {
        self.health_snapshot(HealthProbeKind::Readiness).await
    }

    /// 使用较窄的 `Liveness` 门槛回答服务是否还能形成基本健康视图。
    ///
    /// Liveness 不等于 readiness：当前实现只因明确的 cursor gap 降级，不会因为 EventStore
    /// 缺少持久提交等 admission 能力就自动拒绝这个探针。
    pub async fn liveness(&self) -> Result<HealthSnapshot, CoreError> {
        self.health_snapshot(HealthProbeKind::Liveness).await
    }

    /// 使用初始化阶段的 `Startup` 门槛报告最基本的 EventStore 能力与游标状态。
    ///
    /// 这是只读投影入口；它不会启动组件、修复存储或推进启动状态机。
    pub async fn startup_health(&self) -> Result<HealthSnapshot, CoreError> {
        self.health_snapshot(HealthProbeKind::Startup).await
    }
}
