use anyhow::{anyhow, Result};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

static TELEMETRY_INITIALIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static SIGNAL_HANDLERS_REGISTERED: AtomicBool = AtomicBool::new(false);
static DEFAULT_CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);
static CLEANUP_RAN: AtomicBool = AtomicBool::new(false);
static NEXT_CLEANUP_ID: AtomicU64 = AtomicU64::new(1);
const REMOTE_SETTINGS_FILE_ENV: &str = "KIANA_REMOTE_SETTINGS_FILE";
const REMOTE_SETTINGS_CACHE_FILE_ENV: &str = "KIANA_REMOTE_SETTINGS_CACHE_FILE";
const REMOTE_SETTINGS_STATUS_ENV: &str = "KIANA_REMOTE_SETTINGS_STATUS";

type CleanupFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
type CleanupFn = Arc<dyn Fn() -> CleanupFuture + Send + Sync>;

fn cleanup_handlers() -> &'static Mutex<Vec<(u64, CleanupFn)>> {
    static HANDLERS: OnceLock<Mutex<Vec<(u64, CleanupFn)>>> = OnceLock::new();
    HANDLERS.get_or_init(|| Mutex::new(Vec::new()))
}

#[derive(Debug, Clone)]
pub struct CleanupRegistration {
    id: u64,
}

impl CleanupRegistration {
    pub fn unregister(&self) {
        cleanup_handlers()
            .lock()
            .unwrap()
            .retain(|(id, _)| id != &self.id);
    }
}

pub fn register_cleanup<F, Fut>(cleanup: F) -> CleanupRegistration
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    let id = NEXT_CLEANUP_ID.fetch_add(1, Ordering::SeqCst);
    cleanup_handlers()
        .lock()
        .unwrap()
        .push((id, Arc::new(move || Box::pin(cleanup()))));
    CleanupRegistration { id }
}

pub async fn run_cleanup_handlers() -> Result<()> {
    if CLEANUP_RAN.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let handlers: Vec<CleanupFn> = cleanup_handlers()
        .lock()
        .unwrap()
        .iter()
        .map(|(_, handler)| Arc::clone(handler))
        .collect();
    let mut errors = Vec::new();
    for handler in handlers {
        if let Err(error) = handler().await {
            errors.push(error.to_string());
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!("cleanup failed: {}", errors.join("; ")))
    }
}

pub async fn init() -> Result<()> {
    enable_configs();
    apply_safe_env_vars();
    apply_ca_certs();
    setup_graceful_shutdown();
    init_1p_event_logging();
    populate_oauth_if_needed();
    init_jetbrains_detection();
    detect_repository();
    init_remote_settings().await;
    record_first_start();
    configure_mtls();
    configure_proxy();
    preconnect_api();
    init_upstream_proxy().await;
    set_shell_windows();
    register_cleanup_handlers();
    ensure_scratchpad().await;
    Ok(())
}

pub fn init_telemetry_after_trust() {
    tokio::spawn(async {
        let _ = init_telemetry().await;
    });
}

async fn init_telemetry() -> Result<()> {
    if TELEMETRY_INITIALIZED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }
    set_meter_state().await
}

async fn set_meter_state() -> Result<()> {
    Ok(())
}

pub async fn graceful_shutdown(exit_code: i32) -> ! {
    if let Err(error) = run_cleanup_handlers().await {
        eprintln!("cleanup error: {error}");
    }
    std::process::exit(exit_code);
}

