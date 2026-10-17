//! Daemon 中已授权执行请求的适配层，以及长驻子进程的监督器。
//!
//! 本模块位于组合根 `DaemonHost` 与具体执行设施之间：DaemonHost 在组装
//! `CapabilityBroker` 时调用 [`ExecutionControl::register`]，Broker 校验 Core
//! 交付的授权请求后，再按能力种类和操作名调用这里注册的处理器。本模块收到的
//! `AuthorizedCapabilityRequest` 已经过 Core 的策略、关卡和许可流程；这里负责把
//! 已授权操作交给工作区事务、工具目录、输出存储或 `ProcessSupervisor`，不另起一套
//! 权限决策流程。
//!
//! 主要请求路径如下：
//!
//! ```text
//! operator 命令 / Core 派发
//!     -> DaemonHost 装配的 CapabilityBroker
//!     -> ExecutionHandler
//!     -> ExecutionControl::handle / handle_read_or_continue
//!        -> process.start：准备命令、写 process 事实、启动子进程和后台监督任务
//!        -> process.poll/stdin/resize/stop：校验身份后读取状态或控制仍在运行的进程
//!        -> 其他固定操作：工作区事务、本地包、输出读取、环境信息、工具目录搜索
//! ```
//!
//! `process.start` 返回“已启动”时，后台任务仍会继续观察取消、超时、子进程退出和
//! 项目授权变化；最终结果要通过后续 `process.poll` 查询。进程事实写入 EventStore，
//! 但活动进程句柄、PTY 和滚动输出缓存在当前 Daemon 内存中，所以重启后只可依据
//! EventStore 查询已记录结果，不能凭 PID 重新接管进程。普通模型 `shell.exec` 的
//! Handler 也会复用本模块的 [`store_output`] 保存输出；那条路径与这里为长驻进程
//! 保留内存预览的生命周期不同。
//!
//! 阅读本文件后，可继续看 `kiana-daemon/src/lib.rs` 中的 Broker 装配、
//! `kiana-core/src/dispatch.rs` 中的许可消费、`kiana-daemon/src/process_supervisor.rs`
//! 中的停止与资源限制，以及 `kiana-daemon/src/execution_workspace.rs` 中的暂存工作区。
use crate::process_supervisor::ProcessSupervisor;
use crate::shell_plan::ShellCommandPlan;
use async_trait::async_trait;
use kiana_capability_broker::{CapabilityBroker, CapabilityHandler};
use kiana_domain::{
    AuthorizedCapabilityRequest, CapabilityKind, CapabilityResult, ExecutionOutputRef,
    InvocationId, JobHandle, RequestId, RunId, RuntimeEvent, SessionId, TurnId,
};
use kiana_ports::{EventStorePort, PortError};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};

/// 一条等待写入子进程标准输入或 PTY 的消息。
///
/// `reply` 把实际写入结果送回提交这条消息的请求处理协程；因此排入 channel
/// 只表示写入已排队，只有收到回复才知道写入器是否确认写完。
struct Input {
    /// 待写入字节对应的 UTF-8 文本；上游在入队前限制其长度。
    text: String,
    /// 写入器完成或失败后的一次性结果通道（oneshot channel）。
    reply: oneshot::Sender<Result<usize, String>>,
}
/// 当前 Daemon 进程内仍可直接控制的子进程记录。
///
/// 它与 EventStore 中的 `process.*` 事实用途不同：EventStore 可用于重启后查询已知
/// 事实；本结构持有运行时取消信号、输入 channel、状态、输出预览和（Unix 上的）PTY
/// 文件句柄。进程内没有这条记录时，代码不会根据 PID 重建这些易失资源。
struct ProcessEntry {
    /// 启动时绑定的请求者、项目、授权快照、命令元数据与 JobHandle。
    identity: Value,
    /// 后台监督任务监听的取消信号；超时、显式 stop 和输出超额会通知它。
    cancelled: watch::Sender<bool>,
    /// 向唯一输入写入任务发送 stdin/PTY 文本的有界队列。
    input: mpsc::Sender<Input>,
    /// 后台任务最终写入的 outcome；状态值可为 running/completed/cancelled/failed/result_unknown。
    /// `running` 初值只表示启动阶段已登记，不表示子进程最终成功。
    state: Mutex<Value>,
    /// 当前进程 stdout/stderr 的脱敏滚动预览，不是完整输出档案；tuple 0 号位是 stdout，
    /// 1 号位是 stderr。
    output: Mutex<(String, String)>,
    #[cfg(unix)]
    /// PTY 主端句柄；普通 pipe 模式不需要它，非 Unix 平台没有此字段。
    terminal: Option<std::fs::File>,
}
/// 管理当前 Daemon 的进程句柄，并向 Broker 暴露固定操作处理器。
///
/// `processes` 仅是进程内活动/近期状态表；持久事实由 `events` 记录。`capacity`
/// 限制尚未确认停止的后台执行数量，令牌由后台任务持有到监督结束。
pub(crate) struct ExecutionControl {
    events: Arc<dyn EventStorePort>,
    /// 以生成的 process ID 为 key；Daemon 重启后本表为空，恢复查询必须走 EventStore 分支。
    processes: Mutex<HashMap<String, Arc<ProcessEntry>>>,
    capacity: Arc<tokio::sync::Semaphore>,
}

