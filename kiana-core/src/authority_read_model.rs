//! 把「授权相关的事实」投影成一张**只读查询表**：Cell（执行单元）、Grant（能力授予）、
//! Budget（预算租约）、Lease（存储锁租约）当前各自处于什么状态。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! 已提交的 RuntimeEvent（唯一事实来源，append-only）
//!        ↓  【本文件：纯折叠，不写任何东西】
//! AuthorityReadModel（可删除、可重建的查询投影）
//!        ↓
//! 授权判定 / UI 展示 / 审计查询（「这个 grant 现在还有效吗？」）
//! ```
//!
//! **上游**：`kiana-eventlog` 里已提交的事件流。调用方通常是控制面或查询面，
//! 把一段事件喂进来换一张当前授权状态表。
//! **下游**：本文件**不调用任何东西**——没有文件 I/O、没有网络、没有事件写入，
//! 没有第二套状态存储。
//!
//! # 最重要的一件事：这是投影，不是事实
//!
//! 「投影（projection）」= 从事实推导出来的、**可以随时删掉重建**的视图。
//! EventLog 才是事实。这张表被删了，重跑一次投影就回来了；EventLog 里的事件被删了，
//! 系统就真的丢历史了。所以：
//!
//! - 想确认「真相」，去查 EventLog，不要查这张表；
//! - 想**修改**授权状态，**绝对不能**改这张表，只能追加新事件让投影重新折叠。
//!
//! ⚠ 如果你在别处看到「直接更新 read model 里的 grant 状态」这种代码，那一定是 bug。
//!
//! # 为什么授权状态要用「世代（epoch）」来防回滚
//!
//! `authority_epoch` 是一个**只增不减**的整数，标记「这批授权事实属于第几代」。
//! 它防的是这样一种攻击：有人重放了一批**旧的**授权事件（或者从旧备份里恢复），
//! 让一个已经被撤销的 grant 重新显得有效。
//!
//! 判定规则在折叠循环里：只要发现某条事件的 epoch **小于**已经见过的最大 epoch，
//! 立刻返回 `EpochRollback` 拒绝——因为事件流是按提交顺序给的，后面出现更小的世代
//! 意味着有人在倒带。
//!
//! # 数据流
//!
//! ```text
//! events: [RuntimeEvent]
//!    ↓ 逐条去重（seen.insert）
//!    ↓ 按 kind 前缀分派：cell.* / grant.* / budget.* / lease.* / model.reserved|settled
//!    ↓ 每类写进自己的 BTreeMap（同 key 后写覆盖先写 = 最后一次事件说了算）
//!    ↓ 收尾做交叉校验（子 Cell 的父必须存在、结算必须有预留、释放必须有签发）
//! AuthorityReadModel { cells, grants, budgets, leases }
//!    ↓ validate()
//! ```
//!
//! # 用 BTreeMap 而不是 HashMap 的原因
//!
//! 因为 `validate()` 要求每个列表**按 id 严格升序**。用 `BTreeMap` 天然有序，
//! 输出顺序与事件顺序、哈希随机种子都无关——**同样的事件永远折叠出同样的字节序列**。
//! 这对 digest、对账、回放比对是硬要求。

