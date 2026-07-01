pub const CLAUDE_AI_INFERENCE_SCOPE: &str = "user:inference";
pub const CLAUDE_AI_PROFILE_SCOPE: &str = "user:profile";
pub const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20";

pub const CONSOLE_OAUTH_SCOPES: &[&str] = &["org:create_api_key", CLAUDE_AI_PROFILE_SCOPE];

pub const CLAUDE_AI_OAUTH_SCOPES: &[&str] = &[
    CLAUDE_AI_PROFILE_SCOPE,
    CLAUDE_AI_INFERENCE_SCOPE,
    "user:sessions:claude_code",
    "user:mcp_servers",
    "user:file_upload",
];

pub const MCP_CLIENT_METADATA_URL: &str = "https://claude.ai/oauth/claude-code-client-metadata";

#[derive(Debug, Clone)]
pub struct OauthConfig {
    pub base_api_url: String,
    pub console_authorize_url: String,
    pub claude_ai_authorize_url: String,
    pub claude_ai_origin: String,
    pub token_url: String,
    pub api_key_url: String,
    pub roles_url: String,
    pub console_success_url: String,
    pub claudeai_success_url: String,
    pub manual_redirect_url: String,
    pub client_id: String,
    pub oauth_file_suffix: String,
    pub mcp_proxy_url: String,
    pub mcp_proxy_path: String,
}

impl Default for OauthConfig {
    fn default() -> Self {
        Self::prod()
    }
}

impl OauthConfig {
    pub fn prod() -> Self {
        Self {
            base_api_url: "https://api.anthropic.com".to_string(),
            console_authorize_url: "https://platform.claude.com/oauth/authorize".to_string(),
            claude_ai_authorize_url: "https://claude.com/cai/oauth/authorize".to_string(),
            claude_ai_origin: "https://claude.ai".to_string(),
            token_url: "https://platform.claude.com/v1/oauth/token".to_string(),
            api_key_url: "https://api.anthropic.com/api/oauth/claude_cli/create_api_key".to_string(),
            roles_url: "https://api.anthropic.com/api/oauth/claude_cli/roles".to_string(),
            console_success_url: "https://platform.claude.com/buy_credits?returnUrl=/oauth/code/success%3Fapp%3Dclaude-code".to_string(),
            claudeai_success_url: "https://platform.claude.com/oauth/code/success?app=claude-code".to_string(),
            manual_redirect_url: "https://platform.claude.com/oauth/code/callback".to_string(),
            client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e".to_string(),
            oauth_file_suffix: "".to_string(),
            mcp_proxy_url: "https://mcp-proxy.anthropic.com".to_string(),
            mcp_proxy_path: "/v1/mcp/{server_id}".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_config_default() {
        let config = OauthConfig::default();
        assert_eq!(config.base_api_url, "https://api.anthropic.com");
        assert!(config.client_id.len() > 0);
    }
}
