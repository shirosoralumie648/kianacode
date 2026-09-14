# 持久化与数据层专项：实际设计、处理流程与实施步骤

> `PD-00`–`PD-35` 是对 `module-map.md` 第 8 模块的实现路线补全。它补充存储边界、数据合同、启动/写入/投影/备份/迁移/删除流程和可执行验收，不替代 `ER-00`–`ER-36`（事实、收据、恢复）或 `CM-00`–`CM-39`（上下文、记忆、治理）。所有状态先记为 `⏳`；完成状态只能由 `CURRENT_STATUS.md` 的证据块提升。

## 1. 范围与现状边界

本专项负责“数据如何被保存、读取、重建、迁移和清理”，不负责重新定义 ControlPlane 的授权规则，也不创建第二条 Agent 执行循环。唯一的运行主链仍然是：

```text
entrypoint → DaemonHost → ControlPlane → EventStore/Projector → Broker/handler
```

当前源码快照（2026-09-14、共享工作树仍有 WIP）显示：

- `kiana-eventlog` 已有 Memory/JSONL 适配器、`TransitionBatch`、aggregate version/CAS、command digest、幂等键、逻辑 cursor、v2 frame、锁和部分 torn-tail 处理；这些是局部行为，不能直接宣称跨进程 durable authority。
- `kiana-daemon/src/approval_store.rs` 的 `MemoryApprovalStore` 可以写 JSONL、校验 proof、TTL 和并发更新，但审批事实尚未完全收敛到 EventStore，`PendingInvocation`、Runner/Session/Cell/Budget 仍有进程内状态。
- `kiana-daemon/src/harness_memory.rs` 已有分层 JSONL、candidate/draft、scope/grant、撤销和路径保护；Memory 的事件化提交、索引 generation、备份、恢复和统一保留仍未收口。
- `kiana-query/src/index.rs` 已有索引、artifact manifest、增量 cache 和确定性 hash embedding；这些结果是可重建的查询数据，不能成为事实源。
- `kiana-core/src/artifacts.rs` 已有受限路径、hash/identity 和原子文件写入原语；尚缺统一 ArtifactStore 合同、引用计数/保留、快照清单和恢复绑定。
- `CURRENT_STATUS.md` 记录的所有历史通过只约束对应源码快照。任何本专项条目都不把类型、文件、历史测试或参考项目能力写成当前完成状态。

与已有专项的边界：

| 已有专项 | 已经负责 | 本专项只补充 |
|---|---|---|
| `event-receipt-recovery.md` | 事件种类、Transition、Receipt、Unknown、重放、恢复和结果投递 | 存储根、格式/能力协商、投影底座、备份/迁移/保留，以及不同存储适配器的共同生命周期 |
| `context-memory.md` | Memory 语义、六层 ACL、candidate/qualification、检索与治理 | Memory mutation 如何进入事实边界、怎样物化/重建索引、怎样备份/删除/恢复 |
| `control-plane.md` / `capability.md` | 授权、Gate、审批消费、Broker 和 effect-time fencing | 审批/lease/Cell 等状态的持久化合同和 projector，不改授权决定本身 |
| `companyos.md` | WorkPacket、业务对象、交付和收尾语义 | 工单/收据/Artifact 的存储引用、版本与恢复关系 |
| `ui-entrypoints.md` | cursor、UI snapshot、重连和入口一致性 | UI 读取投影的 freshness/generation/health 元数据 |

## 2. 调研范围与可迁移结论

本次对 `reference/` 按 `event/journal/store/persist/checkpoint/memory/index/backup/retention` 做全量关键词扫描，再对代表性实现做源码级阅读。参考代码只作为行为对照，不复制源码、许可证、凭据格式或第二执行路径。

