//! # 文件职责
//!
//! 本文件定义“请求使用某个操作系统后端时，当前构建是否真的允许选择它”的领域契约。
//! 它属于 `kiana-domain`：负责校验输入、按固定顺序推导选择结果并生成可复核摘要；
//! 不负责发现真实主机、不创建沙箱、不启动进程，也不执行任何能力。
//!
//! # 在系统中的位置与数据流
//!
//! 上游组合根/适配器应提供它观察到的主机平台和服务端持有的
//! `PlatformBackendReport`（平台后端报告），然后沿以下纯计算链路读取结果：
//!
//! ```text
//! 上层提供 host_target + backend report
//!   -> BackendSelectionRequest::new()：封装请求并计算 request_digest
//!   -> BackendSelectionDecision::evaluate()：校验请求信封并按固定顺序推导
//!   -> validate_against()：重新推导并核对决定内容与摘要
//!   -> BackendSelection::{Selected, TargetOnly, NotSupported, Blocked}
//! ```
//!
//! `Selected` 只说明“这个后端可以被选择”，不代表代码已经获得执行权。其余结果全部
//! 表示拒绝。拒绝时不会偷偷改用本机另一个后端：例如，Linux 上不能把请求的 macOS
//! Seatbelt 沙箱替换成 Linux 的 bwrap，再把替代执行说成原请求成功。
//!
//! 只有通过验证、并且声明 `behavior_verified = true` 的报告，才可能得到
//! `Implemented`（已实现）的选择结果。本模块只验证报告，不会创造目标机实现或证据。
//! 当前 workspace 中没有生产模块直接调用这些选择函数；测试会直接使用它们验证拒绝
//! 规则。新增调用方应继续把它当成纯领域判定，不要把它当作主机探测或进程启动入口。

use crate::{
    json_digest, PlatformBackendDisposition, PlatformBackendReport, PlatformTarget, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const BACKEND_SELECTION_SCHEMA: &str = "kiana.backend-selection.v1";
pub const BACKEND_SELECTION_DECISION_SCHEMA: &str = "kiana.backend-selection-decision.v1";
pub const BACKEND_SELECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
/// 后端报告允许携带的限制说明上限，与 `PlatformBackendReport::validate()` 的约束一致。
pub const MAX_BACKEND_SELECTION_LIMITATIONS: usize = 32;

/// 当前 Linux 产品路径实际使用的沙箱后端名称。
///
/// 它必须与 daemon 的 `harness_sandbox::SANDBOX_BACKEND` 保持一致；仓库中的边界测试会
/// 检查两处名称，避免选择契约与真正执行沙箱的代码悄悄分叉。这里是名称常量，不会
/// 自动把任何被拒绝的请求切换到 bwrap。
pub const HOST_EXECUTION_BACKEND: &str = "bwrap";

/// 描述一次后端选择请求，以及服务端已有的该后端能力报告。
///
/// 这是输入值，不是执行命令。`new()` 会填入 schema/version 并计算摘要；
/// `BackendSelectionDecision::evaluate()` 再据此判断当前选择是否可信、可用。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSelectionRequest {
    pub schema: String,
    pub version: SchemaVersion,
    /// 调用方声称的目标平台；它会和组合根提供的真实主机平台比较，不能直接信任。
    pub requested_target: PlatformTarget,
    /// 调用方请求的后端名称，例如 `seatbelt` 或 `bwrap`。
    pub requested_backend: String,
    /// 组合根观察到的当前构建目标平台；本模块不会自行探测该值。
    pub host_target: PlatformTarget,
    /// 服务端对请求后端持有的能力报告。`None` 表示没有已知报告，即没有可证明的支持；
    /// 不能据此挑选其他后端代替。
    pub report: Option<PlatformBackendReport>,
    /// 对上面请求字段（包括可选报告）计算出的摘要，用于发现后续字段被篡改或过期。
    pub request_digest: String,
}