impl ExecutionControl {
    /// 将本控制器支持的操作注册到 DaemonHost 正在组装的 Broker。
    ///
    /// 【调用者】DaemonHost 初始化 CapabilityBroker 时调用一次。
    /// 【输入】可变 Broker 注册表，以及与 Core/Daemon 共用的 EventStore 端口。
    /// 【输出】注册成功返回 `Ok(())`；名称或能力冲突等 Broker 注册错误原样返回。
    /// 【副作用】创建共享控制器状态，并为每个固定操作注册一个持有同一
    /// `Arc<ExecutionControl>` 的 Handler；这里不会启动进程或写入 EventStore。
    /// 【设计原因】操作入口必须先经过 Broker 的许可校验和精确路由，再由 Daemon
    /// adapter 触达系统资源；不能由模型或入口层直接调用本地副作用。
    pub fn register(
        broker: &mut CapabilityBroker,
        events: Arc<dyn EventStorePort>,
    ) -> Result<(), PortError> {
        let control = Arc::new(Self {
            events,
            processes: Mutex::new(HashMap::new()),
            // 全局最多持有 8 个执行令牌；并发请求首先在这里 fail-fast，避免无限等待。
            // 下面还会检查 running 表项，且停止未确认时会保留令牌，即使状态已不再 running。
            capacity: Arc::new(tokio::sync::Semaphore::new(8)),
        });
        for (kind, name) in [
            (CapabilityKind::Filesystem, "workspace.transaction"),
            (CapabilityKind::Filesystem, "local.package"),
            (CapabilityKind::Query, "execution.output.read"),
            (CapabilityKind::Query, "environment.inspect"),
            (CapabilityKind::Query, "tool.search"),
            (CapabilityKind::Process, "process.start"),
            (CapabilityKind::Query, "process.poll"),
            (CapabilityKind::Process, "process.stdin"),
            (CapabilityKind::Process, "process.resize"),
            (CapabilityKind::Process, "process.stop"),
        ] {
            broker.register_static(kind, name, Arc::new(ExecutionHandler(control.clone())))?;
        }
        Ok(())
    }
    /// 将一次进程生命周期事实按 stream 版本追加到 EventStore。
    ///
    /// 【调用者】启动流程写 `process.prepared` / `process.started`，后台监督任务写
    /// `process.finished`。
    /// 【输入】`id` 是该进程专属 stream key；`kind` 是事实类型；`data` 是待记录内容。
    /// 【核心流程】读取当前最大 stream version，先对数据脱敏，再以该版本作为
    /// expected version 追加下一条事实；若并发写入造成 CAS 冲突，则重新读取并最多
    /// 尝试四次。
    /// 【失败】EventStore 读取或非冲突写入错误向上传播；四次均冲突时返回
    /// `result_unknown:process_event_unconfirmed`，调用者不能据此假定事实未提交。
    /// 【副作用】成功时向 `process` stream 追加一条不可变运行时事件；不修改进程本身。
    /// 【设计原因】同一进程的 prepared/started/finished 事实需要有序且并发冲突可见，
    /// 不能用无条件 append 把两个写者都当成成功。
    async fn fact(&self, id: &str, kind: &str, data: Value) -> Result<(), PortError> {
        for _ in 0..4 {
            let previous = self.events.read_stream("process", id).await?;
            let version = previous
                .iter()
                .filter_map(|event| event.stream_version)
                .max()
                .unwrap_or(0);
            let request_id = RequestId::new();
            let event = RuntimeEvent::new(request_id, 1, kind, kiana_domain::redact_value(&data))
                .map_err(|_| error("process_event_invalid"))?
                .with_stream_metadata("process", id, version + 1);
            match self.events.append_expected(event, Some(version)).await {
                Ok(()) => return Ok(()),
                Err(PortError::Conflict(_)) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(error("result_unknown:process_event_unconfirmed"))
    }
    /// 读取启动进程时绑定的项目授权与数据策略快照。
    ///
    /// 项目根目录先 canonicalize，再以其 JSON digest 定位对应的 `authority` stream；
    /// 返回对象包含最新事件的 stream version、revision digest 和当前本地数据策略
    /// revision。后台监督任务会定期重新读取并与启动快照比较，变化或读取失败都会
    /// 使运行中的进程进入停止处理。
    ///
    /// 这里读取的是 EventStore 的 stream version 字段，不自行重建 Domain 授权账本；
    /// 不要把该读取器描述成独立完成了授权判断。项目路径、stream 不存在、EventStore
    /// 或数据策略读取失败时返回错误，由启动/监督调用方按各自阶段处理。
    async fn authority(&self, root: &str) -> Result<Value, PortError> {
        let key = kiana_domain::json_digest(
            &json!({"project_root":Path::new(root).canonicalize().map_err(|_|error("project_root_unavailable"))?}),
        );
        let records = self.events.read_stream("authority", &key).await?;
        let data_revision = crate::data_governance::read_policy(Path::new(root))?.revision;
        records.last().map(|event|json!({"version":event.stream_version,"digest":event.data["revision_digest"],"data_revision":data_revision}))
            .ok_or_else(||error("process_authority_unavailable"))
    }
    /// 启动一个经 Core 授权的 operator 子进程，并安排后台监督。
    ///
    /// 【调用者】`handle()` 在 Broker 将 `process.start` 路由到本 Handler 后调用。
    /// 【输入】`AuthorizedCapabilityRequest` 携带能力种类、操作名、请求 ID、Core
    /// 授权 ID 和 JSON 参数；Core 已执行授权链，这里仍要求参数带有 operator 标记，
    /// 并重新校验项目、沙箱、工作目录、命令计划、写入范围和时限。
    /// 【输出】立即返回进程 ID、绑定请求者/项目/运行范围的 `JobHandle` 与
    /// `state=running`。这只表示 `process.started` 事实已成功写入且监督任务已安排，
    /// 不表示子进程已成功退出或工作区已发布；最终结果由后续 `process.poll` 读取。
    /// 【副作用】可能取得工作区写租约、建立隔离暂存目录、spawn 子进程、创建输入与
    /// 输出异步任务，并向 EventStore 追加 prepared/started 事实；后台任务之后还会停止
    /// 子进程、收集输出、发布或丢弃暂存工作区并追加 finished 事实。
    /// 【失败】operator 标记、项目路径、命令/sandbox/范围/时限、资源容量、PTY 或
    /// workspace 准备失败时，不会进入成功启动结果；EventStore 无法确认 started 事实
    /// 时会尽力停止刚创建的子进程，并返回 `result_unknown`，因为进程启动和事实记录
    /// 是两个不同系统边界，无法伪装成一个原子事务。
    /// 【为什么异步监督】长命令不能占住一次 Broker 调用直至退出；拆成“启动返回 +
    /// 后台观察 + 后续查询”后，仍可响应取消和租约到期，同时把最终事实留在 EventStore。
    async fn start(
        self: Arc<Self>,
        request: AuthorizedCapabilityRequest,
    ) -> Result<Value, PortError> {
        let arguments = &request.request.arguments;
        if arguments["operator_authorized"] != true {
            return Err(error("operator_required"));
        }
        // Semaphore 令牌从准备阶段开始占用，并被 move 到下面的后台任务中。
        // 即使调用者很快收到启动回执，容量仍要覆盖整个子进程生命周期。
        let capacity = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| error("process_capacity_exceeded"))?;
        let root = PathBuf::from(required(arguments, "project_root")?)
            .canonicalize()
            .map_err(|_| error("project_root_unavailable"))?;
        let sandbox = arguments["sandbox"]
            .as_str()
            .unwrap_or("read-only")
            .to_owned();
        if !matches!(sandbox.as_str(), "read-only" | "workspace-write") {
            return Err(error("sandbox_unsupported"));
        }
        let workdir =
            crate::harness_capabilities::confined_workdir(&root, arguments.get("workdir"))?;
        let command_plan = ShellCommandPlan::from_value(arguments.get("command"))?;
        let argv = command_plan.argv.clone();
        let timeout = arguments["timeout_ms"].as_u64().unwrap_or(300_000);
        // 缺失或不是无符号整数时都采用 300000ms（5 分钟）缺省值；显式值限制在
        // 1ms 到 3600000ms（1 小时）内，避免零时限或无限期任务让监督资源失去上界。
        if timeout == 0 || timeout > 3_600_000 {
            return Err(error("process_deadline_invalid"));
        }
        let paths: Vec<String> = arguments["path_allow"]
            .as_array()
            .ok_or_else(|| error("process_scope_required"))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| error("process_scope_invalid"))
            })
            .collect::<Result<_, _>>()?;
        let leases = if sandbox == "workspace-write" {
            // 写模式取得 packet/path 对应的共享资源租约，直到后台任务结束才释放；
            // 只读模式不申请写租约。
            kiana_core::acquire_workspace_resources(&root.to_string_lossy(), &paths)?
        } else {
            Vec::new()
        };
        let workspace = if sandbox == "workspace-write" {
            // 把可能进行大量文件读取/复制的暂存准备放到 blocking 池，避免占用 Tokio
            // worker。子进程会在暂存根目录工作，只有最终成功才由 workspace.finish 发布。
            let owned = request.clone();
            let root = root.clone();
            let workdir = workdir.clone();
            Some(
                tokio::task::spawn_blocking(move || {
                    crate::execution_workspace::ExecutionWorkspace::prepare(&owned, &root, &workdir)
                })
                .await
                .map_err(|_| error("process_workspace_join_failed"))??,
            )
        } else {
            None
        };
        let mut scope = arguments.clone();
        let (execution_root, execution_workdir) = if let Some(workspace) = &workspace {
            // 在临时工作区中，物理写入范围已经被收窄到该目录根；logical_project_root
            // 保留原项目身份供沙箱策略检查，不代表子进程可以直接写回宿主项目。
            scope["path_allow"] = json!(["."]);
            scope["logical_project_root"] = json!(root);
            (&workspace.root, &workspace.workdir)
        } else {
            (&root, &workdir)
        };
        let mut command = crate::harness_capabilities::sandboxed_command_scoped(
            &argv,
            execution_root,
            execution_workdir,
            &sandbox,
            Some(&scope),
        )?;
        let tty = arguments["pty"] == true;
        #[cfg(unix)]
        let terminal = if tty {
            Some(open_terminal(&mut command)?)
        } else {
            None
        };
        #[cfg(not(unix))]
        if tty {
            return Err(error("pty_backend_unsupported"));
        }
        if !tty {
            command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
        }
        let id = RequestId::new().to_string();
        let job_id = RequestId::parse_str(&id).ok_or_else(|| error("process_id_invalid"))?;
        let authority = self.authority(&root.to_string_lossy()).await?;
        // run_id/turn_id 是这份操作参数中的可选关联值；此处将能解析的值写入句柄，
        // 不在本函数内向 Core 查询它们是否仍是活动 Run/Turn。
        let run_id = arguments["run_id"].as_str().and_then(RunId::parse_str);
        let turn_id = arguments["turn_id"]
            .as_str()
            .and_then(TurnId::parse_str)
            .or_else(|| {
                arguments["turn_id"]
                    .as_str()
                    .and_then(|value| serde_json::from_str::<TurnId>(value).ok())
            });
        let actor_id = required(arguments, "actor_id")?;
        let session_id = SessionId::new(required(arguments, "session_id")?);
        let authority_epoch = authority["version"]
            .as_u64()
            .filter(|epoch| *epoch > 0)
            .ok_or_else(|| error("process_authority_epoch_invalid"))?;
        // 当前 JobHandle 的 authority_epoch 由上面读到的 authority stream_version 提供；
        // 此处不重放账本来推导另一种 epoch。
        let project_digest = kiana_domain::json_digest(&json!({
            "project_root": root,
            "session_id": session_id,
        }));
        let expires_at_unix_ms = unix_time_ms()?
            .checked_add(timeout as u64)
            .ok_or_else(|| error("process_expiry_overflow"))?;
        let mut identity = json!({"process_id":id,"actor_id":arguments["actor_id"],"session_id":arguments["session_id"],
            "role_id":arguments["role_id"],"department_id":arguments["department_id"],"project_root":root,"authority":authority,
            "run_id":run_id,"turn_id":turn_id,
            "path_allow":paths,"sandbox":sandbox,"request_id":request.request.request_id,"execution_id":request.authorization_id,
            "timeout_ms":timeout,"pty":tty,"job_handle":Value::Null,"shell_plan":command_plan.metadata()});
        {
            let map = self
                .processes
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if map
                .values()
                .filter(|entry| {
                    entry
                        .state
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())["state"]
                        == "running"
                })
                .count()
                >= 8
            {
                return Err(error("process_capacity_exceeded"));
            }
        }
        // 先记录 prepared 再 spawn：若之后观察到该事实但没有 started，读取者可以区分
        // “准备已记录、启动未确认”的阶段。该顺序不是跨 EventStore 与 OS 的事务，
        // 所以 spawn 失败时 prepared 事实仍会保留。
        self.fact(&id, "process.prepared", identity.clone()).await?;
        let mut child = command
            .spawn()
            .map_err(|cause| error(&format!("process_spawn_failed:{cause}")))?;
        let pid = child.id();
        // Guard 在启动成功后立即接管 PID；若后续某个准备步骤提前返回，析构保护仍会
        // 尝试清理子进程，直到明确确认停止后才 disarm。
        let mut process_guard = ProcessSupervisor::guard(pid);
        let job_handle = JobHandle::new(
            job_id,
            request.request.request_id,
            run_id,
            turn_id,
            actor_id,
            session_id,
            project_digest,
            authority_epoch,
            pid,
            expires_at_unix_ms,
        )
        .map_err(|_| error("job_handle_invalid"))?;
        identity["job_handle"] = json!(job_handle);
        let (cancelled, mut cancellation) = watch::channel(false);
        // channel 最多排队 8 条尚未写入的 stdin 消息；队满时 try_send 立即失败，
        // 不会让每个输入请求各自积压在无限等待队列中。
        let (input, input_rx) = mpsc::channel(8);
        let entry = Arc::new(ProcessEntry {
            identity: identity.clone(),
            cancelled,
            input,
            state: Mutex::new(json!({"state":"running","stop_confirmed":false})),
            output: Mutex::new((String::new(), String::new())),
            #[cfg(unix)]
            terminal,
        });
        let (stdout, stderr, writer) = if tty {
            #[cfg(unix)]
            {
                let terminal = entry
                    .terminal
                    .as_ref()
                    .ok_or_else(|| error("pty_unavailable"))?;
                let reader = tokio::fs::File::from_std(
                    terminal
                        .try_clone()
                        .map_err(|_| error("pty_clone_failed"))?,
                );
                let writer = tokio::fs::File::from_std(
                    terminal
                        .try_clone()
                        .map_err(|_| error("pty_clone_failed"))?,
                );
                (
                    tokio::spawn(capture(reader, entry.clone(), false)),
                    None,
                    tokio::spawn(write_input(writer, input_rx)),
                )
            }
            #[cfg(not(unix))]
            {
                return Err(error("pty_backend_unsupported"));
            }
        } else {
            let stdout = child
                .stdout
                .take()
                .ok_or_else(|| error("process_stdout_unavailable"))?;
            let stderr = child
                .stderr
                .take()
                .ok_or_else(|| error("process_stderr_unavailable"))?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| error("process_stdin_unavailable"))?;
            (
                tokio::spawn(capture(stdout, entry.clone(), false)),
                Some(tokio::spawn(capture(stderr, entry.clone(), true))),
                tokio::spawn(write_input(stdin, input_rx)),
            )
        };
        if let Err(cause) = self.fact(&id, "process.started", identity.clone()).await {
            // OS 已经启动进程，但 EventStore 没能确认 started 事实。此时尽力停进程、
            // 关闭读写任务，再向上返回 Unknown；不能返回普通失败并暗示“没有启动”。
            entry.cancelled.send_replace(true);
            let report = ProcessSupervisor::stop(&id, &mut child, pid).await;
            if report.confirmed {
                process_guard.disarm();
            }
            writer.abort();
            stdout.abort();
            if let Some(stderr) = stderr {
                stderr.abort();
            }
            return Err(error(&format!(
                "result_unknown:process_start_record_failed:{cause}"
            )));
        }
        self.processes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id.clone(), entry.clone());
        let control = self.clone();
        let process_id = id.clone();
        let root_text = root.to_string_lossy().into_owned();
        let job_handle_value = identity["job_handle"].clone();
        // 监督任务持有子进程、写范围 lease、并发令牌和输出任务的 JoinHandle。
        // start() 先返回，下面的循环才负责对最终状态作权威记录。
        tokio::spawn(async move {
            let _leases = leases;
            let _capacity = capacity;
            let deadline = tokio::time::sleep(Duration::from_millis(timeout));
            tokio::pin!(deadline);
            let reason;
            let status;
            loop {
                // biased 使同一轮同时就绪时优先处理取消、再处理 deadline、再看退出；
                // 每 100ms 重新读取授权/数据策略快照，读取失败也按快照不一致处理。
                tokio::select! {
                    biased;
                    _=kiana_ports::wait_for_cancellation(&mut cancellation)=>{reason="cancelled";status=None;break;},
                    _=&mut deadline=>{reason="timed_out";status=None;break;},
                    result=child.wait()=>{reason="exited";status=result.ok();break;},
                    _=tokio::time::sleep(Duration::from_millis(100))=>{
                        if control.authority(&root_text).await.ok().as_ref()!=Some(&authority) {reason="authority_revoked";status=None;break;}
                    },
                }
            }
            let stop_report = ProcessSupervisor::stop(&process_id, &mut child, pid).await;
            // confirmed 是监督器根据 leader 已回收及进程组最终为空给出的确认；它不表示
            // 每个后代都被逐个 wait/reap。没有确认就必须保留 Unknown 和资源占用边界。
            let stopped = stop_report.confirmed;
            if stopped {
                process_guard.disarm();
            }
            writer.abort();
            // 给已读出的 stdout/stderr 任务最多 2 秒完成脱敏尾部刷新；超时或任务错误时
            // captured=false，暂存写入就不会被认定为可发布的成功执行。
            let stdout_result = tokio::time::timeout(Duration::from_secs(2), stdout).await;
            let stderr_result = match stderr {
                Some(stderr) => Some(tokio::time::timeout(Duration::from_secs(2), stderr).await),
                None => None,
            };
            let captured = matches!(stdout_result, Ok(Ok(Ok(()))))
                && stderr_result
                    .as_ref()
                    .is_none_or(|result| matches!(result, Ok(Ok(Ok(())))));
            let successful = stopped
                && captured
                && reason == "exited"
                && status.is_some_and(|status| status.success())
                && control.authority(&root_text).await.ok().as_ref() == Some(&authority);
            // 仅在正常退出、输出任务完整、退出码成功、确认停止且授权快照仍相同的
            // 情况下把隔离工作区提交到项目；其他状态会让 finish(false) 丢弃暂存改动。
            let publication = match workspace {
                Some(workspace) => {
                    tokio::task::spawn_blocking(move || workspace.finish(successful))
                        .await
                        .map_err(|_| error("result_unknown:process_publication_join_failed"))
                        .and_then(|result| result)
                }
                None => Ok(json!({"host_effect":"none","published":false})),
            };
            let unknown = !stopped || publication.is_err();
            let outcome = json!({"state":if unknown {"result_unknown"}else if successful {"completed"}else if reason=="cancelled"||reason=="authority_revoked" {"cancelled"}else {"failed"},
                "reason":reason,"exit_code":status.and_then(|status|status.code()),"stop_confirmed":stopped,"capture_complete":captured,
                "stop_report":stop_report,"workspace":publication.as_ref().ok(),"error":publication.as_ref().err().map(ToString::to_string),
                "resource_budget":ProcessSupervisor::resource_receipt(&kiana_domain::ProcessResourceBudget::for_wall_time_ms(timeout as u64))});
            let fact = control
                .fact(
                    &process_id,
                    "process.finished",
                    json!({"identity":identity,"outcome":outcome}),
                )
                .await;
            *entry
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = if fact.is_ok() {
                outcome
            } else {
                json!({"state":"result_unknown","stop_confirmed":stopped,"error":"process_terminal_record_failed"})
            };
            if !stopped {
                // 未确认整个进程组已停止时，仍可能有后代进程写共享文件。故意泄漏租约
                // 和容量令牌，使同一 Daemon 不会误以为资源已安全释放并立刻复用；
                // 这是异常保护，不是正常清理方式。
                std::mem::forget(_leases);
                std::mem::forget(_capacity);
            }
        });
        // 这只是启动受理回执。`query_extends_lease=false` 表明 poll 不延长 JobHandle
        // 的期限；调用方必须根据句柄期限和后续 outcome 自己结束这次交互。
        Ok(
            json!({"process_id":id,"job_handle":job_handle_value,"state":"running","started":true,"stop_confirmed":false,"timeout_ms":timeout,"pty":tty,"query_extends_lease":false}),
        )
    }
    /// 处理已启动进程的 poll、stdin、PTY resize 和 stop 请求。
    ///
    /// 【调用者】`handle_read_or_continue()` 将非查询类固定操作交给这里；每次调用仍
    /// 是一条新的 Core 授权请求，不因持有旧 JobHandle 而跳过 Broker/Core。
    /// 【输入】授权请求参数中的 process ID、owner/project/session/run 范围，以及操作名
    /// 和操作参数。
    /// 【核心流程】先从本进程的 HashMap 找运行时句柄；活动句柄要匹配身份、JobHandle
    /// 有效期；poll 和 stdin/resize 还会按各自分支重新比较 authority/data revision，
    /// stop 则先发取消信号并等待监督器停止。若 Daemon 重启后表项不存在，则读 EventStore
    /// 恢复已记录身份与 finished outcome，但只允许 poll，不会用事件中的 PID 重建 stdin、
    /// PTY 或后台监督任务。
    /// 【输出】poll 返回 outcome 和 stdout/stderr 各自约 65536 字节的当前内存预览；stdin 返回已确认写入
    /// 字节数；resize 返回终端已调整；stop 返回最终状态或 Unknown。
    /// 【失败】owner/项目/Run 范围不符、句柄过期、authority 改变、进程已结束、队列
    /// 背压、写入超时或平台不支持时返回结构化 `PortError`。显式 stop 最多等待 100 次、
    /// 每次 100ms；到时仍为 running 就返回 `result_unknown`，并不声称进程已停止。
    /// 【设计原因】重启后只把 EventStore 当作事实来源而不靠 PID“接管”进程，避免把
    /// 可能已被操作系统复用的 PID 当作旧子进程；查询当前活动数据也受原 owner 与策略绑定。
    async fn continuation(
        &self,
        request: &AuthorizedCapabilityRequest,
    ) -> Result<Value, PortError> {
        let arguments = &request.request.arguments;
        let id = required(arguments, "process_id")?;
        let entry = self
            .processes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(id)
            .cloned();
        let Some(entry) = entry else {
            // EventStore 只能给出生命周期事实；没有进程内 entry 时，IO channel、PTY 文件
            // 和实时输出都已不可用。故而只对 poll 返回已知终态或保守的 Unknown。
            let records = self.events.read_stream("process", id).await?;
            let identity = records
                .iter()
                .rev()
                .find(|event| event.kind == "process.started")
                .or_else(|| {
                    records
                        .iter()
                        .find(|event| event.kind == "process.prepared")
                })
                .map(|event| &event.data)
                .ok_or_else(|| error("process_not_found"))?;
            check_owner(identity, arguments)?;
            validate_job_handle(identity, arguments)?;
            if request.request.operation != "process.poll" {
                return Err(error(
                    "result_unknown:process_handle_unavailable_after_restart",
                ));
            }
            let outcome = records.iter().rev().find(|event|event.kind=="process.finished").map(|event|event.data["outcome"].clone())
                .unwrap_or_else(||json!({"state":"result_unknown","stop_confirmed":false,"automatic_retry":false}));
            let handle = serde_json::from_value::<JobHandle>(identity["job_handle"].clone())
                .map_err(|_| error("job_handle_invalid"))?;
            return Ok(
                json!({"process_id":id,"job_handle":handle,"outcome":outcome,"cursor":Value::Null,"output_unavailable":"process_handle_not_live"}),
            );
        };
        check_owner(&entry.identity, arguments)?;
        validate_job_handle(&entry.identity, arguments)?;
        let operation = request.request.operation.as_str();
        if operation == "process.poll" {
            // authority 或 data policy revision 变化后仍可以看 outcome，但不再暴露进程输出。
            // 这是一种读取边界，不会让 poll 延长 JobHandle 有效期。
            if self.authority(required(arguments, "project_root")?).await?
                != entry.identity["authority"]
            {
                return Ok(
                    json!({"process_id":id,"outcome":entry.state.lock().unwrap_or_else(|error|error.into_inner()).clone(),"output_unavailable":"authority_or_data_revision_changed"}),
                );
            }
            let handle: JobHandle = serde_json::from_value(entry.identity["job_handle"].clone())
                .map_err(|_| error("job_handle_invalid"))?;
            let output_cursor = {
                let output = entry
                    .output
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                kiana_domain::json_digest(&json!({
                    "job_id": id,
                    "stdout_bytes": output.0.len(),
                    "stderr_bytes": output.1.len(),
                    "handle_digest": handle.handle_digest,
                }))
            };
            let output = entry
                .output
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            return Ok(
                json!({"process_id":id,"job_handle":handle,"outcome":entry.state.lock().unwrap_or_else(|error|error.into_inner()).clone(),"stdout":output.0,"stderr":output.1,"preview_limit_bytes":65536,"cursor":output_cursor}),
            );
        }
        if operation == "process.stop" {
            // stop 只是发取消信号；真正的停止、进程组检查和 finished 事实写入由后台
            // 监督任务完成。这里等待其状态更新，等待窗口耗尽时保留 Unknown。
            entry.cancelled.send_replace(true);
            for _ in 0..100 {
                let state = entry
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone();
                if state["state"] != "running" {
                    return Ok(state);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            return Err(error("result_unknown:process_stop_unconfirmed"));
        }
        if self.authority(required(arguments, "project_root")?).await?
            != entry.identity["authority"]
        {
            return Err(error("process_authority_revoked"));
        }
        if entry
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())["state"]
            != "running"
        {
            return Err(error("process_not_running"));
        }
        if operation == "process.resize" {
            #[cfg(unix)]
            {
                use std::os::fd::AsRawFd;
                // PTY winsize 的字段是 u16，但 handler 额外限制在 1..=1000 行/列，
                // 避免无意义的大值进入 ioctl；这是比某些上游 schema 更严格的本地边界。
                let rows = arguments["rows"]
                    .as_u64()
                    .filter(|value| *value > 0 && *value <= 1000)
                    .ok_or_else(|| error("pty_rows_invalid"))?;
                let cols = arguments["cols"]
                    .as_u64()
                    .filter(|value| *value > 0 && *value <= 1000)
                    .ok_or_else(|| error("pty_cols_invalid"))?;
                let terminal = entry
                    .terminal
                    .as_ref()
                    .ok_or_else(|| error("process_has_no_pty"))?;
                let size = libc::winsize {
                    ws_row: rows as u16,
                    ws_col: cols as u16,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };
                // ioctl 接收已打开的 PTY 文件描述符与 winsize 布局；fd 来自当前 entry
                // 持有的 File，调用期间借用 size，返回负值表示内核拒绝调整。
                if unsafe { libc::ioctl(terminal.as_raw_fd(), libc::TIOCSWINSZ, &size) } < 0 {
                    return Err(error("pty_resize_failed"));
                }
                return Ok(json!({"resized":true,"process_id":id}));
            }
            #[cfg(not(unix))]
            {
                return Err(error("pty_backend_unsupported"));
            }
        }
        let text = arguments["data"]
            .as_str()
            .ok_or_else(|| error("process_input_required"))?;
        // 当前控制消息最多 16384 字节（16 KiB）。长度按 UTF-8 字节计算，而不是字符数；
        // 有界 channel 和长度限制一起避免快速重复调用无限堆积待写输入。
        if text.len() > 16 * 1024 {
            return Err(error("process_input_limit"));
        }
        let (reply, response) = oneshot::channel();
        entry
            .input
            .try_send(Input {
                text: text.to_owned(),
                reply,
            })
            .map_err(|_| error("process_input_backpressure"))?;
        // 最多等待 5 秒确认写入。超时表示写入结果未知而不是“肯定没写”；发送取消信号
        // 让后台监督任务清理可能已收到部分输入的子进程。
        let bytes = tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .map_err(|_| {
                entry.cancelled.send_replace(true);
                error("result_unknown:process_input_timeout")
            })?
            .map_err(|_| error("result_unknown:process_input_unconfirmed"))?
            .map_err(|reason| error(&reason))?;
        Ok(json!({"process_id":id,"written_bytes":bytes}))
    }
}

