pub const PRODUCT_URL: &str = "https://claude.com/claude-code";
pub const CLAUDE_AI_BASE_URL: &str = "https://claude.ai";
pub const CLAUDE_AI_STAGING_BASE_URL: &str = "https://claude-ai.staging.ant.dev";
pub const CLAUDE_AI_LOCAL_BASE_URL: &str = "http://localhost:4000";

pub fn is_remote_session_staging(session_id: Option<&str>, ingress_url: Option<&str>) -> bool {
    session_id.map_or(false, |s| s.contains("_staging_"))
        || ingress_url.map_or(false, |u| u.contains("staging"))
}

pub fn is_remote_session_local(session_id: Option<&str>, ingress_url: Option<&str>) -> bool {
    session_id.map_or(false, |s| s.contains("_local_"))
        || ingress_url.map_or(false, |u| u.contains("localhost"))
}

pub fn get_claude_ai_base_url(session_id: Option<&str>, ingress_url: Option<&str>) -> &'static str {
    if is_remote_session_local(session_id, ingress_url) {
        CLAUDE_AI_LOCAL_BASE_URL
    } else if is_remote_session_staging(session_id, ingress_url) {
        CLAUDE_AI_STAGING_BASE_URL
    } else {
        CLAUDE_AI_BASE_URL
    }
}

pub fn get_remote_session_url(session_id: &str, ingress_url: Option<&str>) -> String {
    let base_url = get_claude_ai_base_url(Some(session_id), ingress_url);
    format!("{}/code/{}", base_url, session_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_remote_session_staging() {
        assert!(is_remote_session_staging(Some("session_staging_123"), None));
        assert!(is_remote_session_staging(
            None,
            Some("https://staging.example.com")
        ));
        assert!(!is_remote_session_staging(Some("session_123"), None));
    }

    #[test]
    fn test_get_claude_ai_base_url() {
        assert_eq!(
            get_claude_ai_base_url(Some("session_local_123"), None),
            CLAUDE_AI_LOCAL_BASE_URL
        );
        assert_eq!(
            get_claude_ai_base_url(Some("session_staging_123"), None),
            CLAUDE_AI_STAGING_BASE_URL
        );
        assert_eq!(
            get_claude_ai_base_url(Some("session_123"), None),
            CLAUDE_AI_BASE_URL
        );
    }
}
