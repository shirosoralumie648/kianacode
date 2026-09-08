# 事件账本与回执（干了什么都有据可查）

> 一句话：Kiana 干的每一件事都会记进一个只能追加、不能涂改的流水账文件，最后还能从账里算出一张"小票"（回执），告诉你这次到底干了什么。
> 本文写的是代码现在的真实样子，不是产品愿景；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

EventLog（事件账本）像一本**只能追加的流水账**：每件事记一行，只往后写，不能改前面已经写下的内容；Receipt（回执）是**从这段流水汇总出来的小结**——它不是新记的账，而是把刚才那一串事件汇总计算出来的结果。

为什么要有它？因为你把活儿交给 AI 干，最关心的就是"它到底动了哪些文件、调了哪些工具、有没有越权"。有了这本账，事后任何时候都能翻出来对质：谁、在哪台机器、什么时候、干了什么。

## 现在能干什么 / 不能干什么

**能：**

- 每次运行的关键步骤都会写进磁盘上的账本文件 `~/.kiana/sessions/events.jsonl`（位置可以用 `KIANA_HOME` 环境变量整体挪走），见 `kiana-eventlog/src/lib.rs:default_sessions_log_path`。
- 账是一行一条、只能追加不能修改的，每行是一个 JSON 事件；写完会强制刷到磁盘，见 `kiana-eventlog/src/jsonl.rs:append_jsonl_line_at`。
- 同一件事记不了两次：事件编号（event_id）重复会被直接拒绝，同一件事的序号必须严格递增，见 `kiana-eventlog/src/event_store_core.rs:reject_conflicts`。
- 同一笔账重试不会记两次：带"幂等键"（下文有解释）重复提交时，账本会把原来那条原样还给你，不写新行，见 `kiana-eventlog/src/event_store_core.rs:plan_idempotent_append`。
- 断电写了一半的最后一行能自动修复：下次打开时把残行裁掉、前面完好的记录都保留；但中间某行坏了整个账本直接拒开（宁可拒绝打开也不读残缺数据），见 `kiana-eventlog/src/jsonl.rs:load_jsonl`。
- 回执是从账里算出来的：改了哪些文件、memory 搜索命中了什么、压缩了几次对话、请求过哪些能力，全部从账目逐条扒出来汇总，见 `kiana-core/src/lib.rs:receipt_from_events`。
- 重启进程后还能补查账：`kiana run --receipt <id>` 或网页版的 `/api/receipt` 接口，都能从磁盘账本重建一张历史回执，不要求原来那个进程还活着，见 `kiana-entrypoints/src/cli.rs`（`--receipt` 参数）和 `kiana-core/src/lib.rs:read_receipt`。
- 写账前先把敏感信息打码：字段名里带 api_key、password、secret、token 之类的内容会被替换成 `[REDACTED]`（打码，就是把敏感内容涂黑），见 `kiana-core/src/lib.rs:redact_event_value`。

**不能：**

- 这整套的官方状态是"代码在、局部测过，还没当真产品验证过"（CURRENT_STATUS.md 能力表：JSONL EventLog / 基础 Receipt = partial / local_behavior）。
- "跨进程接着上次继续跑"做不到：会话和运行的对应关系、待审批的事项、取消的开关，这些**活状态**都只存在内存里，进程一重启就没了（CURRENT_STATUS.md：跨进程完整 resume = deferred，已推迟），见 `kiana-core/src/lib.rs` 里的 `sessions`/`pending_invocations`/`cancellations` 三个内存表。
- 回执绝不猜结果：账里没有"跑完"（run.completed）这条记录，或者同时出现了好几种互相矛盾的结局，一律回答"结果未知"（ResultUnknown），不会替你编一个"应该成功了"，见 `kiana-core/src/lib.rs:read_receipt`。
- 账本不是防篡改保险柜：它防的是"两个人同时抢着记账把账写乱"，不防"有人直接打开文件改历史"——文件是明文 JSON，能碰这台电脑磁盘的人就能改，代码里没有签名或加密。
- 账本只进不出：没有任何裁剪、归档、清理机制，跑得越多文件越大。
- CURRENT_STATUS.md 明确写着："当前按 request sequence；不是 aggregate/CAS durable authority"——翻译过来：按"哪次请求的第几笔"记账已经可靠，但"作为跨进程唯一权威账本"这个更高的定位还没达成。

## 代码怎么跑（走读）

先认识几个出场角色：

