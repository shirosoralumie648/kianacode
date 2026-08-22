# Kiana Code 使用指南

本文只描述当前 Rust workspace 已接上的真实入口。产品边界以 `.planning/PROJECT.md` 为准，当前阶段以 `.planning/ROADMAP.md` 为准，控制平面说明以 [docs/architecture.md](docs/architecture.md) 为准。更短的入口见 [QUICKSTART.md](QUICKSTART.md)，文档地图见 [docs/README.md](docs/README.md)。

## 目录

- [启动方式](#启动方式)
- [配置](#配置)
- [基础对话](#基础对话)
- [权限](#权限)
- [Project Trust](#project-trust)
- [Session 管理](#session-管理)
- [WorkflowRun](#workflowrun)
- [Project Board](#project-board)
- [Bounded Swarm](#bounded-swarm-预派发)
- [Evidence Ledger](#evidence-ledger)
- [Doctor 和 Release](#doctor-和-release)
- [TUI](#tui)
- [当前边界](#当前边界)

## 启动方式

```bash
# 构建发布版
cargo build --release -p kiana-entrypoints --bin kiana

# 交互式 REPL，需要真实终端
./target/release/kiana

# 非交互式 print 模式，适合脚本/CI/管道
./target/release/kiana -p "explain this directory"

# TUI
./target/release/kiana tui
```

开发模式：

```bash
cargo run -p kiana-entrypoints --bin kiana -- -p "hello"
```

## 配置

```bash
kiana config init
kiana login "sk-ant-xxx"
kiana config status
kiana auth status --json
kiana auth logout              # clears config API key and OAuth token file
kiana auth logout --oauth-only # clears only the OAuth token file
```

也可以直接写入配置项：

```bash
kiana config set api_key "sk-ant-xxx"
kiana config set model "claude-sonnet-4-6"
kiana config get api_key
kiana model list
kiana model list --json
kiana model catalog --json
kiana model smoke --json
kiana license status --json
```

环境变量优先级高于配置文件：

```bash
export ANTHROPIC_API_KEY="sk-ant-xxx"
export ANTHROPIC_BASE_URL="https://api.anthropic.com"
```

OpenAI-compatible 文本 provider：

```bash
export KIANA_PROVIDER="openai-compatible"
export KIANA_OPENAI_API_KEY="sk-xxx"
export KIANA_OPENAI_BASE_URL="https://api.openai.com/v1"
kiana -p "summarize README.md"
```

这个 provider 支持 OpenAI Chat Completions 文本与 function-style tools；`kiana model list --json` 会显示 `supports_tools=true`。如果只想走纯文本路径，可以显式传 `--tools ""`。

本地 Ollama 文本 provider：

```bash
export KIANA_PROVIDER="ollama"
export KIANA_OLLAMA_BASE_URL="http://localhost:11434"
export KIANA_OLLAMA_MODEL="llama3.1"
kiana -p "summarize README.md"
```

这个 provider 支持 Ollama `/api/chat` 文本和工具循环，不需要 API key；`kiana model list --json` 会显示 `supports_tools=true`。如果只想走纯文本路径，可以显式传 `--tools ""`。

Provider smoke report 默认只执行无网络 fake provider，并把 Anthropic、OpenAI-compatible、Ollama 标为 skipped，适合 CI/release gate：

```bash
kiana model smoke --json
```

Model catalog report 默认不联网，输出内置模型目录并把需要 live 查询的 OpenAI-compatible、Ollama 标为 skipped：

```bash
kiana model catalog --json
```

需要刷新动态 provider 模型目录时显式 opt-in；OpenAI-compatible 会请求 `<base_url>/models`，Ollama 会请求 `<base_url>/api/tags`：

```bash
KIANA_MODEL_CATALOG_LIVE=1 kiana model catalog --json
# 或
kiana model catalog --live --json
```

真实 provider smoke 需要显式 opt-in：

```bash
KIANA_PROVIDER_SMOKE_LIVE=1 kiana model smoke --json
# 或
kiana model smoke --live --json
```

工具调用 smoke 也是显式 opt-in；默认 release gate 不联网也不要求真实 provider 支持工具调用：

```bash
kiana model smoke --tools --json
KIANA_PROVIDER_SMOKE_LIVE=1 KIANA_PROVIDER_SMOKE_TOOLS=1 kiana model smoke --json
```

商业发布证据用 wrapper 脚本，它会把 live catalog、文本 smoke 和工具调用 smoke 的 JSON 证明保存在 `target/live-smoke/provider/`：

```bash
ANTHROPIC_API_KEY=<key> bash scripts/provider-live-smoke.sh --required
```

## 基础对话

Enterprise license readiness is local and offline by default. It reports whether
account, plan, entitlements, support contact, and managed policy inputs are
configured without printing the raw license key:

```bash
export KIANA_LICENSE_KEY="kiana-enterprise-..."
export KIANA_ENTERPRISE_ACCOUNT_ID="acct_..."
export KIANA_LICENSE_PLAN="enterprise"
export KIANA_LICENSE_ENTITLEMENTS="managed-policy,offline"
kiana license status --json
```

```bash
kiana -p "summarize README.md"
```

交互式模式下可以连续对话。当前 runner 已接入 Read、Write、Edit、Glob、Grep、Bash、Agent、Todo、MCP 等工具路径，但 reference 级权限、展示和边界行为仍在继续核对。

## 权限

```bash
kiana permissions status
kiana permissions profile read-only
kiana permissions profile workspace
kiana permissions profile full
kiana permissions profile ask
kiana permissions profile plan
kiana --permission-profile read-only -p "summarize this repo"
```

`read-only`/`plan` 会阻止默认的变更类工具，`ask` 会在可交互终端请求确认，`workspace` 使用默认工作区策略，`full` 对应跳过权限提示的高风险模式。

## Project Trust

Project Trust 决定当前项目能否加载项目级 skills、hooks、MCP、agents 等资源，以及能否执行 Write、Edit、Bash、TaskCreate 等变更类工具。新项目默认状态是 `unknown`：用户级和内置只读能力仍可用，但项目资源和变更类工具会 fail closed。`bypassPermissions`、用户 allow rule 或 managed allow rule 都不能跳过 Unknown/Untrusted 项目的第一层信任门禁。

```bash
# 查看三态状态、项目身份、外部记录和 legacy 诊断
kiana trust status
kiana trust json

# 显式信任或拒绝当前项目
kiana trust trust
kiana trust untrust

# 删除持久决定并回到 unknown
kiana trust reset

# 只输出当前项目的外部记录路径
kiana trust path
```

持久记录使用 `kiana.project-trust.v2`，按最近 Git 根或当前目录计算稳定的 `project_id`，写在项目外的 `$KIANA_HOME/trust/projects/<project_id>.json`。记录同时绑定 `project_id` 和规范化 `project_root`；schema、路径或绑定被篡改时读取会报错，effective trust 回到 `unknown`。运行时可以由受信任的进程内 session 显式提供临时决定，但普通 prompt、模型输出、项目文件和 direct-connect session 请求体不能自行授权。

旧版项目内 `.kiana/trust.json` 只作为迁移诊断显示，始终标记为 `ignored: true` 和 `project_local_trust_is_not_authoritative`，不能授权项目。升级后的迁移步骤是：

1. 在项目根运行 `kiana trust json`，确认 legacy 文件被忽略且当前状态为 `unknown`。
2. 人工审查项目内的 hooks、skills、agents、MCP 配置和可执行脚本。
3. 审查通过后运行 `kiana trust trust`；需要显式拒绝时运行 `kiana trust untrust`。
4. 要撤销持久决定时运行 `kiana trust reset`，不要手工创建或复制项目内 trust 文件。

`/app/trust/status` 与 `kiana trust json` 使用同一状态合同，都会报告 `allows_project_resources`、`project_id`、`project_root`、外部记录的 `path/status/exists/error`，以及 legacy 文件的 ignored 原因。

## Session 管理

```bash
kiana session list
kiana session status
kiana session current
kiana session show current
kiana session reply current --record-only "本地补充一条消息"
kiana session rename current "调试记录"
kiana session tag current smoke
kiana session fork current
kiana session export current --format text session.txt
kiana session compact current --dry-run
kiana session delete <session_id>
```

本地 slash command 和 TUI 使用同一套 session command。`--record-only` 不调用模型，只更新本地 session；需要模型回复时使用：

```bash
kiana session reply <session_id> "继续解释刚才的问题"
```

## WorkflowRun

`WorkflowRun` 是 Project OS 的持久执行单元。当前真实 CLI 入口位于 `tasks workflow`，不是顶层 `workflow` 命令：

```bash
# 查看内置 DAG 模板
kiana tasks workflow template --json

# 创建运行；request 必填
kiana tasks workflow init --json --type feature --profile standard "实现并验证支付回调"

# 列出、查看和恢复运行
kiana tasks workflow list --json
kiana tasks workflow show <run_id>
kiana tasks workflow continue --json <run_id>

# 只允许沿持久化 DAG 的当前节点合法 decision 推进
kiana tasks workflow advance --json \
  --decision clear \
  --evidence problem-definition.md \
  <run_id>

# 到达 completed 终点后，绑定同一 run 的 pass VerificationPacket 完成
kiana tasks workflow complete --json \
  --verification <verification_id> \
  <run_id>
```

初始化会创建 `.kiana/workflows/<run_id>/`，其中 `eventlog.jsonl` 是追加写入的事实源，`state.json` 是可从 EventLog 重建的投影缓存。恢复时会校验事件序列、`last_event_id`、状态投影和 git dirty state；发现冲突时返回 `blocked` 与修复动作，不会把不一致状态伪装成可继续。

EventLog 写入由 `.eventlog.lock` writer lease 串行化。Linux 上确认持锁 PID 已不存在时可回收 stale lease；无法确认进程存活时使用有限 TTL。显式 Evidence ID 的唯一性检查与事件追加在同一 lease 内完成，避免并发执行先检查后写入造成重复事实。

`advance` 从 run 内的 `workflow_dag.json` 解析唯一 edge，不接受客户端自报目标节点。每个推进至少绑定一个 run 目录内的普通文件；绝对路径、`..`、目录、symlink、空文件、标题占位文件和超过 1 MiB 的文件都会拒绝。成功推进会在一个 writer lease 下连续写入 `gate_evaluated -> node_exited -> node_entered`，三个事件共享 `transition_id`、decision 和 evidence SHA-256；EventLog `sync_data` 成功后才把最终节点投影到 `state.json`。

`complete` 只允许 `status=running,current_node=completed` 的 run。它重新读取同一 run 的 VerificationPacket 与 Evidence Ledger，复验 packet schema、workflow/run 归属、check/evidence 一致性、`verification_completed` 事件和 `final_status=pass`，随后写入唯一 `workflow_completed`。相同 verification 重试返回 `idempotent=true`，不同 packet 会被拒绝；不存在 `--force` 绕过。

## Project Board

`project` 命令把现有 `TaskCreate` / `TaskUpdate` 任务文件投影成固定 Kanban 列，不会创建第二套任务数据库，也不会自动执行选中的任务。

```bash
# 查看默认 task list 的 Board
kiana project board
kiana project board --json

# 查看指定 task list
kiana project board --json review

# 选择下一项 Ready task，并解释排序原因
kiana project next
kiana project next --json
kiana project next --json review

# 将商业发布 blocker action plan 导入 Project Board
kiana project import-release-actions --json release-board
kiana project import-release-actions --json --from dist/proofs/local-rc/blockers/commercial-release-blockers.json release-board
```

Board 使用 `metadata.verification_commands` 判断 pending task 是否 Ready。completed task 只有在 `metadata.evidence` 包含可解析的 `verification:<project-relative-path>#<verification-id>`，且对应 VerificationPacket 与同一 WorkflowRun EventLog、Task ID、required checks、状态和计数完整一致时，才允许进入 Done。legacy evidence、伪造 pass packet、失败或缺失 packet、路径穿越和解析到项目外的 symlink 都会投影到 Blocked。`project next --json` 在没有 Ready task 时会返回 `primary_blockers`。

`project import-release-actions` 读取 `kiana.commercial-release-blockers.v1` 里的 `action_plan`，把每个 blocking action 写成 `.kiana/tasks/<task_list_id>/release-*.json` 任务卡。local action 会进入 `spec` 列，external action 会进入 `review` 列并保留 approval required、owner、resolution scope、验收产物、验证命令和 handoff notes；导入不会把外部证据标记为已完成。

常用 Task metadata：

```json
{
  "priority": 10,
  "verification_commands": ["cargo test -p kiana-tasks"],
  "evidence": ["verification:.kiana/workflows/<run_id>/verification/<verification_id>.json#<verification_id>"],
  "blocker_reason": "waiting for approval",
  "unblock_condition": "approval recorded",
  "allowed_paths": ["kiana-tasks/**"]
}
```

## Bounded Swarm 预派发

`tasks swarm plan` 从 Project Board 的 Ready 列生成一个无副作用的并行预派发计划。它按 priority、创建时间和 task ID 稳定排序，检查 policy 与 `allowed_paths`，并为每个安全候选生成 inline WorkPacket preview 和 path lock。Swarm 模式要求 `--max-workers` 为 `2..32`；单任务应走普通 task/project 执行入口：

```bash
# 默认最多规划 2 个 worker
kiana tasks swarm plan
kiana tasks swarm plan --json

# 指定 worker 上限和 task list
kiana tasks swarm plan --json --max-workers 4 review
```

JSON 输出使用 `kiana.swarm-plan.v1`，包含：

- `assignments`：可并行任务、worker type 和只读 `kiana.swarm-workpacket-preview.v1`；它不是 dispatch 阶段的持久 WorkPacket。
- `path_locks`：逻辑 scope、解析 symlink 后的 `resolved_path` 和 write/exclusive 所有权。
- `skipped`：未派发 task 及 typed reason。
- `summary`：Ready、计划派发和跳过数量。
- `next_action`：下一阶段动作。

当前 skip reason：

- `missing_allowed_paths`：Task 没有文件边界。
- `invalid_allowed_path`：绝对路径、`..` 或无法形成安全 scope。
- `path_conflict`：与已选择 task 的文件或父子目录重叠。
- `approval_required`：Task 要求批准，但 `approval_status` 不是 `approved`。
- `restricted_task_type`：release、deploy、secrets 或 policy 任务禁止进入并行预派发。
- `high_risk_task`：high 或 critical 风险任务禁止进入并行预派发。
- `worker_limit`：超过 `--max-workers`。
- `insufficient_parallelism`：最终不足两个可并行 task。

路径规范化按组件消除内部 `.` 与重复分隔符，glob 保守锁定到最后一个完整目录。项目内 symlink 会解析到真实路径参与冲突检测；解析到项目外或 broken symlink 的 scope 会被拒绝。`Cargo.lock`、`package-lock.json`、`pnpm-lock.yaml`、`yarn.lock` 和 `uv.lock` 使用 exclusive lock。`plan` 的 `execution_mode` 固定为 `plan_only`：该命令本身不会启动 worker、创建 worktree、修改 Task 状态或 merge/commit。计划可通过下方 `dispatch` 持久化，再显式进入 worker lifecycle。

## Bounded Swarm Dispatch 持久化

`tasks swarm dispatch` 对指定 WorkflowRun 重新计算最新安全计划，并持久化正式 WorkPacket、dispatch manifest 和唯一 `work_packet_created` 事件：

```bash
kiana tasks swarm dispatch --workflow <run_id> --json
kiana tasks swarm dispatch --workflow <run_id> --max-workers 4 \
  --max-attempts 2 --max-commands 30 \
  --timeout-seconds 1800 --max-output-bytes 10485760
```

输出使用 `kiana.swarm-dispatch-result.v1`，只包含 WorkflowRun 内的相对 artifact 路径。正式 packet 使用 `kiana.swarm-workpacket.v1`，manifest 使用 `kiana.swarm-dispatch-manifest.v1`。相同 WorkflowRun、任务计划、path locks、verification commands 和 budget 会生成相同 `dispatch_id`；重复执行会验证既有 artifact 并复用原事件，不增加 EventLog sequence。

默认 worker budget：`max_attempts=1`、`max_commands=20`、`timeout_seconds=1800`、`max_output_bytes=10485760`。正式 packet 同时记录允许的终止原因：完成、预算耗尽、超时、scope violation、policy denied、approval required、取消和 worker failure。

该入口保持 `execution_mode=persist_only`：`dispatch` 本身不会启动 worker、创建 worktree、修改 Task 状态或集成 diff。下一动作是 `run_swarm_start`。

## Bounded Swarm Worker 生命周期

```bash
kiana tasks swarm start --workflow <run_id> --dispatch <dispatch_id> --json
kiana tasks swarm status --workflow <run_id> --dispatch <dispatch_id> --json
kiana tasks swarm monitor --workflow <run_id> --dispatch <dispatch_id> --json
kiana tasks swarm cancel --workflow <run_id> --dispatch <dispatch_id> --task <task_id> --json
```

`start` 为每个正式 WorkPacket 创建独立隔离目录。干净且有 HEAD 的 Git 项目使用 detached worktree；存在 tracked/untracked 用户改动或不是 Git 项目时使用 snapshot copy，避免 worker 落在旧 HEAD。runner 默认调用当前 Kiana CLI；测试或受控部署可通过 `KIANA_SWARM_WORKER_EXECUTABLE` 指定已存在的 executable，runner fingerprint 会进入不可变 execution manifest。

Linux worker state 使用 `kiana.swarm-worker-state.v2`。`start` 会记录 `kiana.swarm-process-identity.v1`，绑定 PID、PGID 和 `/proc/<pid>/stat` start-time；`command_sha256` 只作为启动命令来源证明，不作为连续性判断。worker 必须拥有独立进程组，PGID 必须等于 PID，否则不会进入可取消状态。启动时会向 runner 暴露 `KIANA_SWARM_ATTEMPT`，用于 worker 识别当前尝试轮次。

`status` 是只读投影。`monitor` 执行一次 reconcile，按 timeout、stdout/stderr 总字节数、telemetry command 数量和 path-lock scope 分类终止原因，并为终态 worker 提交唯一 `kiana.swarm-result-packet.v1`。当 worker 以非零 exit code 结束、终止原因为 `worker_failed`、没有隔离目录文件变更、没有 scope deviation、主项目 baseline 未漂移，且 `attempt < max_attempts` 时，`monitor` 会先把本次输出归档到 `workers/<dispatch>/<task>/attempts/<attempt>/`，再重新启动下一轮 worker；此时不会生成最终 ResultPacket。重复 monitor 会复用原 ResultPacket event，不增加 EventLog sequence。`cancel` 可取消整个 dispatch 或单个 `--task`，不会停止未选中的 peer worker。

自动重试是保守策略：只覆盖干净的 `worker_failed`，不会重试 `cancelled`、`scope_violation`、`budget_exhausted`、`timeout`、`lost`、process identity mismatch、主项目 baseline 漂移或任何已产生文件变更的失败。尝试次数耗尽后才提交最终 `kiana.swarm-result-packet.v1`。

`status` 与 `monitor` 的每个 worker 项都会带 `health` 对象，schema 为 `kiana.swarm-worker-health.v1`。它汇总 `state`、`reason`、`next_action`、进程 identity、attempt/retry 剩余额度、输出/命令/超时预算和 scope/project drift 计数，供 TUI、dashboard、CI 或上层 orchestrator 不解析日志也能判断 worker 是否 healthy、retrying、completed 或 attention_required。

`status` 与 `monitor` 还会在顶层输出 `process_identity_backend`，schema 为 `kiana.swarm-process-identity-backend.v1`。Linux 当前报告 `backend=linux_procfs` 且 `safe_to_start_workers` / `safe_to_monitor_workers` / `safe_to_cancel_workers` 为 true；非 Linux 平台报告 `backend=unsupported_platform`、`supported=false` 和 `unsupported_reason=process_identity_backend_not_implemented`，用于让 CLI、CI 和上层调度器显式识别“不可安全启动/监控/取消 worker”的边界。

`status`、`monitor` 和 `cancel` 不再用裸 PID 判断存活或发送信号；缺失 identity、start-time/PGID/PID 不匹配、非 Linux procfs 后端不可用都会 fail closed。`cancel` 会先验证所有被选中的非终态 worker，只有全部通过预检后才发送 TERM/KILL；如果任何 worker identity 可疑，本次取消不会对任何 worker 产生副作用。当前安全终止证明只覆盖 Linux procfs，本地文档不声称 Windows/macOS 已具备同等进程身份后端。

Worker lifecycle 本身不会把隔离目录 diff 合并回主工作树，不 commit/push/merge/deploy，也不把 exit code 0 当作 acceptance criteria 已通过。ResultPacket 只记录 changed files、commands telemetry、日志路径、scope deviations 和终止事实；终态 worker 可显式进入下方 serial integration gate。

ResultPacket 中的 runner telemetry 现在同时保留兼容字段 `commands_run` 和结构化 `telemetry` 对象，schema 为 `kiana.swarm-worker-telemetry.v1`。该对象记录 telemetry 是否存在、可解析 command 数、原始 telemetry SHA-256、非字符串 command 条目数量和解析 notes；预算判断使用 `command_count`，因此损坏或混入非字符串的 `commands_run` 条目不会被静默忽略。

## Bounded Swarm 串行集成与清理

```bash
# 只读预检：校验 ResultPacket、scope、主树 baseline、patch hash 和路径冲突
kiana tasks swarm integrate plan --workflow <run_id> --dispatch <dispatch_id> --json

# 显式串行应用：逐 worker apply patch 并运行其 verification commands
kiana tasks swarm integrate apply --workflow <run_id> --dispatch <dispatch_id> --json

# 只读查看 plan/state/packet 的最新状态
kiana tasks swarm integrate status --workflow <run_id> --dispatch <dispatch_id> --json

# 仅在 worker 与 integration 已终态且 ResultPacket 已持久化时回收 isolation
kiana tasks swarm cleanup --workflow <run_id> --dispatch <dispatch_id> --json
```

`integrate plan` 生成 `kiana.swarm-integration-plan.v1` 和每个 worker 的 binary patch artifact。只有 `status=completed`、无 `scope_deviations`、changed files 与 WorkPacket path locks 一致、主树对应路径未漂移、worker 间无 path conflict 且 `git apply --check --binary` 通过时，计划才是 `ready`。计划阶段不修改项目源文件。

`integrate apply` 会重新验证主工作树 fingerprint、ResultPacket SHA-256 和 patch SHA-256。plan 后若有外部修改，返回 `kiana.swarm-integration-state.v1` 的 `stale`，不会应用任何 patch。ready plan 会先创建路径级 checkpoint，再按固定 task 顺序应用 patch、运行 WorkPacket 的 verification commands，并写入 integration verification、state、packet 和 Workflow events。任何 apply 或 verification 失败都会恢复本次集成涉及的所有路径；只有恢复后 fingerprint 与 plan baseline 完全一致才标记 `rolled_back`，否则标记 `rollback_failed`。

成功输出 `kiana.swarm-integration-packet.v1`，并明确记录 `automatic_commit=false`、`automatic_push=false`、`automatic_merge=false`、`automatic_deploy=false`。重复 apply 已完成 integration 会复用原 packet。

`cleanup` 同时检查 worker terminal state、ResultPacket、integration terminal state 和严格的 `.kiana/swarm-worktrees/<dispatch>/<task>` 路径。它可删除 snapshot copy 或 detached Git worktree，但保留 dispatch/execution manifests、WorkPackets、ResultPackets、IntegrationPlan、IntegrationPacket、verification 和 EventLog。重复 cleanup 返回 `removed=0`、`reused=true`。

## Local Structured Memory

```bash
# 查看本地记忆状态；--json 输出稳定契约
kiana memory status
kiana memory status --json

# 兼容旧纯文本记忆，同时追加结构化 JSONL record
kiana memory append "important project note"
kiana memory append --kind decision --source workflow "use bounded swarm retry policy"

# 本地关键词检索结构化记忆
kiana memory search "swarm retry"
kiana memory search --json "swarm retry"

# 查看旧 memory.md 或清空旧文件和结构化 store
kiana memory show
kiana memory clear
```

Local Structured Memory v1 保留旧 `memory.md` 兼容入口，同时写入 `$KIANA_HOME/memory/events.jsonl`。每条结构化记录使用 `kiana.memory-record.v1`，包含 `id`、`kind`、`source`、`text`、`redaction_count` 和 `created_at_ms`；`kind` 与 `source` 只允许 ASCII 字母、数字、`_`、`-`、`.`，避免把路径、命令或未清洗输入混进分类字段。

`memory append` 会在写入 legacy 与 structured store 前脱敏明显的 `token=`、`password=`、`secret=`、`api_key=`、`sk-`、`xoxb-`、`ghp_` 等 secret 形态，并把命中的数量记录到 `redaction_count`。`memory status --json` 输出 `kiana.memory-status.v1`，报告 legacy file、structured store、record count、kind counts、redaction count 和最新时间戳。`memory search --json` 输出 `kiana.memory-search.v1`，使用本地确定性关键词匹配，不调用模型、网络、embedding provider 或外部向量库。结构化 store 损坏时命令 fail closed 并指出 JSONL 行号，不会把损坏记忆静默当作空记忆。

当前 v1 只证明本地可审计、可检索、可恢复、带基础 secret 脱敏的结构化记忆底座；它不声称自动会话挖掘、语义 embedding、跨设备同步、团队权限隔离、完整 PII 分类或 stale-memory 自动降权已经完成。

## Evidence Ledger

```bash
# 列出最新或指定 WorkflowRun 的 EvidenceEvent
kiana evidence list
kiana evidence list --json --task task-1
kiana evidence list --json --workflow <run_id>

# 查看单个 EvidenceEvent
kiana evidence show --json --workflow <run_id> <event_id>

# 只读校验 EventLog、VerificationPacket、check evidence 和 Task 引用
kiana evidence verify --json --workflow <run_id>
```

`evidence verify` 不会重新执行测试或构建命令。发现序列损坏、schema 不兼容、缺失 check evidence、packet/Task 路径或 ID 不匹配时，返回 `status: blocked` 和结构化 findings。VerificationPacket 必须由字段完全匹配的 `VerificationCompleted` 事件背书；仅在目录中放置一个孤儿 packet 不构成完成事实。

## 项目进度报告

```bash
# 汇总最新 WorkflowRun、默认 Project Board、Evidence Ledger 和最新验证包
kiana report progress
kiana report progress --json

# 指定 WorkflowRun 和 task list
kiana report progress --json --workflow <run_id> --task-list review
```

JSON 输出使用 `kiana.progress-report.v1`，包含 `workflow`、`board`、`evidence`、`latest_verification`、`blockers` 和 `next_action`。报告只读取持久化事实，不会重新运行测试。completed task 缺少有效 VerificationPacket 时会出现在 Board 的 Blocked 列；EventLog 损坏时仍返回报告，并设置 `evidence.status: blocked`、`evidence_ledger_unreadable` blocker 和 `repair:reconcile-eventlog` 下一步。

## 严格商业审计

```bash
# 审计最新 WorkflowRun 和默认 task list
kiana audit strict
kiana audit strict --json

# 审计指定 WorkflowRun 和 task list
kiana audit strict --json --workflow <run_id> --task-list review
```

`audit strict` 输出 `kiana.strict-audit.v1`，检查 Evidence Ledger、VerificationPacket、completed task 的验证引用、required skipped/unknown checks、Project Board blockers、生产路径中的 TODO/FIXME/stub/fake/placeholder/hardcoded 标记，以及项目提供的 `scripts/commercial-release-blockers-report.sh --json`。商业报告中的 `local_blocking` 与 `external_blocking` 会原样保留。

报告中的 `surfaces` 会按适用性投影 `test`、`release`、`security` 和 `policy`：只有检测到语言项目、发布意图或插件/MCP/hooks/权限能力时才要求对应证据。test surface 会递归识别 workspace member 的 `tests/` 和 Rust `#[test]`，避免只检查仓库根目录导致 monorepo 误报。适用但缺少测试入口、release workflow、安全控制或 trust/policy 契约时，会生成 `surface:<id>` blocking finding；不适用的 surface 明确返回 `not_applicable`，不会凭空制造项目要求。

正常情况下，审计会为每个 finding 写入 `ReviewFinding` 或 `BlockerRecord` EvidenceEvent，并在 `.kiana/workflows/<run_id>/review/` 原子写入 `kiana.review-packet.v1`。VerificationPacket 与 ReviewPacket 均使用安全 artifact ID、临时文件写入和不可覆盖发布；同名正式 packet 已存在时直接报错，不允许覆盖历史证据。`verification/` 或 `review/` 中残留 `.tmp` 文件会被 strict audit 视为阻塞，避免把崩溃中的半成品当作已发布事实。

ReviewPacket 的 `evidence_refs` 必须真实存在、属于同一 `workflow_id/run_id`，且每个 finding 恰好绑定一个唯一 evidence event。活动 blocker 按 `supersedes_event_id` 链解析，只投影尚未被后续事件解除的链尾；strict audit 复用既有 unresolved blocker 的事件 ID，不会每次审计重复制造同一历史阻塞。

如果 EventLog 已损坏，命令仍返回 blocked 报告，但明确设置 `persistence_status: blocked` 和 `packet_path: null`，不会伪造 ReviewPacket 已持久化。

## 离线评测

`eval run` 对本地 `RuntimeEvent` JSONL 夹具做确定性回放评测，不调用模型、不执行 shell、不访问网络，也不会修改被评测文件。它适合验证事件协议、工具生命周期、终止状态、token 计数和固定文本断言：

```bash
# 运行仓库随附的基础套件并输出人类可读结果
kiana eval run --suite docs/eval/fixtures/basic-runtime-suite.json

# 输出固定 schema 的 JSON；任一 case 失败时返回非零退出码
kiana eval run \
  --suite docs/eval/fixtures/basic-runtime-suite.json \
  --baseline docs/eval/fixtures/basic-runtime-baseline.json \
  --json \
  --fail-on-failure
```

suite 使用 `kiana.eval-suite.v1`，可选 baseline 使用 `kiana.eval-baseline.v1`，报告使用 `kiana.eval-report.v1`。baseline 只做本地回归阈值比较：按 case 校验 event/tool/token 上限和最终状态/停止原因，失败会把报告顶层 `status` 置为 `failed`，并在 `--fail-on-failure` 下返回非零错误。每个 case 当前仅支持 `runtime_event_replay`，fixture 必须位于 suite 文件所在目录内，绝对路径、`..`、目录和逃逸到目录外的 symlink 都会被拒绝。单套件最多 256 个 case，单 fixture 最大 16 MiB、最多 100,000 条事件。

当前可断言 `final_status`、`stop_reason`、最小事件数、工具调用/错误数、必需工具名、最终文本片段以及输入/输出 token 上限。报告包含 suite/fixture SHA-256、稳定 finding code 和汇总指标，可用于 CI、发布包生命周期和回归基线。

## EDA 项目审查

`eda review` 把硬件资料审查接入同一套 WorkflowRun、不可变 Artifact 和 Evidence Ledger。当前版本只做本地、只读、可审计的结构审查：除资料、BOM/CPL 与 Gerber 检查外，还可解析 KiCad XML netlist，统计组件、网络、电源网络、接口网络和悬空网络，并执行有限的 component/BOM coverage、power/interface 结构规则。解析器要求唯一且位于 `<export>` 直接子层的 `<components>`/`<nets>`，拒绝 DTD、处理指令、通用实体引用、非法嵌套和重复 node，并在 BOM coverage 前统一 designator 的空白与大小写。它不修改原理图/PCB，不等价于完整 ERC/CAM，不查询实时库存价格，也不会自动下单或替代工程师签字。

```bash
kiana eda review --json \
  --requirements hardware/requirements.md \
  --schematic hardware/main.kicad_sch \
  --bom hardware/bom.csv \
  --gerber hardware/gerber \
  --cpl hardware/cpl.csv \
  --constraints hardware/constraints.md \
  --netlist hardware/main.xml
```

命令会创建 `WorkflowInputKind::Eda` 的 gated WorkflowRun；也可用 `--workflow <run_id>` 在同一 run 内幂等重试。所有输入必须是项目根目录内的相对路径，绝对路径、`..`、目录逃逸和 symlink escape 会在创建 WorkflowRun 前拒绝。

输出位于 `.kiana/workflows/<run_id>/eda/reviews/<review_id>/`：

- `eda_review.json`：固定 `kiana.eda-review.v1` schema；`eda-review-rules.v2` 增加 netlist 输入、结构 checks 和五项 netlist 汇总指标，同时保持历史规则 v1 报告可验证。
- `bom_risk.md`：BOM/CPL designator、package、MPN 结构风险摘要。
- `bringup-plan.md`：上电前置条件、限流上电、供电轨、复位/时钟/调试、接口逐项验证和停止条件。

`schematic`、`bom`、`gerber` 缺失时报告为 `blocked`；BOM/CPL、Gerber 或 netlist/BOM coverage 出现 error 时为 `review_required`；只有本地规则未发现 blocking/error 时才为 `pass`。netlist 缺失不会伪装成已执行 ERC，而是以 `netlist_structure=not_supplied` 明确记录。`hardware_order`、产生成本、生产文件修改和自动替换器件始终记录为后续 approval requirement，本命令不会执行这些动作。

该能力只证明离线解析、固定规则和发布夹具行为，不代表完整 ERC、电气正确性、PCB 连通性、安全认证、实验室 bring-up、生产签字或客户验收；这些仍需要专业 EDA 工具、真实硬件和工程审批提供独立证据。

## Doctor 和 Release

```bash
kiana doctor
kiana release
```

`doctor` 会报告本地配置、bridge、remote-session、live smoke token 等状态。`commercial_security` 会显示当前平台的商业安全模型：Linux 要求 strict `bwrap` Bash sandbox，Windows/macOS 使用权限审批加 shell exec policy 的平台模型。`ANTHROPIC_AUTH_TOKEN=sk-*` 会被标记为 remote live smoke 的 token 误用，因为它是 Anthropic API key，不是 Claude/remote bearer token。

`release` 会列出发布前 gate：

```bash
cargo fmt --all --check
cargo test --workspace --no-fail-fast
cargo build --release -p kiana-entrypoints --bin kiana
./target/release/kiana --version
./target/release/kiana doctor
bash scripts/release-smoke.sh
```

商业发布缺口可以直接用 release 子命令查看：

```bash
kiana release blockers --json
kiana release evidence --json
kiana release workflow-proof --json --run-id <run_id>
kiana release workflow-proof --json --latest-completed
```

`release blockers --json` 输出 `kiana.commercial-release-blockers.v1`，其中 `action_plan` 是机器可读的分派计划：按 local/external 和 `resolution_scope` 拆出每个 blocker 的 owner、required action、验收产物、验证命令、环境变量和 handoff notes。`release workflow-proof` 只负责把真实已完成 WorkflowRun 的 integrity proof 放到商业 blocker 默认路径，并输出 `KIANA_RELEASE_WORKFLOW_RUN_ID` / `KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE` 绑定；`--latest-completed` 会只读选择 `.kiana/workflows` 中最新的 completed run，避免手工拷贝 run id。它不会伪造 clean tree、签名、远端、渠道、live service 或客户验收证据。

如果要跑 optional remote live smoke，需要设置：

```bash
export KIANA_REMOTE_ACCESS_TOKEN="<remote bearer token>"
# 或
export CLAUDE_ACCESS_TOKEN="<remote bearer token>"
```

## Remote Session

本地入口已经存在，但真实 live smoke 依赖有效的 remote session、org uuid 和 remote bearer token：

```bash
kiana remote-session status
KIANA_REMOTE_ACCESS_TOKEN=<token> kiana remote-session listen \
  --session-id <id> --org-uuid <uuid> --permission-mode deny
kiana remote-session code-session smoke --json
```

商业发布证据用 wrapper 脚本，它会把 CCR v2 code-session smoke JSON 证明保存在 `target/live-smoke/remote/`：

```bash
KIANA_REMOTE_ACCESS_TOKEN=<token> bash scripts/remote-live-smoke.sh --required
```

没有真实 token 时，优先使用 `kiana doctor` / `kiana release` 看诊断，不要把 `ANTHROPIC_AUTH_TOKEN=sk-*` 当作 remote token。

## TUI

```bash
kiana tui
```

产品 TUI 由 `kiana-entrypoints` 驱动 `kiana-screens`，需要交互式终端：

- 本地 slash command、doctor、session resume、record-only reply、compact、fork
- `/history` 打开 prompt 历史选择器
- 历史保存在 `KIANA_HOME/tui-history.jsonl`，去重、最新优先；选中后恢复为输入草稿
- 历史界面内 `/` 搜索，`Ctrl+R` 打开搜索历史

独立 crate `kiana-tui` 是组件库和实验二进制，不是 `kiana tui` 的默认实现。完整边界见 [docs/tui.md](docs/tui.md)。

## Reference Repair Agent

项目内置了一个只读扫描 reference、只修补 Kiana 主仓的专用 agent：

```bash
kiana agents --json
kiana --agent reference-repairer -p \
  "对比 reference/codex、reference/cline 和当前 Kiana 的 permission lifecycle，证明一个真实缺口并完成最小修补"
```

定义文件：

```text
.kiana/agents/reference-repairer.md
```

默认约束：

- `reference/**` 永远只读。
- agent 在隔离 worktree 或 snapshot 中运行。
- 可以并行派发只读扫描 agent，但只有主 agent 可以写入和集成。
- 先证明 Kiana 的真实差距，再生成有界 WorkPacket。
- 一次只修补一个可验证切片。
- 没有测试、命令或 smoke 证据时不能标记完成。
- 项目 agent 受 project trust 控制；项目不可信时不会被加载。

如果只想扫描、不希望改文件，在 prompt 中明确要求：

```bash
kiana --agent reference-repairer -p \
  "只读扫描 reference/GitNexus 和 reference/graphify，对比 Kiana repo map，不修改任何文件"
```

## 常见问题

### `kiana` 在脚本里卡住

无参数 `kiana` 会进入交互式 REPL。脚本、CI、管道里请用：

```bash
kiana -p "your prompt"
```

### 找不到配置

```bash
kiana config init
kiana login "sk-ant-xxx"
kiana config status
```

### remote live smoke 提示 token 无效

如果看到 `ANTHROPIC_AUTH_TOKEN is sk-* API key`，请设置 `KIANA_REMOTE_ACCESS_TOKEN` 或 `CLAUDE_ACCESS_TOKEN`。`ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN=sk-*` 只适合 Anthropic API，不适合 remote live smoke。

## 当前边界

- 可用：Rust workspace 编译、配置、REPL/print、基础工具 runner、本地 session、doctor、release smoke、部分 remote/bridge 诊断。
- 未完成：reference 级 TUI、完整 Skills/Plugins 体验、所有 remote live 场景、商业化安装发布和更细协议 parity。
