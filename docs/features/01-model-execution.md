# 模型执行与 provider 选择（AI 的"大脑"从哪来）

> 一句话：这个功能决定 Kiana 干活时"谁在动脑子"——是调用真实的外部模型 provider，还是用事先写好的 cassette 脚本（预先记录"模型会怎么回答"的测试用脚本文件）。
> 本文写的是代码现在的真实样子，不是产品愿景；"做到什么程度"以 CURRENT_STATUS.md 为准。

## 这个功能是干什么的

Kiana 自己不会"思考"。真正想问题、决定"下一步该跑什么命令、改哪个文件"的，是外部的模型 provider（比如 Anthropic 提供的在线模型服务）。Kiana 自己的角色更像一辆车的**传动系统**：它决定"这次用哪个模型"、把你的问题打包发过去、把答复接回来、再根据答复继续干活。

这里有两个来路：

1. **cassette / fake-script**（环境变量 `KIANA_HARNESS_SCRIPT`）：一份预先写好的脚本，里面记录了"模型会怎么回答"。Kiana 每次"问模型"，实际是按顺序取出脚本里的下一段回答。它的优先级永远最高——只要这个环境变量在，真实 provider 连构造都不会被构造。这是为了测试：结果永远一样、不花钱、不需要联网。
2. **真实 provider**：通过环境变量 `KIANA_PROVIDER`（或配置）指定用哪家——anthropic、openai-compatible（任何"和 OpenAI 接口长得一样"的服务）、ollama（跑在你自己电脑上的本地模型）、fake（另一种内置假模型）。**anthropic 和 openai-compatible 没配 API key（provider 颁发的访问凭证）时，Kiana 会明确报错拒绝干活，绝不假装成功。**

## 现在能干什么 / 不能干什么

**能**（每条都有代码出处）：

- 设了 `KIANA_HARNESS_SCRIPT` 就无条件用 cassette 脚本，优先级最高（`kiana-daemon/src/model_client.rs` 的 `from_config`）。
- 用 `KIANA_PROVIDER` 在 anthropic / openai-compatible / ollama / fake 四家里选一个；什么都不填默认 anthropic（`model_client.rs` 的 `provider_from_env`）。
- anthropic 和 openai-compatible 必须配了 API key 才会真正构造出来；key 来自配置或环境变量 `ANTHROPIC_API_KEY` / `KIANA_OPENAI_API_KEY`、`OPENAI_API_KEY`（`provider.rs` 的 `built_in_provider_registry` + `model_client.rs` 的 `provider_from_env`）。
- ollama 和 fake 不需要 key（本地服务/测试用）（`provider.rs` 的 `built_in_provider_registry`）。
- 没配 API key、或 cassette 文件读不到/内容坏了时，装上占位实现 `UnavailableModel`，跑起来就报 `model_unavailable` 或具体错误——不空转、不伪造成功（`kiana-runner/src/model.rs` 的 `UnavailableModel`）。
- 有 key 时真的会发 HTTP 请求：非流式（一次性拿整段回答）、60 秒超时、单次回答上限 4096 个 token（`model_client.rs` 的 `ProviderModelClient::complete`，token 大白话就是模型计费的"字数"）。
- 系统提示词（system prompt）可以用 `KIANA_SYSTEM_PROMPT` 整个换掉，或用 `KIANA_APPEND_SYSTEM_PROMPT` 往后面追加（`model_client.rs` 的 `provider_system_prompt`）。
- fake provider 支持另一种脚本：环境变量 `KIANA_FAKE_PROVIDER_SCRIPT`，能按步骤回"先调工具、再给最终答案"，甚至能模拟 provider 报错（`provider.rs` 的 `FakeProvider::from_script_value`）。
- 模型名字可以配置：命令行选项 > 该 provider 的环境变量（如 `ANTHROPIC_MODEL`）> 各家默认值（claude-sonnet-4-6 / gpt-4.1 / llama3.1）（`provider.rs` 的 `built_in_provider_registry`）。
- 填了一个不存在的 provider 名字，会明确报 `unknown_provider:xxx`（`model_client.rs` 的 `provider_from_env`）。

**不能**（出处是 CURRENT_STATUS.md 的明确记录或代码里的明确拒绝路径）：

