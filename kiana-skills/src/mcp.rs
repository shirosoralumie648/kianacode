use crate::types::{Command, LoadedFrom, SettingSource};
use kiana_services::mcp::McpClient;
use serde_json::Value;
use std::collections::HashMap;

/// Load MCP prompts as skill commands for a connected MCP client.
///
/// The current `Command` type stores prompt content eagerly, so prompts that
/// cannot be resolved with their provided arguments are skipped instead of
/// being exposed as unusable placeholder skills.
pub async fn fetch_mcp_skills_for_client(client: &McpClient) -> Vec<Command> {
    fetch_mcp_skills_for_client_with_args(client, HashMap::new()).await
}

pub async fn fetch_mcp_skills_for_client_with_args(
    client: &McpClient,
    prompt_arguments: HashMap<String, HashMap<String, Value>>,
) -> Vec<Command> {
    let prompts = match client.list_prompts().await {
        Ok(prompts) => prompts,
        Err(_) => return Vec::new(),
    };

    let server_name = client.server_name().to_string();
    let normalized_server_name = normalize_mcp_name(&server_name);
    let mut commands = Vec::new();

    for prompt in prompts {
        let prompt_args = prompt_arguments
            .get(&prompt.name)
            .cloned()
            .unwrap_or_default();
        let Ok(prompt_result) = client.get_prompt(&prompt.name, prompt_args).await else {
            continue;
        };
        let Some(content) = prompt_result_to_markdown(&prompt_result) else {
            continue;
        };
        if content.trim().is_empty() {
            continue;
        }

        let argument_names = prompt
            .arguments
            .iter()
            .map(|argument| argument.name.as_str())
            .collect::<Vec<_>>();
        let normalized_prompt_name = normalize_mcp_name(&prompt.name);
        let description = prompt
            .description
            .clone()
            .unwrap_or_else(|| format!("MCP prompt {} from {}", prompt.name, server_name));

        commands.push(Command {
            name: format!(
                "mcp__{}__{}",
                normalized_server_name, normalized_prompt_name
            ),
            display_name: Some(format!("{}:{} (MCP)", server_name, prompt.name)),
            description: description.clone(),
            when_to_use: Some(description),
            argument_hint: (!argument_names.is_empty()).then(|| argument_names.join(" ")),
            allowed_tools: Vec::new(),
            model: None,
            disable_model_invocation: false,
            user_invocable: true,
            source: SettingSource::ProjectSettings,
            loaded_from: LoadedFrom::Mcp,
            skill_root: None,
            context: None,
            paths: None,
            content,
        });
    }

    commands
}

fn normalize_mcp_name(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_ascii_lowercase();

    if normalized.is_empty() {
        "server".to_string()
    } else {
        normalized
    }
}

fn prompt_result_to_markdown(result: &Value) -> Option<String> {
    let messages = result.get("messages")?.as_array()?;
    let mut parts = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user");
        let content = message.get("content")?;
        let text = extract_text_content(content)?;
        parts.push(format!("{role}: {text}"));
    }

    Some(parts.join("\n\n"))
}

fn extract_text_content(content: &Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    if let Some(text) = content.get("text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    if let Some(items) = content.as_array() {
        let text = items
            .iter()
            .filter_map(extract_text_content)
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return Some(text);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{fetch_mcp_skills_for_client_with_args, normalize_mcp_name};
    use kiana_services::mcp::{McpClient, McpServerConfig, TransportType};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    #[test]
    fn normalizes_mcp_names_for_command_ids() {
        assert_eq!(normalize_mcp_name("my server"), "my_server");
        assert_eq!(normalize_mcp_name("Browser.Tools"), "browser_tools");
        assert_eq!(normalize_mcp_name(""), "server");
    }

    #[tokio::test]
    async fn mcp_prompts_are_loaded_as_skill_commands() {
        let (url, handle) = start_prompt_mcp_server();
        let mut client = McpClient::new(McpServerConfig {
            name: "Prompt Server".to_string(),
            transport: TransportType::Http,
            url: Some(url),
            command: None,
            args: None,
            env: None,
            headers: None,
        });
        client.connect().await.unwrap();

        let commands = fetch_mcp_skills_for_client_with_args(
            &client,
            HashMap::from([(
                "summarize".to_string(),
                HashMap::from([("topic".to_string(), json!("permissions"))]),
            )]),
        )
        .await;

        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].name, "mcp__prompt_server__summarize");
        assert_eq!(
            commands[0].display_name.as_deref(),
            Some("Prompt Server:summarize (MCP)")
        );
        assert_eq!(commands[0].loaded_from, crate::types::LoadedFrom::Mcp);
        assert_eq!(commands[0].argument_hint.as_deref(), Some("topic"));
        assert!(commands[0].content.contains("permissions"));

        handle.join().unwrap();
    }

    fn start_prompt_mcp_server() -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(7) {
                let Ok(mut stream) = stream else {
                    break;
                };
                handle_prompt_mcp_request(&mut stream);
            }
        });
        (format!("http://{}", addr), handle)
    }

    fn handle_prompt_mcp_request(stream: &mut TcpStream) {
        let body = read_http_body(stream);
        let message: Value = serde_json::from_slice(&body).unwrap();
        let response = prompt_mcp_response(&message);
        let body = response.map(|value| value.to_string()).unwrap_or_default();
        let status = if body.is_empty() {
            "HTTP/1.1 204 No Content"
        } else {
            "HTTP/1.1 200 OK"
        };
        let response = format!(
            "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    fn read_http_body(stream: &mut TcpStream) -> Vec<u8> {
        let mut buffer = Vec::new();
        let mut chunk = [0; 1024];
        let header_end;
        loop {
            let read = stream.read(&mut chunk).unwrap();
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                header_end = position;
                break;
            }
        }

        let headers = String::from_utf8_lossy(&buffer[..header_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);

        let body_start = header_end + 4;
        while buffer.len() < body_start + content_length {
            let read = stream.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        buffer[body_start..body_start + content_length].to_vec()
    }

    fn prompt_mcp_response(message: &Value) -> Option<Value> {
        let id = message.get("id").cloned().unwrap_or(json!(null));
        match message.get("method").and_then(Value::as_str) {
            Some("notifications/initialized") => None,
            Some("initialize") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2024-11-05",
                    "capabilities": { "tools": {}, "resources": {}, "resourceTemplates": {}, "prompts": {} },
                    "serverInfo": { "name": "prompt-server", "version": "1.0.0" }
                }
            })),
            Some("tools/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "tools": [] }
            })),
            Some("resources/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "resources": [] }
            })),
            Some("resources/templates/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": { "resourceTemplates": [] }
            })),
            Some("prompts/list") => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "prompts": [{
                        "name": "summarize",
                        "description": "Summarize a topic",
                        "arguments": [{
                            "name": "topic",
                            "description": "Topic to summarize",
                            "required": false
                        }]
                    }]
                }
            })),
            Some("prompts/get") => {
                let topic = message
                    .get("params")
                    .and_then(|params| params.get("arguments"))
                    .and_then(|args| args.get("topic"))
                    .and_then(Value::as_str)
                    .unwrap_or("fallback");
                Some(json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "messages": [{
                            "role": "user",
                            "content": {
                                "type": "text",
                                "text": format!("Summarize {topic}")
                            }
                        }]
                    }
                }))
            }
            _ => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not found"}
            })),
        }
    }
}
