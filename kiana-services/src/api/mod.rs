pub mod client;
pub mod errors;
pub mod logging;
pub mod messages;
pub mod provider;
pub mod retry;
pub mod streaming;

use crate::errors::{ServiceError, ServiceResult};
use reqwest::{Client, Response};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiClientConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub timeout: Duration,
    pub max_retries: u32,
}

impl Default for ApiClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: "https://api.anthropic.com".to_string(),
            timeout: Duration::from_secs(600),
            max_retries: 10,
        }
    }
}

pub struct ApiClient {
    client: Client,
    config: ApiClientConfig,
}

impl ApiClient {
    pub fn new(config: ApiClientConfig) -> ServiceResult<Self> {
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ServiceError::Unknown(e.to_string()))?;

        Ok(Self { client, config })
    }

    pub async fn request(&self, method: reqwest::Method, path: &str) -> ServiceResult<Response> {
        let url = format!("{}{}", self.config.base_url, path);
        let mut req = self.client.request(method, &url);

        if let Some(api_key) = &self.config.api_key {
            req = req.header("x-api-key", api_key);
        }

        req.header("x-app", "cli")
            .send()
            .await
            .map_err(|e| ServiceError::Reqwest(e))
    }
}

pub fn parse_prompt_too_long_tokens(raw_message: &str) -> (Option<u32>, Option<u32>) {
    let re = regex::Regex::new(r"prompt is too long[^0-9]*(\d+)\s*tokens?\s*>\s*(\d+)").unwrap();

    if let Some(caps) = re.captures(raw_message) {
        let actual = caps.get(1).and_then(|m| m.as_str().parse().ok());
        let limit = caps.get(2).and_then(|m| m.as_str().parse().ok());
        (actual, limit)
    } else {
        (None, None)
    }
}
