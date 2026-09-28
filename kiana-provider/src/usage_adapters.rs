//! Provider wire usage adapters for the neutral billing contract.
//!
//! The response decoders own protocol syntax; this module owns the narrower accounting
//! boundary.  It copies only bounded numeric usage and the provider's served-model observation
//! into [`kiana_domain::NormalizedUsage`].  Requested model, attempt, run and route identity stay
//! server-owned by the admitted [`PreparedModelCall`].  Missing fields remain unknown and are
//! never filled with zero.
//!
//! ## 这个文件在系统里的位置
//!
//! ```text
//!   上游服务商回包（各家 usage 字段名都不一样）
//!        |
//!        v
//!   response.rs 的解码器 —— 负责“协议语法”（SSE/NDJSON 怎么切、JSON 怎么解）
//!        |
//!        v
//!   【本文件 usage_adapters.rs】—— 负责“会计口径”（哪个字段算输入、哪个算输出）
//!        |
//!        v
//!   NormalizedUsage -> ControlPlane -> 成本/预算账本 -> EventLog
//! ```
//!
//! ## 与 response.rs 的分工（初学者最容易搞混）
//!
//! - `response.rs` 回答：**这段字节是什么意思？**（切帧、解析、拼成 `ModelReply`）
//! - 本文件回答：**这些数字分别是什么意思？**（`prompt_tokens` 是输入、
//!   `completion_tokens` 是输出、`prompt_eval_count` 也是输入……）
//!
//! 两者都要处理同一条回包，但关注点不同，所以拆成两个文件。
//!
//! ## 与 usage.rs 的关系
//!
//! `usage.rs::normalize_model_reply` 是**旧路径**（只认 `ModelReply` 里已解析好的
//! `usage` 结构体）；本文件是**更细的路径**（直接从原始 JSON `Value` 里按协议取字段，
//! 支持嵌套的 cache / reasoning / audio 维度）。
//! `rg -n "normalize_provider_usage"` 显示本文件版本目前只在
//! `kiana-core/tests/bq10_normalized_usage_guard.rs` 里被断言存在，
//! 没有生产调用点。两个函数并存是当前状态，不要假设只有一条路径。
//!
//! ## 本文件的第一原则
//!
//! **缺失就是 Unknown，永远不是 0。**
//! `optional_u64` 遇到字段不存在返回 `Ok(None)`；下游据此把 confidence 标成
//! `Partial` 或 `Unknown`。如果这里图省事填 0，成本账本会**静默少算**且无法察觉。

use kiana_domain::{
    json_digest, AttemptId, BillingUnknownReason, ModelProtocol, NormalizedUsage,
    PreparedModelCall, RunId, UsageConfidence, UsageId, UsageObservation, UsageSource, UsageVector,
};
use serde_json::{Map, Value};

/// 用量归一化结果的形状标识，会被拼进 `NormalizedUsage.basis` 字段。
///
/// 消费者靠这个字符串知道“这些数字是由哪一版适配规则、从哪个协议方言抽出来的”。

pub const PROVIDER_USAGE_ADAPTER_SCHEMA: &str = "kiana.provider-usage-adapter.v1";
/// 单个 token 计数字段的上限：10 亿。
///
/// 【为什么需要上限】 这些数字来自**外部服务商的回包**，是不可信输入。
/// 如果对端返回 `prompt_tokens: 18446744073709551615`，直接存进账本会让后续所有
/// 求和运算溢出。
///
/// 【为什么是 10 亿】 单次调用的 token 数在现实里最大也就百万级（长上下文）。
/// 10 亿已经比任何合理值大好几个数量级，留足了未来余量，同时又远小于 `u64::MAX`，
/// 保证几项相加也不会溢出。
///
/// 【⚠ 超限怎么处理】 返回错误 `provider_usage_<字段>_invalid`，
/// 也就是**拒绝这条用量数据**，而不是截断成一个看似合理的数。

pub const MAX_PROVIDER_USAGE_TOKENS: u64 = 1_000_000_000;

/// 统一生成错误码：`provider_usage_<字段>_invalid`。
///
/// 【为什么统一在这里生成】
/// 错误码会被上层写进事件和断言。如果散落在各处手写字符串，
/// 迟早会出现 `usage_input_tokens_invalid` 和 `provider_usage_input_tokens_invalid` 两种写法，
/// 下游按字符串匹配就会漏。

fn invalid(field: &str) -> String {
    format!("provider_usage_{field}_invalid")
}

