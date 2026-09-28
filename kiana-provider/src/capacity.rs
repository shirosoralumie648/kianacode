//! Provider-side UTC RPM/TPM reservation adapter.
//!
//! This is a bounded in-process accounting aid behind the existing `ProviderGateway`. It never
//! authorizes a model call or starts a request; the ControlPlane permit and the transport boundary
//! remain required. Aliases share the same `CapacityWindow` through `Connection` construction.
//!
//! ## 这个文件在系统里的位置
//!
//! 本文件实现 provider 侧的 **RPM/TPM 计量**（requests-per-minute / tokens-per-minute）。
//! 它是挂在既有 `ProviderGateway` 后面的一个有界记账辅助件。
//!
//! ```text
//!   transport::send_inner_attempt()
//!        │  在真正发 HTTP 请求之前
//!        ↓
//!   【本文件 CapacityWindow::reserve()】← 按 UTC 分钟窗口扣减配额
//!        │
//!        ↓
//!   reqwest 发请求
//! ```
//!
//! ## 三层并发闸门的区别（初学者最容易混的地方）
//!
//! `Connection` 上其实有**三个**限制并发的东西，各管各的：
//!
//! | 名字 | 类型 | 管什么 | 满了怎么办 |
//! |---|---|---|---|
//! | `queue_slots` | `Semaphore` | **排队**人数上限 | 立刻拒绝 `provider_capacity_queue_full`（不等） |
//! | `capacity` | `Semaphore` | **同时执行**的请求数上限 | `await` 排队等待 |
//! | `capacity_window` | 本文件 | **每分钟**的请求数/token 数配额 | 拒绝 `provider_capacity_quota_exhausted` |
//!
//! 比喻：`queue_slots` 是候诊椅位数（坐满就劝退），`capacity` 是诊室里同时能看的人数
//! （满了排队），`capacity_window` 是每小时最多看几个病人（超了今天不看了）。
//!
//! ## 上游 / 下游
//!
//! - 上游：`kiana-provider/src/transport.rs::send_inner_attempt` 调用 `reserve`。
//! - 下游：本文件不调用外部任何东西，只读写自己的 `Mutex<WindowState>`。
//!
//! ## ⚠ 别名共享
//!
//! `config::connections()` 会把 provider/endpoint/凭据三者都相同的多个 profile 别名
//! 合并成**同一个** `Arc<CapacityWindow>`。也就是说：改个 profile 名字骗不过配额。

use kiana_domain::ProviderCapacityPolicy;
use std::sync::{Mutex, PoisonError};

/// 计量窗口长度：60 秒。
///
/// 【为什么是固定 60 秒而不是滑动窗口】
/// 滑动窗口（最近 60 秒内任意时刻的计数）需要保留每个请求的时间戳，内存与实现复杂度都高得多。
/// 固定窗口的实现是 O(1) 状态：只记 4 个 `u64`。
///
/// 【⚠ 固定窗口的已知缺陷，读者要知道】
/// 固定窗口在窗口边界会有“惊群”效应：例如 10:00:59 发 100 个、10:01:00 又发 100 个，
/// 实际 1 秒内发了 200 个，但两个窗口各自只看到 100 个，都没超限。
/// 这里接受这个折中——上游服务商自己的限流通常也是滑动窗口，
/// 真被限流了会以 HTTP 429 返回（见 `transport.rs`），那才是权威信号。
///
/// 【为什么窗口对齐到 UTC 的整分钟】
/// `now_unix_ms / WINDOW_MS * WINDOW_MS` 会把窗口边界钉在 Unix 纪元的整分钟上。
/// 这样所有进程、所有机器算出来的窗口边界一致，便于事后对账；代价是本地时区无关。

const WINDOW_MS: u64 = 60_000;

