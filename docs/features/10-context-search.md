# 上下文与搜索（模型干活前先"看一遍"你的代码库）

> 一句话：这个功能负责在 AI 干活之前，把你的项目"翻译"成它能快速看懂的摘要和搜索结果，并且控制好喂给它的量，免得"消化不良"。
> 本文写的是代码现在的真实样子，不是产品愿景；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

AI 干活前不可能一口气读完整个项目，它需要两样东西：一份**项目结构概览**（有哪些文件夹、每个文件大概是干嘛的），和一套**本地搜索工具**（按关键词查代码）。这个功能就是干这个的：程序自己扫一遍你的项目，生成一份"项目地图"（代码里叫 repo map，仓库地图），再配一套本地搜索工具，让 AI 在动手改代码前能先摸清项目结构。

所有这些只发生在你自己的电脑上、只读你信任的项目文件夹——全程只读，不改任何文件。

## 现在能干什么 / 不能干什么

**能**（每条在代码里有出处）：

- 生成"仓库地图"：列出项目里的文本文件、每个文件的语言、大小和开头几个函数/类的名字，有用量上限（`kiana-query/src/repo_map.rs:build_repo_map`）。
- 关键词搜索：在整个项目里按词找代码，返回按打分排序的命中行（`kiana-query/src/index.rs:search_context_index`）。
- "向量搜索"：一种按"长得像不像"打分的模糊搜索，但它用的是本地的、固定的算法（名字里就叫 `kiana.deterministic-hash-embedding.v1`），不是联网的 AI 语义搜索（`kiana-query/src/index.rs:search_context_vectors`）。
- 上下文包（context pack）：把搜索命中的代码片段连同前后几行一起打包输出，还能带上片段之间的关系图（`kiana-query/src/index.rs:build_context_pack`）。
- artifact store（文件登记册）：给每个文件记录路径+内容指纹，还能猜出"哪个测试测哪个文件"这类关系（`kiana-query/src/index.rs:build_context_artifact_store`）。
- 资料入库（ingest）：把一个文件夹里的文本文件复制到项目内的 `.kiana/context-ingest/` 目录，并写一份清单（`kiana-query/src/index.rs:ingest_context_artifacts`）。
- 你可以在命令行直接用这些功能：`kiana context repo-map`、`kiana context search <关键词>`、`kiana context pack` 等一整套子命令（`kiana-commands/src/context.rs` 的帮助文本）。
- 所有 context 查询都走正式的审批链：控制面先校验参数、限制上限（比如最多返回 1000 条、单文件最多 16MB），标记为"只读"风险级别（`kiana-core/src/lib.rs:normalize_context_query_arguments`）。
- 搜索范围被关在项目内部：想搜项目外面的路径、或者用 `..` 往上跳，都会被直接拒绝（`kiana-daemon/src/context_query.rs:confined_context_root`）。
- 对话开始时和收到你消息时，可以由"钩子"（hook，就是你预先配置的小脚本）往对话里塞一段背景资料，比如项目说明（`kiana-query/src/stop_hooks.rs:run_session_start_hooks` 和 `run_user_prompt_submit_hooks`）。
- 每个结果都有统一格式（JSON 或人能读的文本），并且每次执行都会留下回执（receipt，可理解为"办过这件事的盖章小票"）。

**不能**（每条来自 CURRENT_STATUS.md 的明确记录或代码里的明确拒绝路径）：

- 模型**没有**专门的"搜索工具"或"读文件工具"。模型能看到的工具只有五个：shell（执行命令）、apply_patch（改文件）、mcp（外部工具）、memory.search、memory.write。测试明确锁死了这个清单（`kiana-runner/src/tools.rs:tool_schemas`，测试 `tool_schema_exposes_exactly_the_five_fixed_names`）。所以模型搜代码，实际是让 shell 去跑 `rg`（ripgrep，一个常见的命令行搜索软件）等命令——这是有意设计，不是遗漏（docs/coding-pack-matrix.md §0.4 把这条"搜索策略"锁死了）。
- 仓库地图**不会**自动塞进模型的提示词里。代码里没有找到"每次干活前自动生成 repo map 喂给模型"的调用；模型自己想了解项目，靠 shell 跑命令，或者靠你配置的钩子脚本塞背景资料。（未在代码里确认任何自动注入路径）
- 向量搜索不是真的"懂意思"。它是本地固定算法算相似度，模块注释明说"分数是检索提示，不是语义正确性的证明"（`kiana-query/src/index.rs` 文件头注释）。代码在、局部测过，还没当真产品验证过——对应 CURRENT_STATUS.md 的 partial + local_behavior 状态。
- token 预算是"提醒"不是"硬闸"。超过预算时程序给模型发一条"快用完了，抓紧收尾"的消息，而不是强行掐断（`kiana-query/src/token_budget.rs` 模块注释明说"不负责限制模型实际消耗的硬配额"）。
- token 预算这个模块自己也承认：并不是所有入口都接上了这套统计（`kiana-query/src/token_budget.rs` 头注释："阈值是当前实现事实，不代表所有入口都已经接入该 reducer"）。
- 文件超过 128KB 就直接跳过，不进索引也不进搜索结果（`kiana-query/src/index.rs:index_file`，默认上限 `DEFAULT_MAX_BYTES_PER_FILE`）。
- `.gitignore` 只支持简化版：以 `!` 开头的"排除排除"规则会被忽略，通配符只认 `*`（`kiana-query/src/repo_map.rs:IgnoreRules::load` 注释明说"不应被描述成与 Git 完全等价"）。

