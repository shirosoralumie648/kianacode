//! Daemon 内统一的子进程/进程组启动保护、资源限制和停止确认实现。
//!
//! 上游 shell capability 与 `ExecutionControl::start()` 在 spawn 前调用
//! [`ProcessSupervisor::prepare_command`]；命令启动后保留 leader PID/进程组 ID，退出、取消、
//! 超时或 authority 变化时调用 [`ProcessSupervisor::stop`]。停止结果变成 Domain
//! [`StopReport`]，由上层写入 process 事实或决定是否释放路径 lease。
//!
//! ```text
//! ExecutionControl / shell handler
//!     -> 构造 sandbox Command 与 ProcessResourceBudget
//!     -> prepare_command：新建 Unix process group，配置可用 rlimit
//!     -> spawn child
//!     -> stop：TERM → 有限等待 → KILL → 检查 leader / group
//!     -> StopReport.confirmed 决定能否把执行从 Unknown 收敛
//! ```
//!
//! 所有系统限制都受平台和操作系统语义约束。Unix rlimit 主要限制单个进程；RLIMIT_AS 不是
//! RSS/整棵进程树内存证明，RLIMIT_NPROC 在 Linux 上按 real UID 统计，并非只数当前任务的
//! 子进程。wall-clock deadline 由调用方监督循环计时；本模块不负责创建它。非 Unix 编译目标
//! 不配置 process group 或 rlimit，也无法通过当前 group probe 给出停止确认。回执中的静态
//! enforcement 标签不等于逐次运行时探测。

use kiana_domain::{ProcessGroupState, ProcessResourceBudget, StopMethod, StopReport};
use kiana_ports::PortError;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::process::{Child, Command};

/// TERM 后最多等待 leader 100ms；它不是整个进程组的存活宽限期。
const TERM_GRACE: Duration = Duration::from_millis(100);
/// KILL 后最多再等 Tokio Child leader 500ms，再做一次进程组存在性观察。
const KILL_GRACE: Duration = Duration::from_millis(500);

/// 无状态工具集合；所有需要跟踪的 PID/Child/lease 由调用方显式传入。
pub(crate) struct ProcessSupervisor;