/// 当前计量窗口的四个计数。
///
/// 【为什么是 `Copy`】 只有 4 个 `u64`，整体按值进出锁比按引用更省事。
/// 【为什么 `Default`】 新建时 `start_unix_ms = end_unix_ms = 0`，
/// 这个“全 0”状态被 `reserve` 用来表示“还没有开过窗”，见下面 `state.end_unix_ms == 0` 那处判断。
///
/// 字段含义：
/// - `start_unix_ms`：本窗口起点（UTC 整分钟对齐后的毫秒时间戳）。
/// - `end_unix_ms`：本窗口终点 = start + 60_000；**不含**。
/// - `requests`：本窗口已放行的请求数。
/// - `tokens`：本窗口已放行的 token 数。

#[derive(Clone, Copy, Debug, Default)]
struct WindowState {
    start_unix_ms: u64,
    end_unix_ms: u64,
    requests: u64,
    tokens: u64,
}

/// 进程内的每分钟配额计量器。
///
/// 【为什么需要 `Mutex`】
/// 多个请求可能并发调用 `reserve`。如果不加锁，两个并发请求会同时读到
/// `tokens = 90`，各自判断 `90 + 10 <= 100` 通过，于是实际放行了 110 token——超限。
/// 这个竞态在压测下才会偶发，非常难查，所以这里必须串行化。
///
/// 【为什么用 `Mutex` 而不是原子操作】
/// 判定逻辑是“比较 + 条件写回”两步，原子操作无法覆盖；`Mutex` 的临界区只有几条整数运算，
/// 持锁时间极短，不会成为瓶颈。
///
/// 【作用域】 `pub(crate)`——只在 kiana-provider 内部可见，外部拿不到。

#[derive(Debug, Default)]
pub(crate) struct CapacityWindow {
    state: Mutex<WindowState>,
}

