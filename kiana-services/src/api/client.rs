use reqwest::{header, Client};
use std::time::Duration;

pub struct AnthropicClient {
    client: Client,
    api_key: String,
    base_url: String,
    timeout: Duration,
}

impl AnthropicClient {
    pub fn new(api_key: String) -> Self {
        Self::with_base_url(api_key, "https://api.anthropic.com".to_string())
    }

    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        Self::with_base_url_and_timeout(api_key, base_url, Duration::from_secs(600))
    }

    pub fn with_base_url_and_timeout(api_key: String, base_url: String, timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            api_key,
            base_url,
            timeout,
        }
    }

    pub(crate) fn build_headers(&self) -> header::HeaderMap {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            "x-api-key",
            header::HeaderValue::from_str(&self.api_key).unwrap(),
        );
        headers.insert(
            "anthropic-version",
            header::HeaderValue::from_static("2023-06-01"),
        );
        headers.insert(
            "content-type",
            header::HeaderValue::from_static("application/json"),
        );
        headers
    }

    pub(crate) fn client(&self) -> &Client {
        &self.client
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::AnthropicClient;
    use std::time::Duration;

    #[test]
    fn custom_timeout_is_retained() {
        let client = AnthropicClient::with_base_url_and_timeout(
            "test-key".to_string(),
            "http://127.0.0.1".to_string(),
            Duration::from_millis(250),
        );

        assert_eq!(client.timeout(), Duration::from_millis(250));
    }
}