fn enable_configs() {}
fn apply_safe_env_vars() {}
fn apply_ca_certs() {}
fn setup_graceful_shutdown() {
    if SIGNAL_HANDLERS_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async {
        let exit_code = wait_for_shutdown_signal().await;
        graceful_shutdown(exit_code).await;
    });
}
fn init_1p_event_logging() {}
fn populate_oauth_if_needed() {}
fn init_jetbrains_detection() {}
fn detect_repository() {}
async fn init_remote_settings() {
    if env_truthy("KIANA_DISABLE_REMOTE_SETTINGS") {
        set_remote_settings_status("disabled");
        return;
    }

    let cache_path = remote_settings_cache_path();
    let cache_available = apply_remote_settings_cache_if_present(&cache_path, "cache_available");
    let Some(endpoint) = remote_settings_endpoint() else {
        if !cache_available {
            set_remote_settings_status("no_endpoint");
        }
        return;
    };

    match fetch_remote_settings(&endpoint, remote_settings_cached_checksum(&cache_path)).await {
        Ok(RemoteSettingsFetch::Settings { settings, checksum }) => {
            if settings.as_object().is_some_and(|object| object.is_empty()) {
                clear_remote_settings_cache(&cache_path);
                set_remote_settings_status("remote_empty");
                return;
            }
            match save_remote_settings_cache(&cache_path, &settings, checksum.as_deref()) {
                Ok(()) => {
                    apply_remote_settings_cache(&cache_path, "remote_loaded");
                }
                Err(error) => {
                    if cache_available {
                        set_remote_settings_status("remote_save_failed_using_cache");
                    } else {
                        set_remote_settings_status(&format!("remote_save_failed:{error}"));
                    }
                }
            }
        }
        Ok(RemoteSettingsFetch::NotModified) => {
            if !apply_remote_settings_cache_if_present(&cache_path, "remote_not_modified") {
                set_remote_settings_status("remote_not_modified_no_cache");
            }
        }
        Ok(RemoteSettingsFetch::Empty) => {
            clear_remote_settings_cache(&cache_path);
            set_remote_settings_status("remote_empty");
        }
        Err(error) => {
            if cache_available
                || apply_remote_settings_cache_if_present(&cache_path, "fetch_failed_using_cache")
            {
                set_remote_settings_status("fetch_failed_using_cache");
            } else {
                set_remote_settings_status(&format!("fetch_failed_no_cache:{error}"));
            }
        }
    }
}
fn record_first_start() {}
fn configure_mtls() {}
fn configure_proxy() {}
fn preconnect_api() {}
async fn init_upstream_proxy() {}
fn set_shell_windows() {}

enum RemoteSettingsFetch {
    Settings {
        settings: Value,
        checksum: Option<String>,
    },
    NotModified,
    Empty,
}

fn remote_settings_endpoint() -> Option<String> {
    std::env::var("KIANA_REMOTE_SETTINGS_URL")
        .or_else(|_| std::env::var("CLAUDE_REMOTE_SETTINGS_URL"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn remote_settings_cache_path() -> PathBuf {
    if let Ok(path) = std::env::var(REMOTE_SETTINGS_CACHE_FILE_ENV) {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    kiana_home_dir().join("remote-settings.json")
}

fn kiana_home_dir() -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_HOME") {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let home = home.trim();
        if !home.is_empty() {
            return PathBuf::from(home).join(".kiana");
        }
    }
    std::env::temp_dir().join("kiana")
}

async fn fetch_remote_settings(
    endpoint: &str,
    cached_checksum: Option<String>,
) -> Result<RemoteSettingsFetch> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let mut request = client.get(endpoint).header("User-Agent", "kiana-code/0.1");
    if let Ok(api_key) = std::env::var("ANTHROPIC_API_KEY") {
        if !api_key.trim().is_empty() {
            request = request.header("x-api-key", api_key);
        }
    } else if let Ok(token) = std::env::var("CLAUDE_ACCESS_TOKEN") {
        if !token.trim().is_empty() {
            request = request.bearer_auth(token);
        }
    }
    if let Some(checksum) = cached_checksum.filter(|value| !value.trim().is_empty()) {
        request = request.header("If-None-Match", format!("\"{}\"", checksum.trim()));
    }

    let response = request.send().await?;
    match response.status() {
        StatusCode::OK => {
            let value = response.json::<Value>().await?;
            let checksum = value
                .get("checksum")
                .and_then(Value::as_str)
                .map(str::to_string);
            let settings = value
                .get("settings")
                .cloned()
                .unwrap_or_else(|| value.clone());
            if !settings.is_object() {
                return Err(anyhow!("remote settings response must contain an object"));
            }
            Ok(RemoteSettingsFetch::Settings { settings, checksum })
        }
        StatusCode::NO_CONTENT | StatusCode::NOT_FOUND => Ok(RemoteSettingsFetch::Empty),
        StatusCode::NOT_MODIFIED => Ok(RemoteSettingsFetch::NotModified),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            Err(anyhow!("not authorized for remote settings"))
        }
        status => Err(anyhow!("remote settings request failed with {status}")),
    }
}

fn apply_remote_settings_cache_if_present(path: &Path, status: &str) -> bool {
    if path.is_file() {
        apply_remote_settings_cache(path, status);
        true
    } else {
        false
    }
}

fn apply_remote_settings_cache(path: &Path, status: &str) {
    std::env::set_var(REMOTE_SETTINGS_FILE_ENV, path);
    std::env::set_var(REMOTE_SETTINGS_CACHE_FILE_ENV, path);
    set_remote_settings_status(status);
}