impl ProcessSupervisor {
    /// 校验预算并在 spawn 前为命令配置进程组、drop 清理和可用 OS 资源限制。
    ///
    /// 【调用者】Daemon shell command 构造路径和长进程执行控制路径。
    /// 【输入/输出】可变 Tokio `Command` 与领域预算；成功后仍需调用方 spawn，失败返回
    /// `PortError::Failed`。Unix 下任何 `setrlimit` 失败会在子进程 exec 前导致启动失败。
    /// 【实际设置】所有平台先启用 `kill_on_drop`；Unix 令 leader 建立新 process group，并在
    /// `pre_exec` 中设置 CORE=0、CPU、NOFILE、FSIZE、AS、NPROC。CPU 毫秒会向上取整到秒；
    /// 其余限制以相同值设软/硬上限。
    /// 【边界】不会设置 wall-clock deadline、stdout/stderr 上限、RSS 或 cgroup 聚合限制；
    /// 这些需要调用方或别的后端实施。非 Unix 只执行 `kill_on_drop`，却仍可能由调用方生成
    /// 包含 Unix enforcement 标签的 receipt。
    /// 【为什么使用 pre_exec】限制必须对子进程在 exec 前生效，避免命令已经开始消耗资源后
    /// 再尝试补限；该钩子运行在 fork 后的子进程路径，只保留这里所需的低层系统调用。
    pub(crate) fn prepare_command(
        command: &mut Command,
        budget: &ProcessResourceBudget,
    ) -> Result<(), PortError> {
        budget
            .validate()
            .map_err(|error| PortError::Failed(error.to_owned()))?;
        // 即使调用方异常提前 drop Child，也让 Tokio 尝试终止直接 child；这只是兜底，不等同于
        // 后续 `stop()` 对 process group 的有界检查和 confirmed 回执。
        command.kill_on_drop(true);
        #[cfg(unix)]
        {
            // 0 表示让 OS 为新 child 创建以 child PID 为组 ID 的独立 process group，便于后续
            // 对同组发送 TERM/KILL，而不是只停止直接 child。
            command.process_group(0);
            let budget = budget.clone();
            unsafe {
                command.pre_exec(move || {
                    for (resource, value) in [
                        (libc::RLIMIT_CORE, 0u64),
                        (libc::RLIMIT_CPU, budget.max_cpu_time_ms.div_ceil(1_000)),
                        (libc::RLIMIT_NOFILE, budget.max_open_files),
                        (libc::RLIMIT_FSIZE, budget.max_file_bytes),
                        (libc::RLIMIT_AS, budget.max_address_space_bytes),
                        (libc::RLIMIT_NPROC, budget.max_processes),
                    ] {
                        let limit = libc::rlimit {
                            rlim_cur: value as libc::rlim_t,
                            rlim_max: value as libc::rlim_t,
                        };
                        if libc::setrlimit(resource, &limit) != 0 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
        }
        Ok(())
    }

    /// 将预算数值和实现者声明的 enforcement 名称投影到 JSON 回执。
    ///
    /// 此处不读取 OS 当前限制、不统计 RSS/PID，也不保证 receipt 中每个 max 字段被强制执行；
    /// 调用者仍需结合平台、prepare_command 的实际路径和独立 output/deadline 逻辑解释它。
    pub(crate) fn resource_receipt(budget: &ProcessResourceBudget) -> Value {
        json!({
            "schema": budget.schema,
            "max_wall_time_ms": budget.max_wall_time_ms,
            "max_cpu_time_ms": budget.max_cpu_time_ms,
            "max_address_space_bytes": budget.max_address_space_bytes,
            "max_processes": budget.max_processes,
            "max_open_files": budget.max_open_files,
            "max_file_bytes": budget.max_file_bytes,
            "max_output_bytes": budget.max_output_bytes,
            "enforcement": {
                "wall_time": "supervisor_deadline",
                "cpu_time": "per_process_rlimit_cpu",
                "address_space": "per_process_rlimit_as",
                "rss": "observed_only",
                "pids": "per_process_rlimit_nproc",
                "aggregate_memory": "not_hard_enforced",
                "aggregate_pids": "not_hard_enforced",
                "file_and_tmp_quota": "rlimit_fsize_plus_workspace_policy"
            }
        })
    }

    /// 创建 spawn 后的 best-effort 兜底 guard，正常拿到停止确认后须调用 `disarm()`。
    pub(crate) fn guard(pid: Option<u32>) -> ProcessGuard {
        ProcessGuard { pid, active: true }
    }

    /// 便捷判断给定 ID 的进程组是否明确处于 Present 状态。
    ///
    /// Empty 和 Unknown 都会映射成 false，因此需要区分“确定不存在”与“无法观察”的调用方
    /// 应直接使用内部三态观察结果，而不能只用此 bool。
    pub(crate) fn process_group_exists(pid: u32) -> bool {
        observe_process_group(pid) == ProcessGroupState::Present
    }

    /// 尝试终止直接 child 和其 Unix process group，返回受限时长的停止事实。
    ///
    /// 【调用者】shell 的超时/取消/退出处理、`ExecutionControl` 后台进程监督等。
    /// 【输入】execution ID、Tokio Child，以及 spawn 时保留的 process group ID；不要等 leader
    /// 退出后再尝试从 `child.id()` 获取 PGID，因为 Child 被 reap 后 PID 可能已不可读。
    /// 【核心流程】读取 leader 状态并观察 group；尚未完全停止时先发 TERM；如果 leader 尚未回收，
    /// 最多等它 100ms（不会专门等待整组 100ms）；若随后观察到 group 非 Empty/Unknown，则发 KILL；
    /// leader 最多再等待 500ms，最后重查 group。仅 leader 已
    /// reap、PGID 存在且最终 group=Empty 时 `confirmed=true`。
    /// 【输出】StopReport 记录尝试过的信号、leader 回收状态、最终 group 状态和耗时。它没有
    /// 逐一 wait/reap group 内所有后代；Empty 是最终存在性检查，不是每个后代退出码清单。
    /// 【失败/限制】信号发送失败、无 PGID、权限不足或 group 状态 Unknown 都不会变成确认成功；
    /// 本方法仍可能在有限等待后返回 Unknown，调用方必须继续保留 fencing/lease。
    /// 【非 Unix】group probe 固定 Unknown、group signal 固定失败；存在 PGID 时代码不会回退到
    /// `child.start_kill()`，因此无法按此路径确认 group 已停止。没有 PGID 时会尝试杀 leader，
    /// 但最终 group 仍 Unknown，confirmed 仍为 false。
    /// 【设计原因】只等待 Tokio leader 不能证明其派生进程已停；需要同时观察 OS process group，
    /// 观察不确定时宁可返回 Unknown，也不能把权限错误或不可观测误当 Empty。
    pub(crate) async fn stop(
        execution_id: &str,
        child: &mut Child,
        process_group_id: Option<u32>,
    ) -> StopReport {
        // 使用单调时钟测 stop 调用持续时间；它只测这次 TERM/KILL 过程，不是进程 wall-time deadline。
        let started = tokio::time::Instant::now();
        let mut leader_reaped = matches!(child.try_wait(), Ok(Some(_)));
        let mut term_sent = false;
        let mut kill_sent = false;

        let mut group_state = process_group_id
            .map(observe_process_group)
            .unwrap_or(ProcessGroupState::Unknown);
        // leader 尚未回收，或 group 仍有成员/无法确认为空时，先尝试优雅 TERM。
        if !leader_reaped || group_state != ProcessGroupState::Empty {
            if let Some(group) = process_group_id {
                if signal_process_group(group, libc::SIGTERM) {
                    term_sent = true;
                }
            } else if child.start_kill().is_ok() {
                kill_sent = true;
            }
        }

        if !leader_reaped {
            leader_reaped = wait_leader(child, TERM_GRACE).await;
        }
        group_state = process_group_id
            .map(observe_process_group)
            .unwrap_or(ProcessGroupState::Unknown);
        // Unknown 也走强制终止分支，因为“不知道是否为空”不能被当成安全停止。
        if group_state != ProcessGroupState::Empty {
            if let Some(group) = process_group_id {
                if signal_process_group(group, libc::SIGKILL) {
                    kill_sent = true;
                }
            } else if child.start_kill().is_ok() {
                kill_sent = true;
            }
        }
        if !leader_reaped {
            leader_reaped = wait_leader(child, KILL_GRACE).await;
        }
        group_state = process_group_id
            .map(observe_process_group)
            .unwrap_or(ProcessGroupState::Unknown);
        // Domain 校验也会检查确认条件；这里额外要求 PGID 存在，避免仅凭 leader 退出作承诺。
        let confirmed =
            leader_reaped && process_group_id.is_some() && group_state == ProcessGroupState::Empty;
        let method = if kill_sent {
            StopMethod::Kill
        } else if term_sent {
            StopMethod::Term
        } else {
            StopMethod::None
        };
        StopReport::new(
            execution_id,
            child.id(),
            process_group_id,
            method,
            term_sent,
            kill_sent,
            leader_reaped,
            group_state,
            confirmed,
            started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        )
        .unwrap_or_else(|_| StopReport::unknown(execution_id))
    }
}

/// Child 创建后到停止确认前的析构兜底。
///
/// 它不拥有 Tokio Child，也不返回停止报告；它只保留 spawn 得到的进程组 ID，并在未 disarm
/// 时尽力发送一次 SIGKILL，防止异常 return 让清理完全无人负责。
pub(crate) struct ProcessGuard {
    /// spawn 时捕获的 PID；Unix 上也作为新 process group ID 使用。
    pid: Option<u32>,
    /// true 表示析构时仍需执行 best-effort kill；只有确认停止后才应清零。
    active: bool,
}

impl ProcessGuard {
    /// 关闭析构兜底；调用方应先取得 StopReport.confirmed，不能只因发过 kill 就 disarm。
    pub(crate) fn disarm(&mut self) {
        self.active = false;
    }
}

impl Drop for ProcessGuard {
    /// 对仍 active 的 Unix process group 尝试发送一次 SIGKILL。
    ///
    /// 这是最后防线：忽略发送错误、不等待 leader 或后代退出、不检查组状态，也不会生成确认回执。
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            let _ = signal_process_group(pid, libc::SIGKILL);
        }
    }
}

/// 在指定时限内等待 Tokio 直接 child；只报告 leader 是否可被 wait 回收。
///
/// 超时、`Child::wait()` 错误或 Tokio timeout 均映射为 false；此结果不观察进程组后代。
async fn wait_leader(child: &mut Child, timeout: Duration) -> bool {
    matches!(tokio::time::timeout(timeout, child.wait()).await, Ok(Ok(_)))
}

#[cfg(unix)]
/// 向整个 Unix process group 发送信号；负数 PID 是 `kill(2)` 的进程组寻址约定。
///
/// PID 不能转换为 libc 的有符号 pid_t 时返回 false。系统调用成功只代表 OS 接受信号，
/// 不代表组内每个成员已经退出，因此还要由 `observe_process_group()` 复查。
fn signal_process_group(pid: u32, signal: libc::c_int) -> bool {
    let Ok(pgid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // SAFETY: `pgid` 是上面确认可表示的非负 PID，取负值用于 process-group 目标；kill 不解引用外部指针。
    unsafe { libc::kill(-pgid, signal) == 0 }
}

#[cfg(unix)]
/// 通过 `kill(-pgid, 0)` 检查进程组状态；信号 0 只做存在/权限检查，不终止进程。
///
/// 返回 0 表示仍观察到 group；只有 ESRCH 表示 Empty；EPERM 等其他错误保持 Unknown，避免把
/// “当前用户无权观察”误当作“进程组已经不存在”。
fn observe_process_group(pid: u32) -> ProcessGroupState {
    let Ok(pgid) = libc::pid_t::try_from(pid) else {
        return ProcessGroupState::Unknown;
    };
    let result = unsafe { libc::kill(-pgid, 0) };
    if result == 0 {
        return ProcessGroupState::Present;
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => ProcessGroupState::Empty,
        _ => ProcessGroupState::Unknown,
    }
}

#[cfg(not(unix))]
/// 当前非 Unix 实现无法查询进程组，保守返回 Unknown。
fn observe_process_group(_: u32) -> ProcessGroupState {
    ProcessGroupState::Unknown
}

#[cfg(not(unix))]
/// 当前非 Unix 实现没有进程组信号 backend，表示信号未确认发送。
fn signal_process_group(_: u32, _: i32) -> bool {
    false
}

#[cfg(all(test, unix))]
/// ER-19 停止证据行为夹具：真实 spawn 一个子进程，核对 leader 回收与进程组观察
/// 如何决定 `confirmed`。这些夹具只证明本机 supervisor 的观察语义，不是
/// 跨主机 fencing、container 后代或掉电恢复证据。
mod er19_tests {
    use super::*;

    /// 构造一个可被 TERM/KILL 结束的长驻子进程命令。
    fn sleeping_command() -> Command {
        let mut command = Command::new("sleep");
        command.arg("30");
        command.kill_on_drop(true);
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        command
    }

    /// 成功路径：leader 在可观察进程组内被回收，且组被观察为空时才算确认停止。
    #[tokio::test]
    async fn er19_stop_confirms_live_group_leader() {
        let mut command = sleeping_command();
        command.process_group(0);
        let mut child = command.spawn().expect("spawn sleep");
        let process_group_id = child.id().expect("leader pid");
        let report = ProcessSupervisor::stop("er19-live", &mut child, Some(process_group_id)).await;
        assert!(report.confirmed, "live stop must confirm: {report:?}");
        assert!(report.leader_reaped);
        assert_eq!(report.process_group, ProcessGroupState::Empty);
    }

    /// 拒绝：没有可观察的 process-group 身份时，leader 退出/回收也不能被报告为已确认停止。
    #[tokio::test]
    async fn er19_stop_without_group_identity_is_unknown() {
        let mut command = sleeping_command();
        let mut child = command.spawn().expect("spawn sleep");
        let report = ProcessSupervisor::stop("er19-no-group", &mut child, None).await;
        assert!(!report.confirmed, "no group, not confirmed: {report:?}");
        assert_eq!(report.process_group, ProcessGroupState::Unknown);
    }
}
