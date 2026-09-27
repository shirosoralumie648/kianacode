//! Bounded observability queue with explicit critical/best-effort semantics.
//!
//! The queue is a delivery aid, never an EventLog. Critical facts are not silently evicted. When
//! no critical slot is available, enqueue returns immediately so an EventStore commit cannot be
//! held hostage by a slow consumer; the caller must retain the committed fact and rescan it later.

use crate::ObservabilitySignalRecord;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::Notify;

/// 丢弃原因字符串的最大字节数。
///
/// 【为什么需要截断】
/// `last_drop_reason` 会被 `stats()` 返回，而 stats 会被上层写进事件账本或日志。
/// 如果不限制长度，一个构造得很长的原因字符串就能撑爆日志。
/// 128 字节足够表达"为什么被丢"，又不会造成问题。
///
/// ⚠ 用 `truncate` 按字节截断有副作用
/// `String::truncate` 在字节边界不正确时不会 panic，而是停在字符中间导致字符串无效。
/// 不过 `reason` 在这里来自代码里的固定字面量（不是用户输入），所以实际上是安全的。
/// 若将来改成接受外部输入，需改用 `char_indices` 做安全截断。
///
const MAX_DROP_REASON_BYTES: usize = 128;

/// 可观测性信号的分类。
///
/// 【为什么分类，而不是一个扁平的队列】
/// 因为不同信号的重要性差别巨大：
/// 丢掉一条 debug 日志无关紧要，丢掉一条审批记录却是安全事故。
/// 分类让队列能在满的时候做出"保谁、丢谁"的差异化决策。
///
/// 【八个分类的语义】
/// - `Event` —— 运行时事件；
/// - `Audit` —— 审计记录（谁做了什么）；
/// - `Approval` —— 审批决定；
/// - `Recovery` —— 恢复动作；
/// - `Terminal` —— 终端/会话结束的标记；
/// - `Log` —— 普通日志（可丢）；
/// - `Trace` —— 链路追踪（可丢）；
/// - `Metric` —— 指标采样（可丢）。
///
/// 【前五个 vs 后三个的划分】
/// 前五个是关键（critical），后三个是尽力而为（best-effort）。
/// 这个划分由 `is_critical` 判定，它直接决定队列满了之后谁被牺牲。
///
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservabilityQueueClass {
    Event,
    Audit,
    Approval,
    Recovery,
    Terminal,
    Log,
    Trace,
    Metric,
}

impl ObservabilityQueueClass {
    /// 这一类信号是不是关键信号。
    ///
    /// 【作用】
    /// 决定队列满时的取舍策略，见 `ObservabilityQueue::try_enqueue`。
    ///
    /// 【为什么用 `matches!` 而不是 `match`】
    /// `matches!` 是"是否属于某一组模式"的简写。
    /// 这里只需要一个布尔结果，不需要各分支的返回值，所以 `matches!` 最直白。
    ///
    /// ⚠ 这是一条硬编码的分类线，改动它会改变丢数据行为
    /// 往这个列表里加一个变体，等于宣布"它不能被丢弃"；
    /// 删掉一个变体，等于宣布"它可以被丢弃"。
    /// 两种改动都会改变系统在压力下的行为，且不容易从测试里看出来。
    ///
    pub const fn is_critical(self) -> bool {
        matches!(
            self,
            Self::Event | Self::Audit | Self::Approval | Self::Recovery | Self::Terminal
        )
    }

    /// 分类的稳定字符串形式。
    ///
    /// 【作用】
    /// 把枚举变成一个稳定的、可写入事件账本的字符串。
    ///
    /// ⚠ 这些字符串是对外契约
    /// 它们会进事件日志、出现在 CLI 输出和 UI 界面上，
    /// 下游可能有东西按这些字符串做分类统计。
    /// 改写其中任何一个都会让下游静默失效 —— 要改必须同步所有消费方。
    ///
    /// 【为什么用 `&'static str` 而不是 `String`】
    /// `&'static str` 是编译期就存在的字符串切片，没有堆分配。
    /// 在错误信息里反复构造 `String` 是浪费。
    ///
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Audit => "audit",
            Self::Approval => "approval",
            Self::Recovery => "recovery",
            Self::Terminal => "terminal",
            Self::Log => "log",
            Self::Trace => "trace",
            Self::Metric => "metric",
        }
    }
}

