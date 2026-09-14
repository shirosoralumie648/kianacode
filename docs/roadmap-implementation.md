# Roadmap 实现交付（2026-09-12）

本轮按用户指示推进 `docs/roadmap.md` 的 P0–P4 代码，并纳入后来追加的 ControlPlane 专项。
用户明确要求不编写、不运行测试；因此本页中的“已写实现”只表示源码接线，证明等级为
`source`。编译和格式检查的最终回执记录在 `CURRENT_STATUS.md`，不能替代行为、崩溃、
跨进程、live 或 physical 证明。旧 roadmap 的 ✅ 仍表示其历史提交与 CI 证据，不用于本轮 WIP。

## 实现入口

所有用户操作仍进入同一个 `DaemonHost → ControlPlane → Broker → EventStore`。
模型工具保持 `shell`、`apply_patch`、`mcp`、`memory.search`、`memory.write` 五个。
新增管理能力使用人工命令传输，未作为新的模型工具开放。

```bash
cargo build --bin kiana --locked --offline
./target/debug/kiana command company.snapshot.v1 --arguments '{}' --session-id operator --role sponsor
./target/debug/kiana command human.inbox --arguments '{}' --session-id operator --role sponsor
./target/debug/kiana approvals --session-id <原会话> --role <原角色> --permission-profile <原档位>
./target/debug/kiana resume --session-id <原会话> --role <原角色> --permission-profile <原档位>
```

项目需要按 `USER.md` 设置 ProjectTrust；修改命令需要显式的非 Safe 档位和相应角色。
审批还必须回传服务端给出的 nonce 与 request hash。审批只解决具体的 Ask，不能放宽硬拒绝。
Web 和 Workbench 提供人工命令与审批入口；历史会话的继续执行要求显式 Resume。

| 能力 | 命令与使用说明 |
|---|---|
| Objective / Project / Packet / Review / Acceptance / Delivery / Close | [Company API](company-command-api.md)，`company.command.v1` / `company.snapshot.v1` |
| Workflow / Trigger / Swarm / Packet lease | [调度 API](scheduling-command-api.md)，`workflow.command.v1` / `swarm.command.v1` |
| 人工待办、对账、反馈、编辑撤销 | [人工操作 API](human-operations-api.md)，`human.inbox` / `failure.incidents` / `workspace.checkpoint.*` |
| 记忆候选与真实模型蒸馏 | [记忆蒸馏](local-memory-distillation.md)，`memory.distill` / `memory.review` |
| 本地扩展与 Connector | [本地扩展 API](local-extensions-connectors.md)；[连接器专项设计](roadmap/integrations-connectors.md)，`extension.manage` / `connector.manage` / `connector.invoke` |
| 数据删除与撤销 | `data.governance`，携带 action、grant 与 expected_revision；派生缓存、快照及审批失效 |
| 来源回放与版本漂移 | `trace.capture` / `trace.replay` / `version.drift`；回放只折叠已记录事实，不调用模型或工具 |
| 新一轮输入 | `run.turn.v2`，参数 `prompt` / `sandbox` / 可选 `run_id`；前序 Run 必须已知终态，新建关联的 Turn / Run |

`run.turn.v2` 与旧协议 Continue 分开：旧 Continue 保留同 Run 的历史分轮解释，新语义不复活
终态 Run。Resume 只安装经验证的原 Run 暂停材料。Unknown 不自动重试。

## 原 roadmap 单元对应源码

| 单元 | 已写的行为 | 主要源码 |
|---|---|---|
| P0-A / B | ID/schema 注册、稳定错误与状态转移 | `kiana-domain/src/{contracts,errors,states}.rs` |
| P0-F / G | 审批、暂停快照、显式恢复、账本投影、调用身份 | `kiana-core/src/{approvals,recovery,history,projection,invocation_projection}.rs` |
| P0-J1 / K1 | 取消排空、停止确认、角色步数、累计预算、身份与权限版本 | `kiana-core/src/{lifecycle,sessions,model_budget,authority}.rs`，`kiana-runner/src/harness.rs` |
| P0-M1 / P2-M2…M7 | Workbench/Web 人工动作、历史会话、水合、游标重连、详情与无障碍 | `kiana-entrypoints/src/{workbench_chat,web,web_page.html,harness_run,product_command}.*` |
| P1-C / D / E | Cell、权限衰减、ready 谓词、DAG、租约、定向 Handoff、会议 | `kiana-domain/src/{packet_graph,handoff,roles}.rs`，`kiana-core/src/{cell_registry,collaboration}.rs` |
| P1-H / J4 | 固定工具目录、动作规范化、最低风险、containment、stdio MCP 生命周期 | `kiana-domain/src/{tool_catalog,actions}.rs`，`kiana-daemon/src/{harness_sandbox,mcp_stdio,harness_mcp}.rs` |
| P1-J2 | 有序 PromptBundle、来源哈希、角色包、完整 token 预算与模型路由 | `kiana-domain/src/prompts.rs`、`kiana-domain/role-packs/`、`kiana-daemon/src/model_client.rs` |
| P1-J3 | 候选写入、三档准入、层级 ACL、BM25/本地向量/RRF/MMR | `kiana-daemon/src/{harness_memory,memory_retrieval}.rs`、`kiana-core/src/memory_proposals.rs` |
| P1-J8 / K5 / L1 / L4 | 用量、成本未知标记、预算预留、GoldenTrace、源码快照与 freshness | `kiana-domain/src/usage.rs`、`kiana-core/src/{versioning,receipts,model_budget}.rs`、`kiana-query/src/` |
| P2-J5 / P4-K2 | 确定性 workflow、固定版本、显式推进、触发器、重试及补偿 | `kiana-workflow/src/durable.rs`、`kiana-core/src/automation.rs` |
| P2-K3 / K6 / L2 | Human Inbox、六类失败、对账、独立证据、候选反馈 | `kiana-core/src/platform.rs`、`kiana-domain/src/platform.rs` |
| P2-K4 / K7 | 受限文件快照与审批撤销、源数据撤销及派生视图失效 | `kiana-core/src/{workspace_checkpoints,data_governance}.rs`、同名 daemon adapters |
| P3-I | 业务全链、不可变产物、独立验收、关闭与 Outcome | `kiana-domain/src/company.rs`、`kiana-core/src/company.rs` |
| P4-E / J3 | 会议与终态触发的持久蒸馏队列，严格模型输出与 evidence 校验 | `kiana-core/src/memory_distillation.rs`、`kiana-domain/src/memory.rs` |
| P4-J6 | 有父级、有额度、有分区、有上限的 Swarm 与独立合并审查 | `kiana-core/src/swarm.rs`、`kiana-domain/src/swarm.rs` |
| P4-J7 | sequence/epoch、完整终态、慢消费者与重连边界 | `kiana-daemon/src/run_stream.rs`、`kiana-protocol/src/lib.rs` |
| P4-K8 / L3…L6 | 本地 Connector、签名技能包、哈希/license/diff/回滚、版本漂移 | `kiana-daemon/src/{connectors,extensions,local_packages}.rs`、`kiana-core/src/versioning.rs` |
| P4-M6 | 桌面壳的异步启动/停止、托盘与进程组关闭 | `contrib/desktop/`（目录按既有忽略规则保留，未修改 `.gitignore`） |

