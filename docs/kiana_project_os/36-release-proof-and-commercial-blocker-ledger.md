# Volume 36: Release Proof 与商用阻塞账本

## 1. 目标

Kiana 如果要走向长期自用、公开发布或商用交付，必须有一套 release proof 与 blocker ledger。它不能只靠“测试过了”“感觉能用”“文档写了”来判断。

本卷定义：

- 什么是 release proof。
- 什么是 commercial blocker。
- blocker 如何分类、归属、关闭。
- release readiness 如何从证据生成。
- 文档设计阶段如何为后续实现留出验收面。

## 2. Release Proof

Release Proof 是一组可复查证据。

包括：

- build proof。
- test proof。
- smoke proof。
- package proof。
- docs proof。
- security proof。
- policy proof。
- migration proof。
- user workflow proof。

每个 proof 都要有来源、时间、命令或文件、结果。

## 3. Proof 对象

```json
{
  "schema_version": "kiana.release_proof.v1",
  "proof_id": "proof_schema_contract_smoke",
  "release_id": "rel_0_1_candidate",
  "proof_type": "smoke",
  "source": "command",
  "command": "scripts/schema-contract-smoke.sh",
  "status": "pass",
  "captured_at": "2026-07-09T21:00:00+08:00",
  "artifacts": [
    ".kiana/release/proofs/schema-contract-smoke.log"
  ],
  "notes": "P0 schema fixtures validated"
}
```

设计阶段可以定义 proof 类型，但不能伪造 pass。

## 4. Commercial Blocker

Commercial Blocker 是阻止项目达到“可长期自用/可发布/可交付”的问题。

不是所有 bug 都是 commercial blocker。

判定条件：

- 阻止核心用户流程。
- 阻止安装/启动/配置。
- 阻止数据安全。
- 阻止恢复/继续。
- 阻止交付验证。
- 阻止合规或权限边界。
- 阻止用户理解当前状态。

## 5. Blocker 对象

```json
{
  "schema_version": "kiana.commercial_blocker.v1",
  "blocker_id": "cb_resume_state_missing",
  "title": "Resume state lacks stale context detection",
  "severity": "high",
  "class": "recovery",
  "owner": "runtime",
  "owner_status": "local_blocking",
  "affected_workflows": ["/project", "/task"],
  "evidence": ["ev_resume_audit_001"],
  "acceptance_artifacts": [
    "stale context fixture",
    "resume report output"
  ],
  "verification_commands": [
    "run resume-state smoke"
  ],
  "status": "open"
}
```

## 6. Blocker 分类

分类：

| class | 含义 |
| --- | --- |
| install | 安装/启动/配置 |
| runtime | 核心执行 |
| recovery | crash/resume/continue |
| context | memory/repo/context |
| policy | 权限/approval/trust |
| workflow | task/project/swarm |
| verification | test/gate/evidence |
| security | secrets/network/plugin |
| docs | 用户无法理解或操作 |
| packaging | 发布包/平台兼容 |
| external | 外部依赖或账号 |
| eda | 硬件流程风险 |

分类用于 owner 和 release view。

## 7. Owner Status

owner_status：

- local_blocking：本仓库能解决。
- external_blocking：依赖外部系统/账号/网络/硬件。
- user_blocking：需要用户决策。
- upstream_blocking：依赖参考或上游变更。
- deferred：明确不是当前 release。

不能把 local_blocking 伪装成 deferred。

## 8. Severity

Severity：

- critical：核心流程不可用或安全风险严重。
- high：核心流程不完整，不能发布。
- medium：影响可信度或部分用户流程。
- low：不阻止发布，但应记录。

critical/high 必须在 release 前关闭或明确降级并记录风险。

## 9. Release Readiness

Readiness 不直接用百分比。

输出：

