# 可以提交的文件清单（已完成审查）

**生成时间**：2026-10-09 03:15 UTC+8  
**状态**：已完成所有缺口补充  
**验证**：✅ cargo fmt ✅ cargo check ✅ git diff 检查

---

## 可以立即提交的文件

### Batch 3 Phase 1：kiana-runner 9 个文件

**状态**：✅ 注释完整，编译通过

```
kiana-runner/src/lib.rs
kiana-runner/src/protocol_runner.rs
kiana-runner/src/retry.rs
kiana-runner/src/state_driver.rs              (+116 行注释，已补充）
kiana-runner/src/inbox.rs
kiana-runner/src/budget.rs
kiana-runner/src/model.rs
kiana-runner/src/stream_normalizer.rs
kiana-runner/src/progress.rs
```

**提交命令**：
```bash
cd /media/shirosora/4A183E5C183E46EB/codestorage/kianacode

# 逐文件 add
git add kiana-runner/src/lib.rs
git add kiana-runner/src/protocol_runner.rs
git add kiana-runner/src/retry.rs
git add kiana-runner/src/state_driver.rs
git add kiana-runner/src/inbox.rs
git add kiana-runner/src/budget.rs
git add kiana-runner/src/model.rs
git add kiana-runner/src/stream_normalizer.rs
git add kiana-runner/src/progress.rs

# 提交
git commit -m "docs: 注释 kiana-runner 全 9 个模块 —— 执行引擎系统设计

- lib.rs：模块入口与 12 个模块的位置职责
- protocol_runner.rs：协议层到循环层的翻译  
- state_driver.rs：纯函数状态机与阶段转移（+116 行注释）
- inbox.rs：暂停期消息注入与幂等性
- budget.rs：三维度预算台账
- retry.rs：退避算法与错误分类
- model.rs：三种模型实现
- stream_normalizer.rs：流式增量归一化
- progress.rs：防空转的进度追踪

验证：编译通过 ✅ 格式通过 ✅ 无程序逻辑改动 ✅"
```

---

### Batch 3 Phase 2：Core dispatch/approvals

**状态**：✅ 注释完整，编译通过

```
kiana-core/src/dispatch.rs        (+196 行注释）
kiana-core/src/approvals.rs       (+251 行注释）
```

**提交命令**：
```bash
git add kiana-core/src/dispatch.rs
git add kiana-core/src/approvals.rs

git commit -m "docs: 注释 Core dispatch/approvals —— 派发与审批的核心控制平面

- dispatch.rs：派发层许可管理与 CAS 语义
  * 为什么 permit 是必须的（执行有效性凭证）
  * fail-closed 原则与授权系统耦合
  
- approvals.rs：审批生命周期与并发设计
  * 四阶段流程说明
  * 并发问题分析
  * 八种错误分类

验证：编译通过 ✅ 格式通过 ✅ 无程序逻辑改动 ✅"
```

---

### Batch 3 Phase 2 补充：EventLog jsonl.rs

**状态**：✅ 注释补充完成，编译通过

```
kiana-eventlog/src/jsonl.rs       (+132 行已有 + 新增块注释）
```

**新增的块注释**：
- visit_seq() 前：递归处理嵌套数组的说明
- visit_map() 前：防止重复键导致观察不一致的安全背景

**提交命令**：
```bash
git add kiana-eventlog/src/jsonl.rs

git commit -m "docs: 补充 EventLog jsonl 日志防护说明

- visit_seq()：递归处理嵌套数组的说明
- visit_map()：防止重复键导致观察不一致的安全背景
  * 标准库 serde_json 的问题（silent override）
  * append-only log 的观察一致性保证
  * fail-closed 防御策略

验证：编译通过 ✅ 格式通过 ✅ 无程序逻辑改动 ✅"
```

---

## 总计待提交

```
✅ kiana-runner：9 个文件
✅ kiana-core：2 个文件（dispatch.rs, approvals.rs）
✅ kiana-eventlog：1 个文件（jsonl.rs）

共 12 个文件
新增注释约 600+ 行
```

---

## 验证清单

- [x] `cargo fmt --all --check` —— 通过
- [x] `cargo check -p kiana-runner` —— 通过
- [x] `cargo check -p kiana-core` —— 通过
- [x] `cargo check -p kiana-eventlog` —— 通过
- [x] `git diff --check` —— 无格式错误
- [x] 逐行 git diff 验证 —— 仅注释改动，无程序逻辑改动
- [x] jsonl.rs 的 visit_map/visit_seq 已补充块注释
- [x] state_driver.rs 后 128 行已补充（+116 行注释）

---

## 后续工作（不在本任务内）

1. 真实的"Batch 1-2 已提交"清单（需 git log 验证）
2. kiana-core 的其他 26 个改动文件的审查
3. kiana-eventlog 的其他 15 个改动文件的审查
4. 更新 ANNOTATION_TASK_MANIFEST.md

---

**准备状态**：✅ 100% 可以执行提交