/// 队列里的一项。
///
/// 【三个字段的含义】
/// - `class` —— 关键还是尽力而为，决定满了之后它的命运；
/// - `source_cursor` —— 它在事件账本里的位置。
///   这让消费方知道"这条信号对应的是哪个已提交的事实"，
///   即使投递本身失败或延迟，也能回去重新扫描那个位置。
///   这是整个设计的关键：队列是投递辅助，不是事实来源；
/// - `signal` —— 可选的详细信号内容。
///   `None` 表示"只需要知道有这么一件事"，一个游标就够了。
///
/// ⚠ 为什么 `signal` 是 `Option` 而不是必有
/// 关键事实已经写进了 EventLog（那是权威来源），
/// 队列里只需要放一个指针指过去。
/// 把完整内容再抄一份既浪费，又可能造成两处内容不一致。
/// 只有当信号本身不在 EventLog 里时，才需要用 `Some` 带上一份。
///
#[derive(Clone, Debug, PartialEq)]
pub struct QueuedObservabilityItem {
    pub class: ObservabilityQueueClass,
    pub source_cursor: u64,
    pub signal: Option<ObservabilitySignalRecord>,
}

impl QueuedObservabilityItem {
    /// 构造一项只有游标的关键条目。
    ///
    /// 【什么时候用这个】
    /// 事实已经落在 EventLog 里，队列只需要通知"有这么个位置，你可以去取"。
    /// 这是最常见的情况。
    ///
    pub fn critical(class: ObservabilityQueueClass, source_cursor: u64) -> Self {
        Self {
            class,
            source_cursor,
            signal: None,
        }
    }

    /// 构造一项自带内容的条目。
    ///
    /// 【什么时候用这个】
    /// 当这条信号本身没有进 EventLog，必须随队列一起传递时。
    /// 比如 UI 需要的即时状态变化 —— 它不是审计事实，不需要权威存储，
    /// 但需要被尽快看到。
    ///
    pub fn signal(
        class: ObservabilityQueueClass,
        source_cursor: u64,
        signal: ObservabilitySignalRecord,
    ) -> Self {
        Self {
            class,
            source_cursor,
            signal: Some(signal),
        }
    }
}

/// 队列可能返回的错误。
///
/// 【五个变体各自代表什么】
/// - `InvalidCapacity` —— 容量为 0，队列无法工作；
/// - `Closed` —— 队列已关闭，不再接受新项；
/// - `BestEffortDropped` —— 尽力而为的项被丢弃了，这是正常现象，不是故障；
/// - `CriticalQueueFull` —— 关键项也进不去，这才是真正需要注意的情况；
/// - `FlushCancelled` —— 等待清空时被取消。
///
/// ⚠ 后两个的区别是本文件的核心语义
/// `BestEffortDropped` 是设计内的正常损耗 —— 队列满了，日志挤掉了日志，可接受。
/// `CriticalQueueFull` 是需要调用方处理的信号 —— 关键事实没能进入队列。
///
/// 文件头说得很清楚：当没有可用关键槽位时，入队立即返回，
/// 这样 EventStore 的提交不会被慢消费者挟持；
/// 调用方必须自己保留那份已提交的事实，之后重新扫描。
///
/// 换句话说：关键项进不去时，调用方必须自己兜底。
/// 队列不会为此阻塞等待 —— 因为如果它阻塞，
/// 慢消费者就会连带把 EventStore 的提交流程卡住，
/// 那才是真正的灾难：一条日志的延迟拖垮了整个事实记录系统。
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservabilityQueueError {
    InvalidCapacity,
    Closed,
    BestEffortDropped { reason: String },
    CriticalQueueFull { class: ObservabilityQueueClass },
    FlushCancelled,
}

