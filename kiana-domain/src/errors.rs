//! # 能力错误分类与客户端展示策略
//!
//! 本文件属于最底层的 `kiana-domain`，把各模块产生的错误原因归并为一组稳定的
//! `CapabilityErrorCode`。它不执行能力、不写 EventLog，也不是 `DomainError` 等
//! 所有领域错误的总表；它只提供跨能力结果、生命周期投影和客户端展示共用的分类值。
//!
//! 常见数据流如下：
//!
//! ```text
//! Broker / Harness / Domain 能力结果中的 reason 字符串
//!     -> CapabilityResult::failure_code() 或 CapabilityErrorCode::from_reason()
//!     -> Core、Protocol、指标投影或 CLI presenter 按稳定类别处理
//!     -> 用户看到错误状态/消息，CLI 取得退出码
//! ```
//!
//! `from_reason()` 只把原因分类，不会因为策略字段而重试、补偿、重新授权或自动对账。
//! 尤其副作用结果不确定时，调用者必须沿 ControlPlane 的受控恢复路径处理；再次执行
//! 仍要重新通过授权链。错误字符串继续保留给诊断使用，类别字符串和策略字段则是需要
//! 向后兼容的契约，已有名称不能随意改名或复用。
//!
//! `CapabilityErrorCode` 与 `DomainError` 是不同层次：前者主要描述能力执行结果如何
//! 分类，后者描述领域状态或不变量错误。遇到未配置映射的原因时，本文件会保守归到
//! `ExecutionFailed`，不会猜测未知文本的含义；该策略 `never permits automatic retry`
//!（绝不允许 automatic retry），重新尝试必须由上层创建新请求并重新经过授权链。

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CapabilityErrorPolicy {
    /// CLI presenter 可采用的稳定进程退出码；本字段只提供数值，不会结束进程。
    pub cli_exit: u8,
    /// 对应类别建议使用的 HTTP 状态码。当前字段是策略契约数据；是否被某个 HTTP
    /// adapter 实际读取，仍须看该 adapter 的调用代码，不能仅凭字段存在就认定已接通。
    pub http_status: u16,
    /// 该类别是否允许自动重试同一操作。当前错误表全部为 `false`；重新发起尝试仍须
    /// 由上层重新经过 ControlPlane 授权，结果未知时还要先确认副作用是否已经发生。
    pub retryable: bool,
    /// 处理该类错误前是否要求取得新的授权决定；这是供上层读取的要求标记，不会在此处
    /// 创建审批或授权。
    pub requires_new_authorization: bool,
    /// 是否需要补偿已经发生的业务效果；设置此位不会自动运行补偿 handler。
    pub requires_compensation: bool,
    /// 是否需要查明外部副作用的最终状态；此标记本身不执行查询，也不等同于
    /// `ExecutionStatus::ResultUnknown`。
    pub requires_reconciliation: bool,
}

/// 用一张表同时生成错误类型、稳定名称、全量列表和策略，避免这些映射分散后彼此漂移。
///
/// 新增错误时应检查所有真实消费者：字符串解析、CLI 展示、Core 生命周期/结果投影，
/// 以及需要此类别的协议转换。策略位只是数据，具体动作由上层显式实现；不能把
/// `retryable`、`requires_compensation` 或 `requires_reconciliation` 写成自动执行保证。
macro_rules! error_codes {
    ($( $variant:ident => ($name:literal, $exit:literal, $http:literal, $retry:literal, $auth:literal, $compensation:literal, $reconcile:literal) ),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum CapabilityErrorCode { $( $variant, )+ }

        impl CapabilityErrorCode {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $name,)+ }
            }

            pub const fn policy(self) -> CapabilityErrorPolicy {
                match self {
                    $(Self::$variant => CapabilityErrorPolicy {
                        cli_exit: $exit,
                        http_status: $http,
                        retryable: $retry,
                        requires_new_authorization: $auth,
                        requires_compensation: $compensation,
                        requires_reconciliation: $reconcile,
                    },)+
                }
            }
        }
    };
}

