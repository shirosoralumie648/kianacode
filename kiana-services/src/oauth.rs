use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl OAuthTokens {
    pub fn expires_within(&self, skew: Duration) -> bool {
        let Some(expires_at) = self.expires_at else {
            return false;
        };
        let skew =
            ChronoDuration::from_std(skew).unwrap_or_else(|_| ChronoDuration::seconds(i64::MAX));
        expires_at <= Utc::now() + skew
    }

    fn refresh_token_value(&self) -> Option<&str> {
        self.refresh_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
}

pub struct OAuthClient {
    config: OAuthConfig,
    client: reqwest::Client,
}

impl OAuthClient {
    pub fn new(config: OAuthConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub async fn refresh_token(
        &self,
        refresh_token: &str,
    ) -> crate::errors::ServiceResult<OAuthTokens> {
        let response = self
            .client
            .post(&self.config.token_url)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
                ("client_id", &self.config.client_id),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(crate::errors::ServiceError::Auth(format!(
                "OAuth refresh failed: {}",
                response.status()
            )));
        }

        let tokens: OAuthTokenResponse = response.json().await?;
        let tokens = tokens.into_tokens();
        Ok(tokens)
    }
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    expires_in: Option<i64>,
}

impl OAuthTokenResponse {
    fn into_tokens(self) -> OAuthTokens {
        let expires_at = self.expires_at.or_else(|| {
            self.expires_in
                .filter(|seconds| *seconds > 0)
                .map(|seconds| Utc::now() + ChronoDuration::seconds(seconds))
        });
        OAuthTokens {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at,
        }
    }
}

pub fn oauth_config_from_env() -> OAuthConfig {
    let defaults = kiana_constants::oauth::OauthConfig::default();
    OAuthConfig {
        client_id: env_or_default("KIANA_OAUTH_CLIENT_ID", defaults.client_id),
        auth_url: env_or_default("KIANA_OAUTH_AUTH_URL", defaults.claude_ai_authorize_url),
        token_url: env_or_default("KIANA_OAUTH_TOKEN_URL", defaults.token_url),
        redirect_uri: env_or_default("KIANA_OAUTH_REDIRECT_URI", defaults.manual_redirect_url),
    }
}

pub fn oauth_tokens_path() -> PathBuf {
    for key in ["KIANA_OAUTH_TOKENS_FILE", "CLAUDE_CODE_OAUTH_TOKENS_FILE"] {
        if let Some(path) = env::var_os(key).filter(|value| !value.is_empty()) {
            return PathBuf::from(path);
        }
    }
    if let Some(home) = env::var_os("KIANA_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join("oauth.json");
    }
    if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".kiana").join("oauth.json");
    }
    PathBuf::from(".kiana").join("oauth.json")
}

pub fn load_oauth_tokens() -> crate::errors::ServiceResult<Option<OAuthTokens>> {
    load_oauth_tokens_from_path(&oauth_tokens_path())
}

pub fn load_oauth_tokens_from_path(
    path: &Path,
) -> crate::errors::ServiceResult<Option<OAuthTokens>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path)?;
    let tokens: OAuthTokens = serde_json::from_str(&contents)?;
    if tokens.access_token.trim().is_empty() {
        return Err(crate::errors::ServiceError::Auth(format!(
            "OAuth token file {} does not contain access_token",
            path.display()
        )));
    }
    Ok(Some(tokens))
}

pub fn save_oauth_tokens(tokens: &OAuthTokens) -> crate::errors::ServiceResult<PathBuf> {
    let path = oauth_tokens_path();
    save_oauth_tokens_to_path(&path, tokens)?;
    Ok(path)
}

pub fn save_oauth_tokens_to_path(
    path: &Path,
    tokens: &OAuthTokens,
) -> crate::errors::ServiceResult<()> {
    if tokens.access_token.trim().is_empty() {
        return Err(crate::errors::ServiceError::Auth(
            "OAuth tokens require a non-empty access_token".to_string(),
        ));
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        let parent_existed = parent.exists();
        fs::create_dir_all(parent)?;
        if !parent_existed {
            restrict_oauth_dir_permissions(parent)?;
        }
    }
    let contents = serde_json::to_string_pretty(tokens)?;
    write_oauth_tokens_atomically(path, contents.as_bytes())?;
    Ok(())
}

fn write_oauth_tokens_atomically(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let temp_path = oauth_tokens_temp_path(path);
    let write_result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        set_owner_only_create_mode(&mut options);
        let mut file = options.open(&temp_path)?;
        file.write_all(contents)?;
        file.sync_all()?;
        drop(file);
        restrict_oauth_file_permissions(&temp_path)?;
        fs::rename(&temp_path, path)?;
        restrict_oauth_file_permissions(path)?;
        sync_parent_dir(path);
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn oauth_tokens_temp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "oauth.json".into());
    path.with_file_name(format!(".{file_name}.{}.tmp", Uuid::new_v4()))
}

