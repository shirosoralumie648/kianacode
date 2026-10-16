//! 受租约/fence 约束的进程监督请求与观察结果合同，属于 `kiana-domain`。
//!
//! 【预期系统位置与数据流】
//! ```text
//! Core/部署协调器在授权并取得 operation lease 后
//!     -> SupervisorRequest（绑定 operation、owner、root、lease digest、fence、generation）
//!     -> SupervisorPort
//!     -> 平台 supervisor adapter（systemd/launchd/Windows service/container）
//!     -> SupervisorObservation（启动/停止/重启/Unknown/TimedOut）
//!     -> Core 用 validate_against() 核对观察是否对应原请求
//! ```
//!
//! 这是值合同，不是监督器实现。请求携带 lease digest 和 fence token，但不包含可在本模块重新查询的
//! lease/CAS；这里验证的是字段格式、自摘要和请求/观察绑定，不认证调用者、不重验真实租约，也不检查当前权限。
//! 目标只能是带 backend 前缀的服务引用，不能由调用者直接提供 PID；但这里不查服务目录，前缀本身不能证明引用由服务端配置产生。Observation 里的信号和 `StopReport`
//! 仍是 adapter 提交的事实声明；仅有自摘要或 `confirmed = true` 不等于内核/平台签名证明。
//!
//! 当前 Core facade 没有发现生产调用者，Daemon adapter 也未发现生产实例化；其方法目前明确返回
//! `Unavailable`。因此上图描述的是接口预期链路，不是 Kiana 已接通的启动/停止路径。

use crate::{
    json_digest, FenceTokenId, InstanceId, OperationId, SchemaVersion, StopMethod, StopReport,
};
use serde::{Deserialize, Serialize};

/// SupervisorRequest wire schema 标识。
pub const SUPERVISOR_REQUEST_SCHEMA: &str = "kiana.supervisor-request.v1";
/// SupervisorObservation wire schema 标识。
pub const SUPERVISOR_OBSERVATION_SCHEMA: &str = "kiana.supervisor-observation.v1";
/// 请求和观察结果使用的 schema 版本。
pub const SUPERVISOR_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 服务引用最多 256 UTF-8 字节；限制外部输入规模和平台引用格式长度。
pub const MAX_SUPERVISOR_TARGET_BYTES: usize = 256;
/// 观察证据引用最多 512 UTF-8 字节；它只是有限长的引用文本，不是证据内容。
pub const MAX_SUPERVISOR_EVIDENCE_BYTES: usize = 512;
/// 请求声明的最长相对截止时长：120,000 毫秒，即 2 分钟。
pub const MAX_SUPERVISOR_DEADLINE_MS: u64 = 120_000;

/// 请求将动作路由到的后端类别；此枚举不代表相应平台 adapter 已实现或可用。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorBackend {
    /// Linux systemd 服务管理器；该枚举只命名 backend，不证明 host 上存在 systemd adapter。
    Systemd,
    /// Apple launchd 服务管理器；该枚举只命名 backend，不证明已连接 launchd。
    Launchd,
    /// Windows Service Control Manager；该枚举只命名 backend，不证明已连接 Windows 服务控制器。
    WindowsService,
    /// 容器运行时服务引用；该枚举不指定 Docker/Podman 等具体产品，也不证明 runtime 可用。
    Container,
    /// 测试/隔离环境可使用的命名类别；本仓库当前 adapter 仍返回 Unavailable，并无 fake runtime。
    Fake,
}

impl SupervisorBackend {
    /// 返回 `target_ref` 必须使用的服务引用前缀。
    ///
    /// 前缀帮助拒绝把一种平台引用误送到另一类 adapter；它不是平台资源存在性检查。
    pub const fn target_prefix(self) -> &'static str {
        match self {
            Self::Systemd => "systemd:",
            Self::Launchd => "launchd:",
            Self::WindowsService => "windows:",
            Self::Container => "container:",
            Self::Fake => "fake:",
        }
    }

    /// 返回用于错误码和 adapter 选择诊断的稳定短名称。
    pub const fn adapter_name(self) -> &'static str {
        match self {
            Self::Systemd => "systemd",
            Self::Launchd => "launchd",
            Self::WindowsService => "windows-service",
            Self::Container => "container",
            Self::Fake => "fake",
        }
    }
}