/// CapabilityBroker 中每种注册操作共用的薄路由器。
///
/// 该 wrapper 不计算权限、不解析业务参数，也不直接执行系统命令；它只把 Broker
/// 交来的授权请求转发给共享的 `ExecutionControl`。
struct ExecutionHandler(Arc<ExecutionControl>);
#[async_trait]
impl CapabilityHandler for ExecutionHandler {
    /// Broker 的标准 handler 入口；执行结果或错误原样交回 Broker/Core。
    async fn execute(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        self.0.handle(self.0.clone(), request).await
    }
}
impl ExecutionControl {
    /// 将开始进程与其他同步/后续操作分开处理。
    ///
    /// `process.start` 需要取得共享控制器所有权并创建脱离当前请求生命周期的后台任务，
    /// 因此单独调用 `start()`；其余操作不创建新的子进程，统一进入 read/continue 路由。
    async fn handle(
        &self,
        owner: Arc<Self>,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        if request.request.operation == "process.start" {
            let request_id = request.request.request_id;
            let output = owner.start(request).await?;
            return Ok(CapabilityResult::success(request_id, output));
        }
        self.handle_read_or_continue(request).await
    }
    /// 执行工具查询、工作区/输出操作，或将进程操作转给 `continuation()`。
    ///
    /// 【调用者】`ExecutionHandler::execute()` 经 `handle()` 调用。
    /// 【输入】Broker 已校验并交付的 `AuthorizedCapabilityRequest`。
    /// 【输出】把分支生成的 JSON 包装成对应 request ID 的 `CapabilityResult`。
    /// 【副作用】`workspace.transaction` 与 `local.package` 可能写文件；
    /// `execution.output.read` 读本地归档；process 后续操作读/写运行态；
    /// `environment.inspect` 和 `tool.search` 是描述性查询，不启动子进程。
    /// 【失败】需要 operator 标记；参数、catalog、I/O、Tokio blocking task 或进程状态
    /// 失败都会返回 `PortError`。`spawn_blocking` join 失败被明确映射为
    /// `result_unknown`，因为闭包可能已经执行了部分外部写入。
    /// 【设计原因】本模块是 Daemon adapter 而不是第二授权层；这里按精确 operation
    /// 分派已经获准的工作，并继续保持工具发现和真正执行之间的分界。
    async fn handle_read_or_continue(
        &self,
        request: AuthorizedCapabilityRequest,
    ) -> Result<CapabilityResult, PortError> {
        let arguments = &request.request.arguments;
        if arguments["operator_authorized"] != true {
            return Err(error("operator_required"));
        }
        let output = match request.request.operation.as_str() {
            "workspace.transaction" => {
                // 文件遍历和发布属于同步 I/O，把它移出 Tokio async worker；若 join 失败，
                // 不能证明事务闭包完全没写入，所以返回 Unknown 而非普通的确定性失败。
                let root = PathBuf::from(required(arguments, "project_root")?);
                let arguments = arguments.clone();
                tokio::task::spawn_blocking(move || {
                    crate::apply_patch::workspace_transaction(&root, &arguments)
                })
                .await
                .map_err(|_| error("result_unknown:workspace_transaction_join_failed"))??
            }
            "local.package" => {
                // 本地包同样需要读取、编码并发布文件，使用 blocking 池避免阻塞 async worker。
                let arguments = arguments.clone();
                tokio::task::spawn_blocking(move || local_package(&arguments))
                    .await
                    .map_err(|_| error("result_unknown:local_package_join_failed"))??
            }
            "execution.output.read" => read_output(arguments)?,
            "environment.inspect" => {
                // 这里只返回编译平台、所选后端和声明的预算值；`behavior_verified=false`
                // 明确表示它不是沙箱/网络隔离/资源上限已实测通过的证明。
                // 1048576 字节是响应中声明的输出限制（1 MiB），536870912 字节是声明的
                // 工作区限制（512 MiB），8 是声明的进程数；这些值本身不证明平台已强制执行。
                json!({"schema":"kiana.environment-support.v1","host":std::env::consts::OS,
                "backend":crate::harness_sandbox::SANDBOX_BACKEND,"workspace_mode":"isolated_staged",
                "network":"deny_all","file_identity":"descriptor_pinned","limits":{"output_bytes":1048576,"workspace_bytes":536870912,"processes":8},
                "behavior_verified":false})
            }
            "tool.search" => {
                // Tool search 只从当前固定目录中筛选出受角色允许且符合预算的 schema，
                // 结果字段会明确表示“不授予执行权”；真实工具调用仍要重新走 Core 授权链。
                let query = match arguments.get("query") {
                    None => "",
                    Some(value) => value
                        .as_str()
                        .ok_or_else(|| error("tool_search_limits_invalid"))?,
                };
                let role = kiana_domain::RoleSpec::lookup(required(arguments, "role_id")?)
                    .ok_or_else(|| error("role_unknown"))?;
                let defaults = kiana_domain::ToolSearchOptions::default();
                let catalog_version = match arguments.get("catalog_version") {
                    None => defaults.catalog_version,
                    Some(value) => serde_json::from_value(value.clone())
                        .map_err(|_| error("tool_search_catalog_version_invalid"))?,
                };
                let max_results = tool_search_limit(
                    arguments,
                    "max_results",
                    // 默认返回 32 项，最多 128 项，避免发现请求产生过大的上下文响应。
                    32,
                    128,
                    "tool_search_limits_invalid",
                )?;
                let max_schema_bytes = tool_search_limit(
                    arguments,
                    "max_schema_bytes",
                    // 128 KiB 同时是默认值和硬上限；调用者只能缩小预算，不能扩张目录响应。
                    128 * 1024,
                    128 * 1024,
                    "tool_search_limits_invalid",
                )?;
                let max_context_tokens = tool_search_limit(
                    arguments,
                    "max_context_tokens",
                    defaults.max_context_tokens,
                    defaults.max_context_tokens,
                    "tool_search_context_budget_invalid",
                )?;
                // 调用方不能要求跳过 catalog 健康校验，否则可能把过期/不完整 schema
                // 展示成当前可调用能力。
                if !tool_search_filter(arguments, "require_healthy_catalog", true)? {
                    return Err(error("tool_search_catalog_health_required"));
                }
                let options = kiana_domain::ToolSearchOptions {
                    catalog_version,
                    max_context_tokens,
                    require_healthy_catalog: true,
                    replay_safe_only: tool_search_filter(arguments, "replay_safe_only", false)?,
                };
                let response = kiana_domain::search_tool_catalog(
                    query,
                    &role.tools,
                    max_results,
                    max_schema_bytes,
                    &options,
                )
                .map_err(|reason| error(&reason))?;
                let mut output = serde_json::to_value(response)
                    .map_err(|_| error("tool_search_schema_encode_failed"))?;
                output["search_mode"] = json!("ranked_token_bounded");
                output
            }
            _ => self.continuation(&request).await?,
        };
        Ok(CapabilityResult::success(
            request.request.request_id,
            output,
        ))
    }
}