/// 把错误渲染成稳定的原因码字符串。
///
/// ⚠ 这些字符串是对外契约
/// 它们会进日志、事件账本和 CLI 输出。
/// 下游可能按这些码做统计或告警，改写会导致静默失效。
///
/// 【格式约定】
/// 基础错误是纯小写 snake_case；带细节的错误用冒号分隔后缀：
/// - `observability_best_effort_dropped:{reason}`
/// - `observability_critical_queue_full:{class}`
/// 冒号分隔让下游可以按冒号切分，取出结构化的后半部分。
///
impl std::fmt::Display for ObservabilityQueueError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCapacity => formatter.write_str("observability_queue_capacity_invalid"),
            Self::Closed => formatter.write_str("observability_queue_closed"),
            Self::BestEffortDropped { reason } => {
                write!(formatter, "observability_best_effort_dropped:{reason}")
            }
            Self::CriticalQueueFull { class } => {
                write!(
                    formatter,
                    "observability_critical_queue_full:{}",
                    class.as_str()
                )
            }
            Self::FlushCancelled => formatter.write_str("observability_flush_cancelled"),
        }
    }
}

impl std::error::Error for ObservabilityQueueError {}

/// 队列的运行时统计快照。
///
/// 【作用】
/// 给运维和 UI 看的观测数据，本身不参与控制逻辑。
///
/// 【几个计数器的语义】
/// - `depth` —— 当前排队多少项；
/// - `enqueued_total` / `dequeued_total` —— 累计进出计数；
/// - `dropped_best_effort_total` —— 累计丢了多少尽力而为项。
///   持续增长说明容量不足或消费者太慢；
/// - `critical_rejected_total` —— 累计拒绝了多少关键项。
///   这个数字只要大于 0 就是需要关注的：
///   意味着有事实没能及时投递，虽然事实本身还在 EventLog 里；
/// - `flush_sequence` —— 队列清空过几次；
/// - `reopen_total` —— 关闭后又被重开过几次；
/// - `last_drop_reason` —— 最近一次丢弃的原因。
///
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservabilityQueueStats {
    pub capacity: usize,
    pub depth: usize,
    pub enqueued_total: u64,
    pub dequeued_total: u64,
    pub dropped_best_effort_total: u64,
    pub critical_rejected_total: u64,
    pub flush_sequence: u64,
    pub reopen_total: u64,
    pub closed: bool,
    pub last_drop_reason: Option<String>,
}

/// 队列的内部可变状态，全部由一把锁保护。
///
/// ⚠ 为什么 `items` 和计数器要放在同一个结构里
/// 因为它们必须在同一个临界区内被一致地修改。
/// 如果分开加锁，就可能出现"项加进去了但计数没加"这种不一致状态，
/// 导致 `stats()` 报出错误的数字。
///
/// 【`Default` 的来源】
/// 所有字段都有合理的零值，所以可以 `#[derive(Default)]`。
///
#[derive(Default)]
struct QueueState {
    items: VecDeque<QueuedObservabilityItem>,
    enqueued_total: u64,
    dequeued_total: u64,
    dropped_best_effort_total: u64,
    critical_rejected_total: u64,
    flush_sequence: u64,
    reopen_total: u64,
    closed: bool,
    last_drop_reason: Option<String>,
}

