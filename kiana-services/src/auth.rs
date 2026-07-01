use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub key: String,
    pub source: KeySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeySource {
    Environment,
    Config,
    Helper,
}

pub fn get_api_key() -> Option<ApiKey> {
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        return Some(ApiKey {
            key,
            source: KeySource::Environment,
        });
    }
    None
}

pub fn check_oauth_tokens() -> bool {
    // Placeholder for OAuth token validation
    false
}