/// 校验一个**有界的短文本字段**（目前只有 `served_model` 走这里）。
///
/// 【检查三件事】
/// 1. 不许是纯空白 —— 空白字符串当模型名毫无意义。
/// 2. 长度不超上限 —— 防止对端回一个 10 MB 的“模型名”撑爆内存。
/// 3. 不含 NUL / CR / LF —— 这些字符会破坏日志的行结构，也可能被下游当成控制指令。
///
/// 【为什么模型名也要过白名单校验】
/// 因为 `served_model_id` 最终会被写进账本和事件。一个来自外部的、未经校验的自由文本
/// 落进持久化记录，是典型的“持久化注入”入口。

fn bounded_text(value: &str, field: &str, max: usize) -> Result<String, String> {
    if value.trim().is_empty()
        || value.len() > max
        || value.bytes().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(invalid(field));
    }
    Ok(value.to_owned())
}

/// 把一个可选的 JSON 字段读成 `Option<u64>`，并强制上限校验。
///
/// 【三种输入，三种结果】
/// ```text
///   None / JSON null  ->  Ok(None)     「对方没报这个维度」
///   合法非负整数      ->  Ok(Some(n))
///   负数 / 浮点 / 字符串 / 超上限 ->  Err
/// ```
///
/// 【⚠ 为什么 `Value::Null` 等同于 `None`】
/// JSON 里 `"x": null` 和不写 `"x"` 在语义上都是“没有值”。
/// 很多上游会显式回 `null` 而不是省略字段，两种都要归一到 `None`。
///
/// 【⚠ 为什么类型不对就报错而不是跳过】
/// 静默跳过一个类型错误的字段，会让“数字对不上”的问题被掩盖成“对端没报 usage”，
/// 最后变成一条 `Unknown` 观测——看起来很安全，实际是把错误藏起来了。

fn optional_u64(value: Option<&Value>, field: &str) -> Result<Option<u64>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let number = value.as_u64().ok_or_else(|| invalid(field))?;
            if number > MAX_PROVIDER_USAGE_TOKENS {
                return Err(invalid(field));
            }
            Ok(Some(number))
        }
    }
}

/// 把 JSON 值收敛成“一个对象，或者什么都没有”。
///
/// 【为什么 `Object` 之外的一切都算错误】
/// `usage` 字段如果是个数组或字符串，说明上游协议变了或对端在乱发数据。
/// 继续往下取字段会 panic 或产出垃圾，所以这里直接失败。

fn object<'a>(value: &'a Value, field: &str) -> Result<Option<&'a Map<String, Value>>, String> {
    match value {
        Value::Null => Ok(None),
        Value::Object(object) => Ok(Some(object)),
        _ => Err(invalid(field)),
    }
}

/// 读取**嵌套**一层的数字字段，例如 `usage.prompt_tokens_details.cached_tokens`。
///
/// 【为什么需要单独的函数】
/// OpenAI 系的 cache / reasoning 字段比 Anthropic 系多一层嵌套。
/// 这个函数把“取对象 -> 取内层键 -> 按上限校验”三步封起来，
/// 让主 match 里的每个分支都只关心字段名，不用重复写容错样板。
///
/// 【逐层短路】 任何一层缺失都返回 `Ok(None)`（“没有这个维度”），而不是错误。
/// 因为不同模型的回包里这些嵌套字段本来就常常整块缺失。

fn nested_u64(
    value: Option<&Map<String, Value>>,
    field: &str,
    nested: &str,
) -> Result<Option<u64>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let Some(value) = value.get(nested) else {
        return Ok(None);
    };
    optional_u64(Some(value), field)
}

/// 提取“对端声称自己实际使用的模型”。
///
/// 【为什么要三家名字都试一遍】
/// ```text
///   Anthropic / OpenAI-Responses -> "model"
///   Ollama                      -> "model"
///   Gemini (camelCase JSON)     -> "modelVersion"
///   某些兼容端点                -> "model_version"（snake_case）
/// ```
/// 返回 `None` 表示“对方没说”。这与“说了就是我们请求的那个”完全是两回事，
/// 所以这个字段被单独存成 `served_model_id`，不与 `model_id` 合并。

fn served_model(response: &Map<String, Value>) -> Result<Option<String>, String> {
    let value = response
        .get("model")
        .or_else(|| response.get("model_version"))
        .or_else(|| response.get("modelVersion"));
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => bounded_text(
            value.as_str().ok_or_else(|| invalid("served_model"))?,
            "served_model",
            256,
        )
        .map(Some),
    }
}

