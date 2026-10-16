//! Server-owned cancellation facts and stop evidence.
//!
//! A cancellation request is a durable intent separate from the in-process watch signal. The
//! fact records only redacted reason digests and target identities; a terminal `Cancelled` fact
//! requires confirmed stopping, while `ResultUnknown` keeps resources fenced.
//!
//! # 文件职责与主调用链
//!
//! 本文件定义 Run 取消事实（`RunCancellationFact`），属于 Domain 层的数据契约，不负责
//! 发送取消信号、停止 Runner/子进程、写 EventLog 或执行对账。Core 在收到取消命令后构造
//! 事实并把它作为 `run.cancelling` / `run.cancelled` / `run.result_unknown` 事件的一部分
//! 持久化；恢复和审计代码随后重新解析并校验同一事实。
//!
//! ```text
//! CLI / UI cancel request
//!   -> Core::record_cancel_requested() 生成 command_id、CAS 版本和事实
//!   -> Runner / Daemon 收到进程内取消信号并尝试停止
//!   -> Core 根据 stop proof 选择 Cancelled 或 ResultUnknown
//!   -> RunCancellationFact::new()/validate()
//!   -> EventStore 事实 -> lifecycle / projection / recovery / receipt
//! ```
//!
//! `RunCancellationState` 的状态含义必须保持分离：`Requested`/`Stopping` 只说明取消意图
//! 或停止过程正在进行；只有 `Cancelled` 搭配 `stop_confirmed=true` 才能说明已确认停止；
//! `ResultUnknown` 表示无法确认外部副作用状态，后续资源必须保持 fencing（隔离）直到对账。
//! 进程内 Tokio `watch` 信号只是及时通知执行任务的瞬时通道，不能替代这里的可恢复事实。
//!
//! 本文件中的 digest 是未加密钥的内容一致性摘要：`reason` 只保存摘要以避免把原始原因
//! 写进事实，但摘要本身不证明命令来源、审批有效或停止真的发生。授权、幂等 CAS、停止
//! 证明和最终事件顺序由 Core/Daemon/Runner 的调用方负责。

use crate::{json_digest, RequestId, RunCancellationState, RunId, SchemaVersion};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const RUN_CANCELLATION_SCHEMA: &str = "kiana.run-cancellation-fact.v1";
pub const RUN_CANCELLATION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCancellationFact {
    /// 事实 wire schema，必须等于 `RUN_CANCELLATION_SCHEMA`。
    pub schema: String,
    /// 当前事实契约版本；兼容性由 `validate()` 检查。
    pub version: SchemaVersion,
    /// 被取消的 Run 身份，不是取消命令身份。
    pub run_id: RunId,
    /// 发起本次取消命令的稳定 RequestId，用于幂等 receipt 与因果关联。
    pub command_id: RequestId,
    /// 取消状态；`Active` 不允许作为取消事实写出。
    pub state: RunCancellationState,
    /// 原因的 SHA-256 内容摘要；不保存原始原因文本。
    pub reason_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 发起取消的 actor；为空表示调用方没有提供主体标识，并不等同于匿名授权通过。
    pub actor_id: Option<String>,
    #[serde(default)]
    /// 需要停止的能力 invocation；构造时会排序去重，确保同一集合得到相同摘要/比较结果。
    pub target_invocation_ids: Vec<RequestId>,
    /// 写入取消事实前读取到的 Run stream version，用于调用方的 CAS 预期版本。
    pub expected_version: u64,
    /// 是否已经发出停止意图。当前契约要求为 true，防止把普通运行状态伪装成取消事实。
    pub stop_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// 独立的停止证据：`Some(true)` 才足以支持 `Cancelled`；`Some(false)` 或 `None` 都不
    /// 表示已经停止。非终态可能暂时为空，结果未知不能填 true。
    pub stop_confirmed: Option<bool>,
    /// 事实生成时间，Unix 毫秒；由调用方提供并必须非零。
    pub at_unix_ms: u64,
    /// 对除自身外全部字段的内容摘要，用于发现落盘或传输后的修改。
    pub fact_digest: String,
}

