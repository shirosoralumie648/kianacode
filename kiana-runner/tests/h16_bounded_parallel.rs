//! H16 有界并行工具组与独占屏障的运行期行为夹具。
//!
//! 这些用例直接驱动 `kiana_runner::ToolBatchScheduler`：`admit` 代表派发前一刻的逐调用授权
//! 复查，`execute` 代表真正的能力请求路径（此处用可观测的合成负载替代）。并发与屏障用
//! 会合点（rendezvous）和原子计数器证明，而不是靠总耗时推断；历史顺序用源 ordinal 断言。
//! 这里不构造真实 CapabilityRequest，也不替代 H12 的串行 loop 与 H13 的 Invocation ledger。
use kiana_domain::{plan_tool_batch, ModelToolCall, ToolBatchParallelismCap};
use kiana_runner::{ToolBatchAdmission, ToolBatchCallState};
use kiana_runner::{ToolBatchRunReport, ToolBatchScheduler};
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// 三来源（run/部署/角色）取值相同的并行额度上限。
fn cap(limit: u32) -> ToolBatchParallelismCap {
    ToolBatchParallelismCap::new(limit, limit, limit)
}

/// 合成的只读 `memory.search` 调用；ID 同时用作查询文本。
fn read_call(id: &str) -> ModelToolCall {
    ModelToolCall {
        id: id.to_owned(),
        name: "memory.search".to_owned(),
        arguments: json!({"query": id}),
    }
}

/// 合成的独占 `memory.write` 调用；参数只用于构造能力请求形状。
fn write_call(id: &str) -> ModelToolCall {
    let arguments = json!({"collection": "scratch", "text": id, "source": "h16"});
    ModelToolCall {
        id: id.to_owned(),
        name: "memory.write".to_owned(),
        arguments,
    }
}

/// 读取互斥量内容并克隆一份；中毒时取回内部值，避免夹具因其它线程 panic 而连挂。
fn snapshot<T: Clone>(cell: &Mutex<T>) -> T {
    let guard = cell.lock().unwrap_or_else(|poison| poison.into_inner());
    guard.clone()
}

/// 记录一次完成/派发顺序。
fn record(cell: &Mutex<Vec<usize>>, value: usize) {
    let mut guard = cell.lock().unwrap_or_else(|poison| poison.into_inner());
    guard.push(value);
}

/// 历史里的 call ID，按记录顺序（源顺序）。
fn call_ids(report: &ToolBatchRunReport) -> Vec<String> {
    let mut ids = Vec::new();
    for outcome in &report.history {
        ids.push(outcome.call_id.clone());
    }
    ids
}

/// 历史里的源 ordinal，按记录顺序。
fn ordinals(report: &ToolBatchRunReport) -> Vec<usize> {
    let mut ordinals = Vec::new();
    for outcome in &report.history {
        ordinals.push(outcome.ordinal);
    }
    ordinals
}

/// 历史是否全部为已知成功结果。
fn all_completed(report: &ToolBatchRunReport) -> bool {
    for outcome in &report.history {
        if outcome.state != ToolBatchCallState::Completed {
            return false;
        }
    }
    true
}

/// 一个带超时的会合点：只有 `parties` 个线程都到达才放行。
///
/// 用它而不是 `std::sync::Barrier` 是因为后者没有超时；一旦调度器没有真正并发，普通
/// `Barrier` 会让整个测试挂死。这里超时即返回 `false`，测试据此明确失败。
struct Rendezvous {
    parties: usize,
    arrived: Mutex<usize>,
    ready: Condvar,
}

impl Rendezvous {
    fn new(parties: usize) -> Self {
        Self {
            parties,
            arrived: Mutex::new(0),
            ready: Condvar::new(),
        }
    }

    /// 等待所有参与方到达；超时返回 `false`。
    fn wait(&self, timeout: Duration) -> bool {
        let guard = self.arrived.lock();
        let mut arrived = guard.unwrap_or_else(|poison| poison.into_inner());
        *arrived += 1;
        self.ready.notify_all();
        let deadline = Instant::now() + timeout;
        while *arrived < self.parties {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let waited = self.ready.wait_timeout(arrived, remaining);
            let (guard, result) = waited.unwrap_or_else(|poison| poison.into_inner());
            arrived = guard;
            if result.timed_out() && *arrived < self.parties {
                break;
            }
        }
        *arrived >= self.parties
    }
}

