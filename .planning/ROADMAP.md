# Roadmap

完整产品阶梯见 `DESIGN.md`。公司编排见 `COMPANY.md`。git 证据见 `PROCESS.md`。逐期剧本见 `PHASES.md`。
历史 GSD Phase 1–24 作废。下面「当前里程碑」才是可执行阶段。

## 版本阶梯（后版本未打开）

- [ ] **v0.2 Runnable Local Agent** ← 当前
- [ ] **v0.3 Trusted Workbench + planning/executing + one symposium**
- [ ] **v0.4 Coding pack baseline**
- [ ] **v0.5 Five departments + six-layer RAG**
- [ ] **v0.6 Extra surfaces, same core**
- [ ] **v1.0 Personal complete product**
- [ ] **v1.x Team / enterprise**

## 当前里程碑：v0.2

- [ ] **Phase 1: CLI golden path** — `kiana run` / print 经 `DaemonHost` 完成受信真 provider 一回合
- [ ] **Phase 2: Session continue / cancel / visible failure**
- [ ] **Phase 3: Durable receipts**
- [ ] **Phase 4: TUI on harness, or park**

### Phase 1: CLI golden path

**Goal:** 受信本地仓库能用受限 `shell` / `apply_patch` 跑完一回合。
**Requirements:** PATH-01, PATH-02, PATH-03, PATH-04, TRUST-01, TRUST-02
**Success Criteria:**

1. `kiana run` 对受信 fixture 仓创建或 patch 文件，走 live 或录制自真 provider 的路径，不只 fake-script。
2. 未信任 / 无模型 / 空 prompt fail-closed。
3. Print 模式报告 `harness: kiana-harness`，不 dispatch `kiana-tools`。

**Plans:** 无 — 先 discuss，再只计划这一期。

### Phase 2: Session continue / cancel / visible failure

**Depends on:** Phase 1
**Requirements:** SESS-01, SESS-02, SESS-03, TRUST-03

### Phase 3: Durable receipts

**Depends on:** Phase 2
**Requirements:** EVD-01, EVD-02, EVD-03

### Phase 4: TUI on harness, or park

**Depends on:** Phase 1（可与 Phase 3 并行决策）
**Requirements:** SURF-01

## 后版本（不要现在 plan/execute）

| 版本 | 打开条件 | 需求前缀 |
|---|---|---|
| v0.3 | v0.2 成功标准全绿 | WB |
| v0.4 | v0.3 绿；公开行为矩阵草稿签字 | CODE |
| v0.5 | v0.4 核心路径可用 | LONG / DEPT / SYMP / MEM |
| v0.6 | v0.5 resume 真能用 | SURF2 |
| v1.0 | v0.6 至少 SDK 同核；安装升级过关 | REL |
| v1.x | v1.0 个人产品已发布 | ENT |
