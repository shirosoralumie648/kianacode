# Goal — 走完 docs/roadmap.md 的全量 Step 队列

## 目标

按 `docs/roadmap.md` §1.1 的 749 张 Step 总队列顺序，把整个项目做到队列收口。

用户口径（2026-09-27）：

1. **本地不跑任何测试**。Cargo test/build/check/clippy/smoke 全部交给 GitHub Actions。
2. **每做完一个 Step 就提交 + 推送一次**，不等 CI 结果。
3. 文件里旧版限制性指示与当前指示冲突时，以当前指示为准。
4. 能并行的 Step 并行做。

## 队列现状（2026-09-27 盘点）

登记 611 行：✅ 321 / 🔄 241 / ⏳ 49。未收口按前缀：

| 前缀 | 未收口 | 🔄 | ⏳ |
|---|---|---|---|
| DEP | 42 | 24 | 18 |
| CO  | 40 | 39 | 1 |
| UI  | 38 | 38 | 0 |
| INT | 28 | 28 | 0 |
| EQ  | 25 | 25 | 0 |
| BQ  | 21 | 14 | 7 |
| SC  | 16 | 4 | 12 |
| NM  | 16 | 16 | 0 |
| SW  | 14 | 14 | 0 |
| PD  | 11 | 4 | 7 |
| AUT | 11 | 11 | 0 |
| ER  | 9 | 9 | 0 |
| CAP | 5 | 1 | 4 |
| CM/EXT/CP | 13 | 13 | 0 |
| CI  | 1 | 1 | 0 |

## 阶段

### 阶段 0：解 CI（阻塞全部后续）

- [x] 盘点工作树，发现三件在制品：CI 合并（664 个 workflow → `ci.yml`）、DEP-18 半成品、一批 rustfmt 修复
- [x] 发现 **32 个 tracked `.rs` 未 rustfmt-clean** → `cargo fmt --all --check` 必红，每次 CI 都红
- [ ] 跑 `cargo fmt --all` 修掉 32 个文件
- [ ] 修 `scripts/ci/validate-workflows.sh` allowlist 漏了 `dep16-reconcile.yml` / `dep17-capacity.yml`
- [ ] 落地 CI 合并（删 664 个、留 41 个、统一 `ci.yml`）
- [ ] 落地 DEP-18（workflow 不用补，guard 不引用；补 baseline + 证据块 + roadmap 行）

### 阶段 1：按依赖顺序清 ⏳ 队列

波次顺序：DEP-18 → DEP-19/20/21/22（备份恢复）→ DEP-23/24/25/26 → DEP-27..32（迁移）
→ DEP-33..35（发布/回滚/单机 rollout）→ SC/BQ/PD/CAP 的 ⏳ 项。

### 阶段 2：清 🔄 队列

241 张 🔄 是「已写源码、待远端验证或后续加固」。每张按卡片退出条件补齐，不能靠 source-only 证据升 ✅。

## 固定节奏

复现 → 分类 → 最小修复 → 审 diff + 证据 → 提交推送 → 回填完成态。
不等 CI。红了下一轮再修。

## 提交约定

- 一个 Step 一个提交，message `step: add <ID> <主题>`。
- 显式 `git add <路径>`，**绝不 `git add -A`**。
- `docs/roadmap.md` 只由本会话维护，同行一并提交。
- 分支 `step/<id>-<slug>-<yyyymmdd>`，合回 master 后再推。