- **接入真实在线模型这件事，整个仓库的结论是"代码在、没验证"**：CURRENT_STATUS.md §2 能力表写着 "live provider | not_supported | source"——白话是：调真实 provider 的代码确实写好了，但从来没有绑定真实服务的证据（从未用它对真服务跑通并留下记录），所以不许对外宣称"能接真模型"。
- 不提供逐字输出（token streaming，就是打字机一样一个字一个字往外蹦）：CURRENT_STATUS.md 标 `not_supported`；主循环发请求时明确写 `stream: false`（`model_client.rs` 的 `ProviderModelClient::complete`）。
- 没配 cassette 脚本也没配 API key 时跑 `kiana run` / `kiana`：显式失败报 `model_unavailable`，不会"聊两句废话假装完成"（`model.rs` 的 `UnavailableModel` + `harness.rs` 测试 `missing_model_fails_closed`）。
- 一次模型调用失败（断网、超时、key 错）没有任何重试：这个回合直接以失败结束（`harness.rs` 的 `model_step`，错误即 `RunnerEvent::Failed`）。
- openai-compatible 和 ollama 只在本地假服务器（测试里自己起的小 HTTP 服务器）上验证过请求格式，没有对真实服务验证过（`provider.rs` 文件末尾的测试）。

## 代码怎么跑（走读）

以你敲 `kiana run --sandbox workspace-write -- "帮我建个文件"` 为例。

**第 1 步：把模型相关的偏好收集起来。**
命令处理过程走到 `kiana-entrypoints/src/harness_run.rs` 的 `new_local_host_with_options`。它从你的命令选项里挑出四项：provider（用哪家）、api_key、base_url（provider 的接口地址，可以填别的地址来代理）、model（模型名）。四项都可以是空的。它们装进一个叫 `LocalModelConfig` 的结构体（定义在 `kiana-daemon/src/lib.rs`，就是"本次运行的模型偏好"）。

**第 2 步：交给模型构造入口。**
`DaemonHost::local_with_model_config`（`kiana-daemon/src/lib.rs`）拿着这个小盒子，调用 `kiana-daemon/src/model_client.rs` 的 `from_config`。这里是整个功能的岔路口。

**第 3 步：先看有没有 cassette 脚本——它的优先级永远最高。**
`from_config` 第一件事就是看环境变量 `KIANA_HARNESS_SCRIPT` 有没有写一个文件路径。写了的话，直接装 `ScriptedModel`（`kiana-runner/src/model.rs`）——它内部是一个回答队列，每次有人来"问模型"，就弹出队列里的下一段回答，用完为止。后面四家真实 provider 一律不看。为什么这么绝对？因为 cassette 是测试的基础：测试要求"不管外面网络怎么样，模型的表现永远一模一样"，所以只要脚本在，就必须盖住一切。脚本文件读不到或内容不是合法 JSON，会分别报 `harness_script_unavailable` / `harness_script_invalid`，并整体退化为 `UnavailableModel`——不会"播一半凑合继续"。

**第 4 步：没有 cassette，就去构造真实 provider。**
`provider_from_env`（`model_client.rs`）按顺序定下 provider 名字：命令选项里的 provider > 环境变量 `KIANA_PROVIDER` > 默认 anthropic（都转小写）。然后去 provider 注册表（`kiana-services/src/api/provider.rs` 的 `built_in_provider_registry`，一张写死在代码里的表，记录每家的名字、要什么环境变量、默认模型、默认网址）查这名字。查不到 → 报 `unknown_provider:xxx`，后面一步都不会走。

**第 5 步：检查 API key——没有就退回 `UnavailableModel`。**
anthropic 和 openai-compatible 这两家在线服务，必须找到 API key（配置里的，或 `ANTHROPIC_API_KEY` 等），**找不到就直接返回"没有可用模型"**，而不是带着空 key 硬上。第 2 步的构造入口收到"没有可用模型"后，装上 `UnavailableModel`（`model.rs`），它的行为是：不管你问什么，一律回固定错误 `model_unavailable`。这样错误会一路传到你的屏幕上——这就是"没配模型跑 `kiana run` 会明确报错"的来源，也是整个项目"拒绝假成功"原则的日常体现。ollama 和 fake 是本地的，不需要 key，直接通过。