- **JsonlEventLog**（`kiana-eventlog/src/jsonl.rs`）：磁盘事件文件的读写实现。持有一个文件路径，和一份从文件里读出来的事件清单（内存缓存）。规矩是：每次记一笔或查一笔之前，都先重新读一遍磁盘文件，保证看到的是最新内容。
- **RuntimeEvent**（`kiana-domain/src/lib.rs`）：账上的一笔。每笔都有全局唯一编号（event_id）、属于哪次请求（request_id）、是这次请求的第几笔（sequence，序号，从 1 开始数）、发生了什么（kind，比如 run.completed）和细节（data）。
- **ControlPlane**（`kiana-core/src/lib.rs`）：记账的决策方。所有要写进账本的事都经过它，由它决定记什么、要不要脱敏、归哪个 aggregate（分册）。
- **EventStorePort**（`kiana-ports/src/lib.rs`）：规定记账、查账接口的契约。内存版（MemoryEventLog，只用于测试和临时场景）和磁盘版（JsonlEventLog）都按它实现。

现在开始走一遍：你敲了命令之后，第一件事是——

1. **你敲 `kiana run --sandbox workspace-write -- "建一个文件"`。** 命令行入口（`kiana-entrypoints/src/cli.rs`）解析参数，通过 kiana-client 把请求递给 DaemonHost。
2. **DaemonHost 组装时先打开事件文件。** `kiana-daemon/src/lib.rs` 在组装整个系统时，用 `JsonlEventLog::open_default()` 打开 `~/.kiana/sessions/events.jsonl`，把已有事件整个读进内存。这一步存在的意义：不管你从命令行、文件夹工作台还是网页进来，用的都是同一个账本。
3. **控制面开始逐笔记账。** 每逢要紧节点——这次运行被批准（run.authorized）、开始跑（run.started）、请求用某个工具（run.capability_requested）、工具跑完（capability.completed，里面记着改了哪些文件）——都会走到 `kiana-core/src/lib.rs:append_event`。这个函数做三件事：
   - 先打码（调 redact_event_value），免得密钥之类的东西混进永久记录；
   - 再决定这笔账归哪个"分册"（aggregate，指账本按主题分册：带 packet_id 的记进工作包分册，带 run_id 的记进本次运行分册，否则记进本次请求分册），见 `aggregate_for_event`；
   - 最后生成幂等键（格式是"请求号:分册:序号:事件名"）交给写入层。
4. **落盘。** `kiana-eventlog/src/jsonl.rs:append_jsonl_line` 把这笔事件变成一行 JSON，以追加方式写进文件，然后 sync_data（强制让操作系统真正写到磁盘上，而不是躺在缓存里）。Linux 上还有一道防护：打开文件时拒绝跟随符号链接（符号链接可被用来把写入路径偷换到别处），目录句柄也提前固定，防止写错地方。
5. **跑完，汇总回执。** 控制面先记 run.completed，然后调 `kiana-core/src/lib.rs:receipt_from_events`，把这次运行的所有事件翻一遍：从 capability.completed 里取出改动的文件清单，从 memory 搜索记录里取出命中项，数一数压缩了几次，列出用过的能力——汇总成一张回执。这张回执自己也被记成一条事件（run.receipt），最后作为命令的返回结果交给你。
6. **事后查回执。** 过了几天，进程早就关了，你敲 `kiana run --receipt <id>`。这次没有"正在跑的运行"可以依附，控制面的 `read_receipt` 直接从磁盘把事件全读出来，按运行编号筛出相关的那部分，先核对来查的人身份对不对（是不是当初那个会话、那个项目、那个人，对不上直接拒绝），再按事件里的"结局记录"判定结果，最后重新算一张回执给你。

**分支一：失败时怎么走。** 如果工具执行到一半结果说不清（比如进程被杀了，不知道做到哪一步），控制面老老实实记一条 run.result_unknown（结果未知）。之后任何人来查这张运行的回执，`read_receipt` 看到这条记录，返回的就是"结果未知"，而不是猜一个"大概成功了"。账上有几种互相矛盾的结局（既有"跑完"又有"失败"）同样回答"未知"。

**分支二：同一笔账被提交两次怎么走。** 这就是"幂等"登场的地方。幂等（idempotent）的意思是：同一件事做十遍，结果和做一遍一样。每笔可能重试的事件都带一个幂等键（相当于业务单号）。第二次拿着同一单号来记账时，`plan_idempotent_append` 发现单号已经存在、内容也一模一样，就把原来那条事件原样还给你，并标注"这是重放"（replayed），不写新行——所以网络重试、用户手抖重复提交，都不会让事件翻倍。如果单号一样但内容不一样，直接拒绝，因为那说明有东西在冒用单号。