## 代码怎么跑（走读）

### 场景一：你自己敲 `kiana context repo-map` 看项目地图

1. **你敲命令。** 命令先到 `kiana-commands/src/context.rs` 的 `ContextCommand`，它负责认出你想要哪个功能（repo-map？search？pack？）、检查参数写没写对。写错了它直接打印用法提示，不会往下走。

2. **请求被装进信封发给后台。** 命令通过客户端把一个叫 `context.query.v1` 的请求发给 DaemonHost。这一步的存在意义是：所有操作都必须走同一个入口，不能有旁路。

3. **控制面先做校验。** `kiana-core/src/lib.rs:normalize_context_query_arguments` 负责这一步。它核对：操作名是不是认识的那十几个之一、参数是不是多一个少一个都不行、数量有没有超上限（最多 1000 条结果、单文件最多 16MB 等）。同时它会确认你的项目是受信任的（trust，见 04 篇）。全部通过后，这个请求被标成"只读风险"并授权。

4. **broker 把请求派给对应的 handler。** 请求进入 capability broker（按"（能力类型，操作名）→ handler"的精确注册表派发）。`kiana-daemon/src/context_query.rs:register` 在启动时就登记好了：`context.repo_map` 由 `RepoMapHandler` 处理，搜索、打包、入库各有各的 handler。

5. **真正扫盘的是 `kiana-query`。** `RepoMapHandler` 把扫盘这个慢活丢到后台线程（`spawn_blocking`，免得卡住其他事），然后调 `kiana-query/src/repo_map.rs:build_repo_map`。它做四件事：
   - 把你给的路径 canonicalize（把 `./xx/../yy` 这种绕弯路径解析成真实路径），确定从哪里开始扫；
   - 加载忽略规则：默认跳过 `target`、`node_modules`、`.git` 这类生成产物目录，再叠加你项目里 `.gitignore` 写的规则；
   - 递归收齐所有"认识的"文件（按扩展名判断，认识 24 种语言，不认识的直接跳过），按路径排序保证每次结果顺序一致；
   - 逐个文件读内容，跳过二进制和乱码文件，记下语言、大小，并从代码行里提取名字——比如 Rust 文件里看到 `fn beta()` 就记下 "fn beta"。每个文件最多提取 12 个名字，这是简单的文本匹配，不是真正的语法分析。

6. **预算裁剪。** 每个文件条目按"字符数除以 4"粗略折算成 token（模型处理文本的计量单位）。默认总预算 4000 token。从头往后一个个装，装不下的文件**整个跳过**（不会只截半个），并在结果里如实报告"省略了几个文件"（`truncated` 和 `omitted_files` 字段）。

7. **结果打包送回。** 排版成你能读的文本（或 JSON），作为执行结果原路送回给你，同时记进事件账本（EventLog），日后能查这次读过什么。

### 场景二：模型自己去搜代码（这是日常干活时的真实路径）

1. 你对模型说："帮我找一下哪里处理退款。"
2. 模型**没有**搜索按钮。它发出一个 shell 工具调用，内容大概是 `rg 退款` 或 `rg refund`。
3. 这个调用照样走上面的校验链：控制面评估（只读命令通常风险低）、按沙箱（sandbox）规则执行。
4. 命令结果回到模型的对话里，模型再决定下一步读哪个文件。
5. 为什么要这样设计？工具越少，需要审批的口子越少，行为越可控。搜索能力 = shell + 授权，不需要再造一个新工具（这个决策记录在 docs/coding-pack-matrix.md §0.4）。

### 最重要的分支：出错时怎么走

- **想搜到项目外面？** `kiana-daemon/src/context_query.rs:confined_context_root` 里，绝对路径、带 `..` 的路径、解析后跑出项目根的路径，全部当场拒绝，错误信息直说 `context_root_outside_project`。不存在"帮个忙通融一下"。
- **缓存文件坏了？** 上次生成的索引缓存如果被改坏（不是合法 JSON），程序不会崩也不会拿坏数据糊弄，而是标记状态为 `recovered`（已恢复），重新完整扫一遍，这次结果照常写入新缓存（`kiana-query/src/index.rs:build_persistent_context_index`，对应测试 `persistent_context_index_recovers_from_corrupt_cache`）。
- **ingest 中途失败？** `ingest_context_artifacts` 的注释明说：失败时可能已经清掉或复制了一部分文件，**不能**把"函数报错"理解成"源文件夹没动过"。这是一个诚实的、留了警告标记的已知粗糙点。