/// 相邻只读调用真的并发执行（会合点放行），但模型历史仍按源 ordinal 排列。
///
/// 完成顺序被刻意错开并且断言“不等于源顺序”，用来证明历史不是按完成先后拼接的。
#[test]
fn independent_reads_overlap_but_history_is_source_ordered() {
    let calls = vec![read_call("one"), read_call("two"), read_call("three")];
    let plan = plan_tool_batch(&calls).expect("three memory.search calls plan");
    let scheduler = ToolBatchScheduler::new(plan, cap(4)).expect("validated plan");

    let rendezvous = Arc::new(Rendezvous::new(3));
    let completion = Arc::new(Mutex::new(Vec::new()));
    let delays = [120u64, 60, 0];
    let rendezvous_in_execute = Arc::clone(&rendezvous);
    let completion_in_execute = Arc::clone(&completion);

    let report = scheduler.run(
        |_ordinal, _call_id| ToolBatchAdmission::Allow,
        move |ordinal, _call_id| {
            if !rendezvous_in_execute.wait(Duration::from_secs(5)) {
                return ToolBatchCallState::Failed("reads_did_not_overlap".to_owned());
            }
            thread::sleep(Duration::from_millis(delays[ordinal]));
            record(&completion_in_execute, ordinal);
            ToolBatchCallState::Completed
        },
    );

    assert_eq!(report.dispatched, 3);
    assert_eq!(report.max_in_flight, 3);
    assert_eq!(report.not_executed, 0);
    assert!(all_completed(&report));
    assert_eq!(call_ids(&report), ["one", "two", "three"]);
    assert_eq!(ordinals(&report), vec![0, 1, 2]);
    let completion_order = snapshot(&completion);
    assert_ne!(
        completion_order,
        vec![0, 1, 2],
        "completion order == source"
    );
}

/// 独占写调用形成屏障：与前后任何只读调用都不重叠。
///
/// 两个读调用用会合点证明它们自身并发；写调用在其执行期间读取活跃读计数，必须恒为 0。
#[test]
fn write_barrier_never_overlaps_prior_reads() {
    let calls = vec![
        read_call("read-a"),
        read_call("read-b"),
        write_call("write"),
    ];
    let plan = plan_tool_batch(&calls).expect("read/read/write plan");
    assert_eq!(plan.groups.len(), 2);
    let scheduler = ToolBatchScheduler::new(plan, cap(4)).expect("validated plan");

    let rendezvous = Arc::new(Rendezvous::new(2));
    let read_active = Arc::new(AtomicUsize::new(0));
    let write_active = Arc::new(AtomicUsize::new(0));
    let overlap = Arc::new(AtomicBool::new(false));
    let rendezvous_in_execute = Arc::clone(&rendezvous);
    let read_active_in_execute = Arc::clone(&read_active);
    let write_active_in_execute = Arc::clone(&write_active);
    let overlap_in_execute = Arc::clone(&overlap);

    let report = scheduler.run(
        |_ordinal, _call_id| ToolBatchAdmission::Allow,
        move |ordinal, _call_id| {
            if ordinal < 2 {
                read_active_in_execute.fetch_add(1, Ordering::SeqCst);
                if write_active_in_execute.load(Ordering::SeqCst) != 0 {
                    overlap_in_execute.store(true, Ordering::SeqCst);
                }
                let overlapped = rendezvous_in_execute.wait(Duration::from_secs(5));
                read_active_in_execute.fetch_sub(1, Ordering::SeqCst);
                if overlapped {
                    ToolBatchCallState::Completed
                } else {
                    ToolBatchCallState::Failed("reads_did_not_overlap".to_owned())
                }
            } else {
                write_active_in_execute.store(1, Ordering::SeqCst);
                if read_active_in_execute.load(Ordering::SeqCst) != 0 {
                    overlap_in_execute.store(true, Ordering::SeqCst);
                }
                thread::sleep(Duration::from_millis(20));
                if read_active_in_execute.load(Ordering::SeqCst) != 0 {
                    overlap_in_execute.store(true, Ordering::SeqCst);
                }
                write_active_in_execute.store(0, Ordering::SeqCst);
                ToolBatchCallState::Completed
            }
        },
    );

    assert!(!overlap.load(Ordering::SeqCst), "write overlapped a read");
    assert_eq!(read_active.load(Ordering::SeqCst), 0);
    assert_eq!(write_active.load(Ordering::SeqCst), 0);
    assert_eq!(report.dispatched, 3);
    assert!(all_completed(&report));
    assert_eq!(call_ids(&report), ["read-a", "read-b", "write"]);
}