use kiana_domain::{
    BudgetLeaseId, CapabilityGrantId, CellId, CellLifecycle, RuntimeEvent, StorageLockId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// 这张投影表的 schema 标识，带 `.v1` 版本号。
///
/// **为什么 schema 字符串里就写死版本**：投影会被持久化、会被跨进程传递、会被旧版本代码读到。
/// 读方第一件事就是比对这一个字符串，不匹配就整张表拒收——这样「用新代码读旧投影」会在读取入口
/// 失败，而不是在某个字段上悄悄按错误语义解释。
pub const AUTHORITY_READ_MODEL_SCHEMA: &str = "kiana.persistence-authority-read-model.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// 单个 Cell（执行单元，例如一个 worker、一次受限任务）的授权投影。
///
/// 【字段业务含义】
/// - `cell_id`：Cell 的稳定标识，跨事件引用同一个 Cell 靠它；
/// - `parent_cell_id`：**可为空的父 Cell**。`None` 表示这是一个根 Cell。
///   为什么允许为空：组织结构本身有根。如果强制非空，就没法表达「顶层 Cell」；
///   如果允许任意值，`Some(不存在的 id)` 就会造出悬空引用——所以折叠收尾时会强制校验父存在；
/// - `lifecycle`：Cell 的生命周期状态（创建/运行/取消中/阻塞/隔离/失败/退役等）；
/// - `authority_epoch`：这条状态所属的授权世代；
/// - `fenced`：**布尔值含义**——`true` 表示这个 Cell 已经被「围栏（fenced）」，
///   也就是**被判定为不可再发起新动作**。它是由 `lifecycle` 推导出来的派生字段，
///   不是独立事实：处于取消中/阻塞/隔离/失败/退役的 Cell 一律 `true`。
///   写成派生字段而不是每次现算，是为了让读方不必知道完整状态表也能安全判断。
pub struct CellAuthorityProjection {
    pub cell_id: CellId,
    pub parent_cell_id: Option<CellId>,
    pub lifecycle: CellLifecycle,
    pub authority_epoch: u64,
    pub fenced: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// 单个能力授予（Capability Grant）的授权投影。
///
/// 【`state` 是什么】
/// 一个**字符串**而不是枚举——因为授予状态属于 wire 契约，可能随协议演进增加新取值。
/// 枚举会强迫所有读方同时升级；字符串允许旧代码见到未知取值时走「不认识就拒绝」的保守分支。
/// 代价是类型系统不再帮你兜底，所以下面 `validate()` 与调用方必须把它当不可信输入处理。
///
/// 【`expires_at_unix_ms` 为 0 的含义】
/// 约定俗成的「**永不过期**」。用 0 而不是 `Option` 是为了让 wire 形状保持扁平稳定。
/// ⚠ 读方必须显式处理这个特例：把 0 当成一个真实时间点去比较，会得出「1970 年就过期了」
/// 这种荒谬结论，从而拒绝掉一批本来有效的 grant。
pub struct GrantAuthorityProjection {
    pub grant_id: CapabilityGrantId,
    pub state: String,
    pub authority_epoch: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// 单个预算租约的授权投影：只记**次数**，不记金额。
///
/// 【为什么只有计数没有金额】
/// 金额结算是账本（`kiana-ports` 的 ledger / cost ledger）的事，这里刻意不复制一份。
/// 授权投影要回答的是「这个租约名下还有没有**没结清**的预留」，而账本要回答「花了多少钱」。
/// 把金额也抄进来，就等于制造了第二个可能与账本不一致的金额来源——这正是仓库宪法
/// 禁止的「第二个事实源」。
///
/// 【`reservations` 与 `settlements` 的关系】
/// 正常情况下 `reservations >= settlements`，差额就是**当前未结清的预留数**。
/// 每次 `model.reserved` / `budget.reserved` 让前者 +1，每次 `...settled` 让后者 +1，
/// 同时把这条预留从「未结清集合」里移除。出现「结算了但没有对应预留」会被直接拒绝
/// （`SettlementWithoutReservation`），因为那意味着账目凭空多出一笔支出。
pub struct BudgetAuthorityProjection {
    pub budget_lease_id: BudgetLeaseId,
    pub reservations: u64,
    pub settlements: u64,
    pub authority_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// 单个存储锁租约（Storage Lock Lease）的授权投影。
///
/// 【`active` 与 `fenced` 的区别】
/// - `active = true`：这个租约当前**持有**着锁，别人拿不到；
/// - `fenced = true`：这个租约被**作废**了，即便曾经 active 也不再产生效力。
///
/// 两者不是反义词：一个租约可以既不 active 也被 fenced（正常释放路径），
/// 也可以 active 且未被 fenced（正常持有）。真正危险的是「fenced 了却还被当成 active 使用」，
/// 所以折叠时 `lease.released` / `lease.fenced` 事件会把 `active` 置为 `false`。
///
/// 【为什么 `lease_id` 是 `StorageLockId`】
/// 因为这里的租约是**存储锁**租约，不是能力授予、也不是任务租约。三者容易混淆：
/// 授予回答「你能不能做」，预算租约回答「你还能花多少」，存储锁租约回答「你占着哪把锁」。
pub struct LeaseAuthorityProjection {
    pub lease_id: StorageLockId,
    pub active: bool,
    pub fenced: bool,
    pub authority_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// 一张完整的授权读模型（read model）——**四张表 + 它们的来源凭据**。
///
/// 【`source_cursor` 与 `source_event_ids` 为什么必须在表里】
/// 因为投影必须能回答「我这份视图是从哪来的、基于到哪一条事实」。
/// - `source_cursor`：投影覆盖到的 EventLog 位置。0 是非法值（见 `validate`），
///   因为「没覆盖任何事实的投影」和「投影失败」在语义上无法区分；
/// - `source_event_ids`：实际被折叠进去的事件 id 列表，**不允许重复**。
///
/// 有了这两个字段，调用方才能做「投影是否落后于事实」的判断（落后了要重算），
/// 也才能在审计时回答「你给我看的这个授权结论，是基于哪几条事件得出的」。
///
/// 【⚠ 再次强调】
/// 拿到这张表**不等于**拿到授权。要做授权判定，仍然必须把结论回到 ControlPlane。
pub struct AuthorityReadModel {
    pub schema: String,
    pub source_cursor: u64,
    pub source_event_ids: Vec<kiana_domain::EventId>,
    pub authority_epoch: u64,
    pub cells: Vec<CellAuthorityProjection>,
    pub grants: Vec<GrantAuthorityProjection>,
    pub budgets: Vec<BudgetAuthorityProjection>,
    pub leases: Vec<LeaseAuthorityProjection>,
}

impl AuthorityReadModel {
    /// 自检这张投影表自身是否形状合法。
    ///
    /// 【作用】
    /// 检查三件事：表头/schema 与版本、来源凭据是否非空且不重复、四个列表是否严格按 id 升序。
    ///
    /// 【调用者】
    /// 1. 折叠函数收尾时（`project_authority_read_model` 的最后一步）；
    /// 2. 任何从磁盘/网络读回这张表的代码——**反序列化不等于可信**。
    ///
    /// 【输入】
    /// `&self`，纯读取，不修改。
    ///
    /// 【输出】
    /// `Ok(())` 或三条稳定错误码：
    /// - `authority_read_model_header_invalid`：schema 不对，或 `source_cursor`/`authority_epoch` 为 0，
    ///   或事件列表为空；
    /// - `authority_read_model_event_duplicate`：同一个事件 id 出现两次；
    /// - `authority_read_model_order_invalid`：某个列表没有按 id 升序。
    ///
    /// 【副作用】
    /// 无。
    ///
    /// 【为什么必须检查顺序】
    /// 因为这张表会被算 digest。两个内容相同但顺序不同的表，digest 不一样，
    /// 就不可能判断「它们是不是同一份结论」。顺序检查把「顺序」从隐含约定变成硬约束。
    ///
    /// 【为什么 `windows(2)` 这种写法】
    /// `windows(2)` 会把所有相邻元素对交给闭包，一次遍历检查整个序列是否单调。
    /// 比「逐个和前一个比」少写一半代码，而且不会漏掉最后一段。
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUTHORITY_READ_MODEL_SCHEMA
            || self.source_cursor == 0
            || self.authority_epoch == 0
            || self.source_event_ids.is_empty()
        {
            return Err("authority_read_model_header_invalid".to_owned());
        }
        if self
            .source_event_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .len()
            != self.source_event_ids.len()
        {
            return Err("authority_read_model_event_duplicate".to_owned());
        }
        if self
            .cells
            .windows(2)
            .any(|pair| pair[0].cell_id > pair[1].cell_id)
            || self
                .grants
                .windows(2)
                .any(|pair| pair[0].grant_id > pair[1].grant_id)
            || self
                .budgets
                .windows(2)
                .any(|pair| pair[0].budget_lease_id > pair[1].budget_lease_id)
            || self
                .leases
                .windows(2)
                .any(|pair| pair[0].lease_id > pair[1].lease_id)
        {
            return Err("authority_read_model_order_invalid".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
/// 投影失败时的**分类错误**。
///
/// 【为什么用 `thiserror` 而不是字符串】
/// 因为调用方需要**按类别**反应：遇到 `EpochRollback` 应该拒绝并报警（可能有人在倒带），
/// 遇到 `CellInvalid(..)` 可能只是某条事件形状不对。字符串只能靠前缀匹配，脆得很。
/// `#[derive(thiserror::Error)]` 让每个变体自带一句可读描述，`#[error("...")]` 是给人看的。
///
/// 【每个变体对应一类现实故障】
/// - `SourceInvalid`：输入根本不是一张合法的来源（空、cursor 为 0、投影自身校验不过）；
/// - `EpochRollback`：**倒带**——后面出现的授权世代比前面小，最危险的一种；
/// - `EpochStale`：某条该带 epoch 的事件没带，或带的是 0；
/// - `CellInvalid` / `GrantInvalid` / `BudgetInvalid` / `LeaseInvalid(..)`：各对象的事件形状不合法；
/// - `ChildParentMissing`：引用了不存在的父 Cell（悬空组织结构）；
/// - `SettlementWithoutReservation`：**结算没有对应预留**，账目凭空多出一笔。
///
/// 【⚠ 注意】
/// 这些错误**只**表示「投影不出来」。它**不代表**系统安全——恰恰相反，
/// 拒绝投影意味着调用方拿不到授权视图，按 fail-closed 原则应当拒绝继续，而不是「先放行再说」。
pub enum AuthorityProjectionError {
    #[error("authority_source_invalid")]
    SourceInvalid,
    #[error("authority_epoch_rollback")]
    EpochRollback,
    #[error("authority_epoch_stale")]
    EpochStale,
    #[error("authority_cell_invalid:{0}")]
    CellInvalid(String),
    #[error("authority_grant_invalid:{0}")]
    GrantInvalid(String),
    #[error("authority_budget_invalid:{0}")]
    BudgetInvalid(String),
    #[error("authority_lease_invalid:{0}")]
    LeaseInvalid(String),
    #[error("authority_child_parent_missing")]
    ChildParentMissing,
    #[error("authority_settlement_without_reservation")]
    SettlementWithoutReservation,
}

/// 从事件的 `data`（JSON 对象）里取出**第一个存在**的候选字段，反序列化成目标 ID 类型。
///
/// 【调用者】
/// 折叠函数里所有「从事件里取 id」的地方（cell_id / grant_id / budget_lease_id / lease_id）。
///
/// 【输入】
/// - `data`：事件的 JSON 载荷；
/// - `fields`：**候选字段名列表**，按优先级排列。例如租约 id 传
///   `["lease_id", "resource_lease_id", "storage_lock_id"]`。
/// - 返回类型 `T`：任何能从 JSON 反序列化的类型（这里是各种 `*Id`）。
///
/// 【输出】
/// 成功返回解析出的 ID；一个字段都找不到、或找到了但解析失败，都返回 `SourceInvalid`。
///
/// 【为什么要支持多个候选字段名】
/// 因为事件形状在不同来源/版本里不完全一致（有的写 `lease_id`，有的写更啰嗦的
/// `resource_lease_id`）。如果没有这个 helper，每处都得手写一串 `or_else`，而且**很容易漏**
/// ——漏掉的那处会变成一个永远走不到的错误分支。
///
/// 【⚠ 重要边界】
/// 「字段不存在」和「字段存在但值不合法」在这里被合并成同一个错误。这是**有意的**：
/// 对调用方来说，这两种情况的处理方式一样（拒绝该事件），区分它们只会让错误码膨胀。
/// 需要区分的场景（比如「lease 没有签发记录」）由调用方在拿到 `Err` 之后**自己补上下文**，
/// 例如 `map_err(|_| LeaseInvalid("lease_id_missing"))`——这正是各调用点传不同文案的原因。
///
/// 【泛型写法说明】
/// `T: for<'de> Deserialize<'de>` 是生命周期泛型写法，意思是「T 能在任意生命周期的
/// JSON 输入上被反序列化」。写成 `T: DeserializeOwned` 也能表达同样的意思，
/// 这里是仓库里的既有风格。
fn id<T: for<'de> Deserialize<'de>>(
    data: &Value,
    fields: &[&str],
) -> Result<T, AuthorityProjectionError> {
    fields
        .iter()
        .find_map(|field| data.get(*field))
        .ok_or_else(|| AuthorityProjectionError::SourceInvalid)
        .and_then(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| AuthorityProjectionError::SourceInvalid)
        })
}

/// 取出事件里的授权世代（`authority_epoch`）。
///
/// 【为什么要求 `> 0`】
/// 因为 0 是「没有世代」的哨兵值。如果允许 0 混进来，`max()` 计算出来的最大世代就会是 0，
/// 而后续所有「epoch 必须大于 0」的校验都会把这份投影整张拒掉——错误会出现在离原因很远的地方。
/// 在入口处就拒绝，错误才指向真正的来源。
///
/// 【失败情况】
/// 字段缺失、不是数字、或等于 0 → `EpochStale`（注意：不是 `SourceInvalid`，
/// 因为「缺 epoch」是一个语义问题，不是「事件形状完全不对」）。
fn epoch(event: &RuntimeEvent) -> Result<u64, AuthorityProjectionError> {
    event
        .data
        .get("authority_epoch")
        .and_then(Value::as_u64)
        .filter(|epoch| *epoch > 0)
        .ok_or(AuthorityProjectionError::EpochStale)
}

/// 取出事件的 Cell 生命周期状态，**同时兼容两种字段名**。
///
/// 【为什么兼容 `lifecycle` 和 `state`】
/// 早期事件写 `state`，后期写 `lifecycle`。这是协议演进留下的历史包袱。
/// 与其强行迁移历史事件（那需要重写已提交的事实，违反 append-only），不如在读的时候容忍两种。
/// `or_else` 的顺序是「新字段优先」：一旦有 `lifecycle` 就不再看 `state`。
///
/// 【⚠ 这是一个妥协，不是设计优点】
/// 容忍两种字段名意味着**同一事件可能同时带两个且值不同**。当前实现取 `lifecycle`，
/// 静默忽略 `state` 的分歧。如果要收紧，正确做法是让事件生产方保证只有一个字段，
/// 并对「两个都出现且不一致」新增一条拒绝——属于后续工作。
///
/// 【失败情况】
/// 两个字段都没有 → `CellInvalid("lifecycle_missing")`；
/// 有但不是合法枚举值 → `CellInvalid("lifecycle_invalid")`。
/// 区分开是因为「没写」和「写错了」要修的地方不一样。
fn lifecycle(event: &RuntimeEvent) -> Result<CellLifecycle, AuthorityProjectionError> {
    event
        .data
        .get("lifecycle")
        .or_else(|| event.data.get("state"))
        .ok_or_else(|| AuthorityProjectionError::CellInvalid("lifecycle_missing".to_owned()))
        .and_then(|value| {
            serde_json::from_value(value.clone())
                .map_err(|_| AuthorityProjectionError::CellInvalid("lifecycle_invalid".to_owned()))
        })
}

/// 把一段已提交事件**折叠**成一张授权读模型。
///
/// 【作用】
/// 本文件唯一的主入口。它是**纯函数**：同样的输入永远得到同样的输出，
/// 不写文件、不发事件、不改任何全局状态。
///
/// 【调用者】
/// 控制面/查询面在需要「当前授权状态表」时调用。典型场景：某个工具调用到来之前，
/// 先查这张表确认对应的 grant 还没过期、对应 Cell 没被 fenced。
///
/// 【输入】
/// - `events`：已提交的事件切片，**必须按提交顺序**排列（折叠是顺序敏感的）；
/// - `source_cursor`：调用方声称这些事件覆盖到的 EventLog 位置；
/// - `now_unix_ms`：当前时间。⚠ 当前实现**只做校验（不允许为 0）**，
///   并不用它做过期判定——过期判断由调用方拿着 `expires_at_unix_ms` 自己做。
///   这一点必须写清楚，否则读者会以为这里已经做了时间过滤。
///
/// 【输出】
/// 成功返回 `AuthorityReadModel`；失败返回 `AuthorityProjectionError`，且**必须**被调用方
/// 当作「拒绝」处理（fail-closed），不能降级成「空表 = 全部允许」。
///
/// 【核心流程】
/// 1. 入参体检：cursor 为 0、时间为 0、事件为空 → `SourceInvalid`；
/// 2. 逐条事件：先去重（同一 `event_id` 只算一次），再按 `kind` 前缀分派；
/// 3. 每处理一条带 epoch 的事件，就更新全局最大世代，并检查是否倒带；
/// 4. 收尾交叉校验：子 Cell 的父必须存在；
/// 5. 调用 `validate()` 做表头/顺序自检，通过才返回。
///
/// 【为什么用 `BTreeMap` 累加而不是 `HashMap`】
/// 见文件头说明：输出必须与输入顺序、哈希种子无关，才能算稳定 digest。
///
/// 【为什么重复事件是「跳过」而不是「拒绝」】
/// 因为重放同一批事件是**正常的恢复路径**（replay）。折叠必须幂等：
/// 同一条事件出现两次，结果必须和出现一次一样。如果这里报错，恢复流程会在重启时炸掉。
///
/// 【⚠ 不要把这个函数接成第二条授权路径】
/// 它只回答「现在是什么状态」，不回答「你能不能做」。后者是 ControlPlane 的事。
pub fn project_authority_read_model(
    events: &[RuntimeEvent],
    source_cursor: u64,
    now_unix_ms: u64,
) -> Result<AuthorityReadModel, AuthorityProjectionError> {
    if source_cursor == 0 || now_unix_ms == 0 || events.is_empty() {
        return Err(AuthorityProjectionError::SourceInvalid);
    }
    let mut source_event_ids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut authority_epoch = 0;
    let mut cells = BTreeMap::<CellId, CellAuthorityProjection>::new();
    let mut grants = BTreeMap::<CapabilityGrantId, GrantAuthorityProjection>::new();
    let mut budgets = BTreeMap::<BudgetLeaseId, BudgetAuthorityProjection>::new();
    let mut leases = BTreeMap::<StorageLockId, LeaseAuthorityProjection>::new();
    let mut reservations = BTreeSet::<(BudgetLeaseId, String)>::new();

    for event in events {
        // 事件去重（fold 幂等）。
        //
        // `BTreeSet::insert` 在元素已存在时返回 false——这里利用这一点同时完成
        // 「判重」和「记录」两件事，不额外查一次哈希。
        //
        // ⚠ 为什么重复要**跳过**而不是报错：崩溃重启后的重放会把同一批事件再喂一遍，
        // 折叠必须给出和第一次完全相同的结果。改成报错，恢复流程会在最需要它的时候炸掉。
        if !seen.insert(event.event_id) {
            continue;
        }
        source_event_ids.push(event.event_id);
        let event_epoch = match event.kind.as_str() {
            kind if kind.starts_with("cell.")
                || kind.starts_with("grant.")
                || kind.starts_with("lease.")
                || kind.starts_with("budget.")
                || matches!(kind, "model.reserved" | "model.settled") =>
            {
                Some(epoch(event)?)
            }
            _ => None,
        };
        if let Some(event_epoch) = event_epoch {
            // 授权世代（authority epoch）倒带检查。
            //
            // 事件是按提交顺序给的，所以「后面出现一个更小的 epoch」只有一种解释：
            // 有人重放了旧事实，或从旧备份恢复了一段历史。这会让一个已经被撤销的
            // grant 重新显得有效——属于授权事故，不是数据噪音。
            //
            // 因此这里**立即失败**，而不是「取更大的那个继续」。
            // 失败之后调用方应当拒绝服务，而不是拿半张表接着算。
            if authority_epoch > event_epoch {
                return Err(AuthorityProjectionError::EpochRollback);
            }
            authority_epoch = authority_epoch.max(event_epoch);
        }

        if event.kind.starts_with("cell.") {
            let cell_id: CellId = id(&event.data, &["cell_id"])
                .map_err(|_| AuthorityProjectionError::CellInvalid("cell_id_missing".to_owned()))?;
            let parent_cell_id = event
                .data
                .get("parent_cell_id")
                .and_then(|value| serde_json::from_value(value.clone()).ok());
            let state = lifecycle(event)?;
            // 由生命周期状态推导「是否已被围栏（fenced）」。
            //
            // 这五个状态（取消中 / 阻塞 / 隔离 / 失败 / 退役）有一个共同点：
            // **都不应该再发起新的有后果动作**。与其让每个读方各自记住这张状态表，
            // 不如在这里一次性推导成 `fenced: bool`。
            //
            // ⚠ 注意这是**派生字段**，不是独立事实。真正的事实是 `lifecycle`；
            // 如果有人直接改投影里的 `fenced`，投影就不再可信——
            // 所以这张表永远只能由折叠重新生成，不能手工编辑。
            let fenced = matches!(
                state,
                CellLifecycle::CancelRequested
                    | CellLifecycle::Blocked
                    | CellLifecycle::Quarantined
                    | CellLifecycle::Failed
                    | CellLifecycle::Retired
            );
            cells.insert(
                cell_id,
                CellAuthorityProjection {
                    cell_id,
                    parent_cell_id,
                    lifecycle: state,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                    fenced,
                },
            );
        } else if event.kind.starts_with("grant.") {
            let grant_id: CapabilityGrantId = id(&event.data, &["capability_grant_id", "grant_id"])
                .map_err(|_| {
                    AuthorityProjectionError::GrantInvalid("grant_id_missing".to_owned())
                })?;
            let state = event
                .data
                .get("state")
                .or_else(|| event.data.get("status"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| match event.kind.as_str() {
                    "grant.revoked" => "revoked",
                    "grant.expired" => "expired",
                    _ => "active",
                })
                .to_owned();
            let expires_at_unix_ms = event
                .data
                .get("expires_at_unix_ms")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX);
            if expires_at_unix_ms <= now_unix_ms && state == "active" {
                return Err(AuthorityProjectionError::GrantInvalid(
                    "grant_expired_as_active".to_owned(),
                ));
            }
            grants.insert(
                grant_id,
                GrantAuthorityProjection {
                    grant_id,
                    state,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                    expires_at_unix_ms,
                },
            );
        } else if matches!(event.kind.as_str(), "model.reserved" | "budget.reserved") {
            let budget_lease_id: BudgetLeaseId = id(&event.data, &["budget_lease_id"])
                .or_else(|_| {
                    id(
                        event.data.get("reservation_fact").unwrap_or(&Value::Null),
                        &["budget_lease_id"],
                    )
                })
                .map_err(|_| {
                    AuthorityProjectionError::BudgetInvalid("budget_lease_id_missing".to_owned())
                })?;
            let reservation_id = event
                .data
                .get("reservation_id")
                .or_else(|| {
                    event
                        .data
                        .get("reservation_fact")
                        .and_then(|value| value.get("reservation_id"))
                })
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| event.event_id.to_string());
            if !reservations.insert((budget_lease_id, reservation_id)) {
                return Err(AuthorityProjectionError::BudgetInvalid(
                    "reservation_duplicate".to_owned(),
                ));
            }
            let entry = budgets
                .entry(budget_lease_id)
                .or_insert(BudgetAuthorityProjection {
                    budget_lease_id,
                    reservations: 0,
                    settlements: 0,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                });
            // 预留计数 +1。
            //
            // 用 `saturating_add`（饱和加法）而不是 `+= 1`：计数溢出时前者停在 `u64::MAX`，
            // 后者会 panic（release 下还会回绕成 0）。
            //
            // ⚠ 为什么不直接拒绝溢出？因为溢出意味着账本本身已经不可信，
            //   这时最安全的失败方向是「让计数停在极大值、永不回落」——
            //   也就是让所有基于它的准入判断变成拒绝。回绕成 0 则会让预算看起来还很宽裕，
            //   那是危险方向。
            entry.reservations = entry.reservations.saturating_add(1);
        } else if matches!(event.kind.as_str(), "model.settled" | "budget.settled") {
            let budget_lease_id: BudgetLeaseId = id(&event.data, &["budget_lease_id"])
                .or_else(|_| {
                    id(
                        event.data.get("settlement_fact").unwrap_or(&Value::Null),
                        &["budget_lease_id"],
                    )
                })
                .map_err(|_| {
                    AuthorityProjectionError::BudgetInvalid("budget_lease_id_missing".to_owned())
                })?;
            let reservation_id = event
                .data
                .get("reservation_id")
                .or_else(|| {
                    event
                        .data
                        .get("settlement_fact")
                        .and_then(|value| value.get("reservation_id"))
                })
                .and_then(Value::as_str)
                .ok_or(AuthorityProjectionError::SettlementWithoutReservation)?;
            // 结算必须在**先有预留**的前提下发生。
            //
            // `remove` 返回 false，说明这笔 `reservation_id` 根本不在「未结清预留」集合里。
            // 两种现实可能：
            // 1. 账目凭空多出一笔支出（更严重）；
            // 2. 同一笔预留被结算了两次（重放 / 重复投递）。
            //
            // 两种都不允许悄悄放过，所以返回 `SettlementWithoutReservation`。
            // 这条规则和「预算只能收紧、不能凭空放大」是同一个原则。
            if !reservations.remove(&(budget_lease_id, reservation_id.to_owned())) {
                return Err(AuthorityProjectionError::SettlementWithoutReservation);
            }
            let entry = budgets
                .entry(budget_lease_id)
                .or_insert(BudgetAuthorityProjection {
                    budget_lease_id,
                    reservations: 0,
                    settlements: 0,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                });
            entry.settlements = entry.settlements.saturating_add(1);
        } else if matches!(
            event.kind.as_str(),
            "lease.issued" | "lease.released" | "lease.fenced"
        ) {
            let lease_id: StorageLockId = id(
                &event.data,
                &["lease_id", "resource_lease_id", "storage_lock_id"],
            )
            .map_err(|_| AuthorityProjectionError::LeaseInvalid("lease_id_missing".to_owned()))?;
            // 租约状态的判定：**由事件类型直接决定**，不靠增量翻转。
            //
            // `lease.issued` → active；`lease.released` / `lease.fenced` → 非 active。
            // 写成「从事件类型推导」而不是「翻转一个布尔值」，是为了让重放安全：
            // 同一批事件折两次结果一样，而翻转式更新在重放时会把状态翻回去。
            let active = event.kind == "lease.issued";
            let fenced = event.kind == "lease.fenced";
            if !active && !leases.contains_key(&lease_id) {
                return Err(AuthorityProjectionError::LeaseInvalid(
                    "lease_without_issue".to_owned(),
                ));
            }
            leases.insert(
                lease_id,
                LeaseAuthorityProjection {
                    lease_id,
                    active,
                    fenced,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                },
            );
        }
    }

    for cell in cells.values() {
        // 悬空父 Cell 检查。
        //
        // 到这一步所有 cell 事件都折叠完了，才做这个交叉校验——因为父 Cell 可能
        // **在子 Cell 之后**才出现（事件顺序不保证父子先来）。
        //
        // 失败码 `ChildParentMissing` 意味着：有一条 cell 事件指向一个投影里不存在的父。
        // 这在正常流程里不会发生，一旦发生通常是事件被删过、被改过，或者生产方写错了。
        // 宁可整张表拒收，也不能让一个「父不存在」的 Cell 出现在组织树上——
        // 那会让权限沿着一条断掉的链继续往上求交集。
        if let Some(parent) = cell.parent_cell_id {
            if !cells.contains_key(&parent) {
                return Err(AuthorityProjectionError::ChildParentMissing);
            }
        }
    }
    let model = AuthorityReadModel {
        schema: AUTHORITY_READ_MODEL_SCHEMA.to_owned(),
        source_cursor,
        source_event_ids,
        authority_epoch,
        cells: cells.into_values().collect(),
        grants: grants.into_values().collect(),
        budgets: budgets.into_values().collect(),
        leases: leases.into_values().collect(),
    };
    // 最后一道自检。
    //
    // ⚠ 不要因为「上面每条事件都校验过了」就省略这一步：
    // 上面校验的是**单条事件**的形状，这里校验的是**整张表**的一致性
    // （schema、来源非空、事件不重复、四个列表严格升序）。两者是不同的失败面。
    //
    // 投影自身的形状问题在这里被统一收敛成 `SourceInvalid`，
    // 因为对调用方来说「表不可信」就是一件事，不需要知道是哪一条不对。
    //
    // 注意 `.map_err` 把 `validate()` 的细错误码（header/order/duplicate）压成了
    // `SourceInvalid`。这是刻意的：调用方能做的动作只有「拒绝」，细分对处置没有帮助。
    model
        .validate()
        .map_err(|_| AuthorityProjectionError::SourceInvalid)?;
    Ok(model)
}
