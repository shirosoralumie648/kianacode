# Volume 17: Enterprise Offline 与 Cloud Workspace

## 1. 目标

P0 优先 Personal CLI，但架构必须保留企业离线和云端 workspace 的演进路径。否则后续会重构核心协议。

## 2. 三形态共享 Core

共享：

- WorkflowRun。
- EventLog。
- State。
- Packet。
- Evidence Ledger。
- PolicyDecision。
- Plugin Receipt。
- MCP provenance。

不同：

- 执行位置。
- UI。
- worker 管理。
- policy 默认值。
- storage backend。
- sync 模型。

## 3. Personal CLI

特点：

- local-first。
- file-based artifacts。
- local git。
- local commands。
- simple worker。

默认：

- 网络 ask。
- plugin ask。
- push/merge/deploy ask。

## 4. Enterprise Offline

特点：

- no mandatory cloud。
- audited plugin install。
- offline license。
- exportable reports。
- strict policy。
- local MCP。

需求：

- airgap install。
- offline docs。
- signed bundles。
- admin policy。
- redaction。
- audit archive。

## 5. Cloud Workspace

特点：

- remote session。
- worker pool。
- web console。
- artifact sync。
- team visibility。

风险：

- source upload。
- secret exposure。
- worker isolation。
- network policy。

必须有：

- explicit sync consent。
- workspace boundary。
- encrypted storage。
- audit trail。

## 6. Remote Worker

Remote worker 输入仍然是 WorkPacket。

它不能获得：

- full repo unless allowed。
- secrets。
- policy files。
- unrelated workflows。

输出仍然是 ResultPacket。

## 7. Sync Model

同步对象：

- workflow metadata。
- state。
- eventlog。
- packets。
- reports。
- artifacts。

不同步：

- secrets。
- forbidden files。
- unapproved code。

## 8. Conflict Model

Cloud sync 冲突：

- same event id。
- divergent state。
- duplicate task。
- worker stale base。

解决：

- eventlog order。
- vector/sequence metadata。
- manual reconciliation。
- block unsafe merge。

## 9. Enterprise Policy

企业策略：

- allowed commands。
- denied commands。
- plugin allowlist。
- MCP allowlist。
- network allowlist。
- approval roles。
- retention policy。
- redaction policy。

## 10. Audit Export

导出：

- workflow summary。
- eventlog。
- evidence。
- policy decisions。
- approvals。
- plugin receipts。
- MCP status。
- release proof。

## 11. License/Entitlement

Entitlement 不应影响本地数据访问。

可限制：

- cloud worker。
- enterprise features。
- team dashboard。
- hosted sync。

不能限制：

- 用户读取自己的 workflows。
- 导出自己的 evidence。

## 12. Deployment Modes

| 模式 | 描述 |
| --- | --- |
| local | 本地 CLI |
| local-server | 本地 app-server |
| offline-enterprise | 离线企业包 |
| private-cloud | 企业私有云 |
| hosted-cloud | Kiana 托管 |

## 13. P2 前置约束

在做云端前必须完成：

- stable event schema。
- stable packet schema。
- policy model。
- evidence ledger。
- worker boundary。

否则云端只会放大混乱。

## 14. 验收

Enterprise Offline 验收：

- 不联网可启动。
- plugin policy 可用。
- audit export 可生成。
- license 状态可解释。

Cloud Workspace 验收：

- remote worker 只拿 WorkPacket。
- sync 有 audit。
- source upload 有 approval。
- worker conflict 可 block。