/// 定位“用量对象”在回包里的位置——**各协议完全不同**。
///
/// ```text
///   OllamaChat            -> 就在响应根对象上（prompt_eval_count / eval_count 直接在顶层）
///   GeminiInteractions    -> "usageMetadata" 或 "usage_metadata"（两种大小写都见过）
///   Anthropic / OpenAI    -> "usage"
///   Legacy                -> "usage"（虽然 Legacy 协议根本不走网络，这里保持穷举）
/// ```
///
/// 【⚠ 这个 match 是穷举式的】
/// 新增一个 `ModelProtocol` 变体时，编译器会在这里报错强制补分支。
/// 这正是我们要的——漏掉一个协议的后果是“静默地把所有 token 记成 Unknown”。

fn usage_object<'a>(
    response: &'a Map<String, Value>,
    protocol: ModelProtocol,
) -> Result<Option<&'a Map<String, Value>>, String> {
    match protocol {
        ModelProtocol::OllamaChat => Ok(Some(response)),
        ModelProtocol::GeminiInteractions => {
            let usage = response
                .get("usage_metadata")
                .or_else(|| response.get("usageMetadata"));
            usage
                .map(|value| object(value, "usage"))
                .transpose()?
                .flatten()
                .map(Ok)
                .transpose()
        }
        ModelProtocol::AnthropicMessages
        | ModelProtocol::OpenAiChat
        | ModelProtocol::OpenAiResponses
        | ModelProtocol::Legacy => response
            .get("usage")
            .map(|value| object(value, "usage"))
            .transpose()?
            .flatten()
            .map(Ok)
            .transpose(),
    }
}

/// 按协议把回包里的原始字段翻译成统一的 7 元组。
///
/// 【返回的 7 个位置，顺序固定】
/// ```text
///   0. input_tokens            输入 token
///   1. output_tokens           输出 token
///   2. cache_read_tokens       命中缓存读取的 token
///   3. cache_write_tokens      写入缓存的 token
///   4. reasoning_output_tokens 推理/思考 token
///   5. audio_input_tokens      音频输入 token
///   6. total_tokens            上游自报总数
/// ```
///
/// 【⚠ 为什么用 7 元组而不是直接构造 `UsageVector`】
/// 因为**置信度要等所有字段都拿到之后才能定**（见 `normalize_provider_usage` 里的
/// `has_core` / `has_any` 判定）。先收集、再判定、最后构造，是三步而不是一步。
///
/// 【⚠ `Legacy` 分支为什么返回全 None】
/// `ModelProtocol::Legacy` 对应的是本地录好的 cassette 脚本，它没有真实 usage。
/// 诚实地返回“没有”好过编一个假数字。

fn protocol_fields(
    response: &Map<String, Value>,
    protocol: ModelProtocol,
) -> Result<
    (
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
    ),
    String,