**第 6 步：把真实 provider 包进统一接口。**
找到 key 后，会造出对应的 provider 适配器（`provider.rs` 里的 `AnthropicProvider` / `OpenAiCompatibleProvider` / `OllamaProvider`——适配器负责把 Kiana 的统一请求格式转换成各家的 API 格式），再包进 `ProviderModelClient`（`model_client.rs`）。这个包裹实现了 `ModelClient` 接口（`model.rs` 的 trait `ModelClient`）：主循环只依赖这个接口，不关心背后是真实 provider、cassette 还是 `UnavailableModel`。这个设计让"换成 cassette 测试"或"将来换 provider"都不用动主循环一行代码。

**第 7 步：主循环转起来——"问模型 → 它想调工具 → 执行 → 把结果塞回去 → 再问"。**
真正的循环在 `kiana-runner/src/harness.rs` 的 `model_step`，它像这样转圈：

1. 把到目前为止的整段对话（你的话、模型之前说过的话、工具干过的活）加上五个工具的 schema（shell、apply_patch、mcp、memory.search、memory.write），打包成一个 `ModelRequest`，调 `complete`。
2. `complete` 失败（断网/超时/cassette 播完了）→ 这个回合直接以 `RunnerEvent::Failed` 收场，不重试。
3. 成功 → 模型的文字先记下来发给外面（`RunnerEvent::Delta`），然后看它有没有"我要调工具"的声明。有 → 这些调用逐个变成正式申请 `CapabilityRequest`（"能力"是 Kiana 里对一项受控操作的叫法）排队，**一次只发一个**出去审批执行；执行结果回来（`on_capability_result`）→ 把结果作为一条 tool 消息塞回对话 → 排队里还有就发下一个，没有就回到第 1 步再问模型。
4. 模型没说调任何工具、也没有等注入的话 → 这单活干完了，发 `RunnerEvent::Completed`，结束。

注意一个安全细节：模型说"我要跑 `ls`"只是一句**声明**。第 3 步里它变成 CapabilityRequest 去走审批，真正执行在别处（capability broker）——模型从头到尾碰不到你的电脑。

**第 8 步：真发请求时长什么样。**
`ProviderModelClient::complete`（`model_client.rs`）做翻译：内部消息格式 → Anthropic 的 Messages API 格式；系统提示词来自 `KIANA_SYSTEM_PROMPT`（没设就用默认值）；`max_tokens` 写死 4096；`stream: false`（一次性等整段回答）；超时 60 秒。然后 `AnthropicProvider`（`provider.rs`）→ `AnthropicClient::create_message`（`kiana-services/src/api/client.rs` + `messages.rs`）→ 向 `https://api.anthropic.com/v1/messages` 发一个 HTTP POST，头上带你的 key。选了 openai-compatible 就翻译成 `/chat/completions` 的格式，ollama 就翻译成 `/api/chat` 的格式（`provider.rs` 里各自的函数）。

**失败时怎么走（最重要的分支）：**
provider 回了错误（比如 401 key 不对、限流、超时），会被翻译成带错误码的统一错误（`provider.rs` 的 `ProviderError`），再被 `model_client.rs` 的 `map_provider_complete_error` 压成一行字符串，传回主循环变成 `RunnerEvent::Failed`——你会在输出里直接看到失败原因，没有任何"悄悄换一家再试试"的逻辑。cassette 播完了报 `harness_script_exhausted`，脚本内容坏报 `harness_script_invalid`，方便你区分"脚本用完了"和"脚本本身写错了"。

## 关键概念速查

