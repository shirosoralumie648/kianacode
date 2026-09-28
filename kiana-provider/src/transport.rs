//! 真正把请求发出去的那一层：熔断器 + 限流闸门 + 凭据租约 + 超时 + 流式切帧。
//!
//! ## 这个文件在系统里的位置（provider 的最下游）
//!
//! ```text
//!   kiana-runner::KianaHarness
//!        |  prepare_call() -> PreparedModelCall        （冻结，不发请求）
//!        v
//!   ControlPlane 签发 ModelCallPermit + 消费预算
//!        |
//!        v
//!   kiana-provider/src/lib.rs::complete_admitted()
//!        |  再次校验：路由没变 / 凭据版本没变 / 许可仍然有效
//!        v
//!   【本文件 send()】
//!        |
//!        |  1. 总时限兜底（deadline 与 total 取小）
//!        |  2. 熔断器放行判定（Closed / Open / HalfOpenProbe）
//!        |  3. 排队信号量 + 并发信号量 + RPM/TPM 窗口
//!        |  4. 签发并消费一次性凭据租约
//!        |  5. 按协议拼认证头，发 HTTP 请求
//!        |  6. 流式：按帧切 + 空闲超时 + 字节上限；非流式：整体读 + 解析 JSON
//!        v
//!   response.rs::decode() / Accumulator  ->  ModelReply
//! ```
//!
//! ## 为什么要单独一个文件
//!
//! 因为“准备一次调用”和“真的发出去”是两个完全不同的责任：
//! 前者只做本地计算（`request.rs`），后者碰网络、碰密钥、碰时间。
//! 拆开之后，`request.rs` 可以被离线测试，`transport.rs` 里所有与时间/并发相关的逻辑
//! 也都集中在一处，便于逐条推理。
//!
//! ## 本文件回答的四个问题
//!
//! 1. **还要不要发？** —— 熔断器 + 三个闸门 + 总时限
//! 2. **多久算失败？** —— 四个分阶段超时（见 `config.rs::TransportLimits`）
//! 3. **怎么算失败？** —— 错误码 + `ModelRetryClass`（该不该让上层重试）
//! 4. **字节怎么变成结果？** —— `Framer` 切帧 + `response.rs` 累加
//!
//! ## 上游 / 下游
//!
//! - 上游：`lib.rs::complete_admitted` 的最后一行 `transport::send(connection, prepared, sink)`。
//! - 下游：`response.rs` 的 `decode` / `Accumulator`；`reqwest`；`tokio::time::timeout`。
//!
//! ## ⚠ 全局不变量
//!
//! **本文件不做重试。** 一次 `send()` 就是一次尝试。
//! 原因见 `kiana-ports/src/model.rs` 的说明：重试属于上层已获准的 attempt driver。
//! 如果这里偷偷重试，一次用户请求可能消耗数倍预算，而控制面的预算账本只记了一次——
//! 账实不符，审计对不上。本文件只**如实报告**这次尝试该���么分类
//! （`ModelRetryClass::BeforeSend` / `Rejected` / `Never`），由上层决定要不要再来一次。

use crate::{
    config::Connection,
    response::{decode, Accumulator},
};
use futures::StreamExt;
use kiana_domain::*;
use std::time::{Duration, Instant, SystemTime};

/// 发一次请求。**这是 provider 侧唯一的网络出口。**
///
/// 【作用】 在总时限内包住整个尝试过程。
///
/// 【调用者】 `kiana-provider/src/lib.rs::complete_admitted` 的最后一行。
///
/// 【输入】
/// - `connection`：目标连接（端点、凭据、限额、熔断器、已锁死策略的 HTTP 客户端）。
/// - `prepared`：已冻结、已封存、已获许可的调用。
/// - `sink`：增量回调。流式时每收到一段有意义的内容就调一次；非流式时在最后补一次全文。
///   **sink 返回 `Err` 会中断整条流**——这是下游施加背压（backpressure）的唯一手段。
///
/// 【输出】 `ModelReply`（已由 `response.rs` 解析并归一化）。
///
/// 【副作用】 有，且都是不可逆的：
/// - 发出一次真实的 HTTP 请求（**可能已经在上游产生了计费**）；
/// - 消费一次凭据租约；
/// - 占用一次 RPM/TPM 配额与一个并发槽位；
/// - 可能改变熔断器状态。
///
/// 【失败情况】
/// - `model_deadline_expired`：**发之前**就已经超时，一个字节都没发出去。
/// - `model_attempt_deadline`：发出去了但在总时限内没跑完，由 `tokio::time::timeout` 掐断。
///
/// 【⚠ 为什么总时限取 `min(deadline - now, limits.total)`】
/// ```text
///   deadline_unix_ms  是 ControlPlane 给这次调用定的绝对截止时间（业务语义）
//!   limits.total      是 transport 的工程上界（资源保护）
///
///   取两者的较小值：
///     - 只认 deadline -> 传输层可能比业务允许的活得更久，白占并发槽位
///     - 只认 total    -> 可能超过业务截止时间，把已经没意义的回复也算成功
///   min() 让两个约束同时成立，谁更紧听谁的。
/// ```
///
/// 【⚠ 为什么已经过期时直接 `Err` 而不是交给 timeout】
/// 如果 `now >= deadline`，后面的 `Duration::from_millis(deadline - now)` 会下溢成巨大值，
/// 等于没有超时。必须先判。

