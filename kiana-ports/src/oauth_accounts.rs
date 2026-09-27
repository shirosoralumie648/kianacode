//! OAuth account persistence and lifecycle ports.
//!
//! Ports carry server-owned metadata only. Implementations may resolve raw credentials internally
//! at the provider effect boundary; this port never returns access/refresh token material.
//!
//! # 这个文件在系统里的位置
//!
//! Kiana 可以接入多个上游模型服务商（OpenAI、Anthropic、Gemini 等），
//! 每个服务商都需要一套账号凭证。本文件定义的是**存取这些账号元数据的接口**，
//! 不是凭证本身。
//!
//! ```text
//! 控制面（kiana-core）
//!        ↓ 需要读取/轮换账号状态
//! 【本文件：OAuthAccountStore trait】   ← 只声明接口，不含实现
//!        ↓ 由具体实现承接
//!    本地存储 / 数据库适配器
//! ```
//!
//! # 关键设计：这一层绝不返回 token 原文
//!
//! 端口（port）只搬运**服务端自有的元数据**（metadata）—— 账号 ID、
//! 作用域、过期时间、修订号。真正的 access token / refresh token 原文
//! 由具体实现在**触及服务商效果的那一层**（provider effect boundary）内部解析。
//!
//! 为什么要这么设计：如果 token 原文会流经这个 trait，那么任何实现了
//! 它的适配器、任何日志打印、任何错误信息都可能把它泄漏出去。
//! 把它挡在端口之外，等于把"能碰到密钥的代码"压缩到最小面积。
//!
//! # 术语
//!
//! - **CAS（compare-and-swap，比较并交换）**：先检查当前值是否等于我以为的值，
//!   只有相等才写入。本文件的四个写方法都带 `observed_revision` / `observed_generation`
//!   参数，全部是 CAS 守卫。
//! - **revision（修订号）**：记录本身的版本号，任何一次成功写入都递增。
//! - **generation（世代号）**：凭证的"代"。轮换（rotation）、重新认证、吊销
//!   都会让它递增，用来识别"过期在途请求"。
//! - **fail-closed**：证据不足时拒绝，而非猜测放行。
//! - **Port（端口）**：架构中的抽象边界 —— 由核心层定义接口、由外层提供实现，
//!   这样核心层就不必依赖具体的存储技术。
//!
//! # 上游契约
//!
//! Ports carry server-owned metadata only. Implementations may resolve raw credentials internally
//! at the provider effect boundary; this port never returns access/refresh token material.

use crate::PortError;
use async_trait::async_trait;
use kiana_domain::{OAuthAccountRecord, OAuthTokenMetadata, ProviderAccountId};

/// 本端口的 schema 标识符。
///
/// 【作用】
/// 标记这份接口契约的版本。`v1` 是第一版。
///
/// 【为什么需要它】
/// 端口是可能被替换实现的（换存储后端、换服务商）。有了 schema 字符串，
/// 实现方可以在启动时校验"我实现的到底是哪一版契约"，避免版本错配时
/// 出现难以排查的行为差异 —— 比如新增了字段但旧实现没填，读出来是空的。
pub const OAUTH_ACCOUNT_STORE_PORT_SCHEMA: &str = "kiana.oauth-account-store-port.v1";

/// OAuth 账号的读取与生命周期变更接口。
///
/// 【作用】
/// 定义控制面可以怎样查询和修改一个服务商账号的元数据状态。
///
/// 【调用者】
/// `kiana-core`（控制面）在准备向上游服务商发起请求、或在处理账号失效时调用。
/// 上层（CLI / web workbench）不直接调用本 trait —— 按项目的架构约束，
/// 所有状态变更必须经过控制面。
///
/// 【为什么是 async】
/// 实现几乎必然要落盘或访问数据库。`#[async_trait]` 把 trait 方法变成
/// 返回 Future，从而可以用 `.await` 而不必阻塞执行器线程。
///
/// 【全局不变量】
/// 四个写方法全部带 CAS 守卫参数，这是本 trait 最重要的设计约束，
/// 下面每个方法都有详细说明。
#[async_trait]
pub trait OAuthAccountStore: Send + Sync {
    /// 按账号 ID 读取一条账号记录。
    ///
    /// 【输出】
    /// `Ok(Some(record))` 找到；`Ok(None)` 表示该 ID 不存在。
    ///
    /// 【为什么用 `Option` 而不是直接返回记录】
    /// "不存在"是一个**正常的业务结果**，不是错误 —— 上游调用者需要区分
    /// "这个账号从没配过"和"存储层出故障了"这两种情况，它们的处理方式不同。
    /// 如果把"不存在"表达成 `Err`，调用方就不得不用错误类型来表达业务状态，
    /// 那会让真正的存储故障被淹没。
    async fn read_oauth_account(
        &self,
        account_id: ProviderAccountId,
    ) -> Result<Option<OAuthAccountRecord>, PortError>;