fn set_remote_settings_status(status: &str) {
    std::env::set_var(REMOTE_SETTINGS_STATUS_ENV, status);
}

fn save_remote_settings_cache(path: &Path, settings: &Value, checksum: Option<&str>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(settings)?)?;
    set_owner_only_permissions(path)?;
    if let Some(checksum) = checksum {
        let meta = json!({ "checksum": checksum });
        let meta_path = remote_settings_meta_path(path);
        std::fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?)?;
        set_owner_only_permissions(&meta_path)?;
    }
    Ok(())
}

fn clear_remote_settings_cache(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(remote_settings_meta_path(path));
    if std::env::var_os(REMOTE_SETTINGS_FILE_ENV)
        .map(PathBuf::from)
        .is_some_and(|current| current == path)
    {
        std::env::remove_var(REMOTE_SETTINGS_FILE_ENV);
    }
}

fn remote_settings_cached_checksum(path: &Path) -> Option<String> {
    let value = std::fs::read_to_string(remote_settings_meta_path(path)).ok()?;
    let value: Value = serde_json::from_str(&value).ok()?;
    value
        .get("checksum")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn remote_settings_meta_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.meta.json",
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("remote-settings")
    ))
}

fn register_cleanup_handlers() {
    if DEFAULT_CLEANUP_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    register_cleanup(|| async {
        kiana_tools::team_create::cleanup_session_teams()
            .map(|_| ())
            .map_err(anyhow::Error::msg)
    });
    register_cleanup(|| async {
        kiana_tools::lsp_tool::shutdown_lsp_clients().await;
        Ok(())
    });
}

async fn ensure_scratchpad() {
    if !scratchpad_enabled() {
        return;
    }
    match ensure_scratchpad_dir() {
        Ok(path) => {
            std::env::set_var("KIANA_SCRATCHPAD_DIR", path);
        }
        Err(error) => {
            eprintln!("scratchpad init error: {error}");
        }
    }
}

async fn wait_for_shutdown_signal() -> i32 {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
            .expect("install SIGHUP handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => 130,
            _ = sigterm.recv() => 143,
            _ = sighup.recv() => 129,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        130
    }
}

fn scratchpad_enabled() -> bool {
    !env_truthy("KIANA_DISABLE_SCRATCHPAD")
}

fn ensure_scratchpad_dir() -> Result<PathBuf> {
    let path = scratchpad_dir();
    std::fs::create_dir_all(&path)?;
    set_owner_only_permissions(&path)?;
    Ok(path)
}