impl BackendSelectionRequest {
    /// 构造并封存一份后端选择请求。
    ///
    /// 【调用者】
    /// 由需要判断后端是否可选的上层代码调用；当前仓库内的便捷入口是下方
    /// `backend_is_selectable()`，测试也会直接构造请求。
    ///
    /// 【输入与输出】
    /// 输入为调用方期望的平台、后端名称、组合根观察到的主机平台，以及可选的服务端
    /// 后端报告。成功时返回填好 schema/version 和 `request_digest` 的请求值。
    ///
    /// 【失败与副作用】
    /// 后端名称无效、报告与后端名称不匹配、报告本身无效或摘要不一致时返回字符串错误。
    /// 这是纯数据构造，不检查操作系统、不读配置、不调用端口或启动沙箱。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        requested_target: PlatformTarget,
        requested_backend: impl Into<String>,
        host_target: PlatformTarget,
        report: Option<PlatformBackendReport>,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: BACKEND_SELECTION_SCHEMA.to_owned(),
            version: BACKEND_SELECTION_VERSION,
            requested_target,
            requested_backend: requested_backend.into(),
            host_target,
            report,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    /// 完整验证请求及其中的可选后端报告。
    ///
    /// 该方法适合需要证明“整个嵌套请求结构都有效”的调用方。选择决定生成路径会先
    /// 单独验证外层信封，再由 `derive()` 把无效报告转换为明确的 `Blocked` 结果；两者
    /// 区别是：此方法对无效报告直接返回错误，后者可以生成可展示的拒绝决定。
    pub fn validate(&self) -> Result<(), String> {
        self.validate_header()?;
        if let Some(report) = &self.report {
            report.validate()?;
        }
        self.validate_binding_and_digest()
    }

    /// 只验证请求外层信封、报告绑定和请求摘要，不在此处拒绝嵌套报告的内容错误。
    ///
    /// `BackendSelectionDecision::evaluate()` 使用此入口，是为了让外层请求身份可信后，
    /// 再由 `derive()` 把无效报告解释为 `Blocked` 决定。`derive()` 仍会完整校验报告；
    /// 因此这个快捷验证不能单独用于证明后端可执行。
    fn validate_envelope(&self) -> Result<(), String> {
        self.validate_header()?;
        self.validate_binding_and_digest()
    }

    /// 验证外层请求头是否使用受支持的 schema/version，且后端名称有界、非空。
    ///
    /// 后端名最多 128 字节并禁止换行和 NUL，避免把不受控字符串带入报告、日志或后续
    /// 执行边界。失败统一返回 `backend_selection_request_invalid`；此处不做 I/O。
    fn validate_header(&self) -> Result<(), String> {
        if self.schema != BACKEND_SELECTION_SCHEMA
            || !self.version.is_compatible_with(&BACKEND_SELECTION_VERSION)
            || self.requested_backend.trim().is_empty()
            || self.requested_backend.len() > 128
            || self.requested_backend.contains(['\0', '\n', '\r'])
        {
            return Err("backend_selection_request_invalid".to_owned());
        }
        Ok(())
    }

    /// 验证嵌套报告确实属于请求后端，并且请求摘要仍与全部输入字段一致。
    ///
    /// 摘要把 target、host、backend 和可选 report 一起绑定；若任一字段在构造后被修改，
    /// 就必须重新计算摘要，否则这里拒绝该请求。该方法本身不判断报告是否可执行，完整
    /// 的报告校验由 `validate()` 或 `derive()` 负责。
    fn validate_binding_and_digest(&self) -> Result<(), String> {
        if let Some(report) = &self.report {
            if report.backend != self.requested_backend {
                return Err("backend_selection_report_backend_mismatch".to_owned());
            }
        }
        if self.request_digest != self.digest() {
            return Err("backend_selection_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 计算请求内容摘要，不包含 `request_digest` 自身。
    ///
    /// 摘要用于绑定所有影响选择结果的输入；它不是签名，也不能证明报告来自可信机器。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "requested_target": self.requested_target,
            "requested_backend": self.requested_backend,
            "host_target": self.host_target,
            "report": self.report,
        }))
    }
}

