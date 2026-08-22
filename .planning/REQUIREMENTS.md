# Requirements

当前执行集只有 **v0.2**。北星是 Company OS（见 `COMPANY.md`），但 v0.3+ 目录未打开。
v0.2 必须把工人标成 `role=builder`，即使还只有一个角色。

## v0.2 — Current

### PATH — Golden path

- **PATH-01**: `kiana run` 在受信仓库中走 `DaemonHost` → `KianaHarness` 完成一回合
- **PATH-02**: print / `-p` 同一 spine，输出含 `harness: kiana-harness`
- **PATH-03**: 模型可见工具只有 `shell` 与 `apply_patch`；由 daemon broker，不经 `kiana-tools` 注册表
- **PATH-04**: 真 provider 或「录制自真 provider」的回放；fake-script 只算路由回归

### TRUST — Fail-closed

- **TRUST-01**: 未信任项目任何副作用 → deny（`project_untrusted`）
- **TRUST-02**: 默认 sandbox `read-only`；写盘必须显式 `workspace-write` 且项目已 trust
- **TRUST-03**: 无模型 / 空 prompt / provider 错误对用户可见，禁止假成功

### SESS — Continue / cancel

- **SESS-01**: 同一 harness session 可 continue
- **SESS-02**: cancel 停止进行中的 capability
- **SESS-03**: 找不到 session 时失败可见，不静默开新的假装是旧的

### EVD — Receipts

- **EVD-01**: 工具调用与文件变更写入磁盘 eventlog
- **EVD-02**: 用户能列出本次 run 改过的文件
- **EVD-03**: 进程重启后收据仍在；第二次 run 不覆盖第一次

### SURF — One spine

- **SURF-01**: TUI 走同一 `DaemonHost`，或书面 park（v0.2 Phase 4）

### ROLE — Worker is a role (v0.2 最小)

- **ROLE-01**: 每次 harness run 携带 `role_id`（v0.2 固定 `builder`）并写入收据
- **ROLE-02**: 角色提示词与工具白名单来自 RoleSpec，而不是 `runner.rs` 上帝 prompt
- **ROLE-03**: 收据可带 `department_id`（v0.2 固定 `executing`）；类型先落地，五部门行为不在本期

## Later catalog（冻结，不进入 v0.2 计划）

### v0.3 WB — Workbench + company kernel

- **WB-01**: 黄金路径 eval（fixture 仓 + 文件出现 + 收据）
- **WB-02**: `install.sh` / release smoke 验证 demo 命令
- **WB-03**: TUI 迁到 harness 或保持 park
- **ORCH-01**: 确定性编排器按 work packet 派独立上下文的 Builder
- **ORCH-02**: PM / Architect 只能写计划工件，不能 `apply_patch` src
- **ORCH-03**: 跨部门禁止自由群聊总线（不用 TeamCreate/SendMessage）；交接 = packet + eventlog
- **DEPT-01**: 至少存在 `planning` 与 `executing` 两个 DepartmentSpec
- **SYMP-01**: 规划部可开一场有界 Symposium（PM+Architect），硬顶轮次，产出 DecisionRecord + 一个 WorkPacket
- **SYMP-02**: 每个发言者私有 session；共享黑板而非融合 transcript
- **SYMP-03**: Builder 默认不列席规划会；anti-meeting check 通过则可跳过开会

### v0.4 CODE — Coding pack audit

- **CODE-01**: Claude Code 公开核心行为矩阵（每项 owner/测试/证据/许可证）
- **CODE-02**: MCP client 经 daemon，不经 `runner.rs`
- **CODE-03**: skills/hooks 接到 harness（可作为角色包）
- **CODE-04**: 多供应商能力差异显式降级
- **REV-01**: Reviewer 与 Builder 不得同一 session_id（监控部最小编制）

### v0.5 LONG — 五部门 + 六层 RAG

- **LONG-01**: append-only 磁盘 eventlog 可重放
- **LONG-02**: compact / pause / resume 用户可感知
- **LONG-03**: 产品拥有 prompt、context、control flow
- **LONG-04**: 五个部门都有磁盘工件（charter/plan/packet/gate/receipt）
- **DEPT-02**: Initiating / Planning / Executing / Monitoring / Closing 均为控制面对象，可并行存在
- **SYMP-04**: 各部门可开会；跨部门联席必须点名、限时、有议程
- **MEM-01**: 六层分库：company / department / role / project / user / instance-scratch
- **MEM-02**: `memory.search`/`write` 带 `role_id` + `department_id`，按 knowledge_grants 过滤
- **MEM-03**: 检索命中写入收据；无来源不得当已验证结论
- **MEM-04**: instance scratch 默认不晋升；写入更高层必须显式

### v0.6 SURF2 — Extra surfaces

- **SURF2-01**: SDK/print 100% harness
- **SURF2-02**: 下一入口（IDE 优先于 Desktop/Web）打同一 daemon
- **SURF2-03**: 禁止为新入口复制 runner

### v1.0 REL — Personal complete product

- **REL-01**: 安装、升级、回滚、恢复
- **REL-02**: 文档与支持
- **REL-03**: Coding pack 核心路径（不是工具数 100%）
- **REL-04**: Daily/Research 仅在同一核已有黄金路径时才算

### v1.x ENT — Team / enterprise

- **ENT-01**: 租户、RBAC、审计隔离
- **ENT-02**: 远程执行 / 官方云
- **ENT-03**: 个人开源版不掺企业契约