/// 本次对服务引用要求执行的生命周期动作。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorAction {
    /// 启动服务；成功观察应报告比请求 generation 更新的代次。
    Start,
    /// 停止服务；成功观察必须绑定已确认的停止报告。
    Stop,
    /// 先停止再启动服务；成功观察必须有确认停止证据并报告新代次。
    Restart,
}

/// `StopReport` 记录的信号类别；信号发送成功不代表进程已退出。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorSignal {
    /// 没有记录 TERM/KILL 信号；启动或未发信号的结果使用此值。
    None,
    /// TERM：请求目标有序退出，平台仍须观察停止结果。
    Term,
    /// KILL：请求平台强制终止，仍须独立确认目标已停止。
    Kill,
}

/// Supervisor 对动作结果的有限分类；Unknown/TimedOut 保留不确定性，不能折算成成功。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorOutcome {
    /// adapter 声称服务进入新 generation；Domain 只检查它大于请求中的 expected_generation。
    Started,
    /// adapter 声称服务已停止；要通过请求绑定校验，还需要 confirmed StopReport 和匹配的 generation。
    Stopped,
    /// adapter 声称停止旧服务并观察到新 generation；这里只核对 DTO 合同，不验证真实平台状态。
    Restarted,
    /// adapter 声称等待超过其处理期限；具体 elapsed 不在本对象独立验证。
    TimedOut,
    /// adapter 无法可靠判断最终状态。
    Unknown,
}

/// 交给 SupervisorPort 的有版本请求（versioned request）：声明监督谁、做什么，以及用于拒绝旧请求/旧写者的 fencing 上下文。
///
/// 所有字段来自上游构造；本对象不持有 OS process handle。Serde 用于 DTO 序列化/反序列化，未知字段被拒绝；
/// `request_digest` 是无密钥一致性摘要，不是身份签名、approval 或 lease 服务回执。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorRequest {
    /// 固定请求 schema 名称。
    pub schema: String,
    /// 请求 wire 版本。
    pub version: SchemaVersion,
    /// 对应的部署 operation ID。
    pub operation_id: OperationId,
    /// 发起/持有该 operation 的运行实例 ID。
    pub instance_id: InstanceId,
    /// 目标 deployment 配置/修订内容的摘要，用于防止观察被换绑到另一版部署。
    pub deployment_revision_digest: String,
    /// 被操作 storage root 的摘要。
    pub storage_root_digest: String,
    /// 上游已取得租约的摘要引用；这里只核对格式，不读取或重新校验 OperationLease。
    pub lease_digest: String,
    /// 本轮租约的 fence token；真实平台执行边界仍须由授权 adapter 核对它是否最新。
    pub fence_token: FenceTokenId,
    /// 发出请求时绑定的 authority epoch（授权代次）；这里只验证非零并纳入摘要，不回查当前授权快照。
    pub authority_epoch: u64,
    /// 发出请求时绑定的数据 generation；本对象只验证非零并纳入摘要。
    pub data_epoch: u64,
    /// 预期处理此请求的 supervisor backend。
    pub backend: SupervisorBackend,
    /// 要求 adapter 执行的动作。
    pub action: SupervisorAction,
    /// 按服务端约定构造的服务引用，必须以 backend 前缀开头；校验器不查询服务目录，也不证明引用来源可信。
    pub target_ref: String,
    /// 请求方提供的服务代次；用于比较 start/restart 结果，但允许为 0，也没有绑定权威 supervisor 状态源。
    pub expected_generation: u64,
    /// adapter 可用的相对处理时长，单位毫秒（1..=120000），不是 Unix 时间戳；DTO 校验不计时或强制取消。
    pub deadline_ms: u64,
    /// 对本请求其余字段计算的 SHA-256 内容摘要。
    pub request_digest: String,
}

