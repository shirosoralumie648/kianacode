# Volume 19: 测试、评估与 Smoke 策略

## 1. 目标

Kiana 的测试策略必须覆盖软件逻辑，而不只是单元函数。因为 Kiana 的价值在于 workflow、状态恢复、证据和 policy。

## 2. 测试层级

| 层级 | 目标 |
| --- | --- |
| unit | 数据对象和纯逻辑 |
| schema | JSON/YAML contract |
| command | CLI command behavior |
| workflow | end-to-end WorkflowRun |
| recovery | crash/resume/fork |
| policy | allow/deny/ask |
| evidence | Done/Blocked proof |
| integration | multiple modules |
| smoke | release confidence |

## 3. Fixture Repo

需要 fixture：

- clean repo。
- dirty repo。
- repo with failing test。
- repo with fake/stub。
- repo with missing tests。
- repo with plugin。
- repo with MCP。
- EDA sample project。

## 4. Router Tests

测试：

- L0 explanation。
- L1 command。
- L2 bugfix。
- L3 project。
- L4 swarm。
- L5 high-risk。
- EDA domain。
- downgrade。
- conflict priority。

验收：

- RouteDecision has reasons。
- safety wins。

## 5. Workflow Tests

测试：

- create workflow。
- replay eventlog。
- rebuild state。
- plan confirmation。
- task status transition。
- block/unblock。

## 6. Recovery Tests

测试：

- crash after event before state。
- crash after state before event。
- dirty user file。
- stale memory。
- fork workflow。
- interrupted worker。

## 7. Policy Tests

测试：

- shell allow。
- shell ask。
- shell deny。
- file allowed。
- file forbidden。
- network ask。
- plugin blocked。
- MCP hidden。
- approval exact operation。

## 8. Evidence Tests

测试：

- command pass writes evidence。
- command fail writes evidence。
- Done requires evidence。
- Blocked requires reason。
- skipped check requires reason。
- report cites evidence。

## 9. Swarm Tests

测试：

- non-overlap dispatch。
- overlap blocked。
- worker scope violation。
- integration after results。
- unified verification。
- conflict triage。

## 10. Memory Tests

测试：

- source/confidence。
- stale detection。
- live evidence priority。
- memory proposal。
- reject low confidence。

## 11. Repo Intelligence Tests

测试：

- repo map ranks relevant files。
- impact finds tests。
- graph confidence affects ranking。
- missing graph degrades gracefully。

## 12. EDA Tests

测试：

- BOM missing MPN。
- BOM/CPL mismatch。
- Gerber missing drill。
- bring-up plan generated。
- hardware order requires approval。

## 13. Commercial Smoke

Smoke：

- install。
- run first project。
- create workflow。
- validate。
- report。
- plugin status。
- trust status。
- package lifecycle。

## 14. Evaluation

评估维度：

- task completion。
- evidence quality。
- recovery correctness。
- false Done rate。
- policy bypass rate。
- context freshness。
- worker conflict rate。
- report usefulness。

## 15. Regression Matrix

每个 P0 milestone 需要：

- happy path。
- failure path。
- resume path。
- policy path。
- report path。

## 16. Definition of Verified

Verified 需要：

- command run。
- result captured。
- expected checked。
- evidence written。
- failure mode considered。

不能：

- “看起来应该可以”。
- “文档写了”。
- “模型说完成了”。