```json
{
  "schema_version": "kiana.release_readiness.v1",
  "release_id": "rel_0_1_candidate",
  "status": "not_ready",
  "critical_blockers": 0,
  "high_blockers": 3,
  "local_blocking": 2,
  "external_blocking": 1,
  "proofs": {
    "build": "pass",
    "test": "partial",
    "smoke": "missing",
    "security": "missing"
  }
}
```

人类摘要可以说“大致 60/100”，但必须说明分解依据。

## 10. Blocker 生命周期

状态：

- open。
- triaged。
- in_progress。
- waiting_external。
- waiting_user。
- fixed_pending_verification。
- closed。
- deferred。
- rejected。

关闭必须有 evidence。

## 11. Blocker 开启规则

开启 blocker 的触发：

- gate failed。
- smoke failed。
- user workflow 不通。
- recovery 不可信。
- policy 边界缺失。
- security finding。
- docs 与实际不符。
- release proof missing。

缺 proof 也可以是 blocker。

## 12. Blocker 关闭规则

关闭要求：

- root cause 已处理。
- acceptance artifacts 存在。
- verification commands 通过或有合理替代证据。
- release view 更新。
- no regression finding。

不能因为“计划里会做”而关闭。

## 13. Deferred 规则

Deferred 必须满足：

- 不阻止当前 release 目标。
- 用户或 release policy 接受。
- 有后续 milestone。
- 有风险说明。

Deferred 不是垃圾桶。

## 14. Proof 与 Evidence 的关系

Evidence 是通用证据。

Release Proof 是 release gate 认可的证据集合。

一个 EvidenceEvent 可以进入多个 proof：

- test output -> test proof。
- package smoke -> package proof。
- security scan -> security proof。

Release Proof 必须引用 Evidence，而不是复制文本。

## 15. 商用化 Gate

商用化 gate：

- install gate。
- first-run gate。
- project workflow gate。
- resume gate。
- permission gate。
- plugin gate。
- verification gate。
- report gate。
- packaging gate。
- docs gate。

每个 gate 必须有 pass/fail/block。

## 16. P0 Release Boundary

P0 release 不要求：

- 云端团队协作。
- 完整插件市场。
- 自动 EDA 下单。
- 企业 SSO。
- 高级成本统计。
- browser companion。

P0 release 必须要求：

- /project 可持续推进。
- /task 可闭环验证。
- evidence-first done。
- resume/continue 不丢目标。
- policy/approval 高风险可控。
- report 能说明当前进展和问题。

## 17. 商用阻塞报告

报告结构：

```text
Release: rel_0_1_candidate
Status: not_ready
Top blockers:
1. Resume state lacks stale context detection
2. Plugin trust policy missing install approval
3. Smoke proof missing for package lifecycle
Next local slice:
- Close cb_resume_state_missing
External blockers:
- API quota/account verification
```

报告要短，但可下钻。

## 18. 与 Dashboard 的关系

Dashboard 展示 release readiness。

Blocker Ledger 保存事实。

Release Proof 保存证据集合。

Report 生成面向人类的解释。

三者不能互相替代。

## 19. 与参考仓库的关系

参考吸收：

- gstack：关卡式审查、ship/canary/retro 思想。
- Archon：PR/validate/review artifact。
- ECC：跨 harness 状态、policy、MCP 可信边界。
- everything-claude-code：命令、rules、agents、验证生态。
- strix：security finding 必须可验证。

不照搬：

- 不把攻击自动化作为 P0。
- 不把 PR 自动化当成唯一交付面。
- 不把 dashboard 做成只有展示没有 proof 的外壳。

## 20. 验收

Release Proof 与 Commercial Blocker Ledger 设计完成标准：

- 定义 proof/blocker/readiness schema。
- 定义 blocker 分类、severity、owner_status。
- 定义 blocker 生命周期。
- 定义关闭证据要求。
- 定义 P0 release boundary。
- 定义商用化 gate。
- 明确 Dashboard/Proof/Ledger/Report 的边界。
