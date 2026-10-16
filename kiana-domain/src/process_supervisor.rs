//! 子进程资源预算与停止确认回执的领域契约。
//!
//! 本文件位于 Domain 层，只定义可在 Core/Daemon 之间传递和校验的数据类型；它不设置 OS
//! 资源限制、不启动/停止进程，也不采样 RSS 或 cgroup。实际执行在
//! `kiana-daemon/src/process_supervisor.rs`：Daemon 将 OS 观察结果写入 [`StopReport`]，
//! 再由调用者决定能否释放执行租约或把结果记为已停止。
//!
//! ```text
//! ProcessResourceBudget
//!     -> Daemon ProcessSupervisor 设置可用的 rlimit/监督 deadline
//!     -> StopReport 记录 TERM/KILL、leader 与进程组观察结果
//!     -> Core/Daemon 根据 confirmed 保留或释放资源占用
//! ```
//!
//! 重要边界：字段齐全且摘要一致只证明 DTO 自洽；不证明所有预算项由当前平台强制执行，
//! 也不证明 StopReport 是由操作系统签名生成。尤其“leader 已退出”不等于所有后代已退出；
//! 调用者在停止未确认时必须继续保留 Unknown/fencing 语义。

use crate::json_digest;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub const PROCESS_RESOURCE_BUDGET_SCHEMA: &str = "kiana.process-resource-budget.v1";
pub const STOP_REPORT_SCHEMA: &str = "kiana.stop-report.v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// supervisor 最后使用的停止信号类别；它描述记录到的尝试，不单独代表确认成功。
pub enum StopMethod {
    /// 没有记录到 TERM 或 KILL 信号成功发送。
    None,
    /// 记录到向目标发送 TERM（请求进程尽快结束）。
    Term,
    /// 记录到向目标发送 KILL（强制结束）。
    Kill,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// 对进程组存在性检查的三态结果；权限/系统错误不能误写成 Empty。
pub enum ProcessGroupState {
    /// OS 明确报告该进程组不存在。
    Empty,
    /// 检查成功且至少仍观察到一个组成员。
    Present,
    /// 没有可用 process group ID，或 OS 检查无法可靠判断。
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 计划给一次子进程执行使用的资源预算值。
///
/// 该对象可被序列化并写进回执，但“有预算字段”不等于执行端实现了该限制。Daemon 当前只把
/// 部分字段映射到平台 rlimit；wall time 由调用方自己的 deadline 控制，输出上限也由独立的
/// output capture 代码控制。读取回执时要同时看实际 enforcement/backend 说明。
pub struct ProcessResourceBudget {
    /// 固定版本化 schema 名，拒绝未知字段以避免静默忽略预算契约扩展。
    pub schema: String,
    /// 墙钟运行期限预算（毫秒）；Daemon 交由监督任务计时，不是 CPU 时间。
    pub max_wall_time_ms: u64,
    /// CPU 时间预算（毫秒）；Daemon 的 Unix RLIMIT_CPU 会向上取整到整秒。
    pub max_cpu_time_ms: u64,
    /// 单进程虚拟地址空间预算（字节），不是 RSS，也不是整个进程树的聚合内存。
    pub max_address_space_bytes: u64,
    /// 预算声明中的进程数；Unix 的 RLIMIT_NPROC 语义由操作系统/用户身份决定，不一定是本任务树计数。
    pub max_processes: u64,
    /// 单进程可打开文件描述符预算。
    pub max_open_files: u64,
    /// 单个文件大小预算，不表示整个 workspace/tmp 目录的总容量。
    pub max_file_bytes: u64,
    /// 预算 DTO 声明的输出字节上限；不保证由 ProcessSupervisor 本身执行。
    pub max_output_bytes: u64,
}

impl Default for ProcessResourceBudget {
    /// 返回 30 秒墙钟预算对应的默认资源值。
    fn default() -> Self {
        Self::for_wall_time_ms(30_000)
    }
}

impl ProcessResourceBudget {
    /// 创建统一的默认预算，只允许调用者选择 wall-time，其他资源值固定。
    ///
    /// wall time 被夹在 1..=3600000ms（最多 1 小时），避免默认构造出零时限或无限长预算。
    /// 其余默认声明为：CPU 300000ms（5 分钟）、虚拟地址空间 2 GiB、NPROC 数值 1024、
    /// 打开文件 256 个、单文件 64 MiB、输出 1 MiB。最后一项仍须由具体执行 adapter 强制，
    /// 不能因它出现在 budget 中就断言 supervisor 已实施。
    pub fn for_wall_time_ms(max_wall_time_ms: u64) -> Self {
        Self {
            schema: PROCESS_RESOURCE_BUDGET_SCHEMA.to_owned(),
            max_wall_time_ms: max_wall_time_ms.clamp(1, 3_600_000),
            max_cpu_time_ms: 300_000,
            max_address_space_bytes: 2 * 1024 * 1024 * 1024,
            max_processes: 1_024,
            max_open_files: 256,
            max_file_bytes: 64 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
        }
    }