#[cfg(unix)]
fn set_owner_only_create_mode(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;

    options.mode(0o600);
}

#[cfg(not(unix))]
fn set_owner_only_create_mode(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn restrict_oauth_file_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_oauth_file_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_oauth_dir_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_oauth_dir_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn sync_parent_dir(path: &Path) {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return;
    };
    if let Ok(dir) = fs::File::open(parent) {
        let _ = dir.sync_all();
    }
}

pub async fn refresh_stored_oauth_tokens() -> crate::errors::ServiceResult<Option<OAuthTokens>> {
    let Some(stored) = load_oauth_tokens()? else {
        return Ok(None);
    };
    refresh_and_save_oauth_tokens(stored).await
}

pub async fn load_oauth_tokens_refreshing_if_expiring(
    skew: Duration,
) -> crate::errors::ServiceResult<Option<OAuthTokens>> {
    let Some(stored) = load_oauth_tokens()? else {
        return Ok(None);
    };
    if stored.expires_within(skew) && stored.refresh_token_value().is_some() {
        refresh_and_save_oauth_tokens(stored).await
    } else {
        Ok(Some(stored))
    }
}

async fn refresh_and_save_oauth_tokens(
    stored: OAuthTokens,
) -> crate::errors::ServiceResult<Option<OAuthTokens>> {
    let Some(refresh_token) = stored.refresh_token_value().map(str::to_string) else {
        return Ok(None);
    };

    let client = OAuthClient::new(oauth_config_from_env());
    let mut refreshed = client.refresh_token(&refresh_token).await?;
    if refreshed.refresh_token.is_none() {
        refreshed.refresh_token = stored.refresh_token;
    }
    save_oauth_tokens(&refreshed)?;
    Ok(Some(refreshed))
}