// 表格每行的列依次为：Rust 变体名、稳定 reason 名、CLI 退出码、HTTP 状态码、
// 是否允许自动重试、是否要求新授权、是否要求补偿、是否要求对账。
//
// 这些类别按当前调用约定表达：
// - InvalidArguments / SchemaUnsupported：输入内容或协议 schema 不可接受。
// - PathEscape / PermissionDenied / ProjectUntrusted：路径越界、权限拒绝或项目不可信。
// - ApprovalRequired / ApprovalExpired / ApprovalConsumed / ApprovalDenied：审批缺失、过期、
//   已消费或被拒绝；它们不能靠重放旧审批来消除。
// - Conflict / NotFound / Unsupported：当前状态冲突、目标不存在或能力/传输不支持。
// - BudgetExceeded / Cancelled / TimedOut / Unavailable：预算、取消、时限或可用性边界。
// - ResultUnknown：副作用结果无法确认，重放可能造成重复效果；应先走对账/恢复路径。
// - CompensationRequired：已知存在需要补偿的业务效果，需由上层恢复流程处理。
// - ExecutionFailed：其余未能识别或未细分的执行失败，作为保守回退类别。
//
// CLI 数值是进程退出码而不是错误编号；HTTP 数值是响应状态建议，常见的 4xx 表示
// 请求/权限/状态问题，5xx 表示执行或服务可用性问题。当前表里每行 `retryable` 都是
// false；130 仅作为 Cancelled 的 CLI 退出码传递。这里集中记录契约值，不负责真正重试、
// 发 HTTP 响应或停止进程。
error_codes! {
    InvalidArguments => ("invalid_arguments", 2, 400, false, true, false, false),
    SchemaUnsupported => ("schema_unsupported", 2, 400, false, true, false, false),
    PathEscape => ("path_escape", 3, 403, false, true, false, false),
    PermissionDenied => ("permission_denied", 3, 403, false, true, false, false),
    ProjectUntrusted => ("project_untrusted", 3, 403, false, true, false, false),
    ApprovalRequired => ("approval_required", 4, 409, false, true, false, false),
    ApprovalExpired => ("approval_expired", 4, 409, false, true, false, false),
    ApprovalConsumed => ("approval_consumed", 4, 409, false, true, false, false),
    ApprovalDenied => ("approval_denied", 3, 403, false, true, false, false),
    Conflict => ("conflict", 5, 409, false, true, false, false),
    NotFound => ("not_found", 5, 404, false, false, false, false),
    Unsupported => ("unsupported", 2, 501, false, false, false, false),
    BudgetExceeded => ("budget_exceeded", 6, 429, false, true, false, false),
    Cancelled => ("cancelled", 130, 409, false, true, false, false),
    TimedOut => ("timed_out", 7, 504, false, true, false, true),
    Unavailable => ("unavailable", 7, 503, false, true, false, false),
    ResultUnknown => ("result_unknown", 8, 409, false, true, false, true),
    CompensationRequired => ("compensation_required", 8, 409, false, true, true, true),
    ExecutionFailed => ("execution_failed", 1, 500, false, true, false, false),
}