/// A bounded queue that never awaits while accepting an item.
/// 一个有界队列，接受元素时永不等待。
///
/// 【核心设计：为什么"永不等待"是硬要求】
/// 文件头那句 "A bounded queue that never awaits while accepting an item"
/// 是整个设计的出发点。
///
/// 这个队列的消费者是投递器（把信号送到 EventLog / MetricSink）。
/// 如果入队会等待，一旦投递器变慢，
/// 所有调用 `try_enqueue` 的地方都会被卡住 ——
/// 而那些地方往往是正在提交关键事实的代码路径。
/// 结果就是：一个慢消费者拖垮整个事实记录系统。
///
/// 所以这里的设计是：入队永不阻塞，满了就按优先级取舍，
/// 取舍不了就明确报错，由调用方负责后续处理。
///
/// 【三个共享字段的职责】
/// - `state` —— 受锁保护的队列内容与统计；
/// - `item_available` —— 有东西可取时唤醒等待的消费者；
/// - `drained` —— 队列被取空时唤醒等待 flush 的调用方。
///
/// ⚠ 两个通知器是分开的，因为它们回答的是不同的问题
/// 消费者问"有活吗"，flush 调用方问"干完了吗"。
/// 用同一个通知器会让其中一个等待被错误地唤醒，然后重新循环。
///
/// 【为什么用 `Arc`】
/// 队列需要被克隆后分发给多个任务（`#[derive(Clone)]`），
/// `Arc` 让克隆只是增加引用计数，所有副本共享同一份队列。
/// 这是"多个消费者从一个队列取数据"的标准做法。
///
#[derive(Clone)]
pub struct ObservabilityQueue {
    capacity: usize,
    state: Arc<Mutex<QueueState>>,
    item_available: Arc<Notify>,
    drained: Arc<Notify>,
}

impl std::fmt::Debug for ObservabilityQueue {
    /// 为什么手动实现 `Debug` 而不是直接 derive
    ///
    /// 因为内部有 `Arc<Mutex<...>>` 和 `Arc<Notify>`，
    /// derive 出来的 `Debug` 只会打印一堆指针地址和类型名，没有信息量。
    /// 这里改成打印 `stats()` —— 人类真正想看的是队列里有多少项、有没有丢东西。
    ///
    /// ⚠ 注意 `Debug` 里调用 `self.stats()` 会获取锁
    /// 如果在被持有的锁保护下的代码里打印这个队列，
    /// 会触发 `Mutex` 的重入死锁。调用时要注意。
    ///
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ObservabilityQueue")
            .field("stats", &self.stats())
            .finish()
    }
}

impl ObservabilityQueue {
    /// 创建一个指定容量的队列。
    ///
    /// ⚠ 容量 0 是非法的
    /// 容量为 0 意味着任何东西都进不去，队列完全失去作用。
    /// 与其让它在后续操作里莫名其妙地全部被拒，不如在构造时就明确报错。
    /// 这是 fail-fast 原则。
    ///
    /// 【失败情况】
    /// `capacity == 0` 返回 `Err(InvalidCapacity)`。
    ///
    pub fn new(capacity: usize) -> Result<Self, ObservabilityQueueError> {
        if capacity == 0 {
            return Err(ObservabilityQueueError::InvalidCapacity);
        }
        Ok(Self {
            capacity,
            state: Arc::new(Mutex::new(QueueState::default())),
            item_available: Arc::new(Notify::new()),
            drained: Arc::new(Notify::new()),
        })
    }

    /// 取一份当前统计快照。
    ///
    /// ⚠ 关于 `unwrap_or_else(PoisonError::into_inner)` —— 本文件出现很多次
    /// 当一个线程持锁时 panic 了，锁会被标记为中毒（poisoned）。
    /// 标准库的 `lock().unwrap()` 在这种情况下会 panic，把毒性传染给每一个后续调用者。
    ///
    /// `unwrap_or_else(PoisonError::into_inner)` 的意思是：
    /// 拿到毒锁，但把里面的数据取出来继续用。
    ///
    /// 【为什么这里可以安全地忽略毒性】
    /// 因为被保护的 `QueueState` 是一个纯数据容器：
    /// push、pop、计数器自增这些操作都不会 panic 到中途，
    /// 所以锁中毒时里面的数据仍然是结构完整的。
    /// 对纯数据容器忽略锁毒性是合理的。
    ///
    /// ⚠ 但这个判断不适用于你在别处看到的类似写法
    /// 如果锁保护的是一个"多步操作中间态"（比如先改 A 再改 B），
    /// 中毒时数据可能只改了一半，那时就不能忽略毒性。
    /// 照搬这个模式前请先确认自己保护的是什么。
    ///
    pub fn stats(&self) -> ObservabilityQueueStats {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        ObservabilityQueueStats {
            capacity: self.capacity,
            depth: state.items.len(),
            enqueued_total: state.enqueued_total,
            dequeued_total: state.dequeued_total,
            dropped_best_effort_total: state.dropped_best_effort_total,
            critical_rejected_total: state.critical_rejected_total,
            flush_sequence: state.flush_sequence,
            reopen_total: state.reopen_total,
            closed: state.closed,
            last_drop_reason: state.last_drop_reason.clone(),
        }
    }