#[cfg(unix)]
/// 为子进程建立伪终端（PTY），返回由 Daemon 保留的 master 端。
///
/// 子进程的 stdin/stdout/stderr 全部连接到 slave 端，因此终端程序会观察到交互式终端
/// 行为；Daemon 则通过 master 端读取合并后的输出、写入输入和调整窗口大小。此能力只在
/// Unix 编译目标提供；libc 返回的两个文件描述符分别由 `from_raw_fd` 接管一次，随后由
/// Rust `File` 的析构负责关闭。
fn open_terminal(command: &mut tokio::process::Command) -> Result<std::fs::File, PortError> {
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (-1, -1);
    if unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
        )
    } != 0
    {
        return Err(error("pty_unavailable"));
    }
    let master = unsafe { std::fs::File::from_raw_fd(master) };
    let slave = unsafe { std::fs::File::from_raw_fd(slave) };
    command
        .stdin(slave.try_clone().map_err(|_| error("pty_clone_failed"))?)
        .stdout(slave.try_clone().map_err(|_| error("pty_clone_failed"))?)
        .stderr(slave);
    Ok(master)
}
/// 按入队顺序把 stdin/PTY 输入写给子进程，并逐条回传写入结果。
///
/// `mpsc` 是多生产者、单消费者的有界异步 channel；这个单独写入任务保证多个
/// `process.stdin` 请求不会并发交错调用同一个 writer。底层 `write_all` 出错时，当前
/// 请求收到 Unknown 类错误，写入循环随即退出，因为后续消息也无法再可靠送达。
async fn write_input<W: tokio::io::AsyncWrite + Unpin>(
    mut writer: W,
    mut input: mpsc::Receiver<Input>,
) {
    while let Some(input) = input.recv().await {
        let result = writer
            .write_all(input.text.as_bytes())
            .await
            .map(|()| input.text.len())
            .map_err(|error| format!("result_unknown:process_input:{error}"));
        let failed = result.is_err();
        let _ = input.reply.send(result);
        if failed {
            break;
        }
    }
}
/// 读取一个 stdout/stderr/PTY 字节流，脱敏后只保留供活动进程查询的滚动预览。
///
/// 【调用者】`start()` 为 stdout、stderr 或 PTY master 分别启动该任务。
/// 【数据流】异步 reader → 分块 UTF-8 容错解码 → 有状态 `StreamingRedactor` → 安全文本
/// 清理 → `ProcessEntry.output` 对应流的尾部预览 → 后续 `process.poll`。
/// 【输入/边界】每次读取最多 8192 原始字节；每个 reader 单独累计最多 32 MiB 原始输出，
/// 超额时通知监督任务取消进程并返回 `process_output_quota_exceeded`。
/// 【共享状态】stdout 和 stderr 任务只在追加/裁剪预览时短暂持有同一 Mutex；锁中不 await，
/// 避免一个慢 I/O 等待长期阻塞另一个输出流。
/// 【输出保留】循环中每条流滚动保留约 65536 字节的 UTF-8 尾部，以字符边界裁剪；这只是
/// 进程仍在当前 Daemon 内存中的预览，不会写 EventStore 或持久化输出目录。循环结束后
/// 仍要把 redactor 缓存的尾部刷入同一流。
/// 【失败】读取错误会结束任务并向监督者报告；超额会额外发送取消信号，不能静默丢弃
/// 超限之后的数据后仍将该进程标记为正常完成。
async fn capture<R: tokio::io::AsyncRead + Unpin>(
    mut reader: R,
    entry: Arc<ProcessEntry>,
    stderr: bool,
) -> Result<(), PortError> {
    // 敏感字符串可能恰好横跨两次 read；流式 redactor 保留必要上下文，避免只对每块
    // 独立脱敏而漏掉跨块匹配。
    let mut redactor = kiana_domain::StreamingRedactor::new();
    let mut buffer = [0u8; 8192];
    let mut total = 0usize;
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| error("process_output_read_failed"))?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count);
        // 这里按单个 reader 的原始读取字节计数，不是 stdout+stderr 的合计，也不是最终
        // UTF-8 预览大小。不要把它与 ProcessResourceBudget 回执中的 output_bytes 混为一谈。
        if total > 32 * 1024 * 1024 {
            entry.cancelled.send_replace(true);
            return Err(error("process_output_quota_exceeded"));
        }
        let text = redactor.push(&String::from_utf8_lossy(&buffer[..count]));
        let text = crate::execution_output::safe_output_text(&text);
        let mut output = entry
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let target = if stderr { &mut output.1 } else { &mut output.0 };
        target.push_str(&text);
        // 预览限制按 String 的 UTF-8 字节长度计算；向前移动到合法字符边界，避免 drain
        // 后留下无法切片的半个多字节字符。
        if target.len() > 65536 {
            let mut offset = target.len() - 65536;
            while !target.is_char_boundary(offset) {
                offset += 1;
            }
            target.drain(..offset);
        }
    }
    // 最后一次 push 后仍可能有待判断的匹配前缀；只有 reader 到 EOF 才能安全决定如何输出。
    let tail = redactor.finish();
    let mut output = entry
        .output
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if stderr {
        output.1.push_str(&tail);
    } else {
        output.0.push_str(&tail);
    }
    Ok(())
}