    /// 检查 schema 和所有预算均非零，并限制 wall time 不超过一小时。
    ///
    /// 这是数值/格式校验，不检测当前系统能否施加这些限制，也不要求每个字段都被实际执行。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != PROCESS_RESOURCE_BUDGET_SCHEMA
            || self.max_wall_time_ms == 0
            || self.max_cpu_time_ms == 0
            || self.max_address_space_bytes == 0
            || self.max_processes == 0
            || self.max_open_files == 0
            || self.max_file_bytes == 0
            || self.max_output_bytes == 0
            || self.max_wall_time_ms > 3_600_000
        {
            return Err("process_resource_budget_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// 一次终止尝试的字段级结果回执。
///
/// leader 是直接由 Tokio `Child` 持有的进程；process group 表示可连带观察的进程组。`confirmed`
/// 在 Daemon 生成路径中要求 leader 已回收、存在 process group ID 且最终观察为空；它不表示
/// 每个后代都曾被逐个 wait/reap。回执摘要是普通内容 digest，不是内核证明。
pub struct StopReport {
    /// 固定 schema 名。
    pub schema: String,
    /// 对应的执行 ID，用于将停止回执关联到启动记录。
    pub execution_id: String,
    /// supervisor 生成回执时从 Tokio Child 读取到的 leader PID；进程已 reap 时可为空。
    pub leader_pid: Option<u32>,
    /// 被检查/发信号的进程组 ID；确认停止需要该标识存在。
    pub process_group_id: Option<u32>,
    /// 最后成功发出的信号类别；不是 OS 完成确认。
    pub method: StopMethod,
    /// 向目标发送 TERM 的系统调用是否成功。
    pub term_sent: bool,
    /// 向目标发送 KILL 的系统调用是否成功。
    pub kill_sent: bool,
    /// Tokio Child leader 是否已被 wait/try_wait 确认回收。
    pub leader_reaped: bool,
    /// 最后一次进程组观察结果；权限或其他错误保持 Unknown。
    pub process_group: ProcessGroupState,
    /// 停止是否达到 supervisor 的确认条件；非 Unix/无 PGID 不会被强称为已确认。
    pub confirmed: bool,
    /// stop 调用耗时的墙钟毫秒，受校验的 DTO 上限约束。
    pub waited_ms: u64,
    /// 对回执其他字段计算的内容摘要，不是签名。
    pub report_digest: String,
}

impl StopReport {
    /// 根据 supervisor 收集到的信号/leader/group 观察值构造并验证回执。
    ///
    /// 本方法只检查自洽关系，不访问 OS；领域校验比 Daemon 生成路径更宽，不能仅靠它断言
    /// report 真由 supervisor 产生。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        execution_id: impl Into<String>,
        leader_pid: Option<u32>,
        process_group_id: Option<u32>,
        method: StopMethod,
        term_sent: bool,
        kill_sent: bool,
        leader_reaped: bool,
        process_group: ProcessGroupState,
        confirmed: bool,
        waited_ms: u64,
    ) -> Result<Self, String> {
        let mut report = Self {
            schema: STOP_REPORT_SCHEMA.to_owned(),
            execution_id: execution_id.into(),
            leader_pid,
            process_group_id,
            method,
            term_sent,
            kill_sent,
            leader_reaped,
            process_group,
            confirmed,
            waited_ms,
            report_digest: String::new(),
        };
        report.report_digest = report.digest();
        report.validate()?;
        Ok(report)
    }

    /// 构造“无法确认停止”的保守回执，PID 与进程组未知，confirmed=false。
    ///
    /// 如果正常构造路径本身因校验失败而返回 Err，则退回固定 invalid 哨兵对象；该哨兵不满足
    /// 正常 digest 格式，不能当作有效成功回执。
    pub fn unknown(execution_id: impl Into<String>) -> Self {
        Self::new(
            execution_id,
            None,
            None,
            StopMethod::None,
            false,
            false,
            false,
            ProcessGroupState::Unknown,
            false,
            0,
        )
        .unwrap_or_else(|_| Self {
            schema: STOP_REPORT_SCHEMA.to_owned(),
            execution_id: "invalid".to_owned(),
            leader_pid: None,
            process_group_id: None,
            method: StopMethod::None,
            term_sent: false,
            kill_sent: false,
            leader_reaped: false,
            process_group: ProcessGroupState::Unknown,
            confirmed: false,
            waited_ms: 0,
            report_digest: "sha256:invalid".to_owned(),
        })
    }

    /// 校验回执字段范围、部分状态关系以及内容 digest。
    ///
    /// ⚠ 当前 DTO 校验只在 `confirmed=true` 时要求 leader_reaped 且 group=Empty，没有要求
    /// process_group_id 必须为 Some；Daemon supervisor 的 confirmed 条件更严格。信号字段关系也
    /// 不完整：它拒绝“method=None 但某信号成功”及“method=Kill 但 kill_sent=false”，却允许
    /// `method=Term` 时没有 `term_sent`，也允许 kill_sent=true 但 method 仍为 Term。Daemon 生成器
    /// 会用更窄的规则选 method；直接构造的回执可比实际生成路径更宽松。`waited_ms` 最大
    /// 120000ms（2 分钟）是 DTO 接受范围，不是 supervisor 实际等待上限。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != STOP_REPORT_SCHEMA
            || self.execution_id.trim().is_empty()
            || self.execution_id.len() > 256
            || self.leader_pid.is_some_and(|pid| pid == 0)
            || self.process_group_id.is_some_and(|pid| pid == 0)
            || self.waited_ms > 120_000
            || (self.confirmed
                && (!self.leader_reaped || self.process_group != ProcessGroupState::Empty))
            || (self.method == StopMethod::None && (self.term_sent || self.kill_sent))
            || (!self.kill_sent && self.method == StopMethod::Kill)
            || self.report_digest != self.digest()
        {
            return Err("stop_report_invalid".to_owned());
        }
        Ok(())
    }

    /// 计算除 `report_digest` 本身以外所有回执字段的确定性摘要。
    ///
    /// 摘要防止无意字段不一致，但无密钥，不能证明 OS 真的收到过信号或进程组确实为空。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "execution_id": self.execution_id,
            "leader_pid": self.leader_pid,
            "process_group_id": self.process_group_id,
            "method": self.method,
            "term_sent": self.term_sent,
            "kill_sent": self.kill_sent,
            "leader_reaped": self.leader_reaped,
            "process_group": self.process_group,
            "confirmed": self.confirmed,
            "waited_ms": self.waited_ms,
        }))
    }
}