| 来源 | 已核对的实现 | 对 Kiana 的结论 |
|---|---|---|
| Codex rollout | `reference/codex/codex-rs/rollout/src/recorder.rs` 的后台 writer、`Persist/Flush/Shutdown` ack 和 terminal writer failure | 写入必须有显式 flush/shutdown 回执；后台任务失败不能静默丢失；session 文件和查询索引要分开 |
| Beads event journal | `reference/beads/internal/eventsjournal/record.go`、`activation.go`、`autoprune.go` 及 ordered migrations | storage row 与 public envelope 分离；seq 在 mutation 事务内分配且无间隙；激活失败不静默降级；retention 先写 watermark、再分批删除，维护失败不回滚用户事实 |
| OpenCode SyncEvent | `reference/opencode/packages/opencode/src/sync/README.md` | mutation 先写 sync event，再由 projector 改状态；bus 是兼容投递视图，不能直接 publish 或绕过数据库；事件 schema 必须类型化 |
| LangGraph | 本地 SQLite saver 与官方 persistence 文档 | run/thread checkpoint 与跨线程长期 Store 分离；每个 super-step 保存边界；pending writes 防止重复执行已完成节点；生产适配器要有持久介质，InMemory 只用于开发 |
| Temporal | 官方 event-history/replay 文档 | 事件历史是恢复依据；工作流状态必须可确定性重放；外部非确定性 I/O 需要独立 activity/effect 边界 |
| Restate / DBOS | 官方 durable steps 文档 | 文件、HTTP、数据库、UUID 等非确定性结果要序列化为 step 结果；重试按类型、次数和超时有界，不能把未知结果当失败重做 |
| SQLite | 官方 WAL、transaction、online backup 文档 | WAL 适合同机读写但始终只有一个 writer；长读会阻塞 checkpoint；WAL 与 DB 必须一起保存；online backup 是有一致性边界的增量快照；网络文件系统不在支持范围 |
| claude-memory | `reference/claude-memory/README.md` 的 JSONL parser → extractor → SQLite FTS5 | 原始会话、抽取结果、FTS 索引分层；抽取失败不能改写原始记录；查询索引属于派生数据 |
| Memorix / MemPalace | `reference/memorix/docs/ARCHITECTURE.md`、`1.3-MEMORY-ARCHITECTURE.md`、`reference/MemPalace/docs/ARCHITECTURE.md` | observation/reasoning/git evidence 与 curated long-term memory 分开；scope/kind/state/evidence 独立；向量库和关系库是加速/导航层；候选必须显式 qualify/approve，不能自动跨项目泄漏 |
| Graphiti | `reference/graphiti/README.md` | 事实有 validity window，旧事实通过失效/替代保留历史；episode/provenance 指向原始来源；混合检索不能取代来源证据 |
| OpenTelemetry | 官方 signals 文档 | logs、metrics、traces 是不同信号；EventLog 不能代替指标、追踪和告警存储 |

外部设计的共同规律是：一个可恢复系统需要“事实日志、可重建投影、不可变字节、显式 checkpoint、可验证快照”这五个层次；不能靠一个 JSON 文件或一个 ORM 表同时承担全部职责。

## 3. 目标数据架构

### 3.1 权威性矩阵

| 数据对象 | 写入权威 | 物理形态（第一阶段） | 读取形态 | 失败含义 |
|---|---|---|---|---|
| Runtime/Company facts | `EventStorePort::commit_transition` | v2 JSONL transition frame；未来可有同合同 SQLite adapter | EventStore cursor/stream/read-command | 未确认提交为 `Unknown`；禁止执行或生成成功 Receipt |
| Command receipt | 同一 transition frame | command id、digest、first/last cursor、aggregate versions | `read_command` 与 Receipt projector | digest 冲突是 Conflict；查询失败不是“没有记录” |
| Run/Invocation/Approval/Cell/Budget state | projector 消费 EventStore | 可重建 projection store；不另写事实 | 带 `source_cursor`、`projection_version` 的 snapshot | projector 落后可报告 stale；矛盾/坏数据进入 quarantine 并暂停相关动作 |
| PendingInvocation / runner checkpoint | ControlPlane 产生的 checkpoint event + Artifact refs | EventLog 元数据 + Artifact bytes | 认证后的恢复查询 | 缺少执行材料只能暂停，不能从 UI/transcript 猜测 |
| Approval challenge/decision | EventStore 事实；ApprovalStore 是查询/消费适配器 | 事件 + 可选物化表 | proof、scope、state、expiry、epoch | 过期、重复、错绑定或未知消费均拒绝 |
| Artifact bytes | ArtifactStore | 内容寻址文件、manifest、原子 rename | 只读 hash/范围读取 | hash/身份不符、路径竞态或 fsync 失败不能声称已保存 |
| Memory records | Memory mutation event | 分层记录或 SQLite/JSONL projection | scope/ACL/retention 过滤后检索 | candidate/draft 不进入默认上下文；撤销优先于索引命中 |
| Context/Repo index | `kiana-query` rebuild job | generation manifest + 可选 SQLite/FTS/vector | 带 freshness、source cursor、model hash | stale 可回退源扫描；索引损坏不损坏事实 |
| WorkPacket/Delivery/Closing receipt | CompanyOS domain event + Artifact refs | EventLog + immutable artifacts | 只读聚合查询 | 缺 evidence 或版本不一致只能 `incomplete/unknown` |
| Logs/Metrics/Traces | 独立 observability adapters | 各自生命周期与保留策略 | 诊断/告警查询 | 观测不可用不能改变业务事实或授权结果 |

### 3.2 物理布局与根目录规则

不要在代码中拼接固定 `~/.kiana` 或项目相对路径。由现有路径解析器根据 `KIANA_HOME`、项目根、trust 和 profile 解析一个 `StorageRoot`，然后在逻辑命名空间中分配：