    /// Insert a new record or idempotently replay the same digest. Existing records require an
    /// exact expected revision; callers cannot overwrite a newer generation.
    ///
    /// 【作用】
    /// 插入一条新记录，或者**幂等地**（idempotently）重放同一份内容。
    ///
    /// 【为什么这个方法同时承担"插入"和"更新"两种语义】
    /// 因为"幂等重放"是分布式系统的基本要求：同一个请求因为网络重试被发送两次时，
    /// 第二次必须安全地失败或安全地返回已有结果，而不能变成一次意外的覆盖写入。
    /// 拆成 insert / update 两个方法，调用方就得自己判断走哪条路，判断错了就出事。
    ///
    /// 【输入】
    /// - `account`：要写入的记录。
    /// - `expected_revision`：`None` 表示"这是一条全新记录，必须不存在"。
    ///   `Some(n)` 表示"我看到的是第 n 版，请仅当当前仍是第 n 版时才写入"。
    ///
    /// 【为什么 `expected_revision` 是 `Option`】
    /// 它同时表达了两种意图，因为这两种意图的检查逻辑根本不同：
    ///
    /// - `None` → 必须**当前不存在**。如果已经存在，说明有人抢先写入了，应当拒绝。
    /// - `Some(n)` → 必须**当前恰好是第 n 版**。如果当前已经是第 n+1 版，
    ///   说明有别人在你之后改过，你的写入基于过时的认知，必须拒绝。
    ///
    /// 这就是那句话 "callers cannot overwrite a newer generation" 的含义：
    /// **你不能用旧认知覆盖更新的数据。** 没有这个检查，两个并发的运维操作
    /// 会互相覆盖，后写的那个还带着过时的内容。
    ///
    /// 【失败情况】
    /// CAS 条件不满足时返回 `Err`，由调用方决定是重读还是放弃。
    async fn upsert_oauth_account(
        &self,
        account: OAuthAccountRecord,
        expected_revision: Option<u64>,
    ) -> Result<OAuthAccountRecord, PortError>;