impl SupervisorRequest {
    /// 绑定 operation、instance、部署/root/lease 摘要、fence、epoch、backend/action、服务引用、代次和期限，
    /// 计算 request digest 并验证后返回请求。
    ///
    /// 【调用者】预期由已完成权限判断和 lease 获取的 Core/部署协调器构造；当前源码没有生产调用点，测试夹具
    /// 直接创建值对象。
    ///
    /// 【输入/输出】除自动填入 schema/version/digest 外，其余参数由调用方提供；成功返回自洽请求，空 ID、摘要
    /// 格式错误、PID 风格目标、backend 前缀不匹配、文本越界或 deadline 非法时返回稳定字符串。
    ///
    /// 【副作用/限制】只创建内存 DTO，不查询服务、authority、lease 或时钟，不授权也不启动/停止进程。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        operation_id: OperationId,
        instance_id: InstanceId,
        deployment_revision_digest: impl Into<String>,
        storage_root_digest: impl Into<String>,
        lease_digest: impl Into<String>,
        fence_token: FenceTokenId,
        authority_epoch: u64,
        data_epoch: u64,
        backend: SupervisorBackend,
        action: SupervisorAction,
        target_ref: impl Into<String>,
        expected_generation: u64,
        deadline_ms: u64,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: SUPERVISOR_REQUEST_SCHEMA.to_owned(),
            version: SUPERVISOR_VERSION,
            operation_id,
            instance_id,
            deployment_revision_digest: deployment_revision_digest.into(),
            storage_root_digest: storage_root_digest.into(),
            lease_digest: lease_digest.into(),
            fence_token,
            authority_epoch,
            data_epoch,
            backend,
            action,
            target_ref: target_ref.into(),
            expected_generation,
            deadline_ms,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    /// 检查请求 header/身份、epoch、期限、服务引用的受限文本形式和所有引用摘要。
    ///
    /// 【调用者】`new()` 返回前调用；Daemon `NarrowSupervisorAdapter` 在 dispatch 前也会检查，Core 的验证端
    /// 会在匹配 observation 前检查。
    ///
    /// 【失败】多个基础字段任一不满足时返回 `supervisor_request_header_invalid`；目标 backend 后缀为空、
    /// 引用摘要格式错或 request digest 不匹配时返回各自稳定错误。通过只表示 DTO 自洽，不代表 actor 有权限、
    /// lease 未过期或目标服务存在。
    pub fn validate(&self) -> Result<(), String> {
        // 先拒绝无效 header、超出上限、控制字符、含 pid: 的 caller target 或 backend 前缀错误，避免后续按不可信名称分派。
        if self.schema != SUPERVISOR_REQUEST_SCHEMA
            || self.version != SUPERVISOR_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.instance_id.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
            || self.authority_epoch == 0
            || self.data_epoch == 0
            || self.deadline_ms == 0
            || self.deadline_ms > MAX_SUPERVISOR_DEADLINE_MS
            || self.target_ref.len() > MAX_SUPERVISOR_TARGET_BYTES
            || self.target_ref.trim().is_empty()
            || self.target_ref.contains(['\0', '\r', '\n'])
            || self.target_ref.contains("pid:")
            || !self.target_ref.starts_with(self.backend.target_prefix())
        {
            return Err("supervisor_request_header_invalid".to_owned());
        }
        // 前面的 starts_with 用于快速给统一 header error；这里再提取 suffix，以稳定区分只有前缀但没有服务名的请求。
        let target_suffix = self
            .target_ref
            .strip_prefix(self.backend.target_prefix())
            .ok_or_else(|| "supervisor_request_target_backend_mismatch".to_owned())?;
        if target_suffix.trim().is_empty() {
            return Err("supervisor_request_target_empty".to_owned());
        }
        // 这几个摘要字段把不同事实绑定在请求上；仅验证文本外形，稍后 request_digest 比较才检查本对象字段一致性。
        for (digest, field) in [
            (
                &self.deployment_revision_digest,
                "supervisor_request_revision_digest",
            ),
            (
                &self.storage_root_digest,
                "supervisor_request_storage_root_digest",
            ),
            (&self.lease_digest, "supervisor_request_lease_digest"),
            (&self.request_digest, "supervisor_request_digest"),
        ] {
            valid_digest(digest, field)?;
        }
        if self.request_digest != self.digest() {
            return Err("supervisor_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 返回除 `request_digest` 自身外的请求字段摘要。
    ///
    /// `deadline_ms`、expected generation、lease digest 和 fence 都纳入摘要，便于检测字段被替换；
    /// 但摘要无密钥，不会证明上游确实持有对应 lease，也不保证 deadline 被执行。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "operation_id": self.operation_id,
            "instance_id": self.instance_id,
            "deployment_revision_digest": self.deployment_revision_digest,
            "storage_root_digest": self.storage_root_digest,
            "lease_digest": self.lease_digest,
            "fence_token": self.fence_token,
            "authority_epoch": self.authority_epoch,
            "data_epoch": self.data_epoch,
            "backend": self.backend,
            "action": self.action,
            "target_ref": self.target_ref,
            "expected_generation": self.expected_generation,
            "deadline_ms": self.deadline_ms,
        }))
    }
}