```text
StorageRoot
├── meta/          store identity, schema epoch, capability, config
├── facts/         EventStore frames and command receipts
├── projections/   projector state and checkpoints
├── artifacts/     content-addressed immutable bytes and manifests
├── memory/        scoped memory records and governance tombstones
├── indexes/       context/repo/vector generations and cache metadata
├── checkpoints/   run/workspace/continuation checkpoint metadata
├── backups/       snapshot manifests and sealed backup sets
├── migrations/    applied migration records and preflight reports
├── quarantine/    corrupt/unknown records kept outside active reads
└── locks/         process and writer fencing files
```

这些是逻辑命名空间，不是对当前目录结构的事实声明。每个命名空间必须记录 `store_id`、`format_version`、`schema_epoch`、`owner_scope`、`created_at`、`last_cursor` 和 `data_epoch`。临时文件只能是同一父目录中的随机 sibling，提交顺序为写满 → `sync_data` → 原子 rename → `sync_dir`；任何一步失败都返回结构化错误。

### 3.3 版本、摘要和游标

不要把不同版本维度压缩成一个 `version` 字段：

| 字段 | 作用 | 递增条件 |
|---|---|---|
| `schema_version` | wire/domain payload 的兼容版本 | payload 契约变化 |
| `store_format_version` | 文件/表/帧布局版本 | 物理布局变化 |
| `projection_version` | projector 算法/输出 schema | 读模型变化 |
| `source_cursor` | 投影已应用到的 EventStore 逻辑游标 | 每个完整 commit |
| `config_revision` | 非秘密配置 snapshot 摘要 | 有效配置变化 |
| `authority_epoch` | Principal/Assignment/ProjectTrust fencing | 授权撤销、降权、重绑定 |
| `data_epoch` | 项目数据删除/撤销/恢复代际 | governance、restore 或 purge |
| `generation` | index/credential/checkpoint 当前代 | 原子替换或 rotation |

所有摘要使用 canonical bytes 计算。`command_digest` 绑定命令意图，`payload_digest` 绑定事件内容，`artifact_hash` 绑定字节；不能只用时间戳、文件路径或 JSON 的非规范序列化作为身份。cursor 只在已提交事务边界推进，分页不得切开一个 transition。

### 3.4 端口与组合方式

第一阶段不引入绕过 ControlPlane 的 `StorageService` 执行循环。建议把共用值对象和端口下沉到 `kiana-domain`/`kiana-ports`，把 adapter 留在现有 crate，由 `DaemonHost` 组装：

```text
kiana-domain
  StorageRoot / StoreIdentity / StoreCapabilities
  SnapshotManifest / ProjectionCheckpoint / MigrationRecord
  RetentionDecision / IntegrityIncident / ArtifactRef

kiana-ports
  EventStorePort                 (事实、CAS、幂等、cursor)
  ProjectionStorePort            (apply/checkpoint/rebuild/read)
  ArtifactStorePort              (stage/commit/read/verify)
  BackupStorePort                (snapshot/restore/verify)
  MigrationRunnerPort            (plan/apply/record)
  RetentionStorePort             (plan/tombstone/purge)
  StorageHealthPort               (health/capability/limits)

DaemonHost::StorageCoordinator
  resolve root → open adapters → negotiate capabilities
  → migration/recovery → projector catch-up → publish read-only handles
```

`StorageCoordinator` 只负责生命周期、锁、能力、迁移和投影调度；它不接受模型工具请求、不创建 Runner、不调用 Broker，也不拥有授权决定。若新增抽象不能消除跨 adapter 的重复，应继续使用现有 crate 的本地 helper。

### 3.5 JSONL 与 SQLite 的技术选择

事实账本的第一阶段默认保留当前 JSONL v2 frame，先把 framing、fsync、CAS、dedup、cursor 和恢复合同做成 adapter conformance；不要同时把 JSONL 和 SQLite 当作两个写入权威。SQLite 可作为 projector/index 的实现：使用 WAL、单 writer、短事务、定期 checkpoint，且把 `-wal`/`-shm` 与数据库一同纳入快照。SQLite 不放在网络文件系统上，也不把 WAL checkpoint 当作业务提交。

未来若实现 `SqliteEventStore`，必须先通过同一 `EventStorePort` conformance、故障注入和跨进程锁测试，之后才能替换默认 adapter；迁移期间采用单写源 + projector 双读验证，不允许双写两套事实。

## 4. 端到端处理流程

### 4.1 启动、打开和恢复