**顺带解释 CAS。** CAS（Compare-And-Swap，"先核对再落笔"）是记账前的另一个保险：提交时声明"我以为账本现在到第 7 笔了"，写入层核对发现实际是第 9 笔，就拒绝这笔提交（`event_store_core.rs:reject_expected_version`）。这防的是两个人同时抢着写下一笔、互相覆盖的情形——先到的成功，后到的被退回重算。

## 关键概念速查

| 概念 | 白话解释 | 代码在哪 |
|---|---|---|
| EventLog（事件账本） | 只能追加、不能修改的流水账文件，一行一件事 | `kiana-eventlog/src/jsonl.rs:JsonlEventLog` |
| RuntimeEvent（一条事件） | 一次不可变的记录：谁的（请求号）、第几笔（序号）、发生了什么（种类+细节） | `kiana-domain/src/lib.rs:RuntimeEvent` |
| Receipt（回执） | 从事件算出来的总结：谁发起、改了哪些文件、用了哪些能力 | `kiana-core/src/lib.rs:receipt_from_events` |
| 幂等（idempotent） | 同一件事重复做，效果和做一次一样；这里指同一笔账重试不会记两次 | `kiana-eventlog/src/event_store_core.rs:plan_idempotent_append` |
| CAS（先核对再落笔） | 写之前核对"账本现在到第几笔"，对不上就拒绝，防止并发写乱 | `kiana-eventlog/src/event_store_core.rs:reject_expected_version` |
| aggregate（分册） | 账按主题分册归档：工作包一册、单次运行一册、单次请求一册 | `kiana-core/src/lib.rs:aggregate_for_event` |
| fail-closed（坏账拒收） | 账本中间行坏了就整个打不开，绝不肯"跳过坏行继续记" | `kiana-eventlog/src/jsonl.rs:load_jsonl` |
| 打码（redaction） | 写账前把密钥、口令类字段替换成 `[REDACTED]` | `kiana-core/src/lib.rs:redact_event_value` |
| EventStorePort | 规定记账/查账接口的契约，内存版和磁盘版都按它实现 | `kiana-ports/src/lib.rs:EventStorePort` |

## 设计视角：现在最明显的短板

- **运行时状态和事件账本是两回事。** 事实（事件）在磁盘上，但"会话对应哪次运行""哪个审批还在等"这些运行时状态只在内存的 HashMap 里（`kiana-core/src/lib.rs` 开头的 sessions/pending_invocations/cancellations 字段），进程一重启就清零——所以重启后查账必须自己报运行编号，"接着上次继续"没有支撑。
- **每次记账前整本重读。** 写入层每次动手前都把磁盘文件从头读一遍（`kiana-eventlog/src/jsonl.rs:reload_if_present`）来保证多进程一致，账本越大这一步越贵，没有任何索引或增量手段。
- **账本无界增长。** 只追加、不轮转、不归档，长期使用的 `events.jsonl` 会一直变大，而每次读又是全量读，两个短板会互相放大。
- **防并发不防篡改。** CAS 和幂等挡得住程序出错，挡不住人：账本是明文 JSON，没有校验和、签名或加密，任何能写磁盘的程序（包括运行中被记账的 AI 自己用 shell）都能改历史记录。
- **不是唯一权威账本。** CURRENT_STATUS.md 的原话是"当前按 request sequence；不是 aggregate/CAS durable authority"——CAS 骨架已经搭好并用测试钉住了，但"所有跨进程状态都以这本账为准"的产品定位还没落地，CURRENT_STATUS §4 里 P1-02（审批事件游标）、P1-04（账本写入的文件级防调包）、P1-05（密钥不进账）三个条目也都停留在 partial。

## 相关文档

- `docs/company-os-overview.md`：词条"公司流水账 EventLog / RuntimeEvent"（§车间与人物表）、"收据/台账 Receipt"；第 4 条设计原则"明确的记录"；"Receipt 存在 ≠ 现实结果正确"的红线提醒。
- `CURRENT_STATUS.md`：能力表（§2）中"JSONL EventLog / 基础 Receipt"一行（partial / local_behavior）；§4 中 P1-04（EventLog 写入文件边界证据，2026-09-07）、P1-05（密钥不进账/不进回执的回归）、P1-02（审批续跑的事件游标）三个条目；"跨进程完整 resume = deferred"一行。
- `docs/features/` 兄弟文档：02（工具调用与授权脊柱，事件记账发生在它的审批链路里）、05（审批，`approval.*` 事件出在那里）、09（run 生命周期，终点判定依赖本篇的账本）；全部篇目见 `docs/features/README.md` 索引。