## ControlPlane 专项接线

| 专项 | 当前源码实现 |
|---|---|
| CP-01…05 | 服务端本地主体、可信角色目录、角色选择上限、不可变 assignment、统一动作目录与三入口决策；Gate/Hook 不能覆盖硬 Deny |
| CP-06/07/27/28 | `TransitionBatch` 多聚合 read-set、command digest 幂等、完整 JSONL 帧、格式门、逻辑游标、同步边界、限额和有界 blocking worker |
| CP-08/09/10 | authority revision、根目录身份、审批 subject、Approved/Consumed 分离；审批消费与许可准备同一事务；旧审批文件不再发权 |
| CP-11 | 每次模型请求前 journal reserve、usage 核销、Unknown 保留额度、Run 累计次数/token/wall-time；Company 预算约束 Cell 与子任务额度 |
| CP-12/13 | 父目录共享锁与叶节点排他锁、已提交 permit 的单次消费、action/context/epoch/到期核验；公开 authorization 字符串不能直接执行 |
| CP-14…17 | 结果先落账再回灌、统一取消、先停止再退休、Unknown 隔离；项目撤销与 Run 取消屏障合批提交 |
| CP-18/19 | 精确快照 hash、权限版本、数据 epoch、Cell 范围与审批 proof 重检；缺原始或敏感材料不恢复，不从 transcript 猜执行权 |
| CP-20…22/26 | 原事实保留的对账、证据与调用关联、只读 owner 查询、四入口的原权限审批和恢复、迟到终态重放 |
| CP-23/24 | Company / Workflow / Swarm / 人工记录通过受保护事务提交，确定性决策与模型/工具执行复用原脊柱 |
| CP-25 | 项目信任、来源与版本摘要、扩展权限交集、MCP 合同、Memory/data 撤销；未开放任意脚本或 secret 工具 |
| CP-00/29/30 | 源码对照与文档回填；故障注入、竞态、黄金流程、测试和发布验收按用户指令未执行 |

事务格式、升级和平台说明见 [JOURNAL.md](../kiana-eventlog/JOURNAL.md)。这是实现合同，
不是“已验证掉电安全”或“外部效果 exactly-once”的声明。旧格式保留查询；首次原子提交
写入 required v2 header，旧 writer 随后应拒绝写入。回退旧二进制不能继续写新格式日志。

## 配置与明确边界

- `KIANA_LOCAL_ALLOWED_ROLES` 是可信进程配置中的逗号分隔角色上限；空列表禁止所有角色。
  默认仅包含内置目录。wire actor 被本地主体覆盖，角色与模型参数分开。
- `KIANA_MODEL_PROFILES_JSON` 将内置 model_profile 接到已配置 provider/model；凭据仍由本地
  配置管理。本轮未使用真实 provider 产生验收回执，未建立价格来源，货币成本保持 unknown。
- `KIANA_MEMORY_DISTILL_AUTO=1` 才自动消费蒸馏队列，默认只排队。真实蒸馏限制一轮、无工具，
  仅产生候选；部门正式记忆仍需原审批流程。向量适配器为已校验的本地词向量模型，不是 ONNX。
- Connector 提供实际执行的 `local_fixture` adapter；没有假装接通支付、外卖、出行、IoT、
  企业租户或远程执行。P5/P6 在 roadmap 中只有边界登记，没有本轮实施卡。
- 已签名 Skill 可安装/升级/撤销/回滚；migration 只接受无状态版本声明，任意迁移脚本和
  其他插件的执行 adapter 未启用。HTTP MCP 继续返回明确不支持。
- 审批当前只支持 once；扩大到 turn/session/policy 依赖的独立验收未执行，因而明确不支持。
  敏感 payload 未建立加密持久 blob 服务，重启后原始材料缺失就拒绝恢复。
- 编辑 Undo 仅覆盖被捕获的有限 UTF-8 文件变更；shell/MCP 的现实副作用不能凭快照撤销。
  `failure.release` 仅在已有对账且账本能证明相关执行停止后释放隔离；不会修改原 Unknown
  终态或退还未知模型消耗，继续工作需要新请求。
- 没有自动 commit、push、merge、部署或外部发布。本页不把未测试源码标为 CI 完成。