/// authority 撤销后，尚未开始的兄弟调用不再派发，并形成 not_executed 历史。
#[test]
fn revocation_blocks_not_started_parallel_call() {
    let calls = vec![read_call("a"), read_call("b"), read_call("c")];
    let plan = plan_tool_batch(&calls).expect("three read calls plan");
    let scheduler = ToolBatchScheduler::new(plan, cap(3)).expect("validated plan");

    let executed = Arc::new(Mutex::new(Vec::new()));
    let executed_in_execute = Arc::clone(&executed);
    let report = scheduler.run(
        |ordinal, _call_id| {
            if ordinal == 1 {
                ToolBatchAdmission::NotExecuted("authority_revoked".to_owned())
            } else {
                ToolBatchAdmission::Allow
            }
        },
        move |ordinal, _call_id| {
            record(&executed_in_execute, ordinal);
            ToolBatchCallState::Completed
        },
    );

    assert_eq!(snapshot(&executed), vec![0], "a revoked sibling still ran");
    assert_eq!(report.dispatched, 1);
    assert_eq!(report.not_executed, 2);
    assert!(report.halted);
    let revoked = ToolBatchCallState::NotExecuted("authority_revoked".to_owned());
    assert_eq!(report.history[1].state, revoked);
    assert!(matches!(
        report.history[2].state,
        ToolBatchCallState::NotExecuted(_)
    ));
}

/// 已提交的后续结果在恢复时被复用，不会重新执行。
#[test]
fn crash_with_later_outcome_ready_does_not_rerun_it() {
    let calls = vec![read_call("first"), read_call("second")];
    let plan = plan_tool_batch(&calls).expect("two read calls plan");
    let scheduler = ToolBatchScheduler::new(plan, cap(2)).expect("validated plan");

    let executed = Arc::new(Mutex::new(Vec::new()));
    let executed_in_execute = Arc::clone(&executed);
    let report = scheduler.run(
        |ordinal, _call_id| {
            if ordinal == 1 {
                ToolBatchAdmission::AlreadyCommitted
            } else {
                ToolBatchAdmission::Allow
            }
        },
        move |ordinal, _call_id| {
            record(&executed_in_execute, ordinal);
            ToolBatchCallState::Completed
        },
    );

    assert_eq!(
        snapshot(&executed),
        vec![0],
        "an already committed call reran"
    );
    assert_eq!(report.reused, 1);
    assert_eq!(report.dispatched, 1);
    assert_eq!(report.history[1].call_id, "second");
    assert_eq!(report.history[1].state, ToolBatchCallState::Reused);
}

/// 并发额度为 1 时行为与 H12 的串行批次一致：同一时刻至多一个调用在途。
#[test]
fn concurrency_one_matches_the_serial_batch_contract() {
    let calls = vec![read_call("one"), read_call("two"), read_call("three")];
    let plan = plan_tool_batch(&calls).expect("three read calls plan");
    let scheduler = ToolBatchScheduler::new(plan, cap(1)).expect("validated plan");

    let active = Arc::new(AtomicUsize::new(0));
    let high_water = Arc::new(AtomicUsize::new(0));
    let order = Arc::new(Mutex::new(Vec::new()));
    let active_in_execute = Arc::clone(&active);
    let high_water_in_execute = Arc::clone(&high_water);
    let order_in_execute = Arc::clone(&order);

    let report = scheduler.run(
        |_ordinal, _call_id| ToolBatchAdmission::Allow,
        move |ordinal, _call_id| {
            let now = active_in_execute.fetch_add(1, Ordering::SeqCst) + 1;
            high_water_in_execute.fetch_max(now, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(5));
            record(&order_in_execute, ordinal);
            active_in_execute.fetch_sub(1, Ordering::SeqCst);
            ToolBatchCallState::Completed
        },
    );

    assert_eq!(high_water.load(Ordering::SeqCst), 1);
    assert_eq!(report.max_in_flight, 1);
    assert_eq!(snapshot(&order), vec![0, 1, 2]);
    assert_eq!(ordinals(&report), vec![0, 1, 2]);
}