pub(crate) async fn send(
    connection: &Connection,
    prepared: PreparedModelCall,
    sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
) -> Result<ModelReply, ModelError> {
    let now = unix_ms()?;
    if now >= prepared.spec.deadline_unix_ms {
        return Err(ModelError::invalid("model_deadline_expired"));
    }
    let remaining =
        Duration::from_millis(prepared.spec.deadline_unix_ms - now).min(connection.limits.total);
    tokio::time::timeout(remaining, send_inner(connection, prepared, sink))
        .await
        .map_err(|_| {
            ModelError::transport("model_attempt_deadline", ModelRetryClass::Never, true)
        })?
}
/// 在总时限内：熔断器放行 -> 执行尝试 -> 把结果回灌给熔断器。
///
/// 【作用】 熔断器的**完整生命周期管理**都在这里。`send()` 只管时间，
/// 这里管“这个上游现在健康吗”。
///
/// 【核心流程】 五步：
/// ```text
///   1. circuit.allow(now)     —— 问熔断器：现在放行吗？
///   2. HalfOpenProbeGuard::new —— 如果这次是“试探性放行”，装一个看门狗
///   3. send_inner_attempt()    —— 真正去发
//!   4. 观察结果 -> observe_success / observe_failure / abandon_probe
///   5. 若观察成功，解除看门狗（否则 Drop 时会再放弃一次）
/// ```
///
/// 【⚠ 第 2 步的看门狗为什么必要】
/// 熔断器进入半开（HalfOpen）状态时，只放**一个**探测请求过去试探。
/// 如果这个请求在返回结果之前（比如 panic、或者某条提前 return 的路径）
/// 没有汇报结果，熔断器就会永远停留在“有一个探测在飞”的状态，
/// 之后所有请求都被 `provider_circuit_open` 拒绝——**熔断器被卡死**。
/// `HalfOpenProbeGuard` 的 `Drop` 兜住这个洞：只要 guard 被 drop 且没解除，
/// 就自动调用 `abandon_probe` 把熔断器恢复成 Open 并清掉“在飞”标志。

async fn send_inner(
    connection: &Connection,
    prepared: PreparedModelCall,
    sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
) -> Result<ModelReply, ModelError> {
    let now = unix_ms()?;
    let admission = {
        let mut circuit = connection
            .circuit
            .lock()
            .map_err(|_| ModelError::invalid("provider_circuit_lock_poisoned"))?;
        circuit.allow(now).map_err(ModelError::invalid)?
    };
    let mut probe_guard = HalfOpenProbeGuard::new(connection.circuit.clone(), admission, now);
    let result = send_inner_attempt(connection, prepared, sink).await;
    let observed_at = unix_ms().unwrap_or(now);
    let observation_applied = match connection.circuit.lock() {
        Ok(mut circuit) if result.is_ok() => circuit.observe_success(),
        Ok(mut circuit) if result.as_ref().err().is_some_and(trips_circuit) => {
            circuit.observe_failure(observed_at)
        }
        Ok(mut circuit) if probe_guard.is_some() => circuit.abandon_probe(observed_at),
        Ok(_) => Ok(()),
        Err(_) => Err("provider_circuit_lock_poisoned".to_owned()),
    };
    if observation_applied.is_ok() {
        if let Some(guard) = probe_guard.as_mut() {
            guard.disarm();
        }
    }
    result
}

/// 半开探测的看门狗。**只在熔断器放的是探测请求时才存在。**
///
/// 【字段】
/// - `circuit`：熔断器的共享句柄（`Arc<Mutex<...>>`）。
/// - `admitted_at_unix_ms`：放行时刻，用作 `Drop` 时取时间的兜底初值。
/// - `armed`：是否仍然需要兜底。正常路径下 `disarm()` 会把它置 false。

struct HalfOpenProbeGuard {
    circuit: std::sync::Arc<std::sync::Mutex<ProviderCircuitBreaker>>,
    admitted_at_unix_ms: u64,
    armed: bool,
}

impl HalfOpenProbeGuard {
    fn new(
        circuit: std::sync::Arc<std::sync::Mutex<ProviderCircuitBreaker>>,
        admission: CircuitAdmission,
        admitted_at_unix_ms: u64,
    ) -> Option<Self> {
        (admission == CircuitAdmission::HalfOpenProbe).then_some(Self {
            circuit,
            admitted_at_unix_ms,
            armed: true,
        })
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

/// guard 离开作用域时的兜底：把熔断器恢复成 Open。
///
/// 【核心流程】
/// ```text
///   armed == false  ->  已经正常汇报过结果，不做任何事（这是绝大多数情况）
///   armed == true   ->  取当前时间（失败则用 admitted_at 兜底，且至少为 1）
///                     锁上熔断器，调用 abandon_probe(now)
/// ```
///
/// 【⚠ 为什么要 `.max(1)`】
/// `unix_ms()` 失败时回退到 `admitted_at_unix_ms`，它可能是 0（时钟异常）。
/// 时间 0 会让熔断器计算出负的冷却剩余时间，行为不可预期。`.max(1)` 兜一个下界。
///
/// 【⚠ 为什么要 `unwrap_or_else(|poisoned| poisoned.into_inner())`】
/// 熔断器的锁被毒化（某个持锁的线程 panic 了）时，这里**仍然要完成放弃动作**。
/// 拿不到锁就等于让熔断器卡在半开状态，那是更糟的结果。
/// 锁里的数据只是计数器，被 poison 之后依然可读可写。

impl Drop for HalfOpenProbeGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let now_unix_ms = unix_ms().unwrap_or(self.admitted_at_unix_ms).max(1);
        let mut circuit = self
            .circuit
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _ = circuit.abandon_probe(now_unix_ms);
    }
}