impl CapabilityErrorCode {
    /// 将现有错误原因字符串映射为稳定的能力错误类别。
    ///
    /// 【作用】
    /// 让 Domain、Core、Protocol、Daemon 和 CLI 对同一类错误使用一致名称，而不要求
    /// 每个生产边界都重新解析任意诊断文案。
    ///
    /// 【调用者】
    /// 主要调用者包括能力结果归一化、Core 生命周期/事件投影、指标与平台投影，以及
    /// CLI presenter。Runner 的失败文本也会先被 Core 分类，再决定写入哪种终态事件。
    ///
    /// 【输入】
    /// `reason` 是旧接口或适配器产生的错误原因文本。它可能是稳定 reason code，可能带
    /// `port_failed:`、`port_conflict:`、`port_unavailable:` 传输包装，也可能带冒号后的
    /// 诊断详情。
    ///
    /// 【输出】
    /// 始终返回一个 `CapabilityErrorCode`，不会返回 `Result`。命中明确类别时返回该类别；
    /// 无法识别时使用 `ExecutionFailed`，除非已知的 port 包装提供了更具体的冲突/不可用
    /// fallback。
    ///
    /// 【副作用】
    /// 无 I/O、日志、状态写入或策略执行。返回的类别会影响调用者后续的生命周期状态、
    /// UI/CLI 呈现和能力结果元数据。
    ///
    /// 【失败与恢复】
    /// 分类本身不会失败，但未知文本会被归为保守类别；这不会触发自动重试。若结果表示
    /// 外部副作用可能已经发生，调用者必须依据对账标记与 EventLog/恢复流程确认结果后，
    /// 才能决定下一步。
    ///
    /// 【核心流程】
    /// 1. 去掉输入首尾空白；
    /// 2. 只递归剥除三个明确支持的 `port_*:` 包装，并记住冲突/不可用的回退类别；
    /// 3. 取当前原因第一个冒号前的类别文本，先查稳定名称表；
    /// 4. 再查历史别名映射；
    /// 5. 未命中时返回保守 fallback。
    ///
    /// 【为什么不在整段文本中搜索关键词？】
    /// 诊断详情可能偶然包含某个错误词。任意子串搜索会把普通说明误判为权限拒绝、超时
    /// 或未知副作用，进而改变生命周期与恢复决策；因此本函数只看受控前缀的精确类别。
    pub fn from_reason(reason: &str) -> Self {
        let mut reason = reason.trim();
        let mut fallback = Self::ExecutionFailed;
        loop {
            let Some((prefix, detail)) = reason.split_once(':') else {
                break;
            };
            if matches!(prefix, "port_failed" | "port_conflict" | "port_unavailable") {
                fallback = match prefix {
                    "port_conflict" => Self::Conflict,
                    "port_unavailable" => Self::Unavailable,
                    _ => fallback,
                };
                reason = detail.trim();
            } else {
                break;
            }
        }
        let code = reason.split(':').next().unwrap_or_default();
        if let Some(known) = Self::ALL.iter().find(|known| known.as_str() == code) {
            return *known;
        }
        match code {
            "shell_result_unknown"
            | "capability_result_mismatch"
            | "run_terminal_conflict"
            | "result_event_persistence_failed" => Self::ResultUnknown,
            "harness_workdir_outside_project"
            | "apply_patch_path_outside_project"
            | "apply_patch_path_escape"
            | "path_outside_project" => Self::PathEscape,
            "workspace_write_requires_trusted_non_safe_profile" => Self::ProjectUntrusted,
            "hook_blocked"
            | "packet_path_denied"
            | "role_path_denied"
            | "memory_scope_denied"
            | "memory_access_denied"
            | "role_tool_denied"
            | "sandbox_read_only"
            | "authority_epoch_stale"
            | "capability_blocked"
            | "cell_capability_grant_denied"
            | "cell_capability_scope_mismatch"
            | "cell_capability_scope_incomplete" => Self::PermissionDenied,
            "hook_ask_unattended" => Self::ApprovalRequired,
            "cell_capability_grant_expired" => Self::PermissionDenied,
            "approval_already_consumed" | "approval_replayed" => Self::ApprovalConsumed,
            "approval_continuation_unavailable" | "run_resume_unavailable" => Self::Unavailable,
            "run_budget_exceeded"
            | "max_steps_per_turn"
            | "repeated_tool_call"
            | "context_budget_exceeded"
            | "cell_capability_concurrency_exceeded" => Self::BudgetExceeded,
            "session_not_found" | "run_not_found" | "approval_not_found" => Self::NotFound,
            "mcp_transport_unsupported" | "sandbox_unsupported" => Self::Unsupported,
            "model_unavailable"
            | "model_gateway_unavailable"
            | "sandbox_unavailable"
            | "capability_unregistered" => Self::Unavailable,
            "protocol_schema_unsupported" | "unknown_schema" | "schema_version_incompatible" => {
                Self::SchemaUnsupported
            }
            "prompt_required" | "packet_invalid" | "role_unknown" => Self::InvalidArguments,
            "cell_capability_invocation_duplicate" | "run_already_exists" => Self::Conflict,
            "timeout" | "shell_timeout" => Self::TimedOut,
            _ => fallback,
        }
    }
}

impl std::fmt::Display for CapabilityErrorCode {
    /// 将稳定错误码写入格式化器，供日志、错误包装和客户端边界输出。
    ///
    /// 此处只输出机器可识别的 `as_str()` 名称，不展开用户提示、不附加内部诊断详情，
    /// 也不改变错误分类或触发恢复动作。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}