/// 平台 adapter 对一个 SupervisorRequest 提交的结果声明。
///
/// `evidence_ref` 指向外部观察材料的命名空间字符串，但本类型不读取引用目标、验证其存在或证明其真实性。
/// 报告中的 `StopReport.confirmed` 也是 DTO 字段；它的自校验比 Daemon 生成路径更宽松，因此只能作为合同输入，
/// 不能单独当作 OS 已确认目标进程退出的证明。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupervisorObservation {
    /// 固定 observation schema 名称。
    pub schema: String,
    /// observation wire 版本。
    pub version: SchemaVersion,
    /// 原始 request 的内容摘要；`validate_against()` 用它拒绝跨请求重放。
    pub request_digest: String,
    /// 原始部署 operation ID。
    pub operation_id: OperationId,
    /// 原始运行 instance ID。
    pub instance_id: InstanceId,
    /// 原始部署修订摘要。
    pub deployment_revision_digest: String,
    /// 原始 storage root 摘要。
    pub storage_root_digest: String,
    /// 原始 lease 摘要引用；仍未证明真实 lease 在观察时有效。
    pub lease_digest: String,
    /// 原始 fencing token。
    pub fence_token: FenceTokenId,
    /// 实际声称处理请求的 backend。
    pub backend: SupervisorBackend,
    /// 实际声称处理的动作；必须与原 request 匹配。
    pub action: SupervisorAction,
    /// adapter 报告的结果类别。
    pub outcome: SupervisorOutcome,
    /// stop report.method 的信号投影；没有 stop report 时只能为 None。
    pub signal: SupervisorSignal,
    /// 可选停止回执；`None` 不是成功停止证据，`Some` 也仍是供给值而非签名回执。
    pub stop_report: Option<StopReport>,
    /// adapter 观察到或声称的服务代次；按动作/结果与 expected_generation 比较。
    pub observed_generation: u64,
    /// 形式上要求 `observation:` 前缀的证据引用；不包含证据内容本身。
    pub evidence_ref: String,
    /// 对观察结果其他字段计算的 SHA-256 内容摘要。
    pub observation_digest: String,
}

impl SupervisorObservation {
    /// 从请求复制不可变绑定字段，附加 adapter 给出的结果声明，计算摘要并按原请求校验。
    ///
    /// 【调用者】未来的平台 adapter 可在收集外部观察后构造；当前只有 Domain 测试夹具创建，Daemon adapter
    /// 目前返回 Unavailable，不会调用本构造器。
    ///
    /// 【输入/输出】输入有效 request、outcome、signal、可选 StopReport、观察 generation 和 evidence 引用；成功
    /// 返回绑定原 request 的 observation。无效 request、证据字符串、StopReport、动作/结果组合或 generation 关系
    /// 返回稳定字符串错误。
    ///
    /// 【副作用/限制】只复制字段和计算 digest；不发送信号、不读取服务状态、证据存储或系统时钟，也不证明 adapter 声明真实。
    pub fn new(
        request: &SupervisorRequest,
        outcome: SupervisorOutcome,
        signal: SupervisorSignal,
        stop_report: Option<StopReport>,
        observed_generation: u64,
        evidence_ref: impl Into<String>,
    ) -> Result<Self, String> {
        request.validate()?;
        let mut observation = Self {
            schema: SUPERVISOR_OBSERVATION_SCHEMA.to_owned(),
            version: SUPERVISOR_VERSION,
            request_digest: request.request_digest.clone(),
            operation_id: request.operation_id,
            instance_id: request.instance_id,
            deployment_revision_digest: request.deployment_revision_digest.clone(),
            storage_root_digest: request.storage_root_digest.clone(),
            lease_digest: request.lease_digest.clone(),
            fence_token: request.fence_token,
            backend: request.backend,
            action: request.action,
            outcome,
            signal,
            stop_report,
            observed_generation,
            evidence_ref: evidence_ref.into(),
            observation_digest: String::new(),
        };
        observation.observation_digest = observation.digest();
        observation.validate_against(request)?;
        Ok(observation)
    }