```text
resolve StorageRoot and trust
  → acquire instance/writer locks
  → read StoreIdentity and capability limits
  → reject unknown major / owner mismatch / epoch rollback
  → run migration preflight (read-only)
  → apply only approved ordered migrations
  → scan facts: checksum, frame boundary, command index, aggregate versions
  → classify torn tail / corrupt middle / unknown schema
  → load projection checkpoints and verify source cursor
  → rebuild or catch up projections in bounded pages
  → verify Artifact refs and data_epoch
  → publish StorageHealth; expose ControlPlane only when safe
```

空 store、可修复的最后半帧、损坏中间帧、未知 major、owner 不匹配和锁冲突必须是不同错误。修复最后半帧要记录 `recovery.repaired_tail`，不能静默截断；损坏中间帧进入 quarantine 并暂停依赖它的聚合。

### 4.2 一次命令和能力执行

```text
read authority + projection snapshot
  → build read-set (aggregate versions, authority_epoch, data_epoch)
  → canonical command digest
  → commit_transition(batch)
      ├─ Committed: fsync/commit barrier acknowledged
      ├─ Replayed: verify same digest and use original receipt
      ├─ Conflict: return current versions; no effect
      └─ Unknown: read_command/repair; never execute
  → only after Committed/Replayed dispatch one authorized effect
  → stage result/usage/artifact bytes
  → atomically append result + settlement + refs
  → update projections asynchronously with source_cursor
  → deliver Receipt with freshness and evidence refs
```

外部 effect 的网络响应、进程退出、文件写入和 provider usage 不能与本地 EventStore 假装成一个跨系统事务。effect 已发出但 result 事件未确认时只允许 `result_unknown`，随后进入 reconciliation queue；重试必须使用 invocation/attempt 幂等键和明确的 provider verification。

### 4.3 投影、查询和重建

每个 projector 定义 `projector_id`、输入 event kind、`projection_version`、读模型 schema、checkpoint 粒度和失败策略。先校验事件 schema/ACL/epoch，再在自己的短事务中更新读模型，最后原子推进 `source_cursor`；不能先推进 cursor 再写状态。投影失败保留失败 cursor、错误摘要和重试次数，不能跳过事件。

查询顺序为：认证 scope → 选定 source cursor → 验证 projection generation → 读取 read model → 检查 freshness/health → 必要时从 EventStore 或源文件 bounded fallback。返回值必须带 `source_cursor`、`projection_version`、`data_epoch`、`stale` 和 provenance；cache hit 不能隐藏数据撤销或权限变化。

### 4.4 Artifact 写入和引用

```text
prepare confined path / temp sibling
  → stream bytes with size limit
  → classify data/purpose and redact only where contract allows
  → compute content hash and metadata
  → fsync temp + atomic rename + fsync parent
  → write immutable ArtifactManifest
  → append artifact.created/ref_added fact
  → projector updates reference graph and retention counters
```

Artifact 引用先写 manifest 再进入 Receipt；只写路径不写 hash 不算 evidence。相同 hash 的内容可去重，但 manifest 的 owner、scope、source cursor 和 retention 不能合并。读取每次重新验证 file identity、hash 和 data epoch，防止 symlink、hardlink、替换和 TOCTOU。

### 4.5 Memory 写入、检索和治理

```text
derive reader/writer scope and purpose from AuthoritySnapshot
  → create candidate/draft with evidence refs
  → explicit qualify/approve (if required by CM contract)
  → commit memory mutation event with idempotency key
  → materialize scoped MemoryRecord
  → update lexical/vector index generation
  → search only visible states and grants
  → include bounded hits + provenance in ContextPlan/Receipt
```

Memory 的语义、ACL 和候选晋级遵循 `CM`；本专项要求每次 mutation 都可从 EventStore 重建，删除/撤销写 tombstone 并提升 `data_epoch`，所有 projection/index/cache 在看到 tombstone 前不得继续返回旧记录。索引不可用时可降级 lexical/source scan，但不能放宽 scope。

### 4.6 备份、恢复和替换

```text
request maintenance lease / quiescent read boundary
  → capture fact cursor + each projection cursor + data_epoch
  → seal Artifact/Memory/Index manifests
  → copy immutable files and DB snapshot (including SQLite WAL state)
  → write SnapshotManifest with hashes, sizes, versions and source cursors
  → fsync manifest and backup directory
  → verify in a new restore root
  → acquire new instance lock and replay/rebuild projections
  → compare sentinel aggregates, refs, ACL and epoch
  → only then make restored root active
```

恢复默认写入新根目录，不原地覆盖。恢复后的 `store_id`、`instance_id`、`data_epoch` 和所有 authority/credential generation 必须重新围栏；旧 approval、lease、pending invocation 和 scheduler trigger 不能自动复活。备份不等于现实 effect 成功，外部系统需要单独 reconciliation。

### 4.7 迁移、保留和删除