    /// Compare-and-swap both the account revision and credential generation before rotation.
    ///
    /// 【作用】
    /// 轮换（rotation）账号的凭证 —— 通常是旧 refresh token 过期后换发新的一对。
    ///
    /// 【核心流程】
    /// 1. 同时检查**修订号**和**世代号**是否都等于调用方观测到的值；
    /// 2. 都相等 → 写入 `next_metadata`，两个计数器各自递增；
    /// 3. 任一不等 → 拒绝。
    ///
    /// 【为什么要同时比对两个编号，而不是只看一个】
    /// 两者防的是**不同的竞态**：
    ///
    /// - `observed_revision` 防的是"记录本身被改过" —— 比如有人改了账号的作用域。
    ///   如果只比 revision 就能通过，说明有人在你之后动过这条记录。
    /// - `observed_generation` 防的是"凭证被换过" —— 即使记录其他字段没变，
    ///   凭证也可能是上一轮并发刷新已经轮换过了。继续用旧的写，会把新凭证
    ///   覆盖回旧的，导致账号反复掉线。
    ///
    /// 两者都是必要的：只看其中一个都会留下一个可被利用的窗口。
    ///
    /// 【输入】
    /// - `observed_revision` / `observed_generation`：调用方读到的当前值，即 CAS 的比较基准。
    /// - `next_metadata`：要写入的新凭证元数据。
    /// - `now_unix_ms`：当前时间（毫秒），由调用方传入而不是在实现里取。
    ///   为什么这样？为了让实现是**纯函数式的、可确定性测试的** ——
    ///   测试时传入固定时间戳，就能断言"过期时间被写成 X"，
    ///   而不必去 mock 系统时钟。
    ///
    /// 【失败情况】
    /// 任一 CAS 条件不满足即拒绝。调用方应当**重读后重试**，
    /// 而不是强行覆盖 —— 后者会把另一个并发操作的结果抹掉。
    async fn rotate_oauth_account(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        next_metadata: OAuthTokenMetadata,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError>;

    /// Reauthentication is a generation fence. An old in-flight refresh cannot restore a record
    /// after this operation commits.
    ///
    /// 【作用】
    /// 标记这个账号**必须重新认证**（credentials are no longer trustworthy）。
    ///
    /// 【关键设计：这是一道 generation fence（世代栅栏）】
    /// 栅栏的意思是：一旦本操作提交，**之前正在飞行中的凭证刷新就失效了**。
    ///
    /// 考虑这样一个竞态：
    ///
    /// ```text
    /// 时刻 t1：某处发起 token 刷新（网络往返，可能耗时几秒）
    /// 时刻 t2：另一个流程判定该账号必须重新认证，调用本方法，generation → 5
    /// 时刻 t3：t1 那次慢悠悠的刷新回来了，带着一份"看起来有效"的旧凭证
    /// ```
    ///
    /// 如果 t3 被接受，账号就"复活"了 —— 一个已被判定为不可信的账号
    /// 又重新可用，而 t2 的判定被静默抹掉。这正是"旧的在途刷新
    /// 不能在本操作提交后恢复这条记录"（an old in-flight refresh cannot
    /// restore a record after this operation commits）要防的事。
    ///
    /// 机制是：本方法让 `generation` 递增。t3 那次刷新是带着
    /// generation=4 出发的，它回来时必须以 4 为条件做 CAS 写入；
    /// 而当前已经是 5，CAS 失败，刷新被丢弃。
    ///
    /// ⚠ 不要把这里的 generation 检查去掉，也不要改成"总是覆盖"。
    /// 去掉之后，判定为需要重新认证的账号会被一个过期的在途刷新悄悄复活，
    /// 而外部看不到任何异常 —— 这类 bug 极难排查。
    ///
    /// 【失败情况】
    /// CAS 条件不满足（说明账号在这期间又变了）即拒绝。
    async fn require_oauth_reauth(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError>;

    /// Revocation increments the generation and account revision atomically.
    ///
    /// 【作用】
    /// 吊销（revoke）一个账号 —— 凭证已泄露、或用户主动解绑、或服务商侧已失效。
    ///
    /// 【"原子地"（atomically）为什么重要】
    /// `generation` 和 `revision` 必须**一起递增**。如果分两次写：
    ///
    /// ```text
    /// 写 revision = 6   ← 成功
    ///         ↓ 此刻崩溃 / 此刻有并发读
    /// 写 generation = 5 ← 还没执行
    /// ```
    ///
    /// 中间这个窗口里，账号处于"看起来修订过了、但世代没变"的自相矛盾状态。
    /// 一个在途刷新可能只检查 generation，于是它会认为自己仍然有效，
    /// 把凭证写回去 —— 吊销被绕过。原子递增杜绝了这个窗口。
    ///
    /// 【吊销后会发生什么】
    /// 有了新的 generation，所有在途的旧刷新都会 CAS 失败。
    /// 调用方（控制面）应据此拒绝继续使用该账号。
    ///
    /// 【失败情况】
    /// 同其他写方法，CAS 不满足即拒绝。
    async fn revoke_oauth_account(
        &self,
        account_id: ProviderAccountId,
        observed_revision: u64,
        observed_generation: u64,
        now_unix_ms: u64,
    ) -> Result<OAuthAccountRecord, PortError>;
}