> {
    let usage = usage_object(response, protocol)?;
    let fields = usage;
    match protocol {
        ModelProtocol::AnthropicMessages => Ok((
            optional_u64(
                fields.and_then(|value| value.get("input_tokens")),
                "input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("output_tokens")),
                "output_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("cache_read_input_tokens")),
                "cache_read_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("cache_creation_input_tokens")),
                "cache_write_tokens",
            )?,
            None,
            None,
            optional_u64(
                fields.and_then(|value| value.get("total_tokens")),
                "total_tokens",
            )?,
        )),
        ModelProtocol::OpenAiChat => {
            let details = fields.and_then(|value| value.get("prompt_tokens_details"));
            let completion_details =
                fields.and_then(|value| value.get("completion_tokens_details"));
            Ok((
                optional_u64(
                    fields.and_then(|value| value.get("prompt_tokens")),
                    "input_tokens",
                )?,
                optional_u64(
                    fields.and_then(|value| value.get("completion_tokens")),
                    "output_tokens",
                )?,
                nested_u64(
                    object_or_none(details, "prompt_tokens_details")?,
                    "cache_read_tokens",
                    "cached_tokens",
                )?,
                None,
                nested_u64(
                    object_or_none(completion_details, "completion_tokens_details")?,
                    "reasoning_tokens",
                    "reasoning_tokens",
                )?,
                None,
                optional_u64(
                    fields.and_then(|value| value.get("total_tokens")),
                    "total_tokens",
                )?,
            ))
        }
        ModelProtocol::OpenAiResponses => {
            let input_details = fields.and_then(|value| value.get("input_tokens_details"));
            Ok((
                optional_u64(
                    fields.and_then(|value| value.get("input_tokens")),
                    "input_tokens",
                )?,
                optional_u64(
                    fields.and_then(|value| value.get("output_tokens")),
                    "output_tokens",
                )?,
                nested_u64(
                    object_or_none(input_details, "input_tokens_details")?,
                    "cache_read_tokens",
                    "cached_tokens",
                )?,
                None,
                nested_u64(
                    object_or_none(
                        fields.and_then(|value| value.get("output_tokens_details")),
                        "output_tokens_details",
                    )?,
                    "reasoning_tokens",
                    "reasoning_tokens",
                )?,
                None,
                optional_u64(
                    fields.and_then(|value| value.get("total_tokens")),
                    "total_tokens",
                )?,
            ))
        }
        ModelProtocol::OllamaChat => Ok((
            optional_u64(
                fields.and_then(|value| value.get("prompt_eval_count")),
                "input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("eval_count")),
                "output_tokens",
            )?,
            None,
            None,
            None,
            None,
            None,
        )),
        ModelProtocol::GeminiInteractions => Ok((
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("prompt_token_count")
                        .or_else(|| value.get("promptTokenCount"))
                        .or_else(|| value.get("total_input_tokens"))
                }),
                "input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("candidates_token_count")
                        .or_else(|| value.get("candidatesTokenCount"))
                        .or_else(|| value.get("total_output_tokens"))
                }),
                "output_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("cached_content_token_count")
                        .or_else(|| value.get("cachedContentTokenCount"))
                }),
                "cache_read_tokens",
            )?,
            None,
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("thoughts_token_count")
                        .or_else(|| value.get("thoughtsTokenCount"))
                        .or_else(|| value.get("total_thought_tokens"))
                }),
                "reasoning_tokens",
            )?,
            None,
            optional_u64(
                fields.and_then(|value| {
                    value
                        .get("total_token_count")
                        .or_else(|| value.get("totalTokenCount"))
                        .or_else(|| value.get("total_tokens"))
                }),
                "total_tokens",
            )?,
        )),
        // The legacy route is the deterministic Fake adapter in offline fixtures.  It uses the
        // neutral field names directly and is never treated as a live provider claim.
        ModelProtocol::Legacy => Ok((
            optional_u64(
                fields.and_then(|value| value.get("input_tokens")),
                "input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("output_tokens")),
                "output_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("cache_read_tokens")),
                "cache_read_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("cache_write_tokens")),
                "cache_write_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("reasoning_output_tokens")),
                "reasoning_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("audio_input_tokens")),
                "audio_input_tokens",
            )?,
            optional_u64(
                fields.and_then(|value| value.get("total_tokens")),
                "total_tokens",
            )?,
        )),
    }
}

fn object_or_none<'a>(
    value: Option<&'a Value>,
    field: &str,
) -> Result<Option<&'a Map<String, Value>>, String> {
    match value {
        None => Ok(None),
        Some(value) => object(value, field),
    }
}

/// 交叉校验上游自报的 `total_tokens` 与分项是否自洽。
///
/// 【为什么需要这道校验】
/// 上游同时报了 `input_tokens`、`output_tokens` 和 `total_tokens` 时，
/// 三者必须满足 `input + output == total`。
/// 如果对不上，说明至少有一个数字是错的——这时候**必须拒绝**，
/// 因为把一组自相矛盾的数字写进账本，比标记为 Unknown 更有害：
/// 它会让成本统计看起来是精确的，实际上是错的。
///
/// 【三种情形】
/// ```text
///   没有 total                        -> 不校验，放行
///   total + input + output 都有       -> 严格相等，否则 provider_usage_total_inconsistent
///   total 存在但分项不全              -> 只检查“已知分项不得大于 total”
/// ```
///
/// 【为什么用 checked_add】 input 和 output 都是外部来的 `u64`，
/// 相加可能溢出。`checked_add` 返回 `None` 时报 `provider_usage_total_overflow`，
/// 而不是 wrap 成一个很小的数字骗过校验。