    /// 只检查 observation 自身的 schema、字段摘要、证据引用文本和 signal/StopReport 方法一致性。
    ///
    /// 【调用者】`new()` 在返回前调用；已解码 DTO 的下游也可单独检查格式。但单独调用此方法不会比较原 request，
    /// 不检查 action/outcome/generation 的对应关系；若要判断它是否回答了某个请求，必须调用 `validate_against()`。
    ///
    /// `evidence_ref` 只限制长度/控制字符并要求 `observation:` 前缀，空后缀也可通过；digest 无密钥，不能证明
    /// 证据存在、持久或来自操作系统。失败返回稳定字符串，不修复对象或产生副作用。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SUPERVISOR_OBSERVATION_SCHEMA
            || self.version != SUPERVISOR_VERSION
            || self.operation_id.as_uuid().is_nil()
            || self.instance_id.as_uuid().is_nil()
            || self.fence_token.as_uuid().is_nil()
            || self.evidence_ref.len() > MAX_SUPERVISOR_EVIDENCE_BYTES
            || !self.evidence_ref.starts_with("observation:")
            || self.evidence_ref.contains(['\0', '\r', '\n'])
        {
            return Err("supervisor_observation_header_invalid".to_owned());
        }
        for (digest, field) in [
            (
                &self.deployment_revision_digest,
                "supervisor_observation_revision_digest",
            ),
            (
                &self.storage_root_digest,
                "supervisor_observation_storage_root_digest",
            ),
            (
                &self.request_digest,
                "supervisor_observation_request_digest",
            ),
            (&self.lease_digest, "supervisor_observation_lease_digest"),
            (&self.observation_digest, "supervisor_observation_digest"),
        ] {
            valid_digest(digest, field)?;
        }
        // StopReport 的字段/自摘要先按 Domain 合同校验，再将 method 映射为 observation.signal；这只证明两份 DTO 一致。
        // StopReport.validate() 对 confirmed 的约束比 daemon 生成条件宽松，不能据此宣称真实 target 已停止。
        if let Some(report) = &self.stop_report {
            report.validate()?;
            let expected_signal = match report.method {
                StopMethod::None => SupervisorSignal::None,
                StopMethod::Term => SupervisorSignal::Term,
                StopMethod::Kill => SupervisorSignal::Kill,
            };
            if self.signal != expected_signal {
                return Err("supervisor_observation_signal_mismatch".to_owned());
            }
        } else if self.signal != SupervisorSignal::None {
            return Err("supervisor_observation_signal_without_report".to_owned());
        }
        if self.observation_digest != self.digest() {
            return Err("supervisor_observation_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 将 observation 绑定回原 request，并检查 action/outcome 与 generation 的状态合同。
    ///
    /// 【调用者】Core 的 `validate_supervisor_observation()` 只委托此方法；未来消费真实 adapter 结果的协调器也应
    /// 以创建该 observation 的同一请求调用它。当前没有生产调用者。
    ///
    /// 【核心流程】先分别校验 request 和 observation 的自洽性；再精确比较 request digest、operation/instance、
    /// deployment/root/lease digest、fence、backend 和 action；最后按动作检查 stop report 确认位和 generation。
    /// Start/Restart 成功必须 generation 增长，Stop 成功保持原 generation 且有 confirmed report；Unknown/TimedOut
    /// 必须仍是原 generation 且不能 confirmed。其他结果组合拒绝。这里不比较 deadline 与 StopReport.waited_ms，
    /// 也不绑定 StopReport.execution_id 到 request 的 operation/instance/target。
    ///
    /// 【失败/边界】任一字段不匹配或状态组合非法时返回稳定错误字符串；不重试、不写 EventStore、不变更运行状态。
    /// 它没有当前时钟或 operation lease 对象，无法验证 deadline 是否被执行、lease/fence 是否仍最新。StopReport 的
    /// `execution_id` 也没有与 request operation/instance/target 建立校验绑定；确认位和无密钥 digest 不能认证 OS 事实。
    pub fn validate_against(&self, request: &SupervisorRequest) -> Result<(), String> {
        request.validate()?;
        self.validate()?;
        if self.request_digest != request.request_digest
            || self.operation_id != request.operation_id
            || self.instance_id != request.instance_id
            || self.deployment_revision_digest != request.deployment_revision_digest
            || self.storage_root_digest != request.storage_root_digest
            || self.lease_digest != request.lease_digest
            || self.fence_token != request.fence_token
            || self.backend != request.backend
            || self.action != request.action
        {
            return Err("supervisor_observation_request_binding_mismatch".to_owned());
        }
        // 这是一个来自可变 DTO 的布尔声明。下面的分支把它作为必要合同条件，但它不是独立的 OS 证明。
        let confirmed = self
            .stop_report
            .as_ref()
            .is_some_and(|report| report.confirmed);
        match (request.action, self.outcome) {
            (SupervisorAction::Start, SupervisorOutcome::Started) => {
                // 成功启动意味着代次必须前进，且不能附带停止信号/停止回执。
                if self.stop_report.is_some()
                    || self.signal != SupervisorSignal::None
                    || self.observed_generation <= request.expected_generation
                {
                    return Err("supervisor_start_observation_invalid".to_owned());
                }
            }
            (SupervisorAction::Stop, SupervisorOutcome::Stopped) => {
                // 单纯发出信号不等于停止；合同要求有 confirmed 回执且 generation 不增长。
                // 该布尔值来自 StopReport DTO，不是本函数现场观察到的内核事实。
                if !confirmed
                    || self.observed_generation != request.expected_generation
                    || self.stop_report.is_none()
                {
                    return Err("supervisor_stop_observation_required".to_owned());
                }
            }
            (SupervisorAction::Restart, SupervisorOutcome::Restarted) => {
                // Restarted 同时表示旧实例已确认停止和新代次已观察到；缺少其中任一部分都不能宣称完成。
                if !confirmed
                    || self.observed_generation <= request.expected_generation
                    || self.stop_report.is_none()
                {
                    return Err("supervisor_restart_fence_invalid".to_owned());
                }
            }
            (SupervisorAction::Start, SupervisorOutcome::Unknown | SupervisorOutcome::TimedOut)
            | (SupervisorAction::Stop, SupervisorOutcome::Unknown | SupervisorOutcome::TimedOut)
            | (
                SupervisorAction::Restart,
                SupervisorOutcome::Unknown | SupervisorOutcome::TimedOut,
            ) => {
                // 超时/未知时保留原 generation，避免把没有证据的状态变化写成成功。
                if confirmed || self.observed_generation != request.expected_generation {
                    return Err("supervisor_unknown_observation_invalid".to_owned());
                }
            }
            _ => return Err("supervisor_outcome_action_mismatch".to_owned()),
        }
        if matches!(
            self.outcome,
            SupervisorOutcome::Unknown | SupervisorOutcome::TimedOut
        ) && confirmed
        {
            return Err("supervisor_unknown_confirmed_conflict".to_owned());
        }
        Ok(())
    }

    /// 计算除 `observation_digest` 自身外所有观察字段的内容摘要。
    ///
    /// 该摘要可以发现字段被意外改动，但没有密钥签名；request_digest 与证据引用被绑定，外部证据内容/真实性仍未校验。
    pub fn digest(&self) -> String {
        json_digest(&serde_json::json!({
            "schema": self.schema,
            "version": self.version,
            "request_digest": self.request_digest,
            "operation_id": self.operation_id,
            "instance_id": self.instance_id,
            "deployment_revision_digest": self.deployment_revision_digest,
            "storage_root_digest": self.storage_root_digest,
            "lease_digest": self.lease_digest,
            "fence_token": self.fence_token,
            "backend": self.backend,
            "action": self.action,
            "outcome": self.outcome,
            "signal": self.signal,
            "stop_report": self.stop_report,
            "observed_generation": self.observed_generation,
            "evidence_ref": self.evidence_ref,
        }))
    }
}

/// 校验一个 digest 是否为 `sha256:` 加 64 个十六进制字符。
///
/// 这里只检查外形，不重新计算被引用的内容；大小写十六进制都接受，错误字符串以 field 前缀区分字段。
fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