/// 解析 Runner 执行输出归档目录。
///
/// 优先使用 `KIANA_HOME`；未设置时落到用户 `HOME/.kiana`，再固定追加
/// `execution-output`。路径必须是绝对路径，避免进程当前工作目录变化时让归档位置漂移。
/// 该函数只计算路径、不创建目录；真正读写由 `LocalDir` 完成。
fn output_directory() -> Result<PathBuf, PortError> {
    let root = std::env::var_os("KIANA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|value| PathBuf::from(value).join(".kiana")))
        .ok_or_else(|| error("output_store_unavailable"))?;
    if !root.is_absolute() {
        return Err(error("output_store_not_absolute"));
    }
    Ok(root.join("execution-output"))
}
/// 把 Runner shell 执行的完整安全输出持久化，并把返回值改为有界预览。
///
/// 【调用者】`harness_capabilities` 中的 shell capability handler，在成功取得 shell
/// 输出后调用；长驻 `process.start` 的 `capture()` 不走这条持久化路径。
/// 【输入】授权请求中的身份/项目/Run 参数，以及包含可选 stdout/stderr 文本的可变结果 JSON。
/// 【数据流】原始响应文本 → 安全输出过滤 → 写入有大小上限的本地 JSON 归档；传给模型或
/// 后续展示的 `output` 同时被替换成预览，另外增加可分页读取的 `output_ref`。
/// 【身份与时效】output_id 使用 shell 请求 ID；引用记录 owner identity digest、内容摘要、
/// 当前/请求携带的数据策略 revision 和 15 分钟过期时间，读取时会再次核对这些值。
/// 【副作用】在 `KIANA_HOME/execution-output` 或用户 `.kiana/execution-output` 下创建/发布
/// 单个 JSON 文件；不写 EventStore。若没有字符串形式的 stdout/stderr，则直接返回且不存档。
/// 【失败】budget 无效、编码/落盘失败、单记录超持久化上限或引用构造失败都会返回错误；
/// 上游 shell Handler 将看到此错误，而完整输出可能已经经过此前的本地处理。
/// 【设计原因】普通 shell 输出可能远大于模型上下文，调用方用短预览继续对话，需要时再凭
/// 引用按页读取；长进程则只保留活动态尾部预览，不能把两种生命周期混在一起。
pub(crate) fn store_output(
    request: &AuthorizedCapabilityRequest,
    output: &mut Value,
) -> Result<(), PortError> {
    store_output_at(&output_directory()?, request, output)
}

/// `store_output` 的显式归档目录版本。
///
/// 【调用者】生产路径经 `store_output` 传入 `KIANA_HOME` 派生的受控归档目录；本模块的
/// `#[cfg(test)]` 行为夹具传入临时目录，从而在不依赖全局环境变量的情况下驱动同一份
/// 脱敏、有界预览与引用绑定逻辑。
/// 【边界】除归档根目录来源不同外，本函数与 `store_output` 完全一致：它仍只通过受控目录
/// API 发布单文件、仍按 `persist_max_bytes` 限制记录大小，也不放宽任何 owner、Run、数据
/// revision 或过期语义。
fn store_output_at(
    output_root: &Path,
    request: &AuthorizedCapabilityRequest,
    output: &mut Value,
) -> Result<(), PortError> {
    let mut captured = serde_json::Map::new();
    let budget = kiana_domain::ExecutionOutputBudget::default();
    budget
        .validate()
        .map_err(|_| error("output_budget_invalid"))?;
    for name in ["stdout", "stderr"] {
        if let Some(text) = output[name].as_str() {
            let safe = crate::execution_output::safe_output_text(text);
            captured.insert(name.to_owned(), json!(safe));
            output[name] = json!(crate::execution_output::bounded_preview(
                &safe,
                budget.preview_max_bytes
            ));
        }
    }
    if captured.is_empty() {
        return Ok(());
    }
    let id = request.request.request_id.to_string();
    let root = required(&request.request.arguments, "project_root")?;
    let current = crate::data_governance::read_policy(Path::new(root))?.revision;
    // 优先沿用执行时绑定的 source revision；旧调用者未提供时以当前 policy revision 记账。
    // read_output 会与之后的当前 revision 比较，策略变化即可拒绝读取旧输出。
    let epoch = request.request.arguments["source_data_revision"]
        .as_u64()
        .unwrap_or(current);
    let identity = identity(&request.request.arguments);
    let content_digest = kiana_domain::json_digest(&json!(&captured));
    let size_bytes = serde_json::to_vec(&captured)
        .map_err(|_| error("output_encode_failed"))?
        .len() as u64;
    let run_id = request.request.arguments["run_id"]
        .as_str()
        .and_then(RunId::parse_str);
    let now = unix_time_ms()?;
    // 归档只保留 15 分钟；采用 Unix 毫秒绝对时刻，读端必须检查过期，而不是只靠清理任务。
    let expires_at_unix_ms = now
        .checked_add(
            Duration::from_secs(15 * 60)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
        )
        .ok_or_else(|| error("output_expiry_overflow"))?;
    let output_ref = ExecutionOutputRef::new(
        request.request.request_id,
        run_id,
        InvocationId::from_uuid(request.request.request_id.as_uuid()),
        content_digest,
        size_bytes,
        kiana_domain::json_digest(&identity),
        expires_at_unix_ms,
    )
    .map_err(|_| error("output_reference_invalid"))?;
    let record = json!({"schema":"kiana.execution-output.v1","identity":identity,"data_epoch":epoch,
        "expires_at_unix_ms":expires_at_unix_ms,
        "redaction_profile_digest":crate::execution_output::preview_profile_digest(kiana_domain::SecretScanChannel::Stdout),
        "output_ref":output_ref.clone(),"output":captured});
    let bytes = serde_json::to_vec(&record).map_err(|_| error("output_encode_failed"))?;
    if bytes.len() > budget.persist_max_bytes {
        return Err(error("output_persist_quota_exceeded"));
    }
    // LocalDir 通过受控目录 API 发布单文件，避免这里直接拼接不受信任文件路径。
    crate::local_packages::LocalDir::open(output_root, true)?
        .publish(&format!("{id}.json"), &bytes)?;
    let mut output_ref =
        serde_json::to_value(output_ref).map_err(|_| error("output_reference_encode_failed"))?;
    output_ref["sha256"] = json!(format!("sha256:{}", crate::local_packages::sha256(&bytes)));
    output_ref["preview_bytes"] = json!(budget.preview_max_bytes);
    output["output_ref"] = output_ref;
    Ok(())
}
/// 按身份、数据策略和游标读取已存档的 shell 输出片段。
///
/// 【调用者】Broker 将 `execution.output.read` 路由到 `handle_read_or_continue()`。
/// 【输入】output_id、owner/project/run 参数、stdout/stderr 选择、UTF-8 字节 offset、页长
/// 和可选 cursor。
/// 【核心流程】先按预算上限读取 JSON 记录并校验 owner；再校验引用字段与身份 digest、
/// 过期时间、当前数据策略 revision 和内容摘要；最后在合法 UTF-8 边界返回一页及下一游标。
/// 【输出】文本页、next_offset、next_cursor、内容摘要和是否读完；本函数不推进存储状态。
/// 【失败】身份不符、引用损坏/过期、数据已撤销、流名无效、offset 落在字符中间、摘要或
/// cursor 不匹配时拒绝读取。错误会经 Broker/Core 返回给请求者，不会自动重试或刷新引用。
/// 【设计原因】文件内容可以按需分页而不一次塞进上下文；cursor 将起始偏移绑定到输出 ID、
/// stream 和内容 digest，避免客户端把另一个偏移误当成同一页的延续。
fn read_output(arguments: &Value) -> Result<Value, PortError> {
    read_output_at(&output_directory()?, arguments)
}

/// `read_output` 的显式归档目录版本。
///
/// 【调用者】生产路径经 `read_output` 传入 `KIANA_HOME` 派生的受控归档目录；本模块的
/// `#[cfg(test)]` 行为夹具传入临时目录，直接驱动同一份 owner/Run/数据 revision/过期/
/// 内容摘要/游标校验。
/// 【边界】只改变归档目录来源；身份、Run、数据 revision、过期时刻、内容摘要与游标校验
/// 与生产读取路径逐条相同，不新增放宽分支。
fn read_output_at(output_root: &Path, arguments: &Value) -> Result<Value, PortError> {
    let budget = kiana_domain::ExecutionOutputBudget::default();
    let id = serde_json::from_value::<RequestId>(arguments["output_id"].clone())
        .map_err(|_| error("output_id_invalid"))?;
    let bytes = crate::local_packages::LocalDir::open(output_root, false)?
        .read(&format!("{id}.json"), budget.persist_max_bytes)?;
    let record: Value =
        serde_json::from_slice(&bytes).map_err(|_| error("output_record_invalid"))?;
    check_owner(&record["identity"], arguments)?;
    let output_ref: ExecutionOutputRef = serde_json::from_value(record["output_ref"].clone())
        .map_err(|_| error("output_reference_invalid"))?;
    output_ref
        .validate()
        .map_err(|_| error("output_reference_invalid"))?;
    if output_ref.output_id != id
        || output_ref.invocation_id != InvocationId::from_uuid(id.as_uuid())
        || output_ref.scope_digest != kiana_domain::json_digest(&record["identity"])
        || record["expires_at_unix_ms"] != json!(output_ref.expires_at_unix_ms)
    {
        return Err(error("output_reference_integrity_mismatch"));
    }
    if unix_time_ms()? >= output_ref.expires_at_unix_ms {
        return Err(error("output_reference_expired"));
    }
    if record["data_epoch"]
        != json!(
            crate::data_governance::read_policy(Path::new(required(arguments, "project_root")?))?
                .revision
        )
    {
        return Err(error("output_data_revoked"));
    }
    let stream = arguments["stream"].as_str().unwrap_or("stdout");
    if !matches!(stream, "stdout" | "stderr") {
        return Err(error("output_stream_invalid"));
    }
    let text = record["output"][stream].as_str().unwrap_or_default();
    if output_ref.content_digest != kiana_domain::json_digest(&record["output"]) {
        return Err(error("output_content_digest_mismatch"));
    }
    let start = arguments["offset"].as_u64().unwrap_or(0) as usize;
    if start > text.len() || !text.is_char_boundary(start) {
        return Err(error("output_offset_invalid"));
    }
    // limit 最大 65536 字节（64 KiB）；裁到字符边界后 end 可能略小于请求长度，
    // next_offset 返回实际切片位置，供下一页接续。
    // 未提供 limit 时默认取 8192 字节（8 KiB），显式值最多 65536 字节（64 KiB）。
    let mut end = start
        .saturating_add(arguments["limit"].as_u64().unwrap_or(8192).min(65536) as usize)
        .min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let cursor = kiana_domain::json_digest(&json!({
        "output_id": id,
        "stream": stream,
        "offset": start,
        "content_digest": output_ref.content_digest,
    }));
    if arguments
        .get("cursor")
        .and_then(Value::as_str)
        .is_some_and(|provided| provided != cursor)
    {
        return Err(error("output_cursor_invalid"));
    }
    let next_cursor = (end < text.len()).then(|| {
        kiana_domain::json_digest(&json!({
            "output_id": id,
            "stream": stream,
            "offset": end,
            "content_digest": output_ref.content_digest,
        }))
    });
    Ok(
        json!({"output_id":id,"stream":stream,"text":&text[start..end],"next_offset":end,
            "next_cursor":next_cursor,"content_digest":output_ref.content_digest,"complete":end==text.len()}),
    )
}
/// 将工作区中指定、未撤销的源文件打包为一个本地 JSON 文件。
///
/// 【调用者】`handle_read_or_continue()` 将 `local.package` 参数交给 blocking 池执行。
/// 【输入】canonical project root、受 `path_allow` 限制的 destination、manifest，以及每个
/// 源文件路径和调用方预期 SHA-256。
/// 【核心流程】校验目标/源路径不指向私有目录或 `.git`；读取当前 revoked-source policy；
/// 最多处理 128 个 source，每个文件和聚合源内容均不超过 2 MiB；逐项核对 SHA-256 并要求
/// UTF-8；生成 `kiana.local-package.v1` JSON 后再次限制成品大小；目标已存在时仅允许内容
/// 完全一致的幂等复用，否则通过工作区发布 helper 创建新文件。
/// 【输出】包 ID、相对路径、成品 digest/字节数，以及 reused 标志或发布回执。
/// 【副作用】新目标会写入项目工作区并返回发布回执；所有校验通过前不会发布。
/// 【失败】路径越权/被撤销、源摘要变化、非 UTF-8、大小越界或目标内容冲突时拒绝；
/// 文件系统和发布错误向 Broker/Core 传播。它不会重新读取或修改用户授权。
/// 【设计原因】将显式选择的源内容冻结成可校验包，避免悄悄打包读取后变化的文件；
/// 目标路径仍受当前请求的写范围限制。
fn local_package(arguments: &Value) -> Result<Value, PortError> {
    let root = PathBuf::from(required(arguments, "project_root")?)
        .canonicalize()
        .map_err(|_| error("package_root_unavailable"))?;
    let package_id = serde_json::from_value::<RequestId>(arguments["package_id"].clone())
        .map_err(|_| error("package_id_invalid"))?;
    let destination = kiana_domain::normalize_role_path(required(arguments, "destination")?)
        .ok_or_else(|| error("package_destination_invalid"))?;
    if destination == "."
        || destination
            .split('/')
            .any(crate::harness_sandbox::private_component)
        || destination.starts_with(".git/")
    {
        return Err(error("package_destination_denied"));
    }
    let paths = arguments["path_allow"]
        .as_array()
        .ok_or_else(|| error("package_write_scope_required"))?
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if kiana_domain::enforce_path_containment(&paths, &destination).is_err() {
        return Err(error("package_path_denied"));
    }
    if !arguments["manifest"].is_object() {
        return Err(error("package_manifest_required"));
    }
    // 先限制 source 数量和累计字节，再构造整份 JSON；之后还会对序列化成品再限一次，
    // 因为 JSON 转义会让成品略大于原始 UTF-8 内容。
    let sources = arguments["sources"]
        .as_array()
        .ok_or_else(|| error("package_sources_required"))?;
    if sources.len() > 128 {
        return Err(error("package_source_limit"));
    }
    let directory = crate::local_packages::LocalDir::open(&root, false)?;
    let policy = crate::data_governance::read_policy(&root)?;
    let mut captured = Vec::new();
    let mut total = 0usize;
    for source in sources {
        let path = kiana_domain::normalize_role_path(required(source, "path")?)
            .ok_or_else(|| error("package_source_invalid"))?;
        if path
            .split('/')
            .any(crate::harness_sandbox::private_component)
            || path == ".git"
            || path.starts_with(".git/")
            || policy
                .revoked_sources
                .iter()
                .any(|source| path == *source || path.starts_with(&format!("{source}/")))
        {
            return Err(error("package_source_denied"));
        }
        // LocalDir 的单文件读取上限与下方包总量预算共同约束内存使用；单文件合格不代表
        // 聚合后的所有 sources 也合格。
        let bytes = directory.read(&path, 2 * 1024 * 1024)?;
        total = total.saturating_add(bytes.len());
        if total > 2 * 1024 * 1024 {
            return Err(error("package_size_limit"));
        }
        let sha256 = crate::local_packages::sha256(&bytes);
        if source["sha256"] != sha256 {
            return Err(error("package_source_changed"));
        }
        let text = String::from_utf8(bytes).map_err(|_| error("package_source_utf8_required"))?;
        captured.push(json!({"path":path,"sha256":sha256,"content_utf8":text}));
    }
    let package = json!({"schema":"kiana.local-package.v1","package_id":package_id,"manifest":arguments["manifest"],"sources":captured});
    let bytes = serde_json::to_vec_pretty(&package).map_err(|_| error("package_encode_failed"))?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(error("package_size_limit"));
    }
    let digest = crate::local_packages::sha256(&bytes);
    let target = root.join(&destination);
    // 已存在同内容包时允许幂等重用；同路径但内容不同必须冲突退出，不能静默覆盖用户文件。
    if target.exists() {
        if directory.read(&destination, 2 * 1024 * 1024)? != bytes {
            return Err(error("package_destination_conflict"));
        }
        return Ok(
            json!({"package_id":package_id,"path":destination,"sha256":digest,"bytes":bytes.len(),"reused":true}),
        );
    }
    let change = crate::execution_workspace::PublishedFile {
        path: destination.clone(),
        before: None,
        after: Some(bytes.clone()),
        executable: false,
    };
    let result = crate::apply_patch::publish_workspace_files(&root, &[change], package_id)?;
    Ok(
        json!({"package_id":package_id,"path":destination,"sha256":digest,"bytes":bytes.len(),"receipt":result}),
    )
}
/// 从操作参数中挑出后续重读/比较所需的 owner 与项目字段。
///
/// 该 JSON 会随 process 事实和持久化输出记录一起存储；它是供本模块执行精确相等校验的
/// 快照，不会自行向身份服务解析角色，也不等同于签名凭证。
fn identity(arguments: &Value) -> Value {
    json!({"actor_id":arguments["actor_id"],"session_id":arguments["session_id"],"role_id":arguments["role_id"],"department_id":arguments["department_id"],"project_root":arguments["project_root"],"run_id":arguments["run_id"]})
}
/// 解析并验证 JobHandle 与当前 continuation 请求的绑定关系。
///
/// 【调用者】活动进程 continuation 与 Daemon 重启后的 EventStore poll 路径。
/// 【校验范围】句柄格式/摘要、必须存在的进程组身份、job ID 与 process ID、启动 request
/// ID、绝对过期时间、owner/session、Run/Turn 关联值，以及 canonical project root/session
/// 计算出的项目摘要。
/// 【输出/失败】全部匹配时返回领域 `JobHandle`，方便调用方取出 handle digest；任一绑定不符
/// 返回具体稳定错误码，避免只凭调用者提供的 process ID 控制另一主体的进程。
/// 【边界】本函数只比较随请求和记录携带的字段，不重新向 Core 查询授权，也不读取当前
/// authority；调用方仍须先过 Broker/Core，并在适当操作分支另行比较 authority 快照。
fn validate_job_handle(identity: &Value, arguments: &Value) -> Result<JobHandle, PortError> {
    let handle: JobHandle = serde_json::from_value(identity["job_handle"].clone())
        .map_err(|_| error("job_handle_invalid"))?;
    handle.validate().map_err(|_| error("job_handle_invalid"))?;
    if handle.process_group_id.is_none() {
        return Err(error("job_handle_process_identity_missing"));
    }
    let job_id = serde_json::from_value::<RequestId>(identity["process_id"].clone())
        .map_err(|_| error("job_handle_invalid"))?;
    let start_request_id = serde_json::from_value::<RequestId>(identity["request_id"].clone())
        .map_err(|_| error("job_handle_invalid"))?;
    if handle.job_id != job_id || handle.start_request_id != start_request_id {
        return Err(error("job_handle_identity_mismatch"));
    }
    if unix_time_ms()? >= handle.expires_at_unix_ms {
        return Err(error("job_handle_expired"));
    }
    if handle.owner_id != required(arguments, "actor_id")?
        || handle.session_id != SessionId::new(required(arguments, "session_id")?)
    {
        return Err(error("job_handle_owner_mismatch"));
    }
    let requested_run = arguments["run_id"].as_str().and_then(RunId::parse_str);
    let requested_turn = arguments["turn_id"].as_str().and_then(TurnId::parse_str);
    if handle.run_id != requested_run || handle.turn_id != requested_turn {
        return Err(error("job_handle_scope_mismatch"));
    }
    let root = Path::new(required(arguments, "project_root")?)
        .canonicalize()
        .map_err(|_| error("project_root_unavailable"))?;
    let project_digest = kiana_domain::json_digest(&json!({
        "project_root": root,
        "session_id": handle.session_id,
    }));
    if handle.project_digest != project_digest {
        return Err(error("job_handle_project_mismatch"));
    }
    Ok(handle)
}
/// 将存储的 owner/project identity 与本次请求逐字段比较。
///
/// actor、session、role、department 和 project root 必须存在且完全相等；如果原记录带有
/// Run ID，请求也必须携带完全相同的 Run ID。Run 为空的旧/独立请求不在这里强制要求 Run，
/// Turn 绑定则由 JobHandle 校验处理。失败返回 owner mismatch，不修改状态。
fn check_owner(stored: &Value, arguments: &Value) -> Result<(), PortError> {
    for name in [
        "actor_id",
        "session_id",
        "role_id",
        "department_id",
        "project_root",
    ] {
        if stored[name].is_null() || stored[name] != arguments[name] {
            return Err(error("execution_owner_mismatch"));
        }
    }
    if !stored["run_id"].is_null()
        && (arguments["run_id"].is_null() || stored["run_id"] != arguments["run_id"])
    {
        return Err(error("execution_owner_run_mismatch"));
    }
    Ok(())
}
/// 读取一个正整数搜索预算：缺省时用固定默认值，显式但非法时拒绝而不回退到默认值。
///
/// 显式值必须能无损转换为当前平台 `usize`，且在 1..=maximum 内；这种 fail-closed 规则
/// 防止调用方用负数、溢出值或错误类型绕过目录检索的内存/上下文上限。
fn tool_search_limit(
    arguments: &Value,
    name: &str,
    default: usize,
    maximum: usize,
    code: &str,
) -> Result<usize, PortError> {
    match arguments.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0 && *value <= maximum)
            .ok_or_else(|| error(code)),
    }
}