fn check_total(input: Option<u64>, output: Option<u64>, total: Option<u64>) -> Result<(), String> {
    let Some(total) = total else {
        return Ok(());
    };
    if let (Some(input), Some(output)) = (input, output) {
        let calculated = input
            .checked_add(output)
            .ok_or_else(|| "provider_usage_total_overflow".to_owned())?;
        if calculated != total {
            return Err("provider_usage_total_inconsistent".to_owned());
        }
    } else if input.is_some_and(|input| input > total)
        || output.is_some_and(|output| output > total)
    {
        return Err("provider_usage_total_inconsistent".to_owned());
    }
    Ok(())
}

/// Normalize one server-owned provider response into the neutral billing usage vector.
///
/// The response may report a served model different from the requested route model.  The
/// requested model is always copied from `prepared.route.model_id`; the response cannot replace
/// it.  Provider totals are checked when all core components are present, while missing
/// components remain `Partial`/`Unknown` for later reconciliation.
#[allow(clippy::too_many_arguments)]
/// 把一次服务端发起的 provider 回包，归一化成中立的计费用量向量。
///
/// 【作用】 本文件的主入口。做的事，按顺序：
/// ```text
///   1. 校验 prepared 合法
///   2. 校验 run 归属一致（prepared 里记的 run == 传入的 run）
///   3. 校验 retry_ordinal 不荒谬
///   4. 对**整份原始回包**做摘要（raw_digest）——先摘要，后拆解
///   5. 拆出 served_model（对方说自己用了哪个模型）
///   6. 按协议取 7 个计数字段
///   7. 交叉校验 total
///   8. 根据“有几个字段”定 confidence：Known / Partial / Unknown
///   9. 组装 NormalizedUsage，填 served_model_id / retry_ordinal，重算摘要，validate
/// ```
///
/// 【调用者】
/// `rg -n "normalize_provider_usage"` 在生产代码中**未找到调用点**，
/// 仅 `kiana-core/tests/bq10_normalized_usage_guard.rs` 断言它存在。
/// 当前是已就位的契约层函数。
///
/// 【输入】
/// - `prepared`：已冻结、已封存的调用。它的 `route` 提供 provider/model/connection 身份。
/// - `response`：**原始回包 JSON**（未拆解）。
/// - `usage_id` / `attempt_id` / `run_id` / `retry_ordinal`：**全部由服务端提供**。
///
/// 【⚠ 为什么身份字段一律服务端提供】
/// 回包是外部数据。若允许从 `response` 里读 run_id，就等于允许对端自称“我这次属于 run-7”。
/// 所以模型、attempt、run、重试序号全部来自服务端，只有**计数的数值**来自回包。
///
/// 【输出】 `NormalizedUsage`。
///
/// 【副作用】 无。纯函数，不写事件、不扣预算、不重试。
///
/// 【失败情况】
/// - `provider_usage_run_mismatch`：run 归属错配。
/// - `provider_usage_retry_ordinal_invalid`：重试序号 > 100 万（不可能的输入）。
/// - `provider_usage_response_invalid`：回包根节点不是对象。
/// - `provider_usage_<字段>_invalid`：某字段类型错或超上限。
/// - `provider_usage_total_inconsistent` / `..._total_overflow`：总数与分项矛盾。
/// - `NormalizedUsage::new` / `validate` 的领域层错误。