迁移流程为 `detect → preflight → backup/manifest → acquire migration lock → apply ordered idempotent steps → verify → record MigrationRecord → projector catch-up`。未知 major、缺少必需 migration、checksum 不符或 downgrade 都 fail-closed；回滚优先采用前向迁移或只读兼容窗口，不在未验证时删除旧数据。

保留流程为 `policy resolve → legal/audit hold check → tombstone → invalidate derived views → bounded purge/archive → watermark commit`。策略至少按 data class、purpose、owner scope、created/last-used、hold、source cursor 和 artifact ref 判断。派生索引可以先删，事实 metadata 是否保留要按审计合同决定；每个 sweep 有上限、超时和幂等键，维护失败不能使用户事实回滚，也不能被 UI 隐藏。

## 5. 实施步骤（PD-00–PD-35）

每一步都按“先拒绝、再成功、最后回归”执行。表中的路径是代码归属建议，不代表当前已存在；同一行的测试名是待实现的验收意图，不是现有证据。

| Step | 目标与代码归属 | 依赖 | 先拒绝的验收 | 成功与回归验收 |
|---|---|---|---|---|
| `PD-00` | 固定数据层基线、事实/投影/Artifact/Memory/Index/Approval 现状；`docs/roadmap*`、`CURRENT_STATUS.md` | — | 找出所有第二写源、raw file authority、无 hash 引用和未分类 cache；未知状态不被标成 durable | 产出对象矩阵、源码锚点、WIP 快照、证据缺口清单 |
| `PD-01` | 定义 `StorageRoot`、`StoreIdentity`、owner scope、逻辑 namespace 和锁；`kiana-domain`、`kiana-daemon` | PD-00 | 相对/越界根目录、owner/instance 不匹配、锁冲突、非普通文件、网络 FS 不支持均拒绝 | 同一解析器在 CLI/Web/Workbench 得到同一 root；重启保留 identity |
| `PD-02` | 建立 schema registry、canonical JSON/bytes、upcaster 和 unknown-field/major 规则；`kiana-domain`、`kiana-protocol` | PD-00 | 未知 major、重复 schema、非规范数字/时间、不可迁移字段 fail-closed | 每个 data contract 有 round-trip、digest 稳定性和兼容 migration fixture |
| `PD-03` | 统一 `StorageError`、`StoreHealth`、`IntegrityIncident`、能力限制和错误码；`kiana-domain`、`kiana-ports` | PD-01/02 | 将空、不可用、损坏、冲突、未知和 Unknown 混成同一个 I/O error | CLI/HTTP/Receipt 能稳定映射错误、重试和暂停策略 |
| `PD-04` | 下沉 `ProjectionStorePort`、`ArtifactStorePort`、`BackupStorePort`、`MigrationRunnerPort`、`RetentionStorePort`；`kiana-ports` | PD-01..03 | port 返回未声明能力、secret/raw bytes、错误 epoch 或隐式 fallback | Memory fake 与 durable fake 共享 conformance；`StorageCoordinator` 仅负责生命周期 |
| `PD-05` | 为 EventStore 建立 adapter conformance；`kiana-eventlog/tests` | `ER-04`、PD-04 | 非原子 adapter 在调用方要求 transition 时拒绝；CAS/dedup/cursor 失败不得写入 | Memory/JSONL 对同一 batch 返回一致 Committed/Replayed/Conflict/Unknown 语义 |
| `PD-06` | 收口 JSONL v2 frame、checksum、fsync、writer lock、尾部恢复；`kiana-eventlog` | PD-05 | torn middle、checksum 错、目录替换、磁盘满、锁失效、writer worker 崩溃均停止暴露新事实 | kill-9/重开保留完整 prefix；Flush/Shutdown ack 可观察；限制写入吞吐但不静默丢数据 |
| `PD-07` | 完成 command/event/aggregate/cursor 索引和 page boundary；`kiana-eventlog` | PD-05/06 | 重复 command digest、同 key 不同 payload、cursor 落在事务中间、版本回退拒绝 | `read_command/read_from/read_stream` 在重启后等价，分页不切 frame |
| `PD-08` | 启动 integrity scan、quarantine、recovery report 和 health gate；`kiana-eventlog`、`kiana-daemon` | PD-06/07 | 损坏事件被跳过、未知 major 被当空库、health 未 ready 仍可执行均失败 | 空库、可修复尾部、损坏中间帧、未知格式四类 fixture 得到不同状态 |
| `PD-09` | 建立 projector runner、checkpoint、重试/暂停/重建协议；`kiana-core`、`kiana-daemon` | PD-07/08 | 先推进 cursor 后写状态、跳过坏事件、并发 projector 覆盖新 generation | catch-up、全量 rebuild、重复 apply 和 projector crash 均幂等 |
| `PD-10` | Run/Invocation/Attempt/Receipt 读模型；`kiana-core` | `ER-08..16`、PD-09 | foreign run、缺 terminal、矛盾 terminal、旧 epoch 不能被投影为 Completed | 新进程只靠 facts 重建同一状态；Receipt 带 evidence/source cursor |
| `PD-11` | Cell/Grant/Budget/Lease/Authority 状态投影；`kiana-core`、`kiana-domain` | `CP-10`、`CAP-17`、PD-09 | revoked/expired/stale epoch、预算重复结算、子权限并集、未知 lease 均拒绝 | restart 后能重建未决集合；reserve/commit/finish/retire 可重放 |
| `PD-12` | 把 ApprovalStore 从独立 JSONL 权威迁为 EventStore 事实 + 查询适配器；`kiana-daemon` | PD-10/11、`ER-10` | 文件存在但 EventLog 无 activation/decision、proof/nonce/epoch 错配、重复消费均 fail-closed | 旧记录只读兼容迁移；stage→activate→consume 与取消/过期事件原子关联 |
| `PD-13` | PendingInvocation、Runner continuation、workspace checkpoint 的持久化和恢复材料；`kiana-core`、`kiana-daemon` | PD-10..12、`ER-17..22` | 重启后凭 transcript/UI 自动继续、缺 sandbox/authority/artifact ref、旧 approval 复活均拒绝 | 显式 ResumeRequest 重新 admission；同一 invocation 不重复 effect |
| `PD-14` | 统一 ArtifactStore、content hash、manifest、ref、原子读写；`kiana-core`/新 port | PD-04、`ER-03` | symlink/hardlink/rename race、hash mismatch、超限、非 owner scope、fsync 失败均拒绝 | 相同 bytes 可去重；不同 scope 不合并；reopen/verify 保持 manifest |
| `PD-15` | Workspace patch/checkpoint、diff、undo 与 Artifact refs 绑定；`kiana-core` | PD-14、`P2-K4-01` | checkpoint 与 workspace revision/invocation 不匹配、恢复写入未授权路径、旧 ref 替换均拒绝 | capture/preview/restore evidence 可重建，失败产生 Unknown/Incident |
| `PD-16` | Receipt 重算器、evidence graph 和 delivery/closing 引用；`kiana-core`、`kiana-query` | PD-10、PD-14/15 | transcript/cache/模型自述单独生成成功 Receipt、缺 artifact hash、foreign source 均拒绝 | 从 facts+manifest 重算与在线投影一致；延迟投影可标 stale |
| `PD-17` | Memory mutation journal、candidate/draft/qualify/approve/supersede/tombstone；`kiana-daemon`、`kiana-eventlog` | `CM-04/05`、PD-07/09 | model 自批、伪造 origin、scope 超集、无 evidence、重复 mutation 均拒绝 | 同一 mutation key 重放不重复；重建保留六层/状态/来源 |
| `PD-18` | Memory projection、ACL/治理 epoch、retention 和删除索引联动；`kiana-daemon`、`kiana-core` | PD-17、`CM-20..29` | revoked source 仍被检索、candidate 注入 prompt、跨 project 泄漏、tombstone 被 cache 复活 | qualify/approve 后才可检索；删除传播到 lexical/vector/cache/receipt |
| `PD-19` | ContextIndex generation、source fingerprint、freshness、原子切换；`kiana-query` | `CM-10..14`、PD-09 | 读到半 manifest、根目录/忽略规则变化仍报 fresh、模型 hash 漂移仍复用向量 | 增量/全量 rebuild、rename/delete、损坏 cache fallback 都有 generation 断言 |
| `PD-20` | RepoMap、context artifact ingest 和依赖图的持久 manifest；`kiana-query` | PD-14、PD-19 | 越界 source、二进制/超限文件、旧依赖图覆盖新根目录均拒绝 | manifest 带 content hash、source cursor、tool/version；可从源重建 |
| `PD-21` | Cache 与事实/投影分离，淘汰、大小/时间上限和禁用开关；`kiana-query`、`kiana-daemon` | PD-09、PD-18..20 | cache miss 被当空事实、cache write 失败改变业务结果、权限变化仍命中旧 cache 均拒绝 | 删除任意 cache 后行为等价；health 能报告 hit/miss/stale/degraded |
| `PD-22` | SnapshotManifest、增量/全量 backup、文件 hash、cursor/epoch seal；`kiana-daemon`、`kiana-eventlog` | PD-08..21 | 未 quiesce、缺 WAL/manifest、hash 不全、跨 owner/epoch backup、备份路径覆盖活动根均拒绝 | backup 可在新根验证；中断复制可重试且不伪造完成 |
| `PD-23` | Restore verifier、替换 fencing、外部 effect reconciliation；`kiana-daemon`、`kiana-core` | PD-22、`ER-23..28` | 原地覆盖、旧 approval/lease/trigger 自动复活、Artifact ref 缺失、authority rollback 均拒绝 | restore 后 facts/projectors/ACL/epoch 一致；旧实例不能继续写 |
| `PD-24` | 有序 migration runner、preflight、lock、checksum、MigrationRecord；`kiana-domain`、`kiana-daemon` | PD-02/03、PD-22 | unknown major、降级、部分应用、并发 migration、无备份 plan 均停止 | migration 幂等；新旧 fixture 可验证；失败可继续或进入只读 quarantine |
| `PD-25` | Retention policy、legal/audit hold、archive、bounded prune/watermark；`kiana-core`、`kiana-eventlog` | PD-18、PD-22/24 | hold 中数据被删、derived 删除早于 tombstone、无上限 sweep 阻塞命令、错误回滚事实均拒绝 | watermark 先提交；分批/超时/并发 prune 安全；失败不破坏写入 |
| `PD-26` | data revocation/delete/expire 到 facts、Artifact、Memory、Index、cache、checkpoint 的传播；`kiana-core` | PD-18/21/25、`ER-29` | 任一派生层继续返回 revoked data、旧 data_epoch 可写、删除无 receipt 均拒绝 | tombstone/idempotency/epoch 传播有顺序和审计；恢复后仍不复活 |
| `PD-27` | 多进程 writer、backpressure、flush/shutdown、取消和资源上限；`kiana-eventlog`、`kiana-daemon` | PD-06/09、`ER-30` | 队列满后静默丢事件、关闭返回前后台仍写、取消遗留锁/lease 均拒绝 | bounded queue、可观察拒绝、优雅关闭；强杀后下一进程可接管 |
| `PD-28` | 路径/权限/secret redaction/可选加密/文件身份安全边界；`kiana-core`、`kiana-eventlog` | PD-14、`CI-07/11` | secret sentinel 出现在 facts/receipt/cache/log、symlink/hardlink/权限过宽、解密 key 越权均拒绝 | Linux/非 Unix 能力差异显式报告；Debug/Display/backup manifest 不泄密 |
| `PD-29` | storage health、projection lag、backup/migration/retention metrics 和诊断 DTO；`kiana-core`、`kiana-daemon` | PD-03、PD-09、PD-22/25 | 诊断把 stale/unknown/corrupt 显示成 healthy、输出 payload/secret、health 反向授权均拒绝 | logs/metrics/traces 与 facts 分离；UI 能显示 cursor/generation/限制 |
| `PD-30` | 故障注入矩阵：kill-9、磁盘满、权限、锁争用、坏帧、崩溃时机、网络 Unknown；各 adapter tests | PD-05..29 | 每种故障都不能生成 Completed、重复 effect 或越权恢复 | 每个故障有状态分类、恢复动作、fixture、退出码和限制证据 |
| `PD-31` | Memory/JSONL/durable/未来 SQLite adapter conformance；`kiana-eventlog/tests`、`kiana-ports` | PD-05、PD-09、PD-22 | adapter 宣称 unsupported 却被调用；Memory 被误标 durable；不同 adapter 事件顺序/错误码漂移均拒绝 | 相同 contract suite 通过；能力矩阵明确 durable/local_behavior/physical 边界 |
| `PD-32` | 平台和文件系统矩阵：Linux ext4/tmpfs、Windows/macOS fallback、网络 FS、时钟/编码 | PD-06、PD-14、PD-27/28 | 不支持的 rename/fsync/lock 语义被当作等价 durable；时钟回退破坏 TTL/retention 均拒绝 | capability negotiation 和安装前检查；各平台有独立 limitations |
| `PD-33` | 备份→恢复→升级→重启→治理删除的端到端 UAT；CLI/Web/Workbench 共用 DaemonHost | PD-22..32、`ER-33..36` | 恢复后无 auth re-admission、索引/Receipt 不一致、旧入口绕过存储协调器均拒绝 | 黄金项目 fixture 全链通过；无真实外部副作用的 replay 与 fake effect 分离 |
| `PD-34` | 容量、吞吐、延迟和退化预算；事件帧、Artifact、索引、backup/prune 的压力测试 | PD-27、PD-29/33 | 无界 frame/batch/queue、长 reader 阻塞 writer、maintenance 挤占用户命令均拒绝 | 给出上限、P95/P99、背压和降级证明；超过容量进入可诊断拒绝 |
| `PD-35` | 收口文档、CURRENT_STATUS 证据、发布门与迁移 runbook；`docs/`、scripts、CI | PD-00..34 | 只凭类型/单测/文件存在提升 durable/live/physical；遗漏限制或回滚路径均拒绝 | 每个已完成 PD 有 source snapshot、worktree、argv、环境、fixture、exit code、status/proof/limitations/reviewer |

