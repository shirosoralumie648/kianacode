use crate::tool::*;
use async_trait::async_trait;
use kiana_services::network_policy::{
    validate_http_redirect, validate_http_url, HttpNetworkSurface,
};
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use url::Url;

#[derive(Debug, Deserialize)]
struct WebSearchInput {
    query: String,
    #[serde(default)]
    limit: Option<usize>,
}

pub struct WebSearchTool {
    search_endpoint: String,
}

const DEFAULT_SEARCH_ENDPOINT: &str = "https://duckduckgo.com/html/";

impl WebSearchTool {
    pub fn new() -> Self {
        Self::with_search_endpoint(DEFAULT_SEARCH_ENDPOINT)
    }

    fn with_search_endpoint(search_endpoint: impl Into<String>) -> Self {
        Self {
            search_endpoint: search_endpoint.into(),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "WebSearch"
    }

    fn description(&self) -> &str {
        "Search the web using a public HTML search endpoint"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 20 }
            },
            "required": ["query"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" },
                "results": { "type": "array" },
                "source_url": { "type": "string" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, _context: &ToolContext) -> ValidationResult {
        let input: WebSearchInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };
        if input.query.trim().is_empty() {
            return ValidationResult::err("query cannot be empty".to_string(), 2);
        }
        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, _context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: WebSearchInput = serde_json::from_value(input.clone())?;
        let query = input.query.trim();
        if query.is_empty() {
            return Err(ToolError::ValidationError(
                "query cannot be empty".to_string(),
            ));
        }

        let limit = input.limit.unwrap_or(5).clamp(1, 20);
        let source_url = build_search_source_url(&self.search_endpoint, query)
            .map_err(ToolError::ValidationError)?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("kiana-code/0.1")
            .redirect(web_search_redirect_policy())
            .build()
            .map_err(|e| ToolError::Other(e.to_string()))?;
        let html = client
            .get(source_url.clone())
            .send()
            .await
            .map_err(map_web_search_reqwest_error)?
            .text()
            .await
            .map_err(|e| ToolError::Other(e.to_string()))?;
        let results = parse_duckduckgo_results(&html, limit);

        Ok(ToolOutput {
            data: json!({
                "query": query,
                "source_url": source_url.to_string(),
                "results": results,
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let query = output
            .data
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("");
        let mut content = format!("Web search results for query: \"{query}\"\n\n");

        let results = output
            .data
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        if results.is_empty() {
            content.push_str("No links found.\n\n");
        } else {
            for (index, result) in results.iter().enumerate() {
                if let Some(text) = result.as_str() {
                    if !text.trim().is_empty() {
                        content.push_str(text.trim());
                        content.push_str("\n\n");
                    }
                    continue;
                }

                let title = result
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Untitled result");
                let url = result.get("url").and_then(Value::as_str).unwrap_or("");
                if url.is_empty() {
                    content.push_str(&format!("{}. {title}\n\n", index + 1));
                } else {
                    content.push_str(&format!("{}. {title}\nURL: {url}\n\n", index + 1));
                }
            }
        }

        content.push_str(
            "REMINDER: You MUST include the sources above in your response to the user using markdown hyperlinks.",
        );

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": content.trim()
        })
    }
}

fn build_search_source_url(endpoint: &str, query: &str) -> Result<Url, String> {
    let mut url = validate_http_url(HttpNetworkSurface::WebSearch, endpoint)?;
    url.query_pairs_mut().append_pair("q", query);
    validate_http_url(HttpNetworkSurface::WebSearch, url.as_str())
}

fn web_search_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let Some(previous_url) = attempt.previous().last() else {
            return attempt.follow();
        };
        if let Err(reason) =
            validate_http_redirect(HttpNetworkSurface::WebSearch, previous_url, attempt.url())
        {
            return attempt.error(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                reason,
            ));
        }
        attempt.follow()
    })
}

fn map_web_search_reqwest_error(error: reqwest::Error) -> ToolError {
    let detailed = reqwest_error_with_sources(&error);
    if detailed.contains("network policy denied URL")
        || detailed.contains("network policy denied redirect")
    {
        ToolError::ValidationError(detailed)
    } else {
        ToolError::Other(error.to_string())
    }
}

fn reqwest_error_with_sources(error: &reqwest::Error) -> String {
    let mut message = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(error) = source {
        let detail = error.to_string();
        if !message.contains(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = error.source();
    }
    message
}

fn parse_duckduckgo_results(html: &str, limit: usize) -> Vec<Value> {
    let Ok(re) = Regex::new(r#"<a[^>]+class="result__a"[^>]+href="([^"]+)"[^>]*>(.*?)</a>"#) else {
        return Vec::new();
    };
    let tag_re = Regex::new(r"<[^>]+>").ok();

    re.captures_iter(html)
        .take(limit)
        .map(|capture| {
            let url = html_unescape(capture.get(1).map(|m| m.as_str()).unwrap_or_default());
            let raw_title = capture.get(2).map(|m| m.as_str()).unwrap_or_default();
            let stripped = tag_re
                .as_ref()
                .map(|tag_re| tag_re.replace_all(raw_title, "").to_string())
                .unwrap_or_else(|| raw_title.to_string());
            json!({
                "title": html_unescape(&stripped),
                "url": url
            })
        })
        .collect()
}

fn html_unescape(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

#[cfg(test)]
mod tests {
    use super::{parse_duckduckgo_results, WebSearchTool};
    use crate::{Tool, ToolContext, ToolOutput};
    use serde_json::json;
    use std::collections::HashMap;

    fn test_context() -> ToolContext {
        let (_abort_tx, abort_signal) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .to_string_lossy()
                .to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::new(),
            abort_signal,
        }
    }

    #[test]
    fn parses_basic_duckduckgo_result_links() {
        let html = r#"<a rel="nofollow" class="result__a" href="https://example.com?a=1&amp;b=2">Example <b>Result</b></a>"#;
        let results = parse_duckduckgo_results(html, 5);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["title"], "Example Result");
        assert_eq!(results[0]["url"], "https://example.com?a=1&b=2");
    }

    #[test]
    fn maps_search_results_to_model_facing_text_with_source_reminder() {
        let output = ToolOutput {
            data: json!({
                "query": "rust async",
                "source_url": "https://duckduckgo.com/html/?q=rust%20async",
                "results": [
                    {
                        "title": "Rust Async Book",
                        "url": "https://rust-lang.github.io/async-book/"
                    }
                ]
            }),
            metadata: None,
        };

        let result = WebSearchTool::new().map_to_api_result(&output, "toolu_search");

        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["tool_use_id"], "toolu_search");
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("Web search results for query: \"rust async\""));
        assert!(
            content.contains("1. Rust Async Book\nURL: https://rust-lang.github.io/async-book/")
        );
        assert!(content.contains("markdown hyperlinks"));
        assert!(result["content"]["results"].is_null());
    }

    #[tokio::test]
    async fn call_rejects_non_public_search_endpoint_before_request() {
        let tool = WebSearchTool::with_search_endpoint("http://127.0.0.1:9/html/");
        let mut context = test_context();

        let err = tool
            .call(&json!({ "query": "rust async" }), &mut context)
            .await
            .expect_err("local search endpoint must be rejected before request")
            .to_string();

        assert!(err.contains("network policy denied URL"), "{err}");
    }
}