/// 读取可选布尔筛选开关；缺省时采用调用点给定默认值，显式非布尔值返回错误。
///
/// 它只负责类型与缺省处理，是否允许某个开关关闭由调用点继续决定（例如健康目录检查
/// 在路由中被强制为 true）。
fn tool_search_filter(arguments: &Value, name: &str, default: bool) -> Result<bool, PortError> {
    match arguments.get(name) {
        None => Ok(default),
        Some(value) => value
            .as_bool()
            .ok_or_else(|| error("tool_search_limits_invalid")),
    }
}

/// 从 JSON 对象提取非空字符串参数，并用统一错误码报告缺失/类型错误/空串。
///
/// 这里只做字符串形状检查，不会 trim、规范化路径或验证 ID 格式；这些语义约束必须由
/// 后续具体解析器或领域校验完成。
fn required<'a>(value: &'a Value, key: &str) -> Result<&'a str, PortError> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| error(&format!("execution_argument_required:{key}")))
}
/// 将本模块的稳定拒绝原因包装为 Port 层失败；Core/Broker 会负责后续结果归类与回传。
fn error(reason: &str) -> PortError {
    PortError::Failed(reason.to_owned())
}

/// 读取自 Unix epoch 起的墙钟毫秒数，用于绝对过期时间和 JobHandle 有效期比较。
///
/// 这不是单调时钟 deadline；系统时钟回拨会影响绝对过期判断。时钟早于 epoch 时返回错误，
/// 由当前请求作为失败处理，不会自动改用另一个时间源。
fn unix_time_ms() -> Result<u64, PortError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .map_err(|_| error("output_clock_invalid"))
}

#[cfg(test)]
/// `tool.search` 的 handler 级拒绝和返回契约检查。
///
/// 这些测试直接构造内存 EventStore 与 ExecutionControl，不启动真实子进程，也不经过完整
/// DaemonHost/ControlPlane/Broker 组合链；因此它们证明的是本 handler 的目录查询分支行为，
/// 不是进程 sandbox、跨层授权或 live 运行环境的证据。
mod tests {
    use super::*;
    use kiana_domain::{CapabilityRequest, SchemaVersion, TOOL_CATALOG_VERSION};
    use kiana_eventlog::MemoryEventLog;

    /// 构造该组工具目录查询共用的最小参数集。
    fn search_arguments() -> Value {
        json!({
            "operator_authorized": true,
            "role_id": "builder",
            "query": "shell"
        })
    }

    /// 经 `handle_read_or_continue()` 直接调用查询分支，并额外约束其无执行副作用。
    async fn execute_search(arguments: Value) -> Result<CapabilityResult, PortError> {
        let events = Arc::new(MemoryEventLog::new());
        let control = ExecutionControl {
            events: events.clone(),
            processes: Mutex::new(HashMap::new()),
            capacity: Arc::new(tokio::sync::Semaphore::new(8)),
        };
        let request = AuthorizedCapabilityRequest::new(
            "policy:cap30-handler-fixture",
            CapabilityRequest::new(
                RequestId::new(),
                CapabilityKind::Query,
                "tool.search",
                arguments,
            ),
        )
        .expect("typed handler fixture");
        let result = control.handle_read_or_continue(request).await;
        assert!(
            events.read_all().await.expect("in-memory facts").is_empty(),
            "catalog discovery must not write execution facts"
        );
        assert!(
            control.processes.lock().unwrap().is_empty(),
            "catalog discovery must not start processes"
        );
        result
    }

