//! 把调用方提供的一段 `RuntimeEvent` 切片折叠成只读的授权状态视图：Cell（执行单元）、
//! Grant（能力授予）、Budget（预算租约）和 Lease（存储锁租约）的投影记录。
//!
//! # 这个文件在系统里的位置
//!
//! ```text
//! 调用方准备的 RuntimeEvent 切片
//!        ↓  【本文件：纯折叠，不访问 EventLog，也不写入】
//! AuthorityReadModel（本模块计算出的查询投影）
//!        ↓
//! 当前源码中的直接调用点：测试；`kiana-core/src/lib.rs` 将 API 公开导出
//! ```
//!
//! **输入边界**：函数接收 `&[RuntimeEvent]`，但不负责从 `kiana-eventlog` 读取事件，也不检查
//! 这些事件是否真实提交、是否属于同一 aggregate 或是否覆盖了完整历史。调用方必须自行保证
//! 输入来源和顺序，并为 `source_cursor` 提供可信值。
//! **当前接入状态**：源码检索没有找到生产代码直接调用本模块；目前确认的直接调用点是
//! `kiana-core/tests/pd11_authority_projection.rs`，此外 `kiana-core/src/lib.rs` 公开导出 API。
//! 因此这里描述的是投影函数的实现语义，不能据此说它已经参与主路径授权判定或 UI 查询。
//! 本文件自身只做纯计算：不读文件、不发网络请求、不追加事件，也不持有第二份持久化事实。
//!
//! # 最重要的一件事：这是投影，不是事实
//!
//! 「投影（projection）」= 根据调用方提供的事实输入推导出的视图。
//! 在 Kiana 的事件记录路径中，EventLog 保存事件事实；但本文件不会读取 EventLog，是否能够重建
//! 取决于调用方是否提供了完整且可信的事件切片。因此不要把这个值对象当作事件来源或独立授权权威。
//! 若要形成新的授权事实，应走控制面和事件写入路径；只修改投影对象不会写入授权事件，也不会改变授权账本。
//! 对本模块而言，重新调用投影函数只会按新传入的切片和参数计算一个新视图。
//!
//! - 若要确认已经提交的授权事件，查对应 EventLog 流及其消费规则；本投影不会验证来源；
//! - 若要改变系统授权状态，修改这个返回值不够，必须走控制面允许的状态转换和事件写入。
//!
//! ⚠ 若代码只更新 read model 中的 Grant 字段，却把它当作已持久化或已生效的授权变化，
//! 就会让展示/计算结果与 EventLog 授权事实脱节。是否适合修改某个临时显示副本，需看它的具体用途。
//!
//! # 输入切片内的授权世代检查
//!
//! 对识别的授权事件，函数维护输入切片里见过的最大 epoch；若后处理的事件 epoch 更小，返回
//! `EpochRollback`。这只检查**当前输入序列内部**是否出现世代倒退。
//!
//! ⚠ 函数没有外部期望 epoch、签名或可信游标作为比较依据，因此无法仅靠此检查识别一整段
//! 自洽但过期的历史被重放。它也不会验证事件来自 EventLog 或 sequence 连续；这些来源与排序前提
//! 必须由调用方负责，且当前没有找到生产调用方。
//!
//! # 数据流
//!
//! ```text
//! events: [RuntimeEvent]
//!    ↓ 逐条去重（seen.insert）
//!    ↓ cell.* / grant.* 按前缀处理；预算与租约只处理代码列出的具体事件种类
//!    ↓ Cell/Grant/Lease 更新对应 map；预算事件增加计数并按 reservation_id 配对
//!    ↓ 收尾做交叉校验（子 Cell 的父必须存在、结算必须有预留、释放必须有签发）
//! AuthorityReadModel { cells, grants, budgets, leases }
//!    ↓ validate()
//! ```
//!
//! # 用 BTreeMap 而不是 HashMap 的原因
//!
//! 投影构造阶段按对象 ID 排序，使输出列表不受哈希随机种子影响；`validate()` 会拒绝降序排列，
//! 但当前比较条件允许相邻重复 ID（生成函数使用 BTreeMap 时自然不会产生重复 key）。
//! 同一输入切片和相同参数会得到相同字段值；此处不定义或保证特定序列化字节格式。

//! # 导入与基础设置
//!
//! `kiana_domain` 提供稳定的类型和事件定义（不涉及任何投影逻辑）。
//! `serde::Serialize` 为投影结构生成序列化实现；这些公开结构当前没有派生 `Deserialize`，
//! 因而本文件没有实现从外部 JSON 读回投影的流程。
//! `BTreeMap` 用于累加投影结果（保证输出顺序稳定），`BTreeSet` 用于去重和追踪已见事件。