pub fn normalize_provider_usage(
    prepared: &PreparedModelCall,
    response: &Value,
    usage_id: UsageId,
    attempt_id: AttemptId,
    run_id: RunId,
    retry_ordinal: u32,
) -> Result<NormalizedUsage, String> {
    prepared.validate().map_err(|error| error.to_string())?;
    if prepared
        .spec
        .assignment
        .as_ref()
        .is_some_and(|assignment| assignment.run_id != run_id)
    {
        return Err("provider_usage_run_mismatch".to_owned());
    }
    if retry_ordinal > 1_000_000 {
        return Err("provider_usage_retry_ordinal_invalid".to_owned());
    }
    let raw_digest = json_digest(response);
    let response = response.as_object().ok_or_else(|| invalid("response"))?;
    let served_model_id = served_model(response)?;
    let (
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_output_tokens,
        audio_input_tokens,
        total_tokens,
    ) = protocol_fields(response, prepared.route.protocol)?;
    check_total(input_tokens, output_tokens, total_tokens)?;
    // 置信度三档判定。**这是本文件最关键的分支**。
    //
    //   Known   = 输入和输出都有         -> 账单可以按这两个数算
    //   Partial = 有别的维度但缺核心项    -> 部分可用，需要对账补齐
    //   Unknown = 一个都没有             -> 对端完全没报，只能说“不知道花了多少”
    //
    // ⚠ 为什么不把 Partial 也归成 Unknown：
    //    “有缓存命中数但没有输入数”比“完全没报”信息量大得多。
    //    抹平成 Unknown 会丢掉可对账的线索；提升成 Known 则是撒谎。
    //    Partial 就是为这种情况准备的第三档。

    let has_core = input_tokens.is_some() && output_tokens.is_some();
    let has_any = input_tokens.is_some()
        || output_tokens.is_some()
        || cache_read_tokens.is_some()
        || cache_write_tokens.is_some()
        || reasoning_output_tokens.is_some()
        || audio_input_tokens.is_some()
        || total_tokens.is_some();
    let (confidence, unknown_reason) = if has_core {
        (UsageConfidence::Known, None)
    } else if has_any {
        (
            UsageConfidence::Partial,
            Some(BillingUnknownReason::Partial),
        )
    } else {
        (
            UsageConfidence::Unknown,
            Some(BillingUnknownReason::ProviderUnreported),
        )
    };
    let vector = UsageVector {
        schema: kiana_domain::USAGE_VECTOR_SCHEMA.to_owned(),
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_output_tokens,
        audio_input_tokens,
        audio_output_tokens: None,
        tool_calls: 0,
        effect_count: 0,
        wall_time_ms: 0,
        output_bytes: 0,
        artifact_bytes: 0,
        storage_bytes: 0,
    };
    // basis 记录“这些数字是用哪套规则、从哪个方言抽出来的”，格式为
    // `kiana.provider-usage-adapter.v1:<协议短名>`，例如
    // `kiana.provider-usage-adapter.v1:anthropic_messages`。
    //
    // 【为什么值得单独记】
    // 将来若某家改了字段名，历史账单里的旧数字必须还能解释。
    // 有了 basis + raw_digest + usage_digest 三个值，任何一条用量记录都能被追溯到
    // “当时的规则 + 当时的回包”。

    let basis = format!(
        "{}:{}",
        PROVIDER_USAGE_ADAPTER_SCHEMA,
        protocol_name(prepared.route.protocol)
    );
    let mut normalized = NormalizedUsage::new(
        usage_id,
        attempt_id,
        run_id,
        Some(prepared.route.provider_id.clone()),
        prepared.route.model_id.clone(),
        prepared.route.connection_id.clone(),
        vector,
        UsageSource::Provider,
        UsageObservation::Final,
        Some(1),
        confidence,
        unknown_reason,
        basis,
        raw_digest,
    )?;
    // 【与 model_id 的不对称，务必理解】
    //
    //   model_id         = prepared.route.model_id   （我们**请求**的模型）
    //   served_model_id  = 从回包里读到的            （对方**声称**实际服务的模型）
    //
    // 我们请求 gpt-4o，对端完全可能返回一个降级后的模型。合并成一个字段就等于
    // 宣称“我们确认对方按请求执行了”——这是没有证据的。
    // 保留两个字段，**差异本身就是可审计的事实**。

    normalized.served_model_id = served_model_id;
    normalized.retry_ordinal = retry_ordinal;
    // 摘要必须在**所有字段都填完之后**才算。
    // 顺序反了的话，算出来的是一个半成品的摘要，之后再也对不上。

    normalized.usage_digest = normalized.digest();
    normalized.validate()?;
    Ok(normalized)
}

/// 协议枚举 -> 可读短名，用于拼 `basis`。
///
/// 【⚠ 注意 `Legacy` 在这里被映射成 `"fake"` 而不是 `"legacy"`】
/// 这是与 `usage.rs::protocol_name` 的**有意差异**：`Legacy` 协议实际对应的是
/// 本地录好的脚本（fake script / cassette），账本里叫它 `fake` 更贴近使用者的直觉。
/// 两处映射不同不是 bug，但**改一处记得看另一处**。
///
/// 【为什么是穷举 match】 新增 `ModelProtocol` 变体时编译器会在这里报错，
/// 强制补上新分支——不会静默漏掉一个协议。

fn protocol_name(protocol: ModelProtocol) -> &'static str {
    match protocol {
        ModelProtocol::AnthropicMessages => "anthropic_messages",
        ModelProtocol::OpenAiChat => "openai_chat",
        ModelProtocol::OpenAiResponses => "openai_responses",
        ModelProtocol::OllamaChat => "ollama_chat",
        ModelProtocol::GeminiInteractions => "gemini_interactions",
        ModelProtocol::Legacy => "fake",
    }
}