/// 判断一次失败**要不要**计入熔断器。
///
/// 【三个条件必须同时满足，缺一不可】
/// ```text
///   1. error.phase == "transport"        传输层失败
///   2. error.request_sent               请求确实已经发出去了
///   3. code 属于下面这 6 个              是“上游暂时不可用”的典型形态
/// ```
///
/// 【逐条解释为什么】
/// - 条件 1：`invalid` 类错误（参数不合法、schema 不对）是我们自己的问题，
///   上游可能完全健康，重试一百次也一样错。不该熔断。
/// - 条件 2：**没发出去的失败不算**。比如请求体编码失败、对端连不上，
///   这跟“这个上游健康吗”无关。
/// - 条件 3：只有这 6 种错误说明“上游此刻接不住”：
///   ```text
///     provider_http_429          被限流
///     provider_http_408          上游超时
///     provider_http_503          上游过载
///     provider_connection_failed 连接失败
///     provider_headers_timeout   响应头超时
///     provider_read_idle_timeout  流式中途卡住
///   ```
///   注意 `provider_http_401` **不在列表里**：key 无效重试没用，
///   熔断整个上游只会让所有人都用不了。

fn trips_circuit(error: &ModelError) -> bool {
    error.phase == "transport"
        && error.request_sent
        && matches!(
            error.code.as_str(),
            "provider_http_429"
                | "provider_http_408"
                | "provider_http_503"
                | "provider_connection_failed"
                | "provider_headers_timeout"
                | "provider_read_idle_timeout"
        )
}

/// 一次真正的尝试：闸门 -> 凭据 -> 发请求 -> 读响应。
///
/// 【这是本文件最长的函数，分成 6 段来看】
/// ```text
//!   A. 容量策略校验 + 单请求 TPM 预检
///   B. 排队信号量（不等）-> 并发信号量（等）-> 释放排队槽
///   C. 截止时间复查 + RPM/TPM 窗口扣减
///   D. 凭据：取摘要、换租约、校验绑定、消费
///   E. 拼 HTTP 请求与认证头，处理非 2xx 与 retry-after
///   F. 流式 / 非流式两条读响应路径
//! ```
///
/// 【输入 / 输出 / 副作用】 见 `send()`；本函数额外负责第 6 项副作用（凭据租约消费）。
///
/// 【⚠ 整个函数被 `send()` 的 `tokio::time::timeout` 包住】
/// 所以这里不需要自己管总时限；但**分阶段超时仍然要自己设**（见 F 段），
/// 因为分阶段超时能给出比笼统总超时更有信息量的错误码。

