# Commercial Release Workflow Proof Binding Design

## 目标

阻止任意空 WorkflowRun、错误 run 或旧工作树生成的 `workflow.recovery-integrity` proof 被商业发布门禁接受。

## 当前问题

- `workflow integrity verify --json` 只输出 `workflow_id`，没有输出被验证的 `run_id`。
- commercial blocker 只验证恢复完整性计数和 key ID，不知道 release owner 实际选择了哪个 run。
- proof 没有绑定 Git HEAD、staged diff、worktree diff 和 porcelain status；代码变化后旧 proof 仍可能被复用。
- 当前唯一仓库级 WorkflowRun 仍是 `unsigned_legacy` 且停在 `capture`，不能作为 release proof。

## 决策

### 1. 新增嵌套 release binding

`workflow integrity verify --json <run_id>` 增加：

```json
{
  "run_id": "run-...",
  "workflow_id": "wf-...",
  "release_binding": {
    "schema": "kiana.workflow-release-binding.v1",
    "run_id": "run-...",
    "workflow_id": "wf-...",
    "workflow": {
      "status": "completed",
      "current_node": "learn",
      "last_event_seq": 42,
      "last_event_kind": "workflow_completed",
      "state_sha256": "64-hex",
      "eventlog_sha256": "64-hex"
    },
    "git": {
      "repository": true,
      "head": "40-hex",
      "index_diff_sha256": "64-hex",
      "worktree_diff_sha256": "64-hex",
      "status_sha256": "64-hex"
    }
  }
}
```

Workflow binding 从同一 run 的 `state.json` 与已验证 EventLog 生成，要求 state 的 `last_event_seq` 与 EventLog 最后一条事件序号一致。Git 哈希继续复用 Bounded Swarm 已验证的 `git_state_snapshot` 算法和相同排除项：`.kiana`、`target`、`node_modules`、`.venv`、`dist`、`build`。

### 2. 显式选择 release run

commercial blocker 新增必需环境变量：

```text
KIANA_RELEASE_WORKFLOW_RUN_ID
```

proof 顶层 `run_id`、binding `run_id` 必须同时等于该值；workflow ID 必须在顶层和 binding 中一致。

所选 run 必须满足：

- `state.status == completed`；
- EventLog 最后一条认证事件为 `workflow_completed`；
- `state.last_event_seq` 等于最后事件序号；
- 当前 `state.json` 与 `eventlog.jsonl` 的 SHA-256 等于 proof 中的 lifecycle binding。

### 3. 独立重算 Git 状态

商业脚本不能信任 proof 自报。它必须用相同 Git 命令重新计算当前 HEAD 和三个 SHA-256，并逐字段比较。

因此：

- proof 生成后修改 tracked/untracked 文件会使 proof 失效；
- commit 后 HEAD 改变会使旧 proof 失效；
- 写入 `dist/proofs/**` 不会使 proof 自己失效；
- `source.clean-tracked-tree` 仍是独立 blocker，不由本契约替代。

### 4. 发布操作顺序

发布人员必须显式选定 run，不能依赖“最新 run”推断：

```bash
export KIANA_RELEASE_WORKFLOW_RUN_ID=<run_id>
mkdir -p dist/proofs/workflow
kiana tasks workflow integrity verify --json "$KIANA_RELEASE_WORKFLOW_RUN_ID" \
  > dist/proofs/workflow/recovery-integrity.json
bash scripts/commercial-release-blockers-report.sh --json
```

proof 生成后，如果发生 commit、暂存区变化、工作区变化、未跟踪文件集合变化，或所选 WorkflowRun 的 state/EventLog 变化，旧 proof 必须视为 stale 并重新生成。`.kiana/**` 被 Git snapshot 排除，但 `state.json` 与 `eventlog.jsonl` 由 lifecycle binding 单独哈希，因此既不会制造 Git 自失效循环，也不能在 proof 生成后悄悄推进或重写 Workflow。`target/**`、`node_modules/**`、`.venv/**`、`dist/**` 和 `build/**` 同样被 Git snapshot 排除。

### 5. 一致性与失败边界

| 条件 | 结果 | 原因 |
|---|---|---|
| 未设置 `KIANA_RELEASE_WORKFLOW_RUN_ID` | blocking | 不允许隐式选择 release run |
| 顶层、binding、环境变量 run ID 任一不一致 | blocking | proof 所属执行身份不可信 |
| 顶层和 binding 的 workflow ID 不一致 | blocking | run/workflow 关系被替换或拼接 |
| Workflow state/EventLog 缺失、序号不一致或哈希变化 | blocking | proof 不再代表当前执行记录 |
| `status != completed` | blocking | 非终态 run 不能作为发布执行证据 |
| 最后一条事件不是 `workflow_completed` | blocking | 不能只修改 state 字符串伪造完成 |
| Git HEAD 或任一 diff/status 哈希变化 | blocking | proof 不再代表当前待发布源码状态 |
| `event_count == 0` | blocking | 空 WorkflowRun 不能充当发布执行证据 |
| `verified_event_count != event_count` | blocking | EventLog 没有被完整验证 |
| recovery integrity 计数、key ID 或状态不满足 | blocking | 恢复链仍存在 legacy、缺口或不一致 |

Schema 位于 `docs/schemas/kiana-workflow-release-binding.v1.schema.json`，随其他 `docs/schemas/*.json` 一起进入 schema contract 和 package lifecycle 检查。

## 不做

- 不把 `capture` 状态硬改为 completed。
- 不自动选择“最新 run”。
- 不自动 commit、tag、push。
- 不生成或伪造 release proof。
- 不把 HMAC 升级冒充 Ed25519、KMS、TPM 或外部签名。

## 验收

- CLI JSON 输出包含 run ID、Workflow lifecycle binding 和 Git binding。
- human 输出包含 release Workflow 状态、节点、最后事件和 Git HEAD。
- 缺少 `KIANA_RELEASE_WORKFLOW_RUN_ID` 时 blocker 必须阻断。
- run ID 不一致、workflow ID 不一致、Workflow artifact hash 不一致、非 completed、最后事件不正确、HEAD 或任一 Git hash 不一致时必须阻断。
- 只有正确绑定当前 completed WorkflowRun 与当前仓库状态的 fixture 才能 satisfied。
- 现有 recovery integrity、swarm、workspace gates 保持通过。