/// 单次后端选择判定的四种结果。
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendSelection {
    /// 请求的后端在当前主机上有有效报告且行为已验证，可以被选择。
    Selected,
    /// 报告只适用于另一个目标平台；当前主机拒绝使用，不会换成其他后端。
    TargetOnly,
    /// 服务端没有该后端的能力报告，因而无法证明支持，拒绝选择。
    NotSupported,
    /// 后端虽有报告，但报告无效、请求平台不匹配或强制约束不满足，因此拒绝选择。
    Blocked,
}

impl BackendSelection {
    /// 返回供序列化/API 使用的稳定小写字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::TargetOnly => "target_only",
            Self::NotSupported => "not_supported",
            Self::Blocked => "blocked",
        }
    }

    /// 判断此决定是否允许后续流程使用请求的后端。
    ///
    /// 只有 `Selected` 返回 `true`，其余三个状态都表示拒绝。这一判断集中在此处，避免
    /// 不同调用方各自解释状态时把 `TargetOnly` 或 `Blocked` 当成可执行环境。
    pub const fn is_usable(self) -> bool {
        matches!(self, Self::Selected)
    }
}

/// 对一次后端选择请求作出的、可校验且带摘要的决定。
///
/// 该值回答“能否选择”，不授予 capability、不启动执行器。它把请求中的平台/后端信息、
/// 推导出的拒绝或接受原因以及两个摘要一起保留下来，方便上层展示并在消费前重新校验。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSelectionDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub requested_target: PlatformTarget,
    pub requested_backend: String,
    pub host_target: PlatformTarget,
    pub selection: BackendSelection,
    /// 始终为 `true`：拒绝请求时禁止回退到另一后端，此字段让该约束可被校验。
    pub no_host_fallback: bool,
    /// 始终为 `false`：本模块只判定是否可选，不授予 capability 或执行权。
    pub grants_execution: bool,
    /// 机器可识别的拒绝/接受原因码，例如 `backend_selection_target_mismatch`。
    pub reason: String,
    /// 提供给调用方的后续处理提示；它是说明文字，不会由本模块自动执行。
    pub remediation: String,
    /// 实际允许使用的后端名：仅 `Selected` 时等于请求名；拒绝时必须为空，不能暗示有替代品。
    pub effective_backend: String,
    /// 与本决定绑定的输入请求摘要。
    pub request_digest: String,
    /// 对本决定所有字段计算出的摘要，不包含自身。
    pub decision_digest: String,
}