    /// 记录最近一次丢弃的原因。
    ///
    /// 【作用】
    /// 覆盖式的：只保留最近一次，不做历史累积。
    /// 这样字段长度恒定有界，不会因为反复丢弃而无限增长。
    ///
    /// 【为什么是 `&mut QueueState` 而不是 `&self`】
    /// 因为它要修改状态内部。调用方已经持有锁，所以这里不该再自己加锁。
    ///
    fn set_drop_reason(state: &mut QueueState, reason: &str) {
        let mut reason = reason.to_owned();
        reason.truncate(MAX_DROP_REASON_BYTES);
        state.last_drop_reason = Some(reason);
    }

    /// Try to enqueue without waiting. A critical item may evict one best-effort item, never a
    /// critical item. If all slots are critical, the caller gets a synchronous refusal.
    /// 尝试入队，永不等待。
    ///
    /// 【核心流程 —— 队列满了怎么办】
    /// 这是本方法最关键的部分。分三种情况：
    ///
    /// 情况 1：队列没满
    /// 直接 `push_back`，成功。
    ///
    /// 情况 2：队列满了，要入队的是关键项
    /// 找一条尽力而为的项把它挤掉，腾出位置。
    /// - 找到了：挤掉它，计数 `dropped_best_effort_total` 加一，入队成功；
    /// - 找不到（全是关键项）：计数 `critical_rejected_total` 加一，
    ///   返回 `Err(CriticalQueueFull)`。
    ///
    /// 情况 3：队列满了，要入队的是尽力而为项
    /// 直接丢弃自己，计数加一，返回 `Err(BestEffortDropped)`。
    ///
    /// ⚠ 为什么关键项可以挤掉尽力而为项，反之不行
    /// 这条规则保证了：关键事实之间不会互相挤掉。
    /// 如果关键项也能挤掉关键项，
    /// 那么一个瞬时的事件洪峰就会把队列里最早的关键事实挤出去，
    /// 而那些事实可能是审批记录、审计记录 —— 挤掉它们等于丢掉证据。
    ///
    /// ⚠ 为什么找不到位置时必须报错，而不是让关键项也挤掉关键项
    /// 报错让调用方知道"这条事实没投递"，从而触发它自己的保留和重扫逻辑。
    /// 静默挤掉会让调用方以为一切正常，
    /// 然后那条关键事实就永远没人去取了 —— 这是静默丢证据。
    /// 这是 fail-closed 在队列设计里的体现：宁可显式失败，不肯静默丢失。
    ///
    /// ⚠ 关于 `saturating_add`
    /// 所有计数器都用自增的饱和版本。
    /// 如果用 `+= 1`，理论上溢出会 panic（debug 构建）或回绕（release 构建）。
    /// `saturating_add` 让它停在最大值，永不 panic。
    /// 对一个纯统计数字来说饱和到顶是可接受的；
    /// 而在这里 panic 会把一个计数问题升级成系统故障。
    ///
    /// ⚠ 为什么 `drop(state)` 要显式写在通知之前
    /// 必须先释放锁，再发通知。
    /// 显式 `drop` 让"释放"和"通知"两件事的顺序变得明确，
    /// 不会因为以后有人调整语句位置而改变语义。
    ///
    /// 【关于 `notify_one` vs `notify_waiters`】
    /// 入队时用 `notify_one`：只需要唤醒一个消费者。
    /// 唤醒全部会让其他消费者白白醒来发现队列空，又回去睡。
    /// shutdown 时用 `notify_waiters`：那时候所有等待者都需要知道"别等了"。
    ///
    pub fn try_enqueue(
        &self,
        item: QueuedObservabilityItem,
    ) -> Result<(), ObservabilityQueueError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return Err(ObservabilityQueueError::Closed);
        }
        if state.items.len() >= self.capacity {
            if item.class.is_critical() {
                if let Some(index) = state
                    .items
                    .iter()
                    .position(|queued| !queued.class.is_critical())
                {
                    state.items.remove(index);
                    state.dropped_best_effort_total =
                        state.dropped_best_effort_total.saturating_add(1);
                    Self::set_drop_reason(&mut state, "critical_admission_evicted_best_effort");
                } else {
                    state.critical_rejected_total = state.critical_rejected_total.saturating_add(1);
                    Self::set_drop_reason(&mut state, "critical_queue_full");
                    return Err(ObservabilityQueueError::CriticalQueueFull { class: item.class });
                }
            } else {
                state.dropped_best_effort_total = state.dropped_best_effort_total.saturating_add(1);
                Self::set_drop_reason(&mut state, "best_effort_queue_full");
                return Err(ObservabilityQueueError::BestEffortDropped {
                    reason: "best_effort_queue_full".to_owned(),
                });
            }
        }
        state.items.push_back(item);
        state.enqueued_total = state.enqueued_total.saturating_add(1);
        drop(state);
        self.item_available.notify_one();
        Ok(())
    }

    /// 尝试取一项，取不到返回 `None`。不会等待。
    ///
    /// 【核心流程】
    /// 从队首取出（`pop_front`）—— 先进先出（FIFO）。
    /// 取到后才更新计数；如果取完队列空了，
    /// 才递增 `flush_sequence` 并唤醒所有等待 flush 的调用方。
    ///
    /// ⚠ 为什么"取空"才递增 flush_sequence
    /// `flush_sequence` 的语义是"队列被清空过几次"。
    /// 如果每次出队都递增，队列持续有数据时计数会一直涨，
    /// `flush()` 也就失去了判断依据。
    /// 只在真正从有到无的那一刻递增，才对应一次完整的清空。
    ///
    /// 【关于 `notify_waiters`】
    /// 这里用 `notify_waiters`（唤醒全部）而不是 `notify_one`：
    /// 队列空了，所有等待 flush 的调用方都关心这个状态。
    ///
    pub fn try_dequeue(&self) -> Option<QueuedObservabilityItem> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let item = state.items.pop_front();
        if item.is_some() {
            state.dequeued_total = state.dequeued_total.saturating_add(1);
            if state.items.is_empty() {
                state.flush_sequence = state.flush_sequence.saturating_add(1);
                self.drained.notify_waiters();
            }
        }
        item
    }

    /// 取一项，取不到就等待直到有。
    ///
    /// 【核心流程 —— 一个标准的"检查-等待"循环】
    /// 1. 先试一次非阻塞取出；成功就返回；
    /// 2. 队列关了就返回 `None`（否则会永远等下去）；
    /// 3. 挂在 `item_available` 上等；
    /// 4. 被唤醒后回到第 1 步。
    ///
    /// ⚠ 为什么必须先试再等，不能直接等
    /// 这是并发编程里经典的条件变量陷阱：丢失唤醒（lost wakeup）。
    ///
    /// 如果先等后检查：
    ///     消费者 A:  检查队列 → 空
    ///     生产者 B:  放入一项 → 发出通知   ← A 还没开始等，通知丢失
    ///     消费者 A:  开始等待                ← 永远等不到
    ///
    /// 先检查再等，且检查失败后才注册等待，就堵住了这个窗口：
    ///     消费者 A:  检查队列 → 空 → 注册等待
    ///     生产者 B:  放入一项 → 发出通知     ← A 已经在等了，能收到
    ///
    /// ⚠ 不要把这个循环"优化"成先 `notified().await` 再 `try_dequeue()`。
    /// 那样会引入上面那个丢失唤醒的 bug，而且是偶发的、难复现的。
    ///
    /// ⚠ 第 2 步的关闭检查同样必要
    /// 如果没有它，队列关闭后所有等待的消费者会永久挂起，
    /// 持有它们的异步任务永远不结束。
    /// 关闭时 `shutdown()` 会调用 `notify_waiters()`，所以这里能立刻醒来并返回 `None`。
    ///
    pub async fn dequeue(&self) -> Option<QueuedObservabilityItem> {
        loop {
            if let Some(item) = self.try_dequeue() {
                return Some(item);
            }
            if self.stats().closed {
                return None;
            }
            self.item_available.notified().await;
        }
    }

    /// Wait until all queued items have been accepted by a consumer. It does not claim that an
    /// exporter persisted them; EventLog/MetricSink receipts remain separate evidence.
    /// 等到队列被取空。
    ///
    /// ⚠ 它承诺了什么，没承诺什么 —— 这段注释极其重要
    /// 原始注释说：它不声称导出器已经持久化了这些项；
    /// EventLog / MetricSink 的回执仍然是独立的证据。
    ///
    /// 翻译："队列空了" 不等于 "数据存好了"。
    ///
    /// 队列空只说明"消费方把东西从队列里拿走了"。
    /// 消费方拿走后是写进了 EventLog、写进了指标系统、还是直接丢了，
    /// 本方法一无所知。
    ///
    /// 为什么必须写清楚？因为这正是文件头强调的那个区分：
    /// 队列是投递辅助，永远不是事件账本。
    ///
    /// 如果有人误以为"flush 返回了就代表数据安全了"，
    /// 就会在消费方还没落盘时认为可以放心退出进程 —— 数据就没了。
    /// 真正证明数据安全的是 EventLog 的回执，不是队列的空。
    ///
    /// 【返回值】
    /// 当前的 `flush_sequence`（清空序号）。调用方可以对比前后两次的值，
    /// 判断"期间是否又发生了一次完整的清空"。
    ///
    pub async fn flush(&self) -> u64 {
        loop {
            let stats = self.stats();
            if stats.depth == 0 {
                return stats.flush_sequence;
            }
            self.drained.notified().await;
        }
    }

    /// 可取消版本的 `flush`。
    ///
    /// 【为什么需要这个版本】
    /// 因为 `flush` 可能永远等不到 ——
    /// 如果消费方卡住或已死，队列永远不会空。
    /// 一个不能取消的等待，在关停流程里会挂死整个进程。
    /// 这个版本允许外部在超时或用户取消时强行返回。
    ///
    /// 【核心流程】
    /// 循环检查三件事：
    /// 1. 队列空了 → 成功返回；
    /// 2. 已收到取消信号 → 返回 `Err(FlushCancelled)`；
    /// 3. 否则同时等待"队列被取空"或"取消信号变化"，谁先来就走谁。
    ///
    /// 【关于 `cancellation.borrow()` 与 `cancellation.changed()` 的配合】
    /// `borrow()` 是看当前值，不消耗；`changed()` 是等它变化。
    /// 两者配合才能既检查已有状态、又等待新状态。
    ///
    /// ⚠ `changed.is_err()` 的含义：发送端已消失
    /// `watch::Receiver::changed()` 在发送端被 drop 时返回 `Err`。
    /// 这时 `*cancellation.borrow()` 拿到的是最后一个值，未必是 `true`。
    ///
    /// 代码把 `is_err()` 也当作取消处理 —— 这是有意的保守选择：
    /// 发送端消失意味着没有东西能再改变这个标志，
    /// 那这个标志就永远停在当前值上。如果当前值是 `false`，
    /// 继续等下去就是永远等不到任何变化 —— 死等。
    ///
    /// 所以"发送端没了"被当作"当作取消"处理。
    /// 宁可提前返回让调用方重新检查，也不冒死等的风险。
    ///
    pub async fn flush_cancellable(
        &self,
        mut cancellation: tokio::sync::watch::Receiver<bool>,
    ) -> Result<u64, ObservabilityQueueError> {
        loop {
            let stats = self.stats();
            if stats.depth == 0 {
                return Ok(stats.flush_sequence);
            }
            if *cancellation.borrow() {
                return Err(ObservabilityQueueError::FlushCancelled);
            }
            tokio::select! {
                _ = self.drained.notified() => {},
                changed = cancellation.changed() => {
                    if changed.is_err() || *cancellation.borrow() {
                        return Err(ObservabilityQueueError::FlushCancelled);
                    }
                }
            }
        }
    }

    /// 关闭队列，返回关闭时刻的统计。
    ///
    /// 【核心流程】
    /// 1. 置 `closed = true`；
    /// 2. 组装并返回统计快照（此刻队列里还剩多少项）；
    /// 3. 释放锁；
    /// 4. 唤醒所有等待者和所有等待 flush 的调用方。
    ///
    /// ⚠ 关闭时不清空队列
    /// `state.items` 原封不动地保留。
    /// 这是有意的：关闭不等于丢弃。
    /// 剩余项还能被 `try_dequeue` 取走，也能在 `reopen` 后继续投递。
    /// 如果关闭时清空，那些还没送出去的关键事实就真的没了。
    ///
    /// ⚠ 为什么必须唤醒全部等待者
    /// 有两个群体的等待者需要被叫醒：
    /// - `dequeue` 里等"有活"的消费者 —— 它们醒来后会看到 `closed` 为真，
    ///   返回 `None` 正常退出；
    /// - `flush` 里等"取空"的调用方 —— 它们醒来后发现队列仍非空，会继续等。
    ///
    /// ⚠ 一个真实的边界情况
    /// 如果关闭时队列里还有项，等待 flush 的调用方被唤醒后
    /// 会再次检查深度非零，然后重新进入等待。
    /// 只要没有消费者来取空它，它们就会一直等。
    /// 调用方如果需要在关闭后确保退出，应该用 `flush_cancellable` 而不是 `flush`。
    ///
    /// 【返回值的作用】
    /// 返回的统计里 `closed` 为真，深度是关闭时的剩余量，
    /// 调用方可以据此知道"关闭时还有多少没送出去"。
    ///
    pub fn shutdown(&self) -> ObservabilityQueueStats {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        let stats = ObservabilityQueueStats {
            capacity: self.capacity,
            depth: state.items.len(),
            enqueued_total: state.enqueued_total,
            dequeued_total: state.dequeued_total,
            dropped_best_effort_total: state.dropped_best_effort_total,
            critical_rejected_total: state.critical_rejected_total,
            flush_sequence: state.flush_sequence,
            reopen_total: state.reopen_total,
            closed: true,
            last_drop_reason: state.last_drop_reason.clone(),
        };
        drop(state);
        self.item_available.notify_waiters();
        self.drained.notify_waiters();
        stats
    }

    /// Reopen delivery after a controlled shutdown. Existing queued items are retained; this is
    /// a queue lifecycle operation, not a claim that a durable spool survived a process crash.
    /// 重新打开队列。
    ///
    /// 【作用】
    /// 撤销一次 `shutdown`。用于受控的"暂停-恢复"场景。
    ///
    /// ⚠ 这不是崩溃恢复
    /// 原始注释特意强调：这是队列生命周期操作，
    /// 并不是说某个持久化缓冲在进程崩溃后幸存了下来。
    ///
    /// 翻译：重开队列不等于数据恢复。
    ///
    /// `reopen` 只能恢复内存里还在的项。
    /// 如果进程崩溃了，队列内容全部丢失（它只在内存中，没有落盘），
    /// `reopen` 对此毫无帮助 —— 什么也恢复不了。
    ///
    /// 所以不能把 `reopen` 当成崩溃恢复手段。
    /// 真正的事实保存在 EventLog 里，崩溃后应该从 EventLog 重新扫描，
    /// 而不是指望这个队列能自己恢复。这一点必须分清。
    ///
    /// 【为什么 `reopen_total` 要计数】
    /// 反复重开说明有某种循环在关停/启动之间来回。
    /// 正常流程里重开应该是罕见事件，计数能帮助发现异常。
    pub fn reopen(&self) -> ObservabilityQueueStats {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = false;
        state.reopen_total = state.reopen_total.saturating_add(1);
        drop(state);
        self.stats()
    }
}