fn env_or_default(name: &str, default: String) -> String {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Mutex, OnceLock};
    use uuid::Uuid;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kiana-oauth-{name}-{}.json", Uuid::new_v4()))
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("kiana-oauth-{name}-{}", Uuid::new_v4()))
    }

    fn clear_oauth_env() {
        for key in [
            "KIANA_OAUTH_TOKENS_FILE",
            "CLAUDE_CODE_OAUTH_TOKENS_FILE",
            "KIANA_HOME",
            "KIANA_OAUTH_CLIENT_ID",
            "KIANA_OAUTH_AUTH_URL",
            "KIANA_OAUTH_TOKEN_URL",
            "KIANA_OAUTH_REDIRECT_URI",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn load_and_save_oauth_tokens_use_configured_path() {
        let _guard = env_lock().lock().unwrap();
        clear_oauth_env();
        let path = temp_path("store");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &path);
        let tokens = OAuthTokens {
            access_token: "access-token".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at: None,
        };

        let saved = save_oauth_tokens(&tokens).unwrap();
        let loaded = load_oauth_tokens().unwrap().unwrap();

        assert_eq!(saved, path);
        assert_eq!(loaded.access_token, "access-token");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh-token"));

        let _ = fs::remove_file(path);
        clear_oauth_env();
    }

    #[test]
    fn save_oauth_tokens_replaces_existing_file_without_temp_leftovers() {
        let root = temp_dir("atomic");
        let path = root.join("oauth.json");
        let original = OAuthTokens {
            access_token: "old-access".to_string(),
            refresh_token: Some("old-refresh".to_string()),
            expires_at: None,
        };
        let replacement = OAuthTokens {
            access_token: "new-access".to_string(),
            refresh_token: Some("new-refresh".to_string()),
            expires_at: None,
        };

        save_oauth_tokens_to_path(&path, &original).unwrap();
        save_oauth_tokens_to_path(&path, &replacement).unwrap();

        let loaded = load_oauth_tokens_from_path(&path).unwrap().unwrap();
        let entries = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().to_string())
            .collect::<Vec<_>>();

        assert_eq!(loaded.access_token, "new-access");
        assert_eq!(loaded.refresh_token.as_deref(), Some("new-refresh"));
        assert_eq!(entries, vec!["oauth.json".to_string()]);

        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn save_oauth_tokens_sets_owner_only_unix_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_dir("permissions");
        let path = root.join("nested").join("oauth.json");
        let tokens = OAuthTokens {
            access_token: "access-token".to_string(),
            refresh_token: Some("refresh-token".to_string()),
            expires_at: None,
        };

        save_oauth_tokens_to_path(&path, &tokens).unwrap();

        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

        assert_eq!(dir_mode, 0o700);
        assert_eq!(file_mode, 0o600);

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn refresh_stored_oauth_tokens_posts_refresh_grant_and_preserves_refresh_token() {
        let _guard = env_lock().lock().unwrap();
        clear_oauth_env();
        let token_path = temp_path("refresh");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        std::env::set_var("KIANA_OAUTH_CLIENT_ID", "client-123");
        save_oauth_tokens(&OAuthTokens {
            access_token: "old-access".to_string(),
            refresh_token: Some("old-refresh".to_string()),
            expires_at: None,
        })
        .unwrap();

        let (url, handle) = start_token_server(
            json!({
                "access_token": "new-access",
                "expires_in": 3600
            }),
            "old-refresh",
            "client-123",
        );
        std::env::set_var("KIANA_OAUTH_TOKEN_URL", url);

        let refreshed = refresh_stored_oauth_tokens().await.unwrap().unwrap();
        let persisted = load_oauth_tokens().unwrap().unwrap();

        assert_eq!(refreshed.access_token, "new-access");
        assert_eq!(refreshed.refresh_token.as_deref(), Some("old-refresh"));
        assert!(refreshed.expires_at.is_some());
        assert_eq!(persisted.access_token, "new-access");
        assert_eq!(persisted.refresh_token.as_deref(), Some("old-refresh"));

        handle.join().unwrap();
        let _ = fs::remove_file(token_path);
        clear_oauth_env();
    }

    #[tokio::test]
    async fn load_oauth_tokens_refreshing_if_expiring_keeps_fresh_token() {
        let _guard = env_lock().lock().unwrap();
        clear_oauth_env();
        let token_path = temp_path("fresh");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        save_oauth_tokens(&OAuthTokens {
            access_token: "fresh-access".to_string(),
            refresh_token: Some("fresh-refresh".to_string()),
            expires_at: Some(Utc::now() + ChronoDuration::hours(1)),
        })
        .unwrap();

        let loaded = load_oauth_tokens_refreshing_if_expiring(Duration::from_secs(300))
            .await
            .unwrap()
            .unwrap();

        assert_eq!(loaded.access_token, "fresh-access");
        assert_eq!(loaded.refresh_token.as_deref(), Some("fresh-refresh"));

        let _ = fs::remove_file(token_path);
        clear_oauth_env();
    }

    #[tokio::test]
    async fn load_oauth_tokens_refreshing_if_expiring_refreshes_near_expiry() {
        let _guard = env_lock().lock().unwrap();
        clear_oauth_env();
        let token_path = temp_path("expiring");
        std::env::set_var("KIANA_OAUTH_TOKENS_FILE", &token_path);
        std::env::set_var("KIANA_OAUTH_CLIENT_ID", "client-expiring");
        save_oauth_tokens(&OAuthTokens {
            access_token: "expiring-access".to_string(),
            refresh_token: Some("expiring-refresh".to_string()),
            expires_at: Some(Utc::now() + ChronoDuration::seconds(30)),
        })
        .unwrap();

        let (url, handle) = start_token_server(
            json!({
                "access_token": "refreshed-access",
                "refresh_token": "refreshed-refresh",
                "expires_in": 3600
            }),
            "expiring-refresh",
            "client-expiring",
        );
        std::env::set_var("KIANA_OAUTH_TOKEN_URL", url);

        let refreshed = load_oauth_tokens_refreshing_if_expiring(Duration::from_secs(300))
            .await
            .unwrap()
            .unwrap();
        let persisted = load_oauth_tokens().unwrap().unwrap();

        assert_eq!(refreshed.access_token, "refreshed-access");
        assert_eq!(
            refreshed.refresh_token.as_deref(),
            Some("refreshed-refresh")
        );
        assert_eq!(persisted.access_token, "refreshed-access");
        assert_eq!(
            persisted.refresh_token.as_deref(),
            Some("refreshed-refresh")
        );

        handle.join().unwrap();
        let _ = fs::remove_file(token_path);
        clear_oauth_env();
    }

    fn start_token_server(
        response: serde_json::Value,
        expected_refresh_token: &'static str,
        expected_client_id: &'static str,
    ) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 2048];
            let read = stream.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..read]);
            assert!(request.starts_with("POST "));
            assert!(request.contains("grant_type=refresh_token"));
            assert!(request.contains(&format!("refresh_token={expected_refresh_token}")));
            assert!(request.contains(&format!("client_id={expected_client_id}")));
            let body = response.to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(reply.as_bytes()).unwrap();
        });
        (format!("http://{}", addr), handle)
    }
}