impl BackendSelectionDecision {
    /// 从一份已封存请求重新推导并生成后端选择决定。
    ///
    /// 【作用与调用者】
    /// 这是后端选择规则的主要入口。上层提交 `BackendSelectionRequest` 后调用它；当前
    /// workspace 的 macOS/Windows 合约测试也直接使用它，目前没有生产模块直接调用点。
    ///
    /// 【输入与输出】
    /// 输入请求必须有有效的外层 schema、backend 名称、report 绑定和 request digest。
    /// 成功返回含选择状态、原因、处理提示、effective backend 和 decision digest 的决定。
    ///
    /// 【失败情况与副作用】
    /// 外层请求无效或摘要被篡改时返回错误。报告内容无效则不会作为支持证据，而由
    /// `derive()` 产出 `Blocked` 拒绝决定。整个流程是纯计算，不探测平台、不调用端口、
    /// 不授予权限，也不启动进程。
    ///
    /// 【为什么生成后还要 `validate_against()`？】
    /// 返回前再次从原请求推导并检查所有绑定字段，确保将来即使构造逻辑变化，也不会把
    /// 与输入不一致或摘要错误的决定交给调用方。
    pub fn evaluate(request: &BackendSelectionRequest) -> Result<Self, String> {
        request.validate_envelope()?;
        let (selection, reason, remediation) = derive(request);
        let mut decision = Self {
            schema: BACKEND_SELECTION_DECISION_SCHEMA.to_owned(),
            version: BACKEND_SELECTION_VERSION,
            requested_target: request.requested_target,
            requested_backend: request.requested_backend.clone(),
            host_target: request.host_target,
            selection,
            no_host_fallback: true,
            grants_execution: false,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            effective_backend: if selection == BackendSelection::Selected {
                request.requested_backend.clone()
            } else {
                String::new()
            },
            request_digest: request.request_digest.clone(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate_against(request)?;
        Ok(decision)
    }

    /// 重新计算请求的预期结果，并验证当前决定没有被伪造、篡改或替换为回退后端。
    ///
    /// 【调用者】
    /// `evaluate()` 在返回决定前调用；接收序列化决定的外部调用方也应在信任它之前调用。
    ///
    /// 【校验内容】
    /// 先确认请求信封有效，再运行同一个 `derive()` 并比较 schema/version、输入绑定、
    /// 选择状态、reason/remediation、禁止回退/授权的标志、有效后端名称和决定摘要。
    /// 任一字段与重新推导结果不同都会返回对应错误码；成功只证明“该决定符合本地契约”，
    /// 不证明目标平台上的沙箱真实存在。
    pub fn validate_against(&self, request: &BackendSelectionRequest) -> Result<(), String> {
        request.validate_envelope()?;
        let (selection, reason, remediation) = derive(request);
        if self.schema != BACKEND_SELECTION_DECISION_SCHEMA
            || !self.version.is_compatible_with(&BACKEND_SELECTION_VERSION)
            || self.requested_target != request.requested_target
            || self.requested_backend != request.requested_backend
            || self.host_target != request.host_target
            || self.selection != selection
            || self.reason != reason
            || self.remediation != remediation
            || self.request_digest != request.request_digest
        {
            return Err("backend_selection_decision_binding_invalid".to_owned());
        }
        if !self.no_host_fallback {
            return Err("backend_selection_host_fallback_forbidden".to_owned());
        }
        if self.grants_execution {
            return Err("backend_selection_grants_execution".to_owned());
        }
        // 拒绝状态必须不带 effective backend。这样“禁止主机回退”由结构校验保证，
        // 而不是只靠调用方阅读 reason/remediation 后自觉遵守。
        if !selection.is_usable() && !self.effective_backend.is_empty() {
            return Err("backend_selection_substitute_backend_named".to_owned());
        }
        if selection.is_usable() && self.effective_backend != request.requested_backend {
            return Err("backend_selection_effective_backend_mismatch".to_owned());
        }
        if self.decision_digest != self.digest() {
            return Err("backend_selection_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 返回决定是否允许调用方继续使用请求的后端。
    pub fn usable(&self) -> bool {
        self.selection.is_usable()
    }

    /// 计算决定摘要，不包含 `decision_digest` 自身。
    ///
    /// 摘要覆盖请求绑定、选择状态、拒绝说明和回退/授权标志，使这些字段不能被单独修改。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "requested_target": self.requested_target,
            "requested_backend": self.requested_backend,
            "host_target": self.host_target,
            "selection": self.selection,
            "no_host_fallback": self.no_host_fallback,
            "grants_execution": self.grants_execution,
            "reason": self.reason,
            "remediation": self.remediation,
            "effective_backend": self.effective_backend,
            "request_digest": self.request_digest,
        }))
    }
}

/// 按固定优先级从请求和报告推导后端选择结果。
///
/// 顺序是：先确认请求声称的平台等于当前主机，再确认服务端有报告，然后验证报告，
/// 最后根据报告的 disposition 判断能否使用。平台不匹配优先拒绝，因为如果在 Linux 主机
/// 上接受一个声称运行于 macOS 的请求，调用方会误以为 Seatbelt 隔离生效，实际却没有。
/// 这里返回的是状态、原因码和补救提示，不执行任何选择或运行操作。
fn derive(request: &BackendSelectionRequest) -> (BackendSelection, &'static str, &'static str) {
    if request.requested_target != request.host_target {
        return (
            BackendSelection::Blocked,
            "backend_selection_target_mismatch",
            "run on the target platform or select the host execution backend",
        );
    }
    let Some(report) = &request.report else {
        return (
            BackendSelection::NotSupported,
            "backend_selection_unknown_backend",
            "register a backend report before selecting it",
        );
    };
    if report.validate().is_err() {
        return (
            BackendSelection::Blocked,
            "backend_selection_report_invalid",
            "repair the backend report; an invalid report is never treated as support",
        );
    }
    if report.target != request.host_target {
        return (
            BackendSelection::TargetOnly,
            "backend_selection_target_only_backend",
            "run on the target platform; this backend has no implementation here",
        );
    }
    match report.disposition {
        PlatformBackendDisposition::Implemented => {
            if !report.behavior_verified {
                return (
                    BackendSelection::Blocked,
                    "backend_selection_behavior_unverified",
                    "produce target-machine evidence before selecting this backend",
                );
            }
            (BackendSelection::Selected, "backend_selection_accepted", "")
        }
        PlatformBackendDisposition::TargetOnly => (
            BackendSelection::TargetOnly,
            "backend_selection_target_only_backend",
            "run on the target platform; this backend has no implementation here",
        ),
        PlatformBackendDisposition::NotSupported => (
            BackendSelection::NotSupported,
            "backend_selection_not_supported",
            "select a backend implemented for this host",
        ),
        PlatformBackendDisposition::Blocked => (
            BackendSelection::Blocked,
            "backend_selection_blocked",
            "resolve the reported enforcement gap before selecting this backend",
        ),
    }
}

/// 为“仅目标平台可用”的后端生成标准限制说明。
///
/// 调用方可把返回的键值对放入 `PlatformBackendReport::limitations`，让拒绝原因随报告一起
/// 传递。`behavior_verified=false` 表示当前没有目标机行为证据；`evidence` 说明缺失哪种
/// 目标平台回执。本函数只生成字符串数据，不验证或执行后端。当前仓库没有生产代码直接
/// 调用它。
pub fn target_only_limitations(target: PlatformTarget, backend: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("behavior_verified".to_owned(), "false".to_owned()),
        (
            "evidence".to_owned(),
            format!("no {backend} receipt exists for {target:?}; see the CAP-3x baseline"),
        ),
    ])
}

/// 以布尔值回答当前主机能否选择指定后端。
///
/// 【作用与调用者】
/// 这是比完整决定更简单的便捷 API，适合只需要放行/拒绝结果的适配器；当前仓库中由
/// 测试直接调用，没有生产模块直接使用。
///
/// 【输入与输出】
/// `host_target` 同时作为请求目标和当前主机目标，`backend` 是后端名，`report` 是服务端
/// 持有的可选报告。仅完整校验后得到 `Selected` 时返回 `true`。
///
/// 【失败与副作用】
/// 构造请求失败、报告缺失/无效、平台不匹配或选择被拒绝时都返回 `false`；这个简化接口
/// 会收敛错误细节，因此需要原因码和 remediation 的调用方应直接使用 `evaluate()`。
/// 它不会探测当前操作系统、构造后端报告、回退到其他后端或启动任何执行器。
pub fn backend_is_selectable(
    host_target: PlatformTarget,
    backend: &str,
    report: Option<&PlatformBackendReport>,
) -> bool {
    let Ok(request) =
        BackendSelectionRequest::new(host_target, backend, host_target, report.cloned())
    else {
        return false;
    };
    BackendSelectionDecision::evaluate(&request)
        .map(|decision| decision.usable())
        .unwrap_or(false)
}