impl RunCancellationFact {
    /// 由 Core/事件边界创建一条新的取消事实。
    ///
    /// 【输入】Run/命令身份、目标状态、取消原因、actor、待停止 invocation、CAS 预期版本、
    /// stop requested/confirmed 标记和事件时间。原始原因只用于计算摘要，不会写入结构体。
    ///
    /// 【输出】成功时返回已排序去重、已计算 `fact_digest` 且通过 `validate()` 的事实；失败
    /// 时返回稳定字符串。它只构造内存值，不提交事件、不执行取消，也不检查 `expected_version`
    /// 是否仍是 EventStore 的最新版本。
    ///
    /// 【为什么排序去重】
    /// 同一批目标可能由不同入口以不同顺序传入；先规范化才能让幂等比较和 digest 不受
    /// 调用方排列影响。未来不要删掉这一步，否则同一取消意图可能被误判为冲突。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        command_id: RequestId,
        state: RunCancellationState,
        reason: &str,
        actor_id: Option<String>,
        mut target_invocation_ids: Vec<RequestId>,
        expected_version: u64,
        stop_requested: bool,
        stop_confirmed: Option<bool>,
        at_unix_ms: u64,
    ) -> Result<Self, String> {
        target_invocation_ids.sort();
        target_invocation_ids.dedup();
        let mut fact = Self {
            schema: RUN_CANCELLATION_SCHEMA.to_owned(),
            version: RUN_CANCELLATION_VERSION,
            run_id,
            command_id,
            state,
            reason_digest: json_digest(&json!({"reason": reason})),
            actor_id,
            target_invocation_ids,
            expected_version,
            stop_requested,
            stop_confirmed,
            at_unix_ms,
            fact_digest: String::new(),
        };
        fact.fact_digest = fact.digest();
        fact.validate()?;
        Ok(fact)
    }

    /// 从 EventStore/协议 JSON 还原事实，并在进入生命周期逻辑前完整校验。
    ///
    /// `deny_unknown_fields` 会拒绝未认识的字段，避免新旧版本静默吞掉状态；解析失败或
    /// 任何状态/摘要/身份不变量不满足时返回错误。该函数不改变外部状态。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let fact: Self = serde_json::from_value(value.clone())
            .map_err(|_| "run_cancellation_fact_decode_failed".to_owned())?;
        fact.validate()?;
        Ok(fact)
    }

    /// 将已验证事实编码为 JSON，供 RuntimeEvent 载荷或协议边界保存。
    ///
    /// 编码失败表示结构无法转换为当前 serde JSON 形状；不会替调用方写 EventStore，也不
    /// 自动重试。调用方应把错误视为事件无法可靠提交，而不是忽略后继续声称取消成功。
    pub fn to_json(&self) -> Result<Value, String> {
        serde_json::to_value(self).map_err(|_| "run_cancellation_fact_encode_failed".to_owned())
    }

    /// 校验取消事实的 schema、身份、集合规范化、摘要和状态/停止证据关系。
    ///
    /// 【关键不变量】
    /// - RunId/command_id 和目标 invocation 不能是 nil UUID；
    /// - 目标列表必须严格升序且无重复；
    /// - `stop_requested` 必须为 true；
    /// - Requested/Stopping 不能声称已确认停止；Cancelled 必须是 `Some(true)`；
    /// - ResultUnknown 不能同时带 `Some(true)`，因为它代表停止/副作用仍未确认；
    /// - Active 不是取消事实状态；
    /// - `fact_digest` 必须与当前所有字段重新计算的摘要相同。
    ///
    /// 【调用者与失败影响】
    /// Core 事件构造、EventStore 读取和恢复投影都会依赖这里的拒绝结果。返回错误表示事实
    /// 不能安全进入取消生命周期；本函数不负责把错误转换成 `run.failed`，也不会发停止信号。
    /// `expected_version` 与“当前 stream 是否仍匹配”由上层 CAS 检查，这里只保留该值。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != RUN_CANCELLATION_SCHEMA
            || !self.version.is_compatible_with(&RUN_CANCELLATION_VERSION)
            || self.run_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
            || self.at_unix_ms == 0
            || !self.stop_requested
            || self.actor_id.as_ref().is_some_and(|actor| {
                actor.trim().is_empty() || actor.len() > 256 || actor.contains('\0')
            })
        {
            return Err("run_cancellation_fact_header_invalid".to_owned());
        }
        if self
            .target_invocation_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
            || self
                .target_invocation_ids
                .iter()
                .any(|id| id.as_uuid().is_nil())
        {
            return Err("run_cancellation_fact_targets_invalid".to_owned());
        }
        if !valid_digest(&self.reason_digest) || !valid_digest(&self.fact_digest) {
            return Err("run_cancellation_fact_digest_invalid".to_owned());
        }
        match self.state {
            RunCancellationState::Requested | RunCancellationState::Stopping => {
                if self.stop_confirmed == Some(true) {
                    return Err("run_cancellation_fact_stop_conflict".to_owned());
                }
            }
            RunCancellationState::Cancelled => {
                if self.stop_confirmed != Some(true) {
                    return Err("run_cancellation_fact_stop_confirmation_required".to_owned());
                }
            }
            RunCancellationState::ResultUnknown => {
                if self.stop_confirmed == Some(true) {
                    return Err("run_cancellation_fact_unknown_stop_conflict".to_owned());
                }
            }
            RunCancellationState::Active => {
                return Err("run_cancellation_fact_state_invalid".to_owned())
            }
        }
        if self.fact_digest != self.digest() {
            return Err("run_cancellation_fact_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// 对事实内容计算未加密钥的 SHA-256 摘要，故意不把 `fact_digest` 自身放入输入。
    ///
    /// 这是完整性/一致性指纹，不是签名、身份认证或停止证明。任何调用方都能重新计算，
    /// 因此是否相信它必须结合 EventStore CAS、授权上下文和真实 stop proof。
    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "run_id": self.run_id,
            "command_id": self.command_id,
            "state": self.state,
            "reason_digest": self.reason_digest,
            "actor_id": self.actor_id,
            "target_invocation_ids": self.target_invocation_ids,
            "expected_version": self.expected_version,
            "stop_requested": self.stop_requested,
            "stop_confirmed": self.stop_confirmed,
            "at_unix_ms": self.at_unix_ms,
        }))
    }
}

/// 检查 `sha256:` 前缀加 64 个十六进制字符的摘要外形。
///
/// 该 helper 只检查字符串形状，不重新计算摘要、不区分大小写编码，也不证明摘要来自
/// 可信计算过程；真正的一致性由 `validate()` 的 `fact_digest == digest()` 检查完成。
fn valid_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return false;
    };
    hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}