    /// 断言查询请求按预期稳定错误码被拒绝。
    fn deny(result: Result<CapabilityResult, PortError>, expected: &str) {
        assert!(
            matches!(result, Err(PortError::Failed(ref code)) if code == expected),
            "expected {expected}, got {result:?}"
        );
    }

    #[tokio::test]
    async fn cap30_tool_search_stale_catalog_version_is_denied() {
        let mut arguments = search_arguments();
        arguments["catalog_version"] = json!(SchemaVersion::new(
            TOOL_CATALOG_VERSION.major,
            TOOL_CATALOG_VERSION.minor + 1
        ));
        deny(
            execute_search(arguments).await,
            "tool_search_catalog_version_mismatch",
        );
    }

    #[tokio::test]
    async fn cap30_tool_search_context_budget_is_enforced_before_schema_return() {
        let mut arguments = search_arguments();
        arguments["max_context_tokens"] = json!(1);
        deny(
            execute_search(arguments).await,
            "tool_search_context_budget_exceeded",
        );
    }

    #[tokio::test]
    async fn cap30_tool_search_invalid_context_budget_never_defaults_to_success() {
        for value in [
            json!(0),
            Value::Null,
            json!("8192"),
            json!(-1),
            json!(u64::MAX),
        ] {
            let mut arguments = search_arguments();
            arguments["max_context_tokens"] = value;
            deny(
                execute_search(arguments).await,
                "tool_search_context_budget_invalid",
            );
        }
    }

    #[tokio::test]
    async fn cap30_tool_search_malformed_catalog_version_is_denied() {
        for value in [
            Value::Null,
            json!("1.0"),
            json!([]),
            json!({"major":1,"minor":0,"unknown":true}),
        ] {
            let mut arguments = search_arguments();
            arguments["catalog_version"] = value;
            deny(
                execute_search(arguments).await,
                "tool_search_catalog_version_invalid",
            );
        }
    }

    #[tokio::test]
    async fn cap30_tool_search_malformed_limits_never_defaults_or_wraps() {
        for (name, value) in [
            ("query", json!(false)),
            ("max_results", Value::Null),
            ("max_results", json!("32")),
            ("max_results", json!(u64::MAX)),
            ("max_schema_bytes", json!(-1)),
            ("max_schema_bytes", json!(u64::MAX)),
            ("replay_safe_only", json!("true")),
            ("require_healthy_catalog", Value::Null),
        ] {
            let mut arguments = search_arguments();
            arguments[name] = value;
            deny(
                execute_search(arguments).await,
                "tool_search_limits_invalid",
            );
        }
    }

    #[tokio::test]
    async fn cap30_tool_search_cannot_weaken_catalog_health_validation() {
        let mut arguments = search_arguments();
        arguments["require_healthy_catalog"] = json!(false);
        deny(
            execute_search(arguments).await,
            "tool_search_catalog_health_required",
        );
    }

    #[tokio::test]
    async fn cap30_tool_search_operator_and_server_role_filter_are_retained() {
        let mut arguments = search_arguments();
        arguments["operator_authorized"] = json!(false);
        deny(execute_search(arguments).await, "operator_required");

        let mut arguments = search_arguments();
        arguments["role_id"] = json!("architect");
        arguments["allowed_tools"] = json!(["shell"]);
        let response = execute_search(arguments).await.expect("filtered search");
        assert!(response.output["tools"].as_array().unwrap().is_empty());
        assert_eq!(response.output["does_not_grant_execution"], true);
    }

    #[tokio::test]
    async fn cap30_tool_search_returns_exact_pinned_bounded_non_granting_metadata() {
        let response = execute_search(search_arguments())
            .await
            .expect("bounded search");
        assert!(response.success);
        let output = response.output;
        assert_eq!(output["catalog_version"], json!(TOOL_CATALOG_VERSION));
        assert_eq!(
            output["catalog_digest"],
            kiana_domain::ToolCatalogSnapshot::current().digest
        );
        assert_eq!(output["search_mode"], "ranked_token_bounded");
        assert_eq!(output["does_not_grant_execution"], true);
        let tools = output["tools"].as_array().expect("selected schemas");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "shell");
        let selected_bytes = serde_json::to_vec(tools).unwrap().len();
        assert_eq!(output["selected_schema_bytes"], json!(selected_bytes));
        assert_eq!(
            output["estimated_context_tokens"],
            json!(selected_bytes.div_ceil(4))
        );
        assert!(output["estimated_context_tokens"].as_u64().unwrap() <= 8192);
    }

    #[tokio::test]
    async fn cap30_tool_search_replay_filter_only_narrows_current_catalog() {
        let mut arguments = search_arguments();
        arguments["query"] = json!("");
        arguments["replay_safe_only"] = json!(true);
        let response = execute_search(arguments).await.expect("safe discovery");
        let catalog = kiana_domain::ToolCatalogSnapshot::current();
        let tools = response.output["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
        assert!(tools.iter().all(|tool| catalog
            .descriptor(tool["name"].as_str().unwrap())
            .is_some_and(|descriptor| descriptor.replay_class == "replay_safe")));
        assert_eq!(response.output["does_not_grant_execution"], true);
    }

    /// ER-19：构造一份与控制面 `validate_job_handle` 一致的句柄身份材料。
    ///
    /// 返回 `(handle, project_root, arguments)`；`project_root` 与句柄 `project_digest`
    /// 都基于同一个已 canonicalize 的临时目录，因此成功路径可稳定通过。
    fn er19_handle(owner: &str, session: &str) -> (JobHandle, String, Value) {
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("temp dir canonical");
        let project_root = root.to_string_lossy().into_owned();
        let session_id = SessionId::new(session);
        let project_digest = kiana_domain::json_digest(&json!({
            "project_root": root,
            "session_id": session_id.clone(),
        }));
        let arguments = json!({
            "actor_id": owner,
            "session_id": session,
            "project_root": project_root,
            "run_id": Value::Null,
            "turn_id": Value::Null,
        });
        let handle = JobHandle::new(
            RequestId::new(),
            RequestId::new(),
            None,
            None,
            owner,
            session_id,
            project_digest,
            1,
            Some(4242),
            unix_time_ms().expect("clock") + 3_600_000,
        )
        .expect("job handle");
        (handle, project_root, arguments)
    }

    /// ER-19：按 `process.started` 事实形状组装 continuation 身份。
    fn er19_identity(handle: &JobHandle, process_id: RequestId, request_id: RequestId) -> Value {
        json!({
            "job_handle": serde_json::to_value(handle).expect("job handle json"),
            "process_id": process_id,
            "request_id": request_id,
        })
    }

    /// ER-19：断言 `validate_job_handle` 以预期稳定错误码拒绝。
    fn er19_deny(result: Result<JobHandle, PortError>, expected: &str) {
        assert!(
            matches!(result, Err(PortError::Failed(ref code)) if code == expected),
            "expected {expected}, got {result:?}"
        );
    }

    #[test]
    fn er19_stale_handle_cannot_stop_new_execution() {
        let (handle, _project_root, arguments) = er19_handle("er19-actor", "er19-session");

        // 成功路径：句柄与它自己启动的 execution 身份一致时通过校验。
        let identity = er19_identity(&handle, handle.job_id, handle.start_request_id);
        assert!(validate_job_handle(&identity, &arguments).is_ok());

        // 拒绝：旧句柄被拿去控制另一个 execution（不同的 job/process id）。
        let stale = er19_identity(&handle, RequestId::new(), handle.start_request_id);
        er19_deny(
            validate_job_handle(&stale, &arguments),
            "job_handle_identity_mismatch",
        );

        // 拒绝：非原 owner 不能用该句柄续用。
        let mut foreign_owner = arguments.clone();
        foreign_owner["actor_id"] = json!("other-actor");
        er19_deny(
            validate_job_handle(&identity, &foreign_owner),
            "job_handle_owner_mismatch",
        );

        // 拒绝：身份材料被篡改后摘要不符，fail-closed 为 job_handle_invalid。
        let mut tampered = serde_json::to_value(&handle).expect("job handle json");
        tampered["job_id"] = json!(RequestId::new());
        let tampered_identity = json!({
            "job_handle": tampered,
            "process_id": handle.job_id,
            "request_id": handle.start_request_id,
        });
        er19_deny(
            validate_job_handle(&tampered_identity, &arguments),
            "job_handle_invalid",
        );
    }

    #[test]
    fn er19_lease_expiry_does_not_release_live_process() {
        // TTL 到期只拒绝后续操作，不能单独作为“进程已停止”的证据。
        let root = std::env::temp_dir()
            .canonicalize()
            .expect("temp dir canonical");
        let project_root = root.to_string_lossy().into_owned();
        let session = SessionId::new("er19-session");
        let project_digest = kiana_domain::json_digest(&json!({
            "project_root": root,
            "session_id": session.clone(),
        }));
        let expired = JobHandle::new(
            RequestId::new(),
            RequestId::new(),
            None,
            None,
            "er19-actor",
            session,
            project_digest,
            1,
            Some(4242),
            unix_time_ms().expect("clock").max(2) - 1,
        )
        .expect("expired job handle still decodes");
        let arguments = json!({
            "actor_id": "er19-actor",
            "session_id": "er19-session",
            "project_root": project_root,
            "run_id": Value::Null,
            "turn_id": Value::Null,
        });
        let identity = er19_identity(&expired, expired.job_id, expired.start_request_id);
        er19_deny(
            validate_job_handle(&identity, &arguments),
            "job_handle_expired",
        );

        // 运行期资源租约在同一时刻同样以过期拒绝，而不是返回“已释放”。
        let lease = kiana_domain::ResourceLease::new(
            kiana_domain::StorageLockId::new(),
            kiana_domain::FenceTokenId::new(),
            "src/lib.rs",
            RunId::new(),
            None,
            "er19-session",
            1,
            10,
            100,
        )
        .expect("resource lease");
        assert_eq!(
            lease.validate_current(100, 1, lease.fence_token),
            Err("resource_lease_expired".to_owned())
        );
    }
}

#[cfg(all(test, unix))]
/// H15 有界输出归档与受控分页读取的行为夹具。
///
/// 这些测试用隔离的临时归档目录直接驱动 `store_output_at`/`read_output_at`，即生产 shell
/// 输出归档与 `execution.output.read` 校验复用的同一份逻辑：拒绝路径优先覆盖跨 Run、跨
/// owner、过期、数据 revision 撤销、内容篡改、非法 offset/stream 与伪造 cursor；成功路径
/// 覆盖有界预览、typed 引用与稳定游标分页复原。
///
/// 【边界】它们不经过 DaemonHost/ControlPlane/Broker 组合链，不启动真实子进程，也不证明
/// 跨进程 ArtifactStore、删除传播、保留策略或操作系统级内存/磁盘限额。
mod h15_output_read {
    use super::*;
    use kiana_domain::CapabilityRequest;

    const ACTOR: &str = "h15-actor";
    const SESSION: &str = "h15-session";