## 6. 执行波次与交接规则

```text
Wave A  contracts:   PD-00 → PD-01 ∥ PD-02 → PD-03 → PD-04
Wave B  facts:       PD-05 → PD-06 → PD-07 → PD-08 → PD-09
Wave C  authority:   PD-10 → PD-11 → PD-12 → PD-13
Wave D  bytes/query:  PD-14 → PD-15 → PD-16 ∥ PD-17 → PD-18 → PD-19 → PD-20 → PD-21
Wave E  operations:   PD-22 → PD-23 → PD-24 → PD-25 → PD-26
Wave F  hardening:    PD-27 ∥ PD-28 → PD-29 → PD-30 → PD-31 → PD-32 → PD-33 → PD-34 → PD-35
```

并行只适用于没有共享写集的研究或测试；`Cargo.toml`、`Cargo.lock`、schema registry、StorageRoot 和公共端口由集成负责人串行修改。每个 wave 结束时必须把 adapter capability、projection lag、backup cursor、data epoch 和未解决限制写回证据块，再允许下一 wave 消费。

与现有路线的接点：

- `P0-A-01b`、`ER-01`、`CO-08` 提供 schema/迁移语义；PD 不另造一套 registry。
- `P0-G-04`、`ER-05..16`、`CP-21` 提供事实投影和 Receipt 语义；PD 提供 projector/checkpoint/health 底座。
- `P2-K4-01`、`ER-17..28` 提供 workspace/recovery 退出条件；PD 负责 Artifact/backup/restore 的存储证明。
- `P1-J3-01..04`、`CM-04..39` 提供 Memory/Context 行为；PD 负责 mutation journal、generation、retention 和恢复。
- `P2-K7-01`、`ER-29` 提供治理目标；PD 把删除/撤销传播落实为 tombstone、epoch 和 bounded maintenance。
- `CI-02..CI-11` 提供身份、配置和凭据版本；PD 只保存 opaque ref、generation 和审计元数据，不接触 secret 原值。

