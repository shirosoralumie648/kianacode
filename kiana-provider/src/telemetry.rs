//! Safe provider-side metadata for model-attempt instrumentation.
//!
//! The provider owns request compilation and therefore is the last boundary that can prove a
//! route was selected without accidentally serializing the wire request.  This helper returns a
//! small allow-listed summary.  Prompt text, tool arguments, authentication headers, endpoint
//! URLs and response bodies are intentionally not representable in the returned value.
//!
//! ## 这个文件在系统里的位置
//!
//! ```text
//!   kiana-runner::KianaHarness
//!        │  prepare_call() 冻结出 PreparedModelCall（不可变、已封存）
//!        ↓
//!   kiana-daemon/src/model_client.rs::InstrumentedModelClient::prepare_call()
//!        │  包装器：先调本文件校验“可观测字段”合法
//!        ↓
//!   【本文件 safe_prepared_metadata()】  ← 唯一的 provider 侧元数据出口
//!        │  返回一个白名单 JSON（只有 id / 路由 / 哈希，没有正文）
//!        ↓
//!   ControlPlane 落成 model-turn 事件 → EventLog（事实源）
//! ```
//!
//! ## 上游是谁
//!
//! 两个已确认的调用点（`rg -n "safe_prepared_metadata"`）：
//! - `kiana-daemon/src/model_client.rs`：`InstrumentedModelClient::prepare_call` 在把
//!   冻结结果交给上层之前调用一次；
//! - `kiana-provider/src/lib.rs`：`ProviderGateway::complete_admitted` 在真正发请求前再校验一次。
//!
//! ## 下游是谁
//!
//! 本文件**不调用任何东西**，只返回一个 `serde_json::Value`。收下这个值的调用方自行决定用途。
//!
//! ## 为什么需要单独一个文件
//!
//! 因为 provider 是**唯一一个既能看见完整 wire 请求、又不负责写事件**的地方：
//! 要记录“这次模型调用用了哪个模型、哪个账号、哪个路由”，provider 是最自然的取数点——
//! 但它同时也离密钥和 prompt 原文最近。把可观测字段收敛到这一个函数里，
//! 就等于给泄漏面划了一条明确边界：只有 `json!({...})` 里列出的字段可能离开 provider。

use kiana_domain::{ModelError, PreparedModelCall};
use serde_json::{json, Value};

/// 遥测载荷的形状标识，会被写进返回 JSON 的 `schema` 字段。
///
/// 【作用】 让下游（诊断面板、日志、事件投影）能判断“这份模型调用摘要属于哪一版形状”，
/// 而不必靠猜字段。
///
/// 【⚠ 它不是协议版本】 与 `kiana.protocol.v1` 无关，也不参与 daemon 的兼容性检查。
/// 改这个字符串等于换一种形状；老消费者遇到不认识的 `schema` 应当显式拒绝而不是尽力解析。

pub const MODEL_ATTEMPT_TELEMETRY_SCHEMA: &str = "kiana.model-attempt.v1";

/// Produce the provider-side safe request summary used by the existing model-turn event path.
///
/// Calling this function validates the prepared request before exposing any metadata.  The
/// returned JSON is suitable for diagnostics, but it is not an authorization token or a provider
/// receipt; the ControlPlane still derives the durable projection from committed events.
/// 生成 provider 侧的“安全请求摘要”，供既有的 model-turn 事件路径使用。
///
/// 【作用】
/// 把一次已冻结的模型调用压缩成**一小份可观测元数据**：用了哪个 provider、哪个模型、
/// 哪个账号、路由摘要是什么、预算多少、工具清单哈希是多少。
///
/// 【调用者】
/// `kiana-daemon/src/model_client.rs` 的 `InstrumentedModelClient::prepare_call`，
/// 以及本 crate 的 `ProviderGateway::complete_admitted`。
///
/// 【输入】
/// `prepared`：冻结并封存后的调用描述。函数**只读**，不修改它。
///
/// 【输出】
/// 一个 `serde_json::Value`，字段见下方 `json!({...})` 的逐项注释。
///
/// 【副作用】
/// 无。它不写事件、不发网络请求、不读环境变量、不消费预算。
///
/// 【失败情况】
/// `prepared.validate()` 失败时返回 `Err(ModelError)`。
/// 这意味着**一个不合法的冻结调用连元数据都产不出来**——不会出现“请求虽然坏但摘要看起来正常”。
///
/// 【⚠ 为什么这个函数不是“授权”或“证据”】
/// 它产出的东西只能用于诊断。真正的持久事实由 ControlPlane 从**已提交的事件**推导。
/// 源码里那句英文注释 “it is not an authorization token or a provider receipt”
/// 就是在强调这一点：不要拿这个 JSON 去证明“这次调用真的发生过”。
///
/// 【为什么字段里没有 prompt / 工具参数 / endpoint】
/// 因为 `wire_body` 里就有完整 prompt 和工具参数。一旦把这些字段透出去，
/// 任何一处日志/事件/错误上报的疏漏都会变成 prompt 泄漏。
/// 这里只放**标识与摘要**，不放**内容**。

pub fn safe_prepared_metadata(prepared: &PreparedModelCall) -> Result<Value, ModelError> {
    prepared.validate()?;
    let route = &prepared.route;
    // 下面每个字段的入选标准只有一条：**能回答“这次调用是什么”，但不能回答“这次调用说了什么”**。
    //
    //   schema              —— 形状标识，让下游能判断版本。
    //   model_call_id       —— 这次「调用」的 id（一次调用可能包含多次 attempt）。
    //   model_request_id    —— 这次 attempt 的 id。两者不同，不要混用。
    //   run_id              —— 归属哪次运行；没有 assignment 时为 null（表示未绑定运行）。
    //   purpose             —— 这次调用的业务目的（例如规划/执行）。
    //   provider_id         —— 上游服务商 id。
    //   model_id            —— 实际请求的模型 id（注意：这是“请求的”，不是“对方回的”）。
    //   streaming           —— 是否走流式；决定了对端是否会回 SSE/NDJSON。
    //   route_digest        —— 路由摘要。用来证明“用的是这条路由”，不泄漏路由内容。
    //   prompt_version      —— 这里放的是 request_hash（请求体摘要），不是 prompt 原文。
    //   tool_catalog_hash   —— 工具清单摘要；证明“冻结时给模型看的工具集没被中途换过”。
    //   budget              —— 预算快照（输入/系统/工具/schema 各占多少字节、上下文窗口多大）。
    //   provider_account    —— 账号的**摘要 id**，不是账号名，也不是 token。
    //   credential_revision —— 凭据版本号；用来发现“准入之后凭据被换过”。
    //
    // 【⚠ 刻意缺席的字段】 prompt 原文、工具参数、Authorization 头、endpoint 明文 URL、
    // 响应正文。它们都在 `prepared` 里可达，但放进来就等于给泄漏开了口子。

    Ok(json!({
        "schema": MODEL_ATTEMPT_TELEMETRY_SCHEMA,
        "model_call_id": prepared.spec.call_id,
        "model_request_id": prepared.spec.attempt_id,
        "run_id": prepared.spec.assignment.as_ref().map(|assignment| assignment.run_id),
        "purpose": prepared.spec.purpose,
        "provider_id": route.provider_id,
        "model_id": route.model_id,
        "streaming": route.streaming,
        "route_digest": route.digest(),
        "prompt_version": prepared.request_hash,
        "tool_catalog_hash": prepared.tool_catalog_hash,
        "budget": prepared.budget,
        "provider_account": prepared.provider_account,
        "credential_revision": prepared.credential_revision,
    }))
}
