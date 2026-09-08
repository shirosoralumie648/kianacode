//! 将 Kiana 回执投影为 Web 界面使用的 Thread / Turn / Item 视图。
//!
//! 映射关系仅用于展示，不创建新的状态权威：一个 thread 对应 Kiana session，一个 turn
//! 对应一次 `DaemonHost` run 或 continue，而 item 是回执中用户输入、能力、文件变更、
//! 助手文本和错误的顺序化投影。EventLog 与协议回执仍是事实来源；浏览器中的该视图可以
//! 被重新生成，不能据此推断执行已完成或外部副作用正确。
//!
//! 本模块不承诺 token 流式传输，也不处理审批、信任或沙箱判断。那些拒绝路径必须保留
//! 在控制平面，前端只应如实展示已经返回的状态和错误。

use kiana_protocol::ResponseEnvelope;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, Serialize)]
pub struct ThreadView {
    /// 会话的稳定标识，由调用方从 Kiana session 传入。
    pub id: String,
    /// 从初始提示词派生的短标题，仅用于列表辨认，不是用户原文的完整副本。
    pub name: String,
    /// 调用方当前观察到的运行标记；它是瞬时 UI 状态，不是持久生命周期结论。
    pub running: bool,
    /// 该会话中已加载的 turn 投影，保持调用方提供的时间顺序。
    pub turns: Vec<TurnView>,
}

/// 一次模型运行或继续运行在 Web 中的展示单元。
#[derive(Clone, Debug, Serialize)]
pub struct TurnView {
    /// 对应协议请求或会话运行的标识。
    pub id: String,
    /// 从 [`ResponseEnvelope`] 序列化得到的执行状态字符串。
    pub status: String,
    /// 按用户输入、能力、文件、助手文本、错误顺序组织的展示项。
    pub items: Vec<ItemView>,
}

/// Web 时间线中一条可渲染的项目。
#[derive(Clone, Debug, Serialize)]
pub struct ItemView {
    /// 前端区分渲染方式的种类，例如 `userMessage` 或 `fileChange`。
    pub kind: String,
    /// 继承自所属回执的状态；不是对每项单独执行结果的重新判定。
    pub status: String,
    /// 简短标题，供时间线快速扫描。
    pub title: String,
    /// 原始提示、能力名称、文件列表、助手输出或错误文字。
    pub body: String,
}

/// 生成稳定、可读且长度受限的会话标题。
///
/// 连续空白会被折叠，以避免相同语义的提示在列表中显示为不同标题；标题最多保留 48 个
/// Unicode 标量值，超长时附加省略号。该截断只影响显示，绝不应用于发送给模型的原始
/// 提示词。
pub fn thread_name(prompt: &str) -> String {
    let trimmed = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if trimmed.is_empty() {
        return "New thread".to_owned();
    }
    let mut name = trimmed.chars().take(48).collect::<String>();
    if trimmed.chars().count() > 48 {
        name.push('…');
    }
    name
}

/// 将一个协议响应投影成 Web 时间线项目。
///
/// 函数始终先放入用户消息，随后从响应中提取能力、文件变更和助手文本，最后附加错误。
/// 这是一种宽容的反序列化：缺失或形状不匹配的可选 JSON 字段会被忽略而不是伪造数据。
/// 因而空列表不代表没有发生副作用，只表示该回执没有提供本视图可识别的字段。
pub fn items_from_turn(prompt: &str, response: &ResponseEnvelope) -> Vec<ItemView> {
    let mut items = vec![ItemView {
        kind: "userMessage".to_owned(),
        status: "completed".to_owned(),
        title: "You".to_owned(),
        body: prompt.to_owned(),
    }];
    for cap in capabilities(response) {
        items.push(ItemView {
            kind: "commandExecution".to_owned(),
            status: status_of(response),
            title: format!("tool · {cap}"),
            body: cap,
        });
    }
    let files = files_changed(response);
    if !files.is_empty() {
        items.push(ItemView {
            kind: "fileChange".to_owned(),
            status: status_of(response),
            title: format!(
                "{} file{}",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ),
            body: files.join("\n"),
        });
    }
    let text = assistant_text(response);
    if !text.trim().is_empty() {
        items.push(ItemView {
            kind: "agentMessage".to_owned(),
            status: status_of(response),
            title: "Builder".to_owned(),
            body: text,
        });
    }
    if let Some(error) = response.error.as_deref().filter(|value| !value.is_empty()) {
        items.push(ItemView {
            kind: "error".to_owned(),
            status: status_of(response),
            title: "blocked".to_owned(),
            body: error.to_owned(),
        });
    }
    items
}

/// 将协议枚举转换为供 Web API 传输的稳定状态文字。
///
/// 正常路径使用 serde 的枚举表示；序列化异常时退回调试格式以尽量保留可观察性。该回退
/// 不是新增协议契约，前端应以正式协议状态为准。
pub fn status_of(response: &ResponseEnvelope) -> String {
    serde_json::to_value(response.status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{:?}", response.status).to_ascii_lowercase())
}

/// 读取回执中模型输出的文本部分；字段缺失或非字符串时返回空字符串。
pub fn assistant_text(response: &ResponseEnvelope) -> String {
    response.output["output"]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// 读取回执声明的文件变更路径列表。
///
/// 仅保留字符串项，拒绝把任意 JSON 值格式化为路径，从而避免展示层意外制造不存在的
/// 文件记录。列表是模型/执行回执的声明，调用方仍应依赖正式收据或文件系统证据验证它。
pub fn files_changed(response: &ResponseEnvelope) -> Vec<String> {
    response.output["files_changed"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn capabilities(response: &ResponseEnvelope) -> Vec<String> {
    response.output["capabilities"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("operation").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_protocol::{ExecutionStatus, RequestId, PROTOCOL_SCHEMA};
    use serde_json::json;

    #[test]
    fn maps_receipt_to_codex_shaped_items() {
        let response = ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: RequestId::new(),
            status: ExecutionStatus::Completed,
            output: json!({
                "files_changed": ["GOLDEN_PATH.txt"],
                "capabilities": [{"operation": "apply_patch"}],
                "output": {"text": "created GOLDEN_PATH.txt"}
            }),
            error: None,
        };
        let items = items_from_turn("create GOLDEN_PATH.txt containing hello", &response);
        assert_eq!(items[0].kind, "userMessage");
        assert!(items.iter().any(|item| item.kind == "commandExecution"));
        assert!(items.iter().any(|item| item.kind == "fileChange"));
        assert!(items.iter().any(|item| item.kind == "agentMessage"));
        assert_eq!(
            thread_name("create GOLDEN_PATH.txt containing hello"),
            "create GOLDEN_PATH.txt containing hello"
        );
    }
}