use kiana_domain::{
    BudgetLeaseId, CapabilityGrantId, CellId, CellLifecycle, RuntimeEvent, StorageLockId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// 投影格式标识，带 `.v1` 版本号。`AuthorityReadModel::validate()` 会检查该字段与此常量一致。
/// 版本字符串为未来格式演进提供显式区分；当前模块没有实现文件持久化或从外部数据反序列化，
/// 因此不要把此常量描述成已经接通的存储读取兼容门。
pub const AUTHORITY_READ_MODEL_SCHEMA: &str = "kiana.persistence-authority-read-model.v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// 单个 Cell（执行单元，例如一个 worker、一次受限任务）的授权投影。
///
/// 【字段业务含义】
/// - `cell_id`：Cell 的稳定标识，跨事件引用同一个 Cell 靠它；
/// - `parent_cell_id`：可选的父 Cell ID。投影代码把缺字段或无法解码的值都变成 `None`；
///   成功解码为 `Some(id)` 后，折叠收尾会检查该父 ID 是否存在，但不检查循环或层级关系；
/// - `lifecycle`：Cell 的生命周期状态（创建/运行/取消中/阻塞/隔离/失败/退役等）；
/// - `authority_epoch`：这条状态所属的授权世代；
/// - `fenced`：本投影按一份固定状态清单从 `lifecycle` 派生出的布尔值。
///   `true` 仅表示状态属于 `CancelRequested`、`Blocked`、`Quarantined`、`Failed` 或 `Retired`；
///   `false` 不等于已经取得执行许可，实际授权仍须由控制面判断。它是派生视图，不是独立事件事实。
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
/// 当前实现把事件中的字符串状态原样放入投影；它不会将值解析为枚举，也不会拒绝未知状态。
/// 因此这个字段只保留读到的状态文本，不能单独作为授权结论，消费者仍须识别自己允许的取值。
///
/// 【`expires_at_unix_ms` 的来源与边界】
/// 显式无符号整数会被保留；字段缺失或不是无符号整数时，投影代码会填入 `u64::MAX`。
/// 当前投影用 `expires_at_unix_ms <= now_unix_ms` 检查 active 状态，且没有把 0 当作特殊值。
/// `kiana_domain::CapabilityGrant::validate()` 也将 0 判为无效期限；本投影若读到 active + 0，
/// 会在此返回过期错误，而不是将它解释为永不过期。
pub struct GrantAuthorityProjection {
    pub grant_id: CapabilityGrantId,
    pub state: String,
    pub authority_epoch: u64,
    pub expires_at_unix_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// 单个预算租约在输入事件切片中出现的预留与结算计数。
///
/// 每次识别到 `model.reserved` / `budget.reserved` 会增加 `reservations`，每次对应的
/// `...settled` 会增加 `settlements`。具体配对依赖投影函数内部的 reservation ID 集合，
/// 不能只靠两个计数相减。计数用饱和加法；达到 `u64::MAX` 后不再精确反映后续事件，
/// 所以这两个字段是摘要统计，不是逐笔成本账本或单独的准入结论。
pub struct BudgetAuthorityProjection {
    pub budget_lease_id: BudgetLeaseId,
    pub reservations: u64,
    pub settlements: u64,
    pub authority_epoch: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
/// 单个存储锁租约（Storage Lock Lease）的授权投影。
///
/// 【`active` 与 `fenced` 的来源】
/// 两者由当前折叠到的最后一条该租约事件推导：`lease.issued` 为 `true/false`，
/// `lease.released` 为 `false/false`，`lease.fenced` 为 `false/true`。这是事件视图状态，
/// 不会直接查询操作系统或存储层锁，也不证明 `fenced` 永久不可恢复；同一 ID 后续若再出现
/// `lease.issued`，当前实现会重新投影为 active 且未 fenced。ID 是否允许复用需要由上游契约保证。
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
/// 一份由当前投影函数输出的授权查询模型——**四张表 + 调用方提供的来源标签**。
///
/// “完整”仅指这个结构包含本模块当前定义的四类记录，不代表事件切片覆盖完整历史、
/// 来源标签经过认证，或这份数据可直接用于授权决策。
///
/// 【来源字段的含义与限制】
/// - `source_cursor`：调用方传入的来源位置标签；投影只检查它非零，不会从事件推导或核对该位置；
/// - `source_event_ids`：输入切片中按首次出现顺序收集的唯一 EventId，包括本模块不处理的事件种类。
///   投影循环遇到重复 EventId 会跳过后续项，最终自检再要求列表中没有重复 ID。
///
/// 消费方可以用这些字段辅助追踪输入来源，但它们不能证明事件切片完整、与 EventLog 当前 head 一致，
/// 也不能单独作为授权结论。当前源码没有找到生产调用方使用这些字段做新鲜度比较的代码。
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
    /// # 三层自检的设计
    ///
    /// 【作用】
    /// 检查三件事：
    /// 1. **表头**：schema 与预期的 AUTHORITY_READ_MODEL_SCHEMA 匹配；
    /// 2. **来源字段形状**：source_cursor 和 authority_epoch 都非零，source_event_ids 非空且不重复；
    /// 3. **列表顺序**：cell_id / grant_id / budget_lease_id / lease_id 不出现降序。
    ///
    /// 【调用者和时机】
    /// 当前源码中确认的调用点是 `project_authority_read_model` 在返回前执行自检。
    /// 结构体只派生 `Serialize`，没有从外部 JSON 读回的 `Deserialize` 实现。
    ///
    /// 【为什么需要三层检查】
    /// - `project_authority_read_model` 的逐事件校验检查的是**单条事件**的形状；
    /// - 这个 `validate()` 检查的是**整张表**在折叠完成后的**集合级别**约束。
    /// 两者是完全不同的失败面。折叠过程中每条事件都过了不代表最终表的形状就对。
    ///
    /// 【输入】
    /// `&self`，纯读取，不修改。
    ///
    /// 【输出】
    /// - `Ok(())`：所有三层检查都通过；
    /// - `Err(String)`：三条稳定错误码之一：
    ///   - `authority_read_model_header_invalid`：
    ///     schema 不对，或 source_cursor 为 0，或 authority_epoch 为 0，或 source_event_ids 为空；
    ///   - `authority_read_model_event_duplicate`：
    ///     source_event_ids 中有重复的 EventId；
    ///   - `authority_read_model_order_invalid`：
    ///     某个列表（cells / grants / budgets / leases）出现降序。
    ///
    /// 【为什么检查顺序】
    /// 生成函数以 BTreeMap 的排序结果构造列表，因此对生成结果检查顺序可以捕捉后续组装错误。
    /// 当前比较只拒绝降序，不拒绝相邻重复 ID；生成函数自身用 map 去重，但 `validate()` 单独调用时
    /// 不能据此保证列表中每个 ID 都唯一。
    ///
    /// 【为什么 source_event_ids 必须非空】
    /// 这是当前投影表头的输入约束：调用方至少要提供一个事件 ID。它不代表这些 ID 一定来自
    /// 持久化日志，也不验证 `source_cursor` 与事件 ID 列表之间的对应关系。
    ///
    /// 【为什么用 `windows(2)` 检查顺序】
    /// `windows(2)` 会把所有相邻元素对交给闭包；当前闭包检查 `a > b`，因此相等项会通过。
    ///
    /// 【副作用】
    /// 无。这是一个纯检查函数。
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
/// 【为什么分类成 enum】
/// 让调用方可以用 Rust 的 `match` 按错误类别处理，而不必从普通字符串里解析类别。
/// `thiserror::Error` 为这些变体生成标准错误 trait 实现；`#[error("...")]` 提供显示文本。
///
/// 【每个变体对应一类现实故障】
/// - `SourceInvalid`：事件切片为空、cursor/时间为 0，或最终投影未通过 `validate()`；
/// - `EpochRollback`：后处理到的授权事件 epoch 小于此前最大值；该错误不判定输入为何倒退；
/// - `EpochStale`：受检查事件的 epoch 缺失、不是无符号整数或为 0；
/// - `CellInvalid` / `GrantInvalid` / `BudgetInvalid` / `LeaseInvalid(..)`：各对象的事件形状不合法；
/// - `ChildParentMissing`：成功解析出的父 Cell ID 在投影 map 中不存在；格式错误的父字段会被当作 None；
/// - `SettlementWithoutReservation`：settled 事件缺少可读 reservation ID，或当前切片里找不到未结清的预留。
///
/// 【⚠ 注意】
/// 这些错误只表示投影无法生成。本函数不会自行拒绝请求；若未来调用方把结果用于授权，
/// 应将错误视为无法得出安全结论并拒绝继续。当前没有找到生产调用方。
///
/// 【四个聚合的错误类型如何区分】
/// - Cell 错误：Cell id 缺失、生命周期枚举值错（这时 fenced 派生字段无法计算）；
/// - Grant 错误：Grant ID 无法解码，或 state 为 active 但到期时间不晚于 now；
/// - Budget 错误：预算 id 缺失、重复预留、结算没有对应预留（账目凭空多出）；
/// - Lease 错误：Lease ID 无法解码，或 released/fenced 事件之前没有该 ID 的 issued 事件。
///
/// 【三个判定函数的对应关系】
/// - `epoch()` 校验「世代字段」→ EpochStale；
/// - `lifecycle()` 校验「Cell 生命周期枚举」→ CellInvalid；
/// - `id<T>()` 校验「各种 id 字段」→ CellInvalid / GrantInvalid / BudgetInvalid / LeaseInvalid；
/// - 倒带检查 → EpochRollback；
/// - 悬空父 Cell 检查 → ChildParentMissing；
/// - 结算-预留不匹配 → SettlementWithoutReservation。
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

/// 从事件的 `data`（JSON 对象）里按优先级选择**第一个存在**的候选字段，并反序列化成目标 ID 类型。
///
/// # 设计原理与调用约定
///
/// 【为什么需要这个 helper 函数】
/// 一个事件可能用多个候选字段名表达同一 ID；调用点把可接受的名称按优先级传进来。
/// 集中处理可以统一字段读取方式。候选名称是否对应历史版本需由事件生产/消费契约证明，不能仅凭别名推断。
///
/// 【输入参数】
/// - `data: &Value`：事件的 JSON 载荷（`event.data`）；
/// - `fields: &[&str]`：**候选字段名列表**，按**优先级排列**。
///   例如 Cell id 传 `["cell_id"]`；
///   Lease id 传 `["lease_id", "resource_lease_id", "storage_lock_id"]`。
/// - 返回类型 `T`：任何能从 JSON 反序列化的类型（这里主要是各种 `*Id`）。
///
/// 【算法流程】
/// `find_map` 先找列表中第一个**存在**的字段；找不到任何字段时返回 `SourceInvalid`。
/// 找到后只解析这个值：解析失败立即返回 `SourceInvalid`，不会继续尝试后面的候选字段。
/// 如果调用方需要在值格式错误时也尝试其他位置，必须像预算事件分支那样在外层显式写 `or_else`。
///
/// 【输出】
/// - 成功：`Ok(T)`，反序列化后的 ID；
/// - 失败：`Err(SourceInvalid)`（无论是「没找到字段」还是「字段存在但解析失败」）。
///
/// 【为什么「字段不存在」和「值不合法」合并成同一个错误】
/// 因为从**调用方的角度**，这两种情况的处理方式一样（拒绝该事件），区分它们只会让错误码膨胀。
///
/// 需要区分的场景（比如「这是字段缺失还是值错误」）由调用方在拿到 `Err` 之后**自己补上下文**。
/// 例如，cell ID 缺失或格式错误时，内部调用点都会把底层错误映射成字段错误码：
/// ```text
/// id(&event.data, &["cell_id"])
///     .map_err(|_| AuthorityProjectionError::CellInvalid("cell_id_missing".to_owned()))
/// ```
/// 这样错误信息是清晰的（"cell_id_missing" 而不是 "source_invalid"），
/// 但底层的 `id()` 函数不必为每个字段特殊化。
///
/// 【泛型写法说明】
/// `T: for<'de> Deserialize<'de>` 是生命周期泛型写法，意思是「T 能在**任意生命周期**的
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
/// 这个投影把 0 用作“尚未从事件中观察到有效授权代次”的初始值，最终表头也拒绝 0。
/// 因此识别到需要携带授权代次的事件时，若它缺失/非法/为 0，就在读取该事件时返回 `EpochStale`，
/// 避免先把它当成有效状态折叠、最后才在表头处失败。这里仅执行非零形状检查，不与其他来源的当前代次比较。
///
/// 【失败情况】
/// 字段缺失、不是无符号整数、或等于 0 → `EpochStale`。具体原因会归并到同一个错误变体，
/// 调用方不能仅凭该变体区分「漏字段」「负数/错误类型」和「0 哨兵值」。
///
/// 【何时调用】
/// 折叠循环对 `cell.*`、`grant.*`、`budget.*`、`lease.*` 前缀以及精确的 `model.reserved`、
/// `model.settled` 种类调用它。后续只有 Cell/Grant 前缀和列出的预算/租约精确种类会更新表；
/// 未知 `budget.*` / `lease.*` 事件仍可能先经过 epoch 读取，然后不匹配任何投影分支。
/// 是否所有这些事件都应承载授权代次，属于事件契约问题，本 helper 只反映当前分派范围。
///
/// 【与 SourceInvalid 的区别】
/// `epoch()` 自己把缺失/非法/为零都归入 `EpochStale`；`SourceInvalid` 由投影入参或通用 ID
/// 读取 helper 等其他位置返回。不要把缺少 epoch 解释成 `SourceInvalid`。
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
/// 【字段选择规则】
/// 当前代码先读取 `lifecycle`，只有该字段不存在时才读取 `state`；若 `lifecycle` 存在但值无法
/// 反序列化，会直接报错，不再回退到 `state`。这说明解析器接受两个名称，但字段名本身不能证明
/// 哪个来自旧版本或新版本。
///
/// 【容易误改的边界】
/// 同一事件可能同时带两个且值不同；当前实现优先使用 `lifecycle`，不检查二者是否一致。
/// 收紧该规则会改变现有输入的接受范围，需要先确认事件生产方契约和历史数据形状。
///
/// 【失败情况】
/// 两个字段都没有 → `CellInvalid("lifecycle_missing")`；
/// 有但不是合法枚举值 → `CellInvalid("lifecycle_invalid")`。
/// 区分开是因为「**没写**」和「**写错了**」要修的地方不一样。
///
/// `CellLifecycle` 是 `kiana-domain/src/states.rs` 定义的领域枚举，包含 Proposed、Validated、
/// Spawning、Ready、Running、WaitingInput、Blocked、Checkpointing、ReadyToMerge、Merging、
/// Succeeded、Retiring、Retired、CancelRequested、Cancelled、Stalled、Retrying、Failed 和
/// Quarantined。此 helper 只负责从 JSON 解码状态；它不校验状态转移是否合法，也不直接判断授权。
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

/// 从调用方提供的事件切片**折叠**成一张授权读模型——当前模块唯一的投影入口。
///
/// # 核心职责与设计原则
///
/// 【作用】
/// 本文件唯一的纯计算入口。相同事件切片及相同参数会得到相同字段结果。
/// 不写文件、不发事件、不改全局状态；`now_unix_ms` 会用于 active grant 过期一致性检查，
/// 但函数不会删除/筛除已过期 grant。
///
/// 【调用者】
/// 当前源码中由 `kiana-core/src/lib.rs` 公开导出，并由 `kiana-core/tests/pd11_authority_projection.rs`
/// 直接调用；没有找到生产路径的调用点。不要把可能的授权查询/审计用途写成当前已经接入的路径。
///
/// 【核心流程】
/// ```text
/// 1. 入参体检
///    ├─ source_cursor 为 0 → SourceInvalid
///    ├─ now_unix_ms 为 0 → SourceInvalid
///    └─ events 为空 → SourceInvalid
/// 2. 逐条事件处理
///    ├─ 去重（同一 event_id 只算一次）→ 跳过已见事件
///    ├─ cell.* / grant.* 前缀进入对应分支；budget 与 lease 只处理明确列出的事件类型
///    ├─ 每条带 epoch 的事件更新全局最大世代
///    └─ 同时检查世代是否倒带（EpochRollback）
/// 3. 收尾交叉校验
///    ├─ 悬空父 Cell 检查（子的父必须存在）
///    └─ 结算-预留匹配检查（结算必须有对应预留）
/// 4. 表头自检与顺序检查
///    ├─ schema 和版本号匹配
///    ├─ source_cursor / authority_epoch 非零
///    ├─ source_event_ids 非空且无重复
///    └─ 四个列表不得出现降序（当前自检允许相邻相同 ID）
/// ```
///
/// 【输入参数】
/// - `events: &[RuntimeEvent]`：调用方提供的事件切片，预期按提交顺序排列。
///   后写到的同 ID 对象会覆盖 map 中较早的投影；本函数不检查 stream version、sequence 连续性、
///   aggregate 一致性或输入是否完整。
/// - `source_cursor: u64`：调用方传入的非零位置标签。本函数不把它与事件内容核对，因此不是完整性证明。
/// - `now_unix_ms: u64`：当前时间（毫秒），必须非零；如果某条 grant 记录为 `active` 且到期时间
///   小于等于该值，函数会返回 `GrantInvalid("grant_expired_as_active")`。它不负责过滤过期记录。
///
/// 【输出】
/// - 成功：`AuthorityReadModel`，包含四个投影表 + 来源凭据；
/// - 失败：`AuthorityProjectionError` 的某个变体。
///
/// 如果某个调用方把该投影用于授权输入，必须把错误视为无法得出结论，不能降级为空表放行。
/// 本函数只返回分类错误，不会自己拒绝 API 请求或更改权限状态。
///
/// 【为什么用 BTreeMap 累加而不是 HashMap】
/// `into_values()` 会按 key 顺序生成 Vec，使同一事件切片的输出字段顺序稳定；本函数没有计算摘要，
/// 也不保证某种外部序列化后的字节格式。
///
/// 【为什么重复事件是「跳过」而不是「拒绝」】
/// 同一 EventId 再次出现时，`BTreeSet::insert` 返回 false，后续副本会被跳过。
/// 这令完全相同的重复输入不重复计数；但当前代码不会核对相同 EventId 的其他字段是否也相同，
/// 因此“ID 相同但内容冲突”也会静默采用首次出现的事件，不能把去重描述成完整的重放身份校验。
///
/// 【四个表的累加顺序与覆盖关系】
/// - `cells`、`grants`、`budgets`、`leases` 分别用 `BTreeMap` 存储；
/// - 同一个对象 id 由不同 EventId 的事件多次更新时，后处理的事件覆盖 map 中先前的值；
/// - 同一 EventId 的后续副本则会在进入分支前跳过；覆盖语义依赖调用方传入顺序，不等于本函数验证了事件合法转移。
///
/// 【为什么需要 `reservations` 集合】
/// 追踪「哪些预留还没被结算」。每次 `model.reserved` / `budget.reserved`
/// 就 insert 一个 (budget_lease_id, reservation_id)，每次结算就 remove。
/// 如果 remove 失败（结算了但没有对应预留），说明账目凭空多出一笔，拒绝。
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
        // 同一 EventId 的后续条目会被跳过；这里没有比较其 kind、payload 或版本是否与首次出现一致。
        // 因而完全相同的重放不会重复处理，但同 ID 不同内容也会静默丢弃后者，属于需要上游保证的身份前提。
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
            // 只要后处理到的 epoch 小于先前最大值，就说明输入切片中的授权代次不单调，函数立即失败。
            // 本函数不验证事件确为提交顺序，也无法仅凭这个现象判断是重放、恢复、排序错误还是来源损坏。
            if authority_epoch > event_epoch {
                return Err(AuthorityProjectionError::EpochRollback);
            }
            authority_epoch = authority_epoch.max(event_epoch);
        }

        if event.kind.starts_with("cell.") {
            // 【Cell 投影处理（执行单元的生命周期）】
            //
            // Cell 是 domain 层定义的执行单元。本分支把同一 Cell 的最新事件折叠成一条状态记录，
            // 并按下面的固定清单计算 `fenced`；这个布尔值不代替 ControlPlane 的实际授权判断。
            //
            // 【Cell id 和父 id】
            // `cell_id` 是强制的，用于跨事件引用。
            // `parent_cell_id` 表示可选父 Cell。当前解析使用 `from_value(...).ok()`：字段缺失或
            // 值格式错误都会变成 None；因此后续父引用检查只覆盖成功解析为 Some 的值，
            // 本分支也没有检测父子循环或计算权限继承。
            let cell_id: CellId = id(&event.data, &["cell_id"])
                .map_err(|_| AuthorityProjectionError::CellInvalid("cell_id_missing".to_owned()))?;
            let parent_cell_id = event
                .data
                .get("parent_cell_id")
                .and_then(|value| serde_json::from_value(value.clone()).ok());
            let state = lifecycle(event)?;

            // 【fenced 派生字段的推导】
            //
            // `fenced: bool` 是**派生字段**，从 `lifecycle` 推导而来。
            // 这五个状态在此被标记为 fenced；本函数不验证此清单是否等同于每种能力的授权策略：
            // - `CancelRequested`：正在取消中（cancel-in-flight）；
            // - `Blocked`：被阻塞了（无法继续执行）；
            // - `Quarantined`：被隔离了（怀疑有风险）；
            // - `Failed`：已经失败了（不再尝试）；
            // - `Retired`：已经退役了（生命周期结束）。
            //
            // 其他 CellLifecycle 状态得到 false；false 只表示不在这份清单里，不代表允许新动作。
            //
            // 【为什么是派生而不是独立字段】
            // 这样做的好处是：
            // 1. 保持单一事实源：只有 `lifecycle` 是事实，`fenced` 由它推导；
            // 2. 不用在事件生产方写两个字段（容易不一致）；
            // 3. 便于读方展示该派生分类；如需据此授权，仍须走 ControlPlane 的权限流程。
            //
            // ⚠ 代价是：这张表永远**只能由折叠重新生成，不能手工编辑**。
            // 如果有人直接改投影里的 `fenced` 字段，投影就不再可信。
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
            // 【Grant 投影处理】
            //
            // 本分支仅保留 Grant 自身的 ID、状态文本、epoch 和到期时间；它不读取 Cell、Budget
            // 或 Lease 投影来验证 Grant 的关联，也不判断该 Grant 是否足以授权某个动作。
            // ID 字段按 `capability_grant_id`、`grant_id` 的顺序查找；若第一个存在但格式错误，
            // `id()` 会返回错误，不会尝试第二个字段。支持多个名称本身不能证明它们的历史来源。
            let grant_id: CapabilityGrantId = id(&event.data, &["capability_grant_id", "grant_id"])
                .map_err(|_| {
                    AuthorityProjectionError::GrantInvalid("grant_id_missing".to_owned())
                })?;

            // 【Grant 状态的推导规则】
            //
            // 字段选择时先取 `state`，只有它不存在才取 `status`；选中值是字符串时原样使用，
            // 选中字段不是字符串时则从 kind 推导：
            // - `grant.revoked` → "revoked"
            // - `grant.expired` → "expired"
            // - 其他 grant.* → "active"。这是实现中的默认值，不是拒绝未知事件的保守策略；
            //   未知的 grant.* 种类也会走这里，调用方不能把它当成已验证的有效授权。
            //
            // 因此 state 存在但不是字符串时，不会回退到 status，而是直接进入 kind 推导分支。
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

            // 【过期时间处理】
            //
            // 读出无符号 expires_at_unix_ms；字段缺失或不是无符号整数时填入 u64::MAX。
            // 显式 0 会保留为 0，随后会参与下方 <= now_unix_ms 的检查；本实现没有将 0 作为特殊值处理。
            let expires_at_unix_ms = event
                .data
                .get("expires_at_unix_ms")
                .and_then(Value::as_u64)
                .unwrap_or(u64::MAX);

            // 【逻辑矛盾检测】
            //
            // 当前时间已达到或超过 expires_at 时，仍标为 active 的记录会使整个投影失败。
            // 这段逻辑拒绝这种组合，但不证明原因一定是生产方错误。当前 `CapabilityGrant` 类型
            // 的校验也拒绝 0 到期时间；若事件绕过该类型校验直接进入本函数，active + 0 会在此失败。
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
            // 【预留事件处理：增加预留计数】
            //
            // 两种事件都代表「有一笔预算被预留了」：
            // - `model.reserved`：模型运行消耗的预算；
            // - `budget.reserved`：通用预算租约的预留。
            // 两者都要增加 `reservations` 计数。
            //
            // 【Budget id 的两个位置】
            // 正常情况下在 `event.data.budget_lease_id`。
            // 载荷也可能把预算 ID 放在 `event.data.reservation_fact` 对象里，
            // 所以尝试两个位置。`or_else` 在外层字段无法解析时才继续尝试内层字段。
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

            // 【预留 id 的生成规则】
            //
            // 先取顶层 `reservation_id`，它不存在时再取 `reservation_fact.reservation_id`；
            // 选中的值若不是字符串，代码不会尝试另一个字段，而是回退到当前事件的 EventId 字符串。
            // 结算时必须能提供相同的 reservation ID 才能配对。这里没有验证 EventId 全局唯一，
            // 因而无显式 reservation_id 的事件依赖上游保证事件 ID 不冲突。
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

            // 【预留重复检查】
            //
            // `reservations` 集合追踪「哪些 (budget_lease_id, reservation_id) 还没结算」。
            // `insert` 返回 false 意味着这个预留**已经存在**（重复）。
            // 不同 EventId 的 reserved 事件若重复使用同一对 ID，会被拒绝。
            // 同一 EventId 的重复副本已在循环开头跳过；本检查不会把那种普通重复报告为错误。
            if !reservations.insert((budget_lease_id, reservation_id)) {
                return Err(AuthorityProjectionError::BudgetInvalid(
                    "reservation_duplicate".to_owned(),
                ));
            }

            // 【预算投影的按需创建】
            //
            // 如果这个 budget_lease_id 是第一次出现，创建一个新的投影对象。
            // 后续预留/结算事件都会更新这个对象。
            let entry = budgets
                .entry(budget_lease_id)
                .or_insert(BudgetAuthorityProjection {
                    budget_lease_id,
                    reservations: 0,
                    settlements: 0,
                    authority_epoch: event_epoch.unwrap_or(authority_epoch),
                });

            // 【预留计数增加（饱和算术）】
            //
            // `saturating_add(1)` 在达到 u64::MAX 后保持最大值，避免整数回绕成较小计数。
            // 代价是此后计数不再精确；本投影没有使用该计数做准入判断，消费者不能假设它仍代表真实总数。
            entry.reservations = entry.reservations.saturating_add(1);
        } else if matches!(event.kind.as_str(), "model.settled" | "budget.settled") {
            // 【结算事件处理】
            //
            // 结算是「消费预留」的动作。每条 settled 事件对应一条已经被完成的预留。
            // 结算必须在**先有预留**的前提下发生。
            //
            // 【结算-预留的配对原理】
            // - 预留阶段：输入事件先记录一条 reserved；
            // - 结算阶段：后续 settled 通过 reservation_id 指向这条预留；
            // - 投影检验：每条 settled 都必须在当前切片中找到一条尚未移除的对应 reserved。
            //
            // 如果找不到匹配的 reserved，说明这段输入无法把结算事件关联到一条预留；
            // 投影拒绝继续，但这本身不核验现实中的金额或是否真的发生了支出。
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

            // 【结算 id 的读取】
            //
            // 与预留 id 对应，从相同位置读取。不能有回退（不像 reserved 那样回退到 event_id），
            // 因为结算**必须**有明确的 reservation_id 才能匹配。
            // 如果结算事件没写 reservation_id，返回 `SettlementWithoutReservation`。
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

            // 【预留-结算配对检验】
            //
            // `reservations.remove((budget_lease_id, reservation_id))` 返回 true
            // 意味着这笔预留之前被 insert 过且现在被移除。
            // 返回 false 意味着：要么预留根本不存在，要么已经被结算过一次了。
            //
            // 两种都是问题：
            // 可能是预留未包含在输入切片中，也可能是同一 ID 已经被前一条 settled 移除。
            // 本检查只维护事件间的配对关系；它不核验支出金额，也不直接产生预算准入结论。
            if !reservations.remove(&(budget_lease_id, reservation_id.to_owned())) {
                return Err(AuthorityProjectionError::SettlementWithoutReservation);
            }

            // 【结算计数增加】
            //
            // 与预留计数相同使用饱和加法；到达上限后该统计不再精确增长。
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
            // 【Lease 投影处理（Storage Lock 租约）】
            //
            // 本分支只保留每个 lease ID 最近事件对应的两个状态位；投影结构没有记录持有者，
            // 也不会向实际存储锁服务查询当前锁状态。
            // - `lease.issued`：投影为 active；
            // - `lease.released`：投影为 inactive 且未 fenced；
            // - `lease.fenced`：投影为 inactive 且 fenced。事件本身没有说明释放/作废原因。
            //
            // 【为什么 active / fenced 需要分开】
            // - `active`：最近处理的事件是否为 issued；
            // - `fenced`：最近处理的事件是否为 fenced。
            // 这是投影规则，不单独表达锁是否真实仍由某个运行时持有，也不定义永久作废语义。
            let lease_id: StorageLockId = id(
                &event.data,
                &["lease_id", "resource_lease_id", "storage_lock_id"],
            )
            .map_err(|_| AuthorityProjectionError::LeaseInvalid("lease_id_missing".to_owned()))?;

            // 【从事件类型直接推导 active 状态】
            //
            // `active = (event.kind == "lease.issued")`
            // 这种「从事件类型推导状态」的做法**对重放安全**：
            // 同一批事件折两次结果完全一样（幂等性）。
            //
            // 反面例子是「active 切换」：如果用 `active = !active` 更新，
            // 重放时会把状态翻回去——同一批事件折两次得到**不同结果**，破坏了幂等性。
            // 正确方向是「从事实推导」而不是「维护翻转状态」。
            let active = event.kind == "lease.issued";
            let fenced = event.kind == "lease.fenced";

            // 【悬空租约检查】
            //
            // 如果收到 released / fenced，但该 ID 尚未在当前切片的折叠结果中出现，返回错误。
            // 这说明当前输入无法证明租约曾经签发；具体原因需结合调用方提供的事件范围追查。
            //
            // ⚠ 注意这检查的是「投影不出来」，不是「权限被拒绝」。
            // 本函数只返回投影错误；若调用方把结果用于授权，必须把这种缺失按 fail-closed 处理。
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

    // 【第一层交叉校验：悬空父 Cell 检查】
    //
    // 到这一步所有 cell 事件都折叠完了，才检查成功解析出的父 ID；因此父 Cell 可以
    // 出现在子 Cell 事件之后。此处只检查父 ID 是否存在，不检查循环或父级生命周期。
    //
    // 逻辑：对于每个 Cell，如果它有父 Cell（`parent_cell_id.is_some()`），
    // 检查这个父 id 是否**存在**于投影的 cells 表中。
    //
    // 失败码 ChildParentMissing 表示成功解析出的父 ID 不在当前投影中；它可能是事件切片不完整，
    // 也可能是上游引用错误。本函数无法判断具体原因，并且不会生成部分表。
    for cell in cells.values() {
        if let Some(parent) = cell.parent_cell_id {
            if !cells.contains_key(&parent) {
                return Err(AuthorityProjectionError::ChildParentMissing);
            }
        }
    }

    // 【构造最终投影表对象】
    //
    // 四个 BTreeMap 都转换成有序 Vec（`into_values().collect()`），
    // 加上来源凭据字段。schema 和 authority_epoch 都是已计算的。
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

    // 【第二层校验：表头和顺序自检】
    //
    // ⚠ 不要因为「上面每条事件都校验过了」就省略这一步：
    // - 上面校验的是事件载荷字段、已识别的状态关系和 epoch 单调性；
    // - 这里校验整张表的 schema、来源字段、事件 ID 重复和列表是否降序。
    // 两者是**不同的失败面**。
    //
    // 【为什么 `.map_err` 要压缩错误码】
    // `validate()` 返回的细错误码有：
    // - `authority_read_model_header_invalid`：schema 不对或 cursor/epoch 为 0；
    // - `authority_read_model_event_duplicate`：source_event_ids 有重复；
    // - `authority_read_model_order_invalid`：某个列表没按 id 升序。
    //
    // 这些细分错误在这里被折叠成 `SourceInvalid`，因此调用方看不到最终自检失败的具体原因。
    // 列表顺序由 BTreeMap 生成路径保证；当前 `validate()` 比较条件只拒绝降序，不拒绝相等 ID。
    model
        .validate()
        .map_err(|_| AuthorityProjectionError::SourceInvalid)?;
    Ok(model)
}