fn scratchpad_dir() -> PathBuf {
    if let Ok(path) = std::env::var("KIANA_SCRATCHPAD_DIR") {
        return PathBuf::from(path);
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    std::env::temp_dir()
        .join("kiana")
        .join(sanitize_path_component(&cwd))
        .join(std::process::id().to_string())
        .join("scratchpad")
}

fn sanitize_path_component(path: &Path) -> String {
    let value = path.to_string_lossy();
    let mut sanitized: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    while sanitized.contains("__") {
        sanitized = sanitized.replace("__", "_");
    }
    sanitized = sanitized.trim_matches('_').to_string();
    if sanitized.is_empty() {
        "root".to_string()
    } else {
        sanitized.chars().take(96).collect()
    }
}

fn env_truthy(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn set_owner_only_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
fn reset_for_tests() {
    cleanup_handlers().lock().unwrap().clear();
    CLEANUP_RAN.store(false, Ordering::SeqCst);
    DEFAULT_CLEANUP_REGISTERED.store(false, Ordering::SeqCst);
    SIGNAL_HANDLERS_REGISTERED.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-init-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    fn clear_remote_settings_env() {
        for key in [
            "KIANA_DISABLE_REMOTE_SETTINGS",
            "KIANA_REMOTE_SETTINGS_URL",
            "CLAUDE_REMOTE_SETTINGS_URL",
            REMOTE_SETTINGS_FILE_ENV,
            REMOTE_SETTINGS_CACHE_FILE_ENV,
            REMOTE_SETTINGS_STATUS_ENV,
            "KIANA_HOME",
            "KIANA_CONFIG_FILE",
            "KIANA_SETTINGS_FILE",
            "KIANA_SETTINGS_JSON",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_BASE_URL",
            "ANTHROPIC_MODEL",
            "CLAUDE_ACCESS_TOKEN",
        ] {
            std::env::remove_var(key);
        }
    }

    #[tokio::test]
    async fn cleanup_handlers_run_once_and_can_unregister() {
        reset_for_tests();
        let counter = Arc::new(AtomicUsize::new(0));
        let registered_counter = Arc::clone(&counter);
        register_cleanup(move || {
            let registered_counter = Arc::clone(&registered_counter);
            async move {
                registered_counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
        let unregistered_counter = Arc::clone(&counter);
        let registration = register_cleanup(move || {
            let unregistered_counter = Arc::clone(&unregistered_counter);
            async move {
                unregistered_counter.fetch_add(100, Ordering::SeqCst);
                Ok(())
            }
        });
        registration.unregister();

        run_cleanup_handlers().await.unwrap();
        run_cleanup_handlers().await.unwrap();

        assert_eq!(counter.load(Ordering::SeqCst), 1);
        reset_for_tests();
    }

    #[test]
    fn scratchpad_dir_uses_custom_env_and_owner_only_permissions() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        reset_for_tests();
        let root = temp_root("scratchpad");
        let scratchpad = root.join("scratch");
        std::env::set_var("KIANA_SCRATCHPAD_DIR", &scratchpad);

        let created = ensure_scratchpad_dir().unwrap();
        assert_eq!(created, scratchpad);
        assert!(created.is_dir());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&created).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o700);
        }

        std::env::remove_var("KIANA_SCRATCHPAD_DIR");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scratchpad_can_be_disabled_by_env() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var("KIANA_DISABLE_SCRATCHPAD", "true");
        assert!(!scratchpad_enabled());
        std::env::remove_var("KIANA_DISABLE_SCRATCHPAD");
    }

    #[tokio::test]
    async fn remote_settings_cache_is_applied_without_endpoint() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        clear_remote_settings_env();
        let root = temp_root("remote-cache");
        std::fs::create_dir_all(&root).unwrap();
        let cache = root.join("remote-settings.json");
        std::fs::write(
            &cache,
            r#"{"model":"remote-cache-model","settings":{"brief":true}}"#,
        )
        .unwrap();
        std::env::set_var("KIANA_REMOTE_SETTINGS_CACHE_FILE", &cache);

        init_remote_settings().await;

        assert_eq!(
            std::env::var(REMOTE_SETTINGS_FILE_ENV).unwrap(),
            cache.to_string_lossy()
        );
        assert_eq!(
            std::env::var(REMOTE_SETTINGS_STATUS_ENV).unwrap(),
            "cache_available"
        );
        let config = kiana_bootstrap::config::load_config();
        assert_eq!(config.model, "remote-cache-model");
        assert!(config.settings.brief);

        clear_remote_settings_env();
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn remote_settings_fetch_writes_cache_and_enables_overlay() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        clear_remote_settings_env();
        let root = temp_root("remote-fetch");
        std::fs::create_dir_all(&root).unwrap();
        let cache = root.join("remote-settings.json");
        let url = spawn_remote_settings_server(json!({
            "uuid": "settings-1",
            "checksum": "sha256:test",
            "settings": {
                "model": "remote-live-model",
                "settings": { "theme": "dark" }
            }
        }))
        .await;
        std::env::set_var("KIANA_REMOTE_SETTINGS_CACHE_FILE", &cache);
        std::env::set_var("KIANA_REMOTE_SETTINGS_URL", url);
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");

        init_remote_settings().await;

        assert_eq!(
            std::env::var(REMOTE_SETTINGS_STATUS_ENV).unwrap(),
            "remote_loaded"
        );
        assert_eq!(
            std::env::var(REMOTE_SETTINGS_FILE_ENV).unwrap(),
            cache.to_string_lossy()
        );
        let cached = std::fs::read_to_string(&cache).unwrap();
        assert!(cached.contains("remote-live-model"));
        assert_eq!(
            remote_settings_cached_checksum(&cache).as_deref(),
            Some("sha256:test")
        );
        let config = kiana_bootstrap::config::load_config();
        assert_eq!(config.model, "remote-live-model");
        assert_eq!(config.settings.theme.as_deref(), Some("dark"));

        clear_remote_settings_env();
        let _ = std::fs::remove_dir_all(root);
    }

    async fn spawn_remote_settings_server(body: Value) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0; 4096];
            let _ = socket.read(&mut buffer).await.unwrap();
            let body = body.to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{addr}/settings")
    }
}