## 7. 验收矩阵与证据模板

最低负向矩阵必须覆盖：空/损坏/未知 store、owner/锁冲突、未知 schema、CAS/dedup/cursor 冲突、torn tail/middle corruption、projector lag/重复/崩溃、foreign run、过期/撤销 approval、pending 恢复越权、Artifact TOCTOU/hash mismatch、Memory ACL/tombstone 泄漏、index stale/corrupt、backup 缺文件/WAL、restore epoch rollback、migration partial/downgrade、retention hold、磁盘满/队列满、secret sentinel 泄漏和 effect `result_unknown`。负向用例必须断言 Broker/effect 调用数为零，或明确记录“可能已发生”的 Unknown。

成功矩阵必须覆盖：Memory 与 JSONL contract、重启重建、同 digest replay、aggregate CAS、单次 approval consume、Artifact hash/manifest、ContextIndex generation、backup/restore 新根、ordered migration、tombstone propagation、bounded prune、跨 CLI/Web/Workbench 同一 snapshot，以及 fake effect 的结果结算。SQLite/WAL 仅在实际 adapter 加入后验收，不能以文档或依赖存在代替证据。

每个完成 step 在 `CURRENT_STATUS.md` 使用以下证据块：

```text
source_snapshot: <commit or dirty checkout description>
worktree_status: <WIP/clean; unrelated changes preserved>
command_argv: <exact commands, locked/offline where applicable>
cwd/environment: <repository root, OS, toolchain, env flags>
fixture·cassette: <store root, event fixture, crash injection, fake effect>
exit_code: <each command and focused test count>
status change: <feature_status; never infer from type existence>
proof-level change: <source/local_behavior/durable/live/physical>
limitations: <adapter, platform, external effect, scale and recovery limits>
reviewer: <person or focused review>
```

专项的最终退出条件不是“所有文件都写了”或“单测全绿”，而是：事实只有一个写入权威；每个读模型都可从 cursor 重建；Artifact/backup 有 hash 与清单；迁移/保留/删除有可重放记录；未知结果不会被改写成成功；并且 `CURRENT_STATUS.md` 对每个声明给出与当前源码快照绑定的证明等级和限制。

## 8. 参考链接

- [SQLite WAL](https://www.sqlite.org/wal.html)、[SQLite transactions](https://www.sqlite.org/lang_transaction.html)、[SQLite online backup](https://www.sqlite.org/backup.html)
- [LangGraph persistence](https://docs.langchain.com/oss/python/langgraph/persistence)
- [Temporal event history and replay](https://github.com/temporalio/documentation/blob/main/docs/encyclopedia/event-history/python.mdx)
- [Restate durable steps](https://docs.restate.dev/develop/ts/durable-steps)
- [DBOS steps](https://docs.dbos.dev/python/tutorials/step-tutorial)
- [OpenTelemetry signals](https://opentelemetry.io/docs/concepts/signals/)