impl CapacityWindow {
/// 尝试在当前窗口里为一次调用预留配额。
///
/// 【作用】 三个动作合一：校验策略 → 判断/滚动窗口 → 扣减计数。
/// 这三步必须在**同一个锁持有期**内完成，否则并发下会超限。
///
/// 【调用者】 `transport.rs::send_inner_attempt`，在任何网络 I/O 之前。
///
/// 【输入】
/// - `policy`：服务端下发的容量策略（每分钟请求数、每分钟 token 数）。不可变、服务端所有。
/// - `now_unix_ms`：当前时间。由调用方注入，方便测试。
/// - `requested_tokens`：本次预计消耗的 token 数（调用方用预算上限 `max(1)` 传入）。
///
/// 【输出】 `Ok(())` 表示已扣减；`Err(String)` 是稳定错误码。
///
/// 【副作用】
/// 会修改本对象的窗口状态（这是它唯一的副作用，也是它存在的意义）。
/// **不**发网络请求、**不**写事件、**不**扣任何账户余额。
///
/// 【失败情况】
/// | 错误码 | 触发条件 |
/// |---|---|
/// | `policy.validate()` 的错误 | 策略自身不合法（0 上限等） |
/// | `provider_capacity_window_request_invalid` | `now_unix_ms == 0` 或 `requested_tokens == 0` |
/// | `provider_capacity_clock_rollback` | 当前时间早于本窗口起点（系统时钟被回拨） |
/// | `provider_capacity_window_overflow` | 窗口边界计算溢出 `u64` |
/// | `provider_capacity_quota_exhausted` | 请求数或 token 数已到上限 |
/// | `provider_capacity_requests_overflow` / `..._tokens_overflow` | 计数累加溢出 |
///
/// 【⚠ 为什么时钟回拨要报错而不是继续】
/// 如果系统时间从 10:05 被拨回 10:01，当前窗口看起来“还没用满”，配额就会被重复使用。
/// 这里选择直接失败：宁可这一次调用被拒，也不要在无法判断时间基准时放行。
    pub(crate) fn reserve(
        &self,
        policy: &ProviderCapacityPolicy,
        now_unix_ms: u64,
        requested_tokens: u64,
    ) -> Result<(), String> {
        // 策略先校验：策略是服务端下发的不可变数据，本地只消费不修改。
        // 先校验再动锁，避免为一个非法策略白拿一次锁。

        policy.validate()?;
        if now_unix_ms == 0 || requested_tokens == 0 {
            return Err("provider_capacity_window_request_invalid".to_owned());
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        // 时钟回拨检测。`end_unix_ms != 0` 表示“已经开过窗”，此时若 now 早于窗口起点，
        // 说明系统时间被往回拨了。放行会导致配额被重复消费，所以这里 fail-closed。
        //
        // ⚠ 注意这是**启发式**判断：正常的 NTP 微调不会触发它（微调幅度远小于一分钟），
        // 但如果有人在窗口进行中把时钟拨了 5 分钟，就会被这里挡住。

        if state.end_unix_ms != 0 && now_unix_ms < state.start_unix_ms {
            return Err("provider_capacity_clock_rollback".to_owned());
        }
        // 整数除法向下取整得到“第几分钟”，再乘回毫秒 → 得到本分钟窗口的起点。
        // 这样所有时刻自然落到同一个对齐网格上，跨进程/跨机器也一致。
        // checked_mul：万一时间戳大到乘法溢出，明确报错而不是 wrap 成一个很小的数。

        let start = (now_unix_ms / WINDOW_MS)
            .checked_mul(WINDOW_MS)
            .ok_or_else(|| "provider_capacity_window_overflow".to_owned())?;
        let end = start
            .checked_add(WINDOW_MS)
            .ok_or_else(|| "provider_capacity_window_overflow".to_owned())?;
        // 窗口滚动：两种情况需要开新窗——
        //   1) end_unix_ms == 0  → 从来没开过窗（首次调用）
        //   2) now >= end        → 上一窗已到期
        //
        // ⚠ 关键：整个赋值（重置计数）发生在持锁期间，所以并发请求不会各自
        // “看到旧窗口已过期”然后重复重置。
        // 窗口是**左闭右开** [start, end)：正好等于 end 的那一毫秒已经属于新窗口。

        if state.end_unix_ms == 0 || now_unix_ms >= state.end_unix_ms {
            *state = WindowState {
                start_unix_ms: start,
                end_unix_ms: end,
                requests: 0,
                tokens: 0,
            };
        }
        // 配额判定。用的是“**先判后加**”而不是“加完再比”：
        //   requests >= limit            → 请求数配额已满
        //   requested > limit - tokens    → token 配额不够
        //
        // 第二行用 saturating_sub 而不是 `limit - tokens`：
        // 万一 tokens 因某种原因已经大于 limit（正常不该发生），普通减法会下溢 panic，
        // saturating_sub 会得到 0，于是本行必然为真 → 拒绝。宁可多拒，不可 panic。
        //
        // ⚠ 这一段必须在锁内，否则并发请求会同时通过判定（见 CapacityWindow 的 Mutex 说明）。

        if state.requests >= policy.requests_per_minute
            || requested_tokens > policy.tokens_per_minute.saturating_sub(state.tokens)
        {
            return Err("provider_capacity_quota_exhausted".to_owned());
        }
        state.requests = state
            .requests
            .checked_add(1)
            .ok_or_else(|| "provider_capacity_requests_overflow".to_owned())?;
        state.tokens = state
            .tokens
            .checked_add(requested_tokens)
            .ok_or_else(|| "provider_capacity_tokens_overflow".to_owned())?;
        Ok(())
    }

/// 读出当前窗口的 `(start_ms, end_ms, requests, tokens)`，供诊断展示。
///
/// 【作用】 只读快照，用于 `/health` 之类的诊断投影与测试断言。
/// 【副作用】 只短暂加锁读取，不修改任何计数。
///
/// 【⚠ 返回值不是配额真相】
/// 它是**本进程**的记账视图，不是上游服务商的真实用量。
/// 真正的账单以上游返回的 usage 为准（见 `usage.rs`）。

    pub(crate) fn snapshot(&self) -> (u64, u64, u64, u64) {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        (
            state.start_unix_ms,
            state.end_unix_ms,
            state.requests,
            state.tokens,
        )
    }
}