## 关键概念速查

| 概念 | 白话解释 | 代码在哪 |
|---|---|---|
| repo map（仓库地图） | 项目里有哪些文本文件的清单：路径、语言、大小、每个文件开头的几个函数名，总量有预算上限 | `kiana-query/src/repo_map.rs:build_repo_map` |
| 上下文索引（context index） | 更细一层：每个文件的路径、语言、行数、内容指纹（hash，内容一变指纹就变） | `kiana-query/src/index.rs:build_context_index` |
| 关键词搜索 | 把你的查询拆成词，全项目逐文件逐词比对打分，出现越多、文件名越沾边分越高 | `kiana-query/src/index.rs:search_context_index` |
| 向量搜索 | 本地固定算法的相似度匹配：不是联网 AI，只能当检索参考 | `kiana-query/src/index.rs:search_context_vectors` |
| context pack（上下文包） | 搜索命中的代码片段 + 前后几行原文 + 片段关系图，打包输出给上层用 | `kiana-query/src/index.rs:build_context_pack` |
| artifact store（文件登记册） | 给每个文件记录路径+内容指纹，再按文件名和内容猜"谁测试谁、谁提到谁" | `kiana-query/src/index.rs:build_context_artifact_store` |
| ingest（资料入库） | 把另一个文件夹的文本文件复制进项目内的 `.kiana/context-ingest/`，写清单，支持对比上次多了/少了什么 | `kiana-query/src/index.rs:ingest_context_artifacts` |
| token 预算 | 程序估算每次上下文消耗的 token，超预算就提醒模型抓紧收尾 | `kiana-query/src/token_budget.rs:check_token_budget` |
| 钩子（hook） | 你预先配置的小脚本，在特定时机（如对话开始、收到消息）自动执行，可以往对话里塞背景资料 | `kiana-query/src/stop_hooks.rs:run_session_start_hooks` |
| 五工具面 | 模型能看到的全部工具就五个：shell、apply_patch、mcp、memory.search、memory.write，搜索靠 shell 跑 rg | `kiana-runner/src/tools.rs:tool_schemas` |
| 只读风险级别 | 这套功能全部只看不改，校验时标为只读风险，审批比写文件宽松 | `kiana-core/src/lib.rs:normalize_context_query_arguments` |
| 信任（trust） | 对项目文件夹的信任声明；未信任的项目，上下文功能不会放行 | `kiana-core/src/lib.rs`（控制面校验） |

## 设计视角：现在最明显的短板

- **没有"搜完就记住"的索引**：每次搜索都是从头把项目扫一遍、把文件重新读一遍。那份"持久化缓存"只存文件清单和指纹，真正搜索时不省任何读盘的功夫——项目一大，每次搜都慢。
- **仓库地图的预算裁剪是"整文件丢弃"**：超预算的文件直接整个省略，不会只留它的函数名列表，所以大文件多的项目，地图里可能恰恰看不到最核心的那些文件。
- **向量搜索名不副实**：本地 hash 算法只是"文字长得像"的粗略比较，搜"支付"不一定能找到只写"结算"的代码。代码注释自己都承认这只是候选提示。
- **依赖关系靠猜**：判断"哪个测试测哪个源文件"靠文件命名约定，判断"谁引用谁"靠"文件内容里出现对方的路径字符串"。缺一条线不等于真没关系，多一条线也不等于真有关系，注释里写得很明白。
- **模型与这套上下文系统基本是"两条平行线"**：这套搜索/打包功能目前主要是给你（人）在命令行用的；模型日常干活搜代码走的是 shell 跑 rg，repo map 和 context pack 并没有接入"干活前自动喂给模型"的流程。（未在代码里确认任何自动接入点）

## 相关文档

- docs/company-os-overview.md：相关词条见"刻意收窄工具面"（模型只见五个工具、搜索靠 shell 跑 rg 的设计决策）、"CapabilityDescriptor"（能力说明书）、"Tool Search"（工具搜索，注意"搜到 ≠ 有权用"）。
- docs/coding-pack-matrix.md：§0.4 与 §1 的"P0 搜索策略"——搜索能力 = shell + 授权、不新增工具面、P1-READ（专门的读取/搜索工具）已明确跳过，这是本设计的出处。
- CURRENT_STATUS.md：§2"当前产品主路径"（本文走读的链条就是它）；§4"高优先级未完成项"表中没有 context/搜索的专项条目——说明这个领域不在当前最高优先级修复清单里，整体状态按"代码在、局部测过、未当真产品验证"理解（即 partial + local_behavior 的通用含义）。
- docs/features/ 兄弟文档：02（模型能看到的五个工具——模型搜代码为什么只能靠 shell）、04（trust 与沙箱，context 查询的信任校验出处）、01（模型循环）；全部篇目见 `docs/features/README.md` 索引。