async fn send_inner_attempt(
    connection: &Connection,
    prepared: PreparedModelCall,
    sink: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
) -> Result<ModelReply, ModelError> {
    // Capacity policy is immutable, server-owned state bound to the provider/origin/credential
    // quota group.  Validate the same policy before touching a waiter or active semaphore so a
    // profile alias, credential revision or model switch cannot bypass RPM/TPM admission.
    // ===== A. 容量策略 =====
    // 策略是服务端下发的不可变状态，绑定在 provider/端点/凭据配额组上。
    // ⚠ 在碰任何信号量之前先校验：别名、凭据换版、模型切换
    //   都不该绕过 RPM/TPM 准入。先校验再抢槽，避免为非法策略白等一次队列。

    connection
        .capacity_policy
        .validate()
        .map_err(ModelError::invalid)?;
    // 本次预计消耗的 token 数。`.max(1)` 保证至少为 1：
    // 0 会被 `capacity.rs::reserve` 判成 `provider_capacity_window_request_invalid`，
    // 而“预算为 0 的请求”本身就不该发出去。

    let requested_tokens = prepared.budget.total.max(1);
    // 单请求自身的 token 预检。即使当前窗口还很空，一次请求的用量也不能超过整分钟的配额——
    // 否则它会把整分钟的额度一次吃掉。
    // `ModelRetryClass::Never`：立刻失败，不重试。等到下一个窗口自然就能过。

    if requested_tokens > connection.capacity_policy.tokens_per_minute {
        return Err(ModelError::transport(
            "provider_capacity_tpm_exceeded",
            ModelRetryClass::Never,
            false,
        ));
    }
    // ===== B. 排队闸门 =====
    // `try_acquire_owned` 是**非阻塞**的：拿不到立刻失败。
    //
    // 为什么先排“候诊椅”再排“诊室”？因为候诊椅满了说明系统已经严重过载，
    // 让新请求继续排进诊室队列只会让所有人等更久。提前劝退（fail fast）更友好。

    let queue_slot = connection
        .queue_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            ModelError::transport(
                "provider_capacity_queue_full",
                ModelRetryClass::Never,
                false,
            )
        })?;
    // 并发闸门。`acquire_owned` 是**阻塞等待**的：候诊椅有位置就等诊室空出来。
    //
    // ⚠ 这里 await 的时间**也算在总时限里**（外层有 timeout）。所以一个请求
    //   可能大量时间花在排队上，留给真正发请求的时间就不多了——
    //   这就是为什么候诊椅要有上限。

    let _capacity = connection
        .capacity
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| {
            ModelError::transport("provider_capacity_closed", ModelRetryClass::Never, false)
        })?;
    // 一旦拿到诊室名额，立刻把候诊椅让出来。
    // 顺序很重要：**先占诊室再让椅子**，中间不留空隙，
    // 否则会出现“明明还有空位却没人来”的空转。

    drop(queue_slot);
    // ===== C. 窗口配额 =====
    // ⚠ 排队可能耗掉不少时间，所以**必须重新取一次时间**再判截止时间，
    // 不能复用函数开头那个 `now`。否则会出现“已经超时了还去扣配额”。

    let lease_now = unix_ms()?;
    if lease_now >= prepared.spec.deadline_unix_ms {
        return Err(ModelError::invalid("model_deadline_expired"));
    }
    connection
        .capacity_window
        .reserve(&connection.capacity_policy, lease_now, requested_tokens)
        .map_err(|reason| {
            if reason == "provider_capacity_quota_exhausted" {
                ModelError::transport(&reason, ModelRetryClass::Rejected, false)
            } else {
                ModelError::invalid(reason)
            }
        })?;
    // ===== D. 凭据 =====
    // 端点摘要：凭据租约要绑定“发到哪个端点”，而绑定时不能存明文 URL
    //（URL 会进错误信息、进快照）。用摘要就足够做等值比较。

    let endpoint_digest = json_digest(&serde_json::json!(connection.endpoint.as_str()));
    let mut material = connection
        .credential_ref
        .as_ref()
        .map(|secret_ref| {
            connection.credential_store.issue(
                secret_ref,
                &connection.provider_account,
                &connection.route.provider_id,
                &endpoint_digest,
                lease_now,
            )
        })
        .transpose()?;
    // 拿到材料后**立刻**做三件事，顺序不可换：
    //   1. validate_for(...)  校验租约绑定：账号/用途/audience/端点摘要/时效
    //   2. 比对 secret_ref    确保租约说的就是我们要用的那把密钥
    //   3. compare revision   确保准入之后凭据没被换过
    // 最后 consume()  —— 租约一次性，用掉即作废。
    //
    // ⚠ 三道检查缺一不可：少了 1，租约可能被跨端点复用；
    //   少了 2，可能用 A 的租约去取 B 的密钥；少了 3，准入后换 key 也发现不了。

    if let (Some(secret_ref), Some(material)) =
        (connection.credential_ref.as_ref(), material.as_mut())
    {
        material
            .lease
            .validate_for(
                lease_now,
                &connection.provider_account,
                "provider.request",
                &connection.route.provider_id,
                &endpoint_digest,
            )
            .map_err(ModelError::invalid)?;
        if &material.lease.secret_ref != secret_ref {
            return Err(ModelError::invalid("credential_lease_reference_mismatch"));
        }
        if material.credential_revision != connection.credential_revision {
            return Err(ModelError::invalid("model_credential_revision_changed"));
        }
        material
            .lease
            .consume(lease_now)
            .map_err(ModelError::invalid)?;
    }
    // ===== E. 拼请求 =====
    // 三件固定的事：POST 到端点、带上账号标识、带上冻结好的请求体。
    //
    // ⚠ `x-kiana-provider-account` 这个头**不含密钥**，只是账号摘要 id，
    //    方便上游侧网关按账号做路由与限流归类。

    let mut request = connection
        .client
        .post(connection.endpoint.clone())
        .header("x-kiana-provider-account", &connection.provider_account)
        .json(&prepared.wire_body);
    // 认证头的三家方言。**没有凭据时整个 if 块被跳过**
    // （只有本地 Ollama 属于这种情况）。
    //
    //   AnthropicMessages -> x-api-key + anthropic-version
    //        Anthropic 不读 Authorization 头，它要 x-api-key。
    //        anthropic-version 是必填的协议版本号，缺了会 400。
    //   GeminiInteractions -> x-goog-api-key
    //        Google 的 API 惯例。
    //   其它（OpenAI / Ollama / 兼容端点）-> Bearer 认证
    //        标准 `Authorization: Bearer <key>`。

    if let Some(material) = material.as_ref() {
        let key = &material.value;
        request = match connection.route.protocol {
            ModelProtocol::AnthropicMessages => request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
            ModelProtocol::GeminiInteractions => request.header("x-goog-api-key", key),
            _ => request.bearer_auth(key),
        };
    }
    // 发请求，等**响应头**。`limits.headers` 只覆盖到这里，
    // 不包括后续读 body 的时间——body 由下面的分阶段空闲超时管。
    //
    // 超时归类为 `Never`（不可重试）且 `request_sent = true`。
    // 之所以不可重试：请求很可能已经到达上游并开始计费了，
    // 重发一次就是双倍费用。这与 FAQ Q5 的 result_unknown 原则一致。

    let response = tokio::time::timeout(connection.limits.headers, request.send())
        .await
        .map_err(|_| {
            ModelError::transport("provider_headers_timeout", ModelRetryClass::Never, true)
        })?
        .map_err(|err| {
            let retry = classify_connect_failure(err.is_connect(), io_error_kind(&err));
            ModelError::transport(
                "provider_connection_failed",
                retry,
                retry != ModelRetryClass::BeforeSend,
            )
        })?;
    // 非 2xx 一律失败。这里**不读错误响应体**——
    // 错误体里常包含上游的内部堆栈、请求 id 甚至（某些网关的）凭据回显。
    // 只保留状态码作为稳定错误码 `provider_http_<状态码>`。

    let status = response.status();
    // 状态码到重试分类的映射（只区分两种）：
    // ```text
    //   429 / 408  ->  Rejected  “我方已经收到，但上游明确要求稍后再试”
    //   其它非 2xx ->  Never     “重试也不会变好”（401 key 错、400 参数错、404 路径错…）
    // ```
    //
    // ⚠ 注意 `Rejected` 这个名字容易被误解：它**不是**“拒绝执行”，
    //    而是“这次尝试被上游拒绝，但换个时间重试可能成功”。
    //    真正的执行与否由上层根据 `ModelRetryClass` 决定。
    //
    // ⚠ 紧接着把 `side_effect_state` 设成 `None`：HTTP 错误意味着这次尝试没有产生副作用，
    //    上层可以安全地按 `retry_after_ms` 重试。

    if !status.is_success() {
        let mut error = ModelError::transport(
            &format!("provider_http_{}", status.as_u16()),
            if matches!(status.as_u16(), 429 | 408) {
                ModelRetryClass::Rejected
            } else {
                ModelRetryClass::Never
            },
            true,
        );
        error.side_effect_state = ModelSideEffectState::None;
        error.retry_after_ms = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| parse_retry_after(value, SystemTime::now()));
        // Error bodies and authentication headers are never copied into logs or public errors.
        return Err(error);
    }
    // 读 Content-Type。它决定后面走流式切帧还是整体解析，
    // 也是**协议方言的第一道校验**：内容类型不对就直接拒绝，
    // 避免把一段 HTML 错误页当成 SSE 去切。

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    // 上游请求 id。三家服务商的头名不同，按顺序试。
    //
    // 【为什么要拿它】 它是向上游问“这是哪一次调用”的唯一凭据。
    // 用户报障时把这个 id 给我们，上游就能定位到那一次请求。
    //
    // 【⚠ 为什么要限制长度 < 256】 它是外部数据。无限长的字符串进日志/事件就是注入面。

    let provider_request_id = ["request-id", "x-request-id", "x-goog-request-id"]
        .iter()
        .find_map(|key| {
            response
                .headers()
                .get(*key)
                .and_then(|v| v.to_str().ok())
                .filter(|v| v.len() < 256)
                .map(str::to_owned)
        });
    // ===== F. 读响应：流式 / 非流式两条路 =====
    //
    // ⚠ 先判**期望的**流式方言，再看实际 Content-Type 是否匹配，不匹配就报错。
    //    宁可报错也不“将就着解析”——将就解析出来的结果可能是错的，
    //    而错的模型输出比明确的失败危险得多。

    if prepared.route.streaming {
        let ndjson = prepared.route.protocol == ModelProtocol::OllamaChat;
        if !(if ndjson {
            content_type.starts_with("application/x-ndjson")
                || content_type.starts_with("application/json")
        } else {
            content_type.starts_with("text/event-stream")
        }) {
            return Err(ModelError::invalid("provider_content_type_invalid"));
        }
    // 流式主循环的四根支柱：
    // ```text
    //   Framer      把任意切分的字节流还原成一个个完整的帧（SSE 或 NDJSON）
    //   Accumulator 把帧累积成 ModelReply，并逐块回调 sink
    //   total       累计字节数，超过 max_body 立刻中止
    //   idle/first  分阶段空闲超时，见下面的 loop
    // ```

        let mut chunks = response.bytes_stream();
        let mut framer = Framer::new(ndjson, connection.limits.max_frame);
        let mut accumulator = Accumulator::new(prepared.route.protocol);
        let mut total = 0usize;
        let mut semantic = false;
        let started = Instant::now();
        // 【为什么循环里每次都重算超时预算】
        // ```text
        //   还没收到第一个“有意义”的事件 -> 用 first_event 预算，且要减去已经过去的时间
        //     （一个冷启动的大模型可能 50 秒才开始吐第一个 token，
        //       如果每个 chunk 都重新给 60 秒，总时限早就被吃光了却没人发现）
        //
        //   已经收到过语义事件            -> 用固定的 idle 预算
        //     （流式输出中模型“正在想”的间隔）
        // ```
        //
        // ⚠ `first_event` 走的是 `saturating_sub(started.elapsed())`：
        //   已经用掉的时间会被扣掉，这才是“首事件总预算”的正确算法。

        loop {
            let idle = if semantic {
                connection.limits.idle
            } else {
                connection
                    .limits
                    .first_event
                    .saturating_sub(started.elapsed())
            };
            // 预算耗尽。注意用 `is_zero()` 而不是 `<= Duration::ZERO`，
            // 因为 saturating_sub 可能正好减到 0，此时 timeout(0) 会立刻超时——
            // 语义上正确。

            if idle.is_zero() {
                return Err(ModelError::transport(
                    "provider_first_semantic_timeout",
                    ModelRetryClass::Never,
                    true,
                ));
            }
            let next = tokio::time::timeout(idle, chunks.next())
                .await
                .map_err(|_| {
                    ModelError::transport(
                        "provider_read_idle_timeout",
                        ModelRetryClass::Never,
                        true,
                    )
                })?;
            // 流正常结束。跳出循环后走下面的 `framer.finish()` 收尾。

            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(|_| {
                ModelError::transport("provider_stream_read_failed", ModelRetryClass::Never, true)
            })?;
            // 累计字节数。`checked_add` 溢出时报错而不是 wrap。
            // 超过 `max_body`（8 MiB）立刻中止——防止一个失控的流把内存吃光。

            total = total
                .checked_add(chunk.len())
                .ok_or_else(|| ModelError::invalid("provider_body_limit"))?;
            if total > connection.limits.max_body {
                return Err(ModelError::invalid("provider_body_limit"));
            }
            for data in framer.push(&chunk)? {
                if !is_heartbeat(&data) {
                    semantic = true;
                }
                if accumulator.push(&data, sink)? {
                    let mut reply = accumulator.finish(&prepared)?;
                    reply.provider_request_id = provider_request_id;
                    return Ok(reply);
                }
            }
        }
        // 流结束后把缓冲区里剩下的内容 flush 出来
        // （SSE 的最后一个帧可能没有以空行结尾；NDJSON 的最后一行可能没有换行符）。

        for data in framer.finish()? {
            if accumulator.push(&data, sink)? {
                let mut reply = accumulator.finish(&prepared)?;
                reply.provider_request_id = provider_request_id;
                return Ok(reply);
            }
        }
        // 流结束了但 `Accumulator` 从没返回 true（表示“我拼完了”）。
        // 这说明流是**被截断的**——上游没说完就断了。
        //
        // ⚠ 这里必须失败，绝不能把半截内容当成完整回复返回。
        //    一个被截断的 JSON / 一个只说了一半的句子，
        //    如果被当成成功结果写进事件，后面所有基于它的判断都是错的。

        Err(ModelError::transport(
            "provider_stream_incomplete",
            ModelRetryClass::Never,
            true,
        ))
    } else {
        if !content_type.starts_with("application/json") {
            return Err(ModelError::invalid("provider_content_type_invalid"));
        }
        let mut chunks = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = tokio::time::timeout(connection.limits.idle, chunks.next())
            .await
            .map_err(|_| {
                ModelError::transport("provider_read_idle_timeout", ModelRetryClass::Never, true)
            })?
        {
            let chunk = chunk.map_err(|_| {
                ModelError::transport(
                    "provider_response_read_failed",
                    ModelRetryClass::Never,
                    true,
                )
            })?;
            if bytes.len().saturating_add(chunk.len()) > connection.limits.max_body {
                return Err(ModelError::invalid("provider_body_limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let value = serde_json::from_slice(&bytes)
            .map_err(|_| ModelError::invalid("provider_response_json_invalid"))?;
        let mut reply = decode(value, &prepared)?;
        reply.provider_request_id = provider_request_id;
        if !reply.output.text.is_empty() {
            sink(ModelDelta::Text {
                text: reply.output.text.clone(),
            })
            .map_err(ModelError::invalid)?;
        }
        Ok(reply)
    }
}
/// 解析 HTTP `Retry-After` 头，返回**毫秒**。
///
/// 【作用】 上游说“过 X 秒再试”，把它变成绝对毫秒数供上层使用。
///
/// 【两种格式，HTTP 规范都允许】
/// ```text
///   delay-seconds   "120"                     -> 120 * 1000 毫秒
///   HTTP-date       "Wed, 21 Oct 2026 07:28:00 GMT"  -> 减去当前时间
/// ```
///
/// 【⚠ 为什么用 `saturating_mul` 而不是 `* 1000`】
/// 一个恶意或异常的服务端可以回 `Retry-After: 18446744073709551615`。
/// 普通乘法会溢出 panic；saturating 会得到 `u64::MAX`——
/// 上层看到“几乎无限久”自然会拒绝重试，这是安全的方向。
///
/// 【⚠ HTTP-date 已经在过去时返回 None】
/// `duration_since(now)` 对负值返回 `Err`，这里用 `?` 把它变成 `None`。
/// 含义是“别等了，现在就重试”。

fn parse_retry_after(value: &str, now: SystemTime) -> Option<u64> {
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds.saturating_mul(1000));
    }
    let when = httpdate::parse_http_date(value).ok()?;
    let duration = when.duration_since(now).ok()?;
    Some(duration.as_millis().min(u128::from(u64::MAX)) as u64)
}

/// 从 `reqwest::Error` 的**错误链**里挖出底层 `io::Error` 的 kind。
///
/// 【作用】 reqwest 把网络错误层层包装，真正有用的信息（`ConnectionRefused`、
/// `TimedOut`…）埋在最内层。直接看最外层只能看到笼统的 "error sending request"。
///
/// 【核心流程】 沿 `source()` 链一路向下走，找到第一个能 downcast 成
/// `std::io::Error` 的节点，返回它的 `kind()`。找不到返回 `None`。

fn io_error_kind(error: &reqwest::Error) -> Option<std::io::ErrorKind> {
    let mut cause = Some(error as &(dyn std::error::Error + 'static));
    while let Some(error) = cause {
        if let Some(io_error) = error.downcast_ref::<std::io::Error>() {
            return Some(io_error.kind());
        }
        cause = error.source();
    }
    None
}

/// 判断一个连接失败**该不该让上层重试**。
///
/// 【两个条件都必须满足】
/// ```text
///   is_connect == true     reqwest 明确说这是“连接阶段”的错误
///   AND kind 属于这 7 种   ConnectionRefused / ConnectionReset / TimedOut
///                          / AddrNotAvailable / NotConnected
///                          / NetworkUnreachable / HostUnreachable
/// ```
///
/// 【为什么只要 is_connect 就够】
/// 因为**连接失败意味着请求根本没发出去**，上游没有产生任何副作用，
/// 重试是安全的。这正是 `ModelRetryClass::BeforeSend` 的含义：
/// “可以重试，且重试前不会有副作用”。
///
/// 【为什么 TLS 失败不可重试】
/// `is_connect == true` 但 `kind == None`（拿不到 io kind，通常是 TLS 握手问题）时，
/// 落到 `else` 分支返回 `Never`。证书不受信是配置/环境问题，
/// 重试一万次还是不受信。
///
/// 【为什么 `InvalidData` 不可重试】
/// 数据损坏重试也不会变好——同一份坏数据再发一次还是坏。

fn classify_connect_failure(is_connect: bool, kind: Option<std::io::ErrorKind>) -> ModelRetryClass {
    if is_connect
        && matches!(
            kind,
            Some(
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::AddrNotAvailable
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::NetworkUnreachable
                    | std::io::ErrorKind::HostUnreachable
            )
        )
    {
        ModelRetryClass::BeforeSend
    } else {
        ModelRetryClass::Never
    }
}

/// 判断一帧是不是**心跳**（保活帧，不含内容）。
///
/// 【背景】 流式协议里，服务商可能定期发 `{"type":"ping"}` 保持连接不被中间设备掐断。
/// 这些帧**不是**模型输出。
///
/// 【为什么要单独识别】
/// 见主循环里的 `semantic` 标志：只有非心跳帧才把超时模式从
/// “首事件预算”切换到“常规空闲预算”。如果把心跳当成内容，
/// 一个只发心跳不发内容的连接会被误判为“已经在正常输出”，
/// 于是永远等不到真正的首事件。

fn is_heartbeat(data: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(data)
        .ok()
        .is_some_and(|v| v["type"] == "ping")
}
/// 取当前 Unix 毫秒时间戳，失败时报错。
///
/// 【⚠ 为什么要返回 Result 而不是直接 unwrap】
/// `SystemTime::duration_since(UNIX_EPOCH)` 在系统时钟被设到 1970 年之前时会失败。
/// 这时候**绝不能**悄悄返回 0——0 会被误解成“1970 年”，进而让所有截止时间判断失效。
/// 宁可让整次调用失败（`model_clock_untrusted`）。

pub(crate) fn unix_ms() -> Result<u64, ModelError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|v| u64::try_from(v.as_millis()).ok())
        .ok_or_else(|| ModelError::invalid("model_clock_untrusted"))
}

#[cfg(test)]
// This module covers the circuit breaker guard and sits above `Framer`; the transport parsing
// tests live in a second `mod tests` further down, which is why this one is named.
mod circuit_tests {
    use super::HalfOpenProbeGuard;
    use kiana_domain::{CircuitAdmission, ProviderCircuitBreaker, ProviderCircuitState};
    use std::sync::{Arc, Mutex};

    #[test]
    fn dropping_half_open_probe_guard_reopens_breaker_and_clears_busy_fence() {
        let mut breaker = ProviderCircuitBreaker::new("config.v1", 1, 100).expect("breaker");
        breaker.observe_failure(1).expect("open breaker");
        breaker.allow(101).expect("claim half-open probe");
        let circuit = Arc::new(Mutex::new(breaker));
        let guard = HalfOpenProbeGuard::new(circuit.clone(), CircuitAdmission::HalfOpenProbe, 101)
            .expect("half-open guard");

        drop(guard);

        let mut circuit = circuit.lock().expect("circuit lock");
        assert_eq!(circuit.state, ProviderCircuitState::Open);
        assert!(!circuit.half_open_probe_in_flight);
        let open_until = circuit.open_until_unix_ms.expect("reopened cooldown");
        assert_eq!(
            circuit.allow(open_until - 1).unwrap_err(),
            "provider_circuit_open"
        );
        assert_eq!(
            circuit.allow(open_until).expect("probe after cooldown"),
            CircuitAdmission::HalfOpenProbe
        );
    }
}

pub(crate) struct Framer {
    buffer: Vec<u8>,
    data: String,
    ndjson: bool,
    limit: usize,
}
impl Framer {
    pub(crate) fn new(ndjson: bool, limit: usize) -> Self {
        Self {
            buffer: Vec::new(),
            data: String::new(),
            ndjson,
            limit,
        }
    }
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, ModelError> {
        let mut frames = Vec::new();
        // Limit each line while receiving it, without copying an oversized chunk into our buffer.
        for byte in chunk {
            if self.buffer.len() >= self.limit {
                return Err(ModelError::invalid("provider_frame_limit"));
            }
            self.buffer.push(*byte);
            if *byte == b'\n' {
                let bytes = std::mem::take(&mut self.buffer);
                let line = std::str::from_utf8(&bytes)
                    .map_err(|_| ModelError::invalid("provider_frame_utf8_invalid"))?
                    .trim_end_matches(['\r', '\n']);
                if self.ndjson {
                    if !line.trim().is_empty() {
                        frames.push(line.to_owned());
                    }
                } else if line.is_empty() {
                    if !self.data.is_empty() {
                        frames.push(std::mem::take(&mut self.data));
                    }
                } else if let Some(value) = line.strip_prefix("data:") {
                    let value = value.strip_prefix(' ').unwrap_or(value);
                    if self
                        .data
                        .len()
                        .saturating_add(value.len())
                        .saturating_add(1)
                        > self.limit
                    {
                        return Err(ModelError::invalid("provider_frame_limit"));
                    }
                    if !self.data.is_empty() {
                        self.data.push('\n');
                    }
                    self.data.push_str(value);
                } else if line.starts_with(':')
                    || line.starts_with("event:")
                    || line.starts_with("id:")
                    || line.starts_with("retry:")
                {
                } else {
                    return Err(ModelError::invalid("provider_sse_field_invalid"));
                }
            }
        }
        Ok(frames)
    }
    pub(crate) fn finish(&mut self) -> Result<Vec<String>, ModelError> {
        if self.ndjson && !self.buffer.is_empty() {
            let bytes = std::mem::take(&mut self.buffer);
            let value = String::from_utf8(bytes)
                .map_err(|_| ModelError::invalid("provider_frame_utf8_invalid"))?;
            return Ok(vec![value]);
        }
        if !self.buffer.is_empty() || !self.data.is_empty() {
            return Err(ModelError::invalid("provider_frame_truncated"));
        }
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    #[test]
    fn retry_after_http_date_uses_injected_clock() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let date = httpdate::fmt_http_date(now + Duration::from_secs(7));
        assert_eq!(parse_retry_after(&date, now), Some(7_000));
        assert_eq!(parse_retry_after(&date, now + Duration::from_secs(8)), None);
        assert_eq!(
            parse_retry_after("18446744073709551615", now),
            Some(u64::MAX)
        );
    }

    #[test]
    fn retry_after_large_delta_is_preserved_for_absolute_deadline_check() {
        let now = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        assert_eq!(parse_retry_after("3600", now), Some(3_600_000));
    }

    #[test]
    fn only_typed_network_connect_failures_are_retryable() {
        assert_eq!(
            classify_connect_failure(true, Some(std::io::ErrorKind::ConnectionRefused)),
            ModelRetryClass::BeforeSend
        );
        assert_eq!(
            classify_connect_failure(true, Some(std::io::ErrorKind::TimedOut)),
            ModelRetryClass::BeforeSend
        );
        assert_eq!(
            classify_connect_failure(true, Some(std::io::ErrorKind::InvalidData)),
            ModelRetryClass::Never
        );
        assert_eq!(
            classify_connect_failure(false, Some(std::io::ErrorKind::ConnectionRefused)),
            ModelRetryClass::Never
        );
    }

    #[test]
    fn tls_failure_is_not_transient() {
        assert_eq!(classify_connect_failure(true, None), ModelRetryClass::Never);
        assert_eq!(
            classify_connect_failure(true, Some(std::io::ErrorKind::InvalidData)),
            ModelRetryClass::Never
        );
    }

    #[test]
    fn sse_survives_arbitrary_utf8_chunking_and_crlf() {
        let payload = "data: {\"text\":\"你好\"}\r\n\r\n";
        let mut framer = Framer::new(false, 128);
        let mut frames = Vec::new();
        for chunk in payload.as_bytes().chunks(1) {
            frames.extend(framer.push(chunk).expect("sse frame"));
        }
        frames.extend(framer.finish().expect("sse complete"));
        assert_eq!(frames, vec![r#"{"text":"你好"}"#.to_owned()]);
    }

    #[test]
    fn sse_multiline_data_and_comments_are_bounded() {
        let mut framer = Framer::new(false, 128);
        let mut frames = Vec::new();
        frames.extend(
            framer
                .push(b": heartbeat\r\ndata: first\r\ndata: second\r\n\r\n")
                .expect("sse frame"),
        );
        assert_eq!(frames, vec!["first\nsecond".to_owned()]);
        assert!(framer.finish().expect("sse complete").is_empty());
    }

    #[test]
    fn ndjson_flushes_complete_lines_and_one_bounded_tail() {
        let mut framer = Framer::new(true, 64);
        let mut frames = Vec::new();
        for chunk in b"{\"a\":1}\n{\"b\":2}".chunks(2) {
            frames.extend(framer.push(chunk).expect("ndjson frame"));
        }
        frames.extend(framer.finish().expect("ndjson tail"));
        assert_eq!(
            frames,
            vec![r#"{"a":1}"#.to_owned(), r#"{"b":2}"#.to_owned()]
        );
    }

    #[test]
    fn oversized_and_truncated_frames_fail_closed() {
        let mut oversized = Framer::new(true, 4);
        assert_eq!(
            oversized.push(b"12345").unwrap_err().code,
            "provider_frame_limit"
        );

        let mut invalid_utf8 = Framer::new(true, 16);
        assert_eq!(
            invalid_utf8.push(&[0xff, b'\n']).unwrap_err().code,
            "provider_frame_utf8_invalid"
        );

        let mut truncated = Framer::new(false, 64);
        truncated.push(b"data: partial\n").expect("partial sse");
        assert_eq!(
            truncated.finish().unwrap_err().code,
            "provider_frame_truncated"
        );
    }
}