    /// 建立一个隔离的临时项目目录（同时作为归档目录的父目录）。
    fn root(label: &str) -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).expect("time");
        let stamp = stamp.as_nanos();
        let path = std::env::temp_dir().join(format!("kiana-h15-{label}-{stamp}"));
        std::fs::create_dir_all(&path).expect("temp project root");
        path
    }

    /// 把可选 Run 序列化成 wire 值，保持与生产请求参数一致。
    fn run_value(run_id: Option<RunId>) -> Value {
        match run_id {
            Some(id) => serde_json::to_value(id).expect("run id"),
            None => Value::Null,
        }
    }

    /// 构造与生产 `identity()` 字段一致的 owner/scope 参数。
    fn arguments(project_root: &Path, run_id: Option<RunId>) -> Value {
        json!({
            "operator_authorized": true,
            "actor_id": ACTOR,
            "session_id": SESSION,
            "role_id": "builder",
            "department_id": "engineering",
            "project_root": project_root.to_string_lossy(),
            "run_id": run_value(run_id),
        })
    }

    /// 组装一份已授权 shell 输出请求；`store_output_at` 从这里读取 project_root/Run。
    fn request(project_root: &Path, run_id: Option<RunId>) -> AuthorizedCapabilityRequest {
        AuthorizedCapabilityRequest::new(
            "policy:h15-output-fixture",
            CapabilityRequest::new(
                RequestId::new(),
                CapabilityKind::Filesystem,
                "shell.exec",
                arguments(project_root, run_id),
            ),
        )
        .expect("typed handler fixture")
    }

    /// 断言读取按预期稳定错误码被拒绝。
    fn deny_value(result: Result<Value, PortError>, expected: &str) {
        assert!(
            matches!(result, Err(PortError::Failed(ref code)) if code == expected),
            "expected {expected}, got {result:?}"
        );
    }

    /// 用生产结构与摘要直接发布一份归档记录，用于精确控制过期、数据 revision 与内容篡改。
    fn publish_record(
        project: &Path,
        archive: &Path,
        run_id: Option<RunId>,
        stdout: &str,
        expires_at_unix_ms: u64,
        data_epoch: u64,
    ) -> (RequestId, Value) {
        let output_id = RequestId::new();
        let identity = identity(&arguments(project, run_id));
        let captured = json!({"stdout": stdout, "stderr": ""});
        let content_digest = kiana_domain::json_digest(&captured);
        let captured_bytes = serde_json::to_vec(&captured).expect("captured bytes");
        let size_bytes = captured_bytes.len() as u64;
        let reference = ExecutionOutputRef::new(
            output_id,
            run_id,
            InvocationId::from_uuid(output_id.as_uuid()),
            content_digest,
            size_bytes,
            kiana_domain::json_digest(&identity),
            expires_at_unix_ms,
        )
        .expect("synthetic output reference");
        let record = json!({
            "schema": "kiana.execution-output.v1",
            "identity": identity,
            "data_epoch": data_epoch,
            "expires_at_unix_ms": expires_at_unix_ms,
            "redaction_profile_digest":
                crate::execution_output::preview_profile_digest(kiana_domain::SecretScanChannel::Stdout),
            "output_ref": serde_json::to_value(&reference).expect("reference json"),
            "output": captured,
        });
        republish(archive, output_id, &record);
        (output_id, record)
    }

    /// 用受控目录 API 覆盖发布一份已存在的归档记录。
    fn republish(archive: &Path, output_id: RequestId, record: &Value) {
        let bytes = serde_json::to_vec(record).expect("record bytes");
        crate::local_packages::LocalDir::open(archive, true)
            .expect("archive dir")
            .publish(&format!("{output_id}.json"), &bytes)
            .expect("publish record");
    }

    #[test]
    fn h15_behavior_truncated_result_reads_in_verified_pages() {
        let project = root("pages");
        let archive = project.join("execution-output");
        let run_id = RunId::new();
        let request = request(&project, Some(run_id));
        let output_id = request.request.request_id;

        let text = "h15-line-payload\n".repeat(600);
        let mut output = json!({"stdout": text.clone(), "stderr": ""});
        store_output_at(&archive, &request, &mut output).expect("store bounded output");

        // 模型侧只拿到有界预览与 typed 引用，不是被称作“完整结果”的原始文本。
        let budget = kiana_domain::ExecutionOutputBudget::default();
        let preview = output["stdout"].as_str().expect("preview");
        assert!(preview.contains("...preview-truncated..."));
        assert!(preview.len() <= budget.preview_max_bytes + "\n...preview-truncated...".len());
        let expected_output_id = serde_json::to_value(output_id).expect("output id json");
        assert_eq!(output["output_ref"]["output_id"], expected_output_id);

        // 分页读取：稳定 cursor、UTF-8 边界与 next_offset 能把归档复原成完整安全文本。
        let mut assembled = String::new();
        let mut offset = 0u64;
        let mut cursor: Option<Value> = None;
        loop {
            let mut read = arguments(&project, Some(run_id));
            read["output_id"] = serde_json::to_value(output_id).expect("output id json");
            read["stream"] = json!("stdout");
            read["offset"] = json!(offset);
            read["limit"] = json!(4096);
            if let Some(value) = &cursor {
                read["cursor"] = value.clone();
            }
            let page = read_output_at(&archive, &read).expect("verified page");
            assembled.push_str(page["text"].as_str().expect("page text"));
            assert_eq!(
                page["content_digest"],
                output["output_ref"]["content_digest"]
            );
            if page["complete"] == json!(true) {
                break;
            }
            offset = page["next_offset"].as_u64().expect("next offset");
            cursor = Some(page["next_cursor"].clone());
            assert!(cursor.as_ref().expect("cursor").is_string());
        }
        assert_eq!(assembled, text);
    }

    #[test]
    fn h15_behavior_artifact_from_other_run_is_denied() {
        let project = root("scope");
        let archive = project.join("execution-output");
        let run_id = RunId::new();
        let request = request(&project, Some(run_id));
        let output_id = request.request.request_id;
        let mut output = json!({"stdout": "scoped output\n", "stderr": ""});
        store_output_at(&archive, &request, &mut output).expect("store scoped output");

        // 同一 output id，但请求携带另一个 Run：拒绝。
        let mut other_run = arguments(&project, Some(RunId::new()));
        other_run["output_id"] = serde_json::to_value(output_id).expect("output id json");
        deny_value(
            read_output_at(&archive, &other_run),
            "execution_owner_run_mismatch",
        );

        // 不携带 Run 的请求同样不能读走绑定 Run 的输出。
        let mut no_run = arguments(&project, None);
        no_run["output_id"] = serde_json::to_value(output_id).expect("output id json");
        deny_value(
            read_output_at(&archive, &no_run),
            "execution_owner_run_mismatch",
        );

        // 换成另一个 owner：拒绝。
        let mut other_actor = arguments(&project, Some(run_id));
        other_actor["actor_id"] = json!("h15-other-actor");
        other_actor["output_id"] = serde_json::to_value(output_id).expect("output id json");
        deny_value(
            read_output_at(&archive, &other_actor),
            "execution_owner_mismatch",
        );

        // 成功对照：原 owner + 原 Run 仍可读取。
        let mut owner = arguments(&project, Some(run_id));
        owner["output_id"] = serde_json::to_value(output_id).expect("output id json");
        let page = read_output_at(&archive, &owner).expect("owner read");
        assert_eq!(page["complete"], json!(true));
        assert_eq!(page["text"], json!("scoped output\n"));
    }

    #[test]
    fn h15_behavior_expired_output_reference_is_not_returned() {
        let project = root("expiry");
        let archive = project.join("execution-output");
        let policy = crate::data_governance::read_policy(&project).expect("policy");
        let now = unix_time_ms().expect("clock");
        let (output_id, _record) = publish_record(
            &project,
            &archive,
            None,
            "expired payload\n",
            now - 1_000,
            policy.revision,
        );
        let mut read = arguments(&project, None);
        read["output_id"] = serde_json::to_value(output_id).expect("output id json");
        deny_value(read_output_at(&archive, &read), "output_reference_expired");

        // 成功对照：未过期的同形状记录仍可读取。
        let (fresh_id, _fresh) = publish_record(
            &project,
            &archive,
            None,
            "fresh payload\n",
            now + 60_000,
            policy.revision,
        );
        let mut fresh = arguments(&project, None);
        fresh["output_id"] = serde_json::to_value(fresh_id).expect("output id json");
        let page = read_output_at(&archive, &fresh).expect("fresh read");
        assert_eq!(page["text"], json!("fresh payload\n"));
    }

    #[test]
    fn h15_behavior_revoked_data_revision_is_denied() {
        let project = root("epoch");
        let archive = project.join("execution-output");
        let policy = crate::data_governance::read_policy(&project).expect("policy");
        let now = unix_time_ms().expect("clock");
        // 记录绑定到比当前策略更新的数据 revision，读取时即视为已被撤销。
        let (output_id, _record) = publish_record(
            &project,
            &archive,
            None,
            "revocable payload\n",
            now + 60_000,
            policy.revision + 1,
        );
        let mut read = arguments(&project, None);
        read["output_id"] = serde_json::to_value(output_id).expect("output id json");
        deny_value(read_output_at(&archive, &read), "output_data_revoked");
    }

    #[test]
    fn h15_behavior_tampered_content_is_denied() {
        let project = root("tamper");
        let archive = project.join("execution-output");
        let policy = crate::data_governance::read_policy(&project).expect("policy");
        let now = unix_time_ms().expect("clock");
        let (output_id, mut record) = publish_record(
            &project,
            &archive,
            None,
            "original payload\n",
            now + 60_000,
            policy.revision,
        );
        // 保持引用与摘要不变，只改归档正文：内容摘要校验必须拒绝。
        record["output"]["stdout"] = json!("tampered payload\n");
        republish(&archive, output_id, &record);
        let mut read = arguments(&project, None);
        read["output_id"] = serde_json::to_value(output_id).expect("output id json");
        deny_value(
            read_output_at(&archive, &read),
            "output_content_digest_mismatch",
        );
    }

    #[test]
    fn h15_behavior_invalid_offset_stream_and_cursor_are_denied() {
        let project = root("cursor");
        let archive = project.join("execution-output");
        let policy = crate::data_governance::read_policy(&project).expect("policy");
        let now = unix_time_ms().expect("clock");
        // 多字节字符：offset 落在字符中间时不能返回半个字符。
        let text = "中".repeat(64);
        let (output_id, _record) = publish_record(
            &project,
            &archive,
            None,
            &text,
            now + 60_000,
            policy.revision,
        );

        let mut mid = arguments(&project, None);
        mid["output_id"] = serde_json::to_value(output_id).expect("output id json");
        mid["offset"] = json!(1);
        deny_value(read_output_at(&archive, &mid), "output_offset_invalid");

        let mut stream = arguments(&project, None);
        stream["output_id"] = serde_json::to_value(output_id).expect("output id json");
        stream["stream"] = json!("pty");
        deny_value(read_output_at(&archive, &stream), "output_stream_invalid");

        // 伪造 cursor：游标绑定 output/stream/offset/content digest，错误值必须拒绝。
        let mut forged = arguments(&project, None);
        forged["output_id"] = serde_json::to_value(output_id).expect("output id json");
        forged["offset"] = json!(0);
        forged["limit"] = json!(3);
        forged["cursor"] = json!(kiana_domain::json_digest(&json!("forged")));
        deny_value(read_output_at(&archive, &forged), "output_cursor_invalid");

        // 成功对照：合法 cursor 允许读取下一稳定页，next_offset 落在字符边界。
        let mut first = arguments(&project, None);
        first["output_id"] = serde_json::to_value(output_id).expect("output id json");
        first["offset"] = json!(0);
        first["limit"] = json!(3);
        let page = read_output_at(&archive, &first).expect("first page");
        assert_eq!(page["next_offset"], json!(3));
        let mut second = arguments(&project, None);
        second["output_id"] = serde_json::to_value(output_id).expect("output id json");
        second["offset"] = json!(3);
        second["limit"] = json!(3);
        second["cursor"] = page["next_cursor"].clone();
        let page2 = read_output_at(&archive, &second).expect("second page");
        assert_eq!(page2["complete"], json!(false));
    }
}