| 概念 | 白话解释 | 代码在哪 |
|---|---|---|
| cassette（cassette / fake-script） | 预先写好的"模型回答"脚本文件，用 `KIANA_HARNESS_SCRIPT` 指定；永远优先于真实 provider | `kiana-runner/src/model.rs` 的 `ScriptedModel`、`kiana-daemon/src/model_client.rs` 的 `from_config` |
| `KIANA_PROVIDER` | 环境变量里写 provider 名字（anthropic / openai-compatible / ollama / fake），不写默认 anthropic | `kiana-daemon/src/model_client.rs` 的 `provider_from_env` |
| provider 注册表（built_in_provider_registry） | 代码里写死的一张表：每家叫什么、要哪些环境变量、默认模型和网址 | `kiana-services/src/api/provider.rs` 的 `built_in_provider_registry` |
| API key | provider 颁发的访问凭证，没有它在线服务会拒绝请求 | 环境变量 `ANTHROPIC_API_KEY` 等，见注册表 |
| `UnavailableModel` | 没配模型时顶上去的占位实现，问什么都回 `model_unavailable` | `kiana-runner/src/model.rs` 的 `UnavailableModel` |
| `ModelClient` | 主循环唯一依赖的接口："把对话给我，还我一段模型回答"；cassette、真实 provider、`UnavailableModel` 都实现它 | `kiana-runner/src/model.rs` 的 trait `ModelClient` |
| provider 适配器 | 把 Kiana 的统一请求格式转换成某家的 API 格式（Anthropic / OpenAI / Ollama 各一个） | `kiana-services/src/api/provider.rs` 的 `AnthropicProvider` 等 |
| 非流式 | 一次请求、等整段回答一起回来；不是打字机式逐字输出 | `model_client.rs` 的 `ProviderModelClient::complete`（`stream: Some(false)`） |
| `model_unavailable` | "没有可用模型"的固定报错；没配 cassette 也没配 API key 时的标准结局 | `model.rs` 的 `UnavailableModel::default` |
| `harness_script_exhausted` | cassette 播完了（脚本里的回答用光了） | `model.rs` 的 `ScriptedModel::complete` |
| CapabilityRequest | 模型说"我想调工具"后生成的正式申请，要走审批才执行（详见审批篇 05） | `kiana-runner/src/harness.rs` 的 `model_step` / `emit_tool_request` |

## 设计视角：现在最明显的短板

1. **一次失败就全盘结束，没有重试。** 主循环里模型调用出错直接 `RunnerEvent::Failed`（`harness.rs` 的 `model_step`），网络抖一下、限流碰一下，整单活就停了。
2. **回答只能整段等。** `stream: false` 加 60 秒超时（`model_client.rs`）：模型想得久一点就可能超时，你也看不到逐字输出；而且 Anthropic 客户端自己还有个默认 600 秒超时的旧路径（`client.rs` 的 `with_base_url`），两条超时规则并存，容易混淆。
3. **单次回答上限 4096 写死在代码里。** `DEFAULT_MAX_TOKENS` 是常量（`model_client.rs`），温度（temperature，控制回答随机度）也没设；想调只能改代码。
4. **模型配置每次启动重新决定，没有存档。** provider / API key / 模型名来自环境变量和命令选项，进程一退就忘；没有"配置文件里记着我常用哪家"这回事。key 靠环境变量保存，也容易漏配。
5. **翻译过程有损耗。** 内部消息翻译成 Anthropic 格式时，"系统消息"被改写成带 `[system]` 前缀的用户消息（`model_client.rs` 的 `map_messages`）；provider 返回的 token 用量统计在 `output_from_content` 里被直接扔掉，上层完全不知道这次消耗了多少 token。
6. **"能接真模型"这件事从未被证明。** 代码里 HTTP 客户端、错误分类、格式翻译都真实存在，测试用本地假服务器验证过请求长相（`provider.rs` 末尾的测试），但对真实 provider 跑通的记录一条都没有——CURRENT_STATUS.md 把 live provider 钉在 `not_supported`，这和"代码在"是两回事。

## 相关文档

- `docs/company-os-overview.md`：§2"现在真的能做什么（诚实版）"（"传动系统装好了、发动机还没接的车"的比喻出处，以及 `model_unavailable` 的预期管理）；§4 走读里第 ④ 步模型循环；§5 术语表里的 "cassette / fake-script" 词条。
- `CURRENT_STATUS.md`：§1 状态与证明等级（"代码存在不等于已实现"）；§2 能力表的 "live provider | not_supported | source" 和 "token streaming | not_supported | source"。
- `docs/features/` 兄弟文档：02（工具调用与授权脊柱，第 7 步主循环发出的 CapabilityRequest 在那里走完审批和执行）、05（审批）、09（run 生命周期）；全部篇目见 `docs/features/README.md` 索引。
