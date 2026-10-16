//! Kiana 进程级初始化、清理和本地配置缓存。
//!
//! `init` 只负责准备入口进程需要的环境、信号处理、远程设置缓存、首次启动标记和
//! scratchpad；它不创建模型执行循环，也不授予工具能力。真正的请求授权与生命周期仍由
//! `DaemonHost`/`ControlPlane` 处理。这里使用的环境变量是启动期间的配置投影，不能把它们
//! 当作 EventLog、审批记录或持久运行状态。
//!
//! 远程设置采用“缓存先行、网络可选”的策略：读取和保存缓存失败会被记录为状态文字，
//! 但不会伪造已加载的设置；首次启动文件使用 create-new 和 owner-only 权限，尽量避免
//! 并发初始化覆盖用户记录。清理处理器只运行一次，单个处理器失败会汇总后返回。

use anyhow::{anyhow, Result};
use reqwest::StatusCode;
use serde_json::{json, Value};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// 进程内遥测初始化闸门；只记住本进程是否启动过初始化，不代表远程发送成功。
static TELEMETRY_INITIALIZED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// 防止重复安装 Ctrl-C / Unix signal 等退出监听任务。
static SIGNAL_HANDLERS_REGISTERED: AtomicBool = AtomicBool::new(false);
/// 防止同一进程把默认 Team/LSP 清理器重复注册多次。
static DEFAULT_CLEANUP_REGISTERED: AtomicBool = AtomicBool::new(false);
/// 全部 cleanup handlers 的进程级“一次执行”闸门。
static CLEANUP_RAN: AtomicBool = AtomicBool::new(false);
/// 为 cleanup 注册表分配 ID 的进程级递增计数器。
static NEXT_CLEANUP_ID: AtomicU64 = AtomicU64::new(1);
/// 远程设置正文的环境变量名；下游配置加载器据此读取缓存路径。
const REMOTE_SETTINGS_FILE_ENV: &str = "KIANA_REMOTE_SETTINGS_FILE";
/// 远程设置缓存路径的环境变量名。
const REMOTE_SETTINGS_CACHE_FILE_ENV: &str = "KIANA_REMOTE_SETTINGS_CACHE_FILE";
/// 远程设置本次启动的来源/失败状态环境变量名；它只是进程内诊断投影。
const REMOTE_SETTINGS_STATUS_ENV: &str = "KIANA_REMOTE_SETTINGS_STATUS";
/// 可覆盖首次启动记录文件位置的环境变量名。
const FIRST_START_FILE_ENV: &str = "KIANA_FIRST_START_FILE";
/// 首次启动记录写入结果的进程内状态环境变量名。
const FIRST_START_STATUS_ENV: &str = "KIANA_FIRST_START_STATUS";
/// 首次启动 JSON 记录使用的 schema 标识。
const FIRST_START_SCHEMA: &str = "kiana.first-start.v1";

/// 将不同闭包产生的异步清理工作统一成可放入同一注册表的 future 类型。
type CleanupFuture = Pin<Box<dyn Future<Output = Result<()>> + Send>>;
/// 可复制的清理回调；Arc 允许 `run_cleanup_handlers()` 在释放注册表锁后继续执行回调。
type CleanupFn = Arc<dyn Fn() -> CleanupFuture + Send + Sync>;

/// 延迟初始化进程级清理注册表。
///
/// Mutex 只保护注册/快照操作，运行异步 handler 时不持锁，避免一个 handler await 期间阻塞注册表访问。
fn cleanup_handlers() -> &'static Mutex<Vec<(u64, CleanupFn)>> {
    static HANDLERS: OnceLock<Mutex<Vec<(u64, CleanupFn)>>> = OnceLock::new();
    HANDLERS.get_or_init(|| Mutex::new(Vec::new()))
}

#[derive(Debug, Clone)]
/// 已注册的进程清理处理器句柄；持有它的调用方可以在退出前撤销对应注册。
///
/// 该值只携带注册表 ID，不拥有或直接运行清理 future；注册表保留实际回调。
pub struct CleanupRegistration {
    /// 注册表按注册调用分配的 ID，用于精确移除本次注册。
    id: u64,
}

impl CleanupRegistration {
    /// 注销该清理处理器；重复调用是幂等的。
    ///
    /// 注销只从共享注册表移除 ID；如果 `run_cleanup_handlers()` 已复制本轮快照，快照中的 Arc 仍可能执行，
    /// 因而注销不能撤销已快照或已开始的回调，也不会影响其他注册。
    pub fn unregister(&self) {
        cleanup_handlers()
            .lock()
            .unwrap()
            .retain(|(id, _)| id != &self.id);
    }
}

/// 注册一个进程退出前运行的异步清理处理器。
///
/// 注册顺序决定运行顺序。闭包会被保存为 `Arc`，因此必须是 `Send + Sync + 'static`；
/// 返回的句柄可在退出前注销。注册本身不执行清理，也不保证跨进程或崩溃场景一定运行。
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

/// 将本次注册表快照中的清理处理器按注册顺序串行运行。
///
/// 使用一次性原子闸门保证整个进程最多执行一轮；即使某个处理器返回 `Err`，也会继续运行快照中的
/// 其余处理器并在最后汇总错误。失败结果不会重试，之后再次调用会因闸门已置位而返回 `Ok(())`。
/// 成功返回只表示处理器返回了 `Ok(())`，不证明外部资源已被对方系统释放。
/// ⚠ 闸门先于注册表快照设置。若另一个任务恰好在快照之后才注册 handler，新 handler 不属于本轮；
/// 后续调用又会被一次性闸门拦住，因此不能把并发晚注册的 handler 当作一定会执行。
/// handler 若 panic 而非返回 `Err`，当前代码没有捕获 panic，后续 handler 会被跳过。
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

/// 执行 CLI 入口所需的初始化顺序。
///
/// 顺序刻意先准备安全环境和信号处理，再加载远程设置/首次启动状态，最后注册清理和
/// scratchpad。若某个当前实现为空的兼容钩子未来获得副作用，应继续保持它不绕过
/// ControlPlane 的约束；本函数本身不启动 harness。
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

/// 在项目 trust 已确认后异步初始化遥测。
///
/// 当前实现只设置一次进程内闸门；调用方不得把该函数的返回视为远程上报成功，也不应在
/// trust 之前调用它来加载项目本地资源。
pub fn init_telemetry_after_trust() {
    tokio::spawn(async {
        let _ = init_telemetry().await;
    });
}

/// 进程内只初始化一次 meter adapter；原子标志只拦重复调用，不代表任何遥测已发出。
async fn init_telemetry() -> Result<()> {
    if TELEMETRY_INITIALIZED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(());
    }
    set_meter_state().await
}

/// 设置遥测 meter adapter 的状态占位。
///
/// 当前函数体没有实际 adapter 副作用，只返回成功；调用链保留异步 `Result` 形状以便以后接入统一处理。
async fn set_meter_state() -> Result<()> {
    // 当前 meter adapter 尚未实现；保留 Result 形状供异步遥测调用链统一处理。
    Ok(())
}

/// 运行清理处理器后以给定退出码终止当前进程。
///
/// 清理失败只写入标准错误，不覆盖调用方指定的退出码；该函数永不返回，适合信号处理
/// 协程使用。处理器按注册顺序逐个 await，未设置单个处理器超时；某个处理器一直不返回时，后续处理器
/// 和 `process::exit()` 都无法到达。它不保证操作系统已回收所有子进程或网络连接。
pub async fn graceful_shutdown(exit_code: i32) -> ! {
    if let Err(error) = run_cleanup_handlers().await {
        eprintln!("cleanup error: {error}");
    }
    std::process::exit(exit_code);
}

// 这些名称保留初始化顺序中的扩展点；当前函数体为空，调用它们不会加载配置、改环境变量或安装证书。
fn enable_configs() {}
fn apply_safe_env_vars() {}
fn apply_ca_certs() {}
/// 在当前 Tokio runtime 中启动后台信号监听器；重复 init 只保留一个 listener task。
///
/// `tokio::spawn` 的 JoinHandle 未保存，任务在 runtime 中独立等待；收到信号后自行执行 graceful shutdown。
/// Unix signal handler 注册失败会在后台任务里 `expect` panic，不会作为 `init()` 的 Result 传回 main。
fn setup_graceful_shutdown() {
    if SIGNAL_HANDLERS_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async {
        let exit_code = wait_for_shutdown_signal().await;
        graceful_shutdown(exit_code).await;
    });
}
// 以下 hook 目前也都是空函数；名称表示预留的初始化位置，不代表对应能力已在当前版本实现。
fn init_1p_event_logging() {}
fn populate_oauth_if_needed() {}
fn init_jetbrains_detection() {}
fn detect_repository() {}
/// 尝试恢复缓存的远程设置，再按可选 endpoint 刷新；所有失败都记录为状态并回退，不阻断 CLI 启动。
///
/// 【调用者】`init()` 在配置路径准备后调用一次。
/// 【数据流】disable 开关 → 本地 cache 路径投影 → 可选 HTTP fetch/条件请求 → 更新 cache 与进程环境指针；
/// 204/404 或空对象会清除旧 cache，未修改响应继续使用当前 cache，网络/权限/落盘错误尽量沿用已有路径。
/// 【副作用】读写用户配置目录文件并设置 `REMOTE_SETTINGS_*` 环境变量；不将远程配置写入 EventLog。
/// 【失败边界】这里的 `cache_available` 只表示 `Path::is_file()` 为真，不会先解析/验证正文；远程失败不会
/// 作为 Err 传出，因此状态变量是入口诊断提示，不是 live fetch 或下游成功加载配置的证明。
async fn init_remote_settings() {
    // 先应用本地缓存，再尝试网络刷新。这样网络不可用时仍能保持上一次明确写入的
    // 配置，但状态变量会标记来源，避免把 cache 当作 live fetch 证据。
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
/// 尝试写入首次启动记录，并把 created/existing/error 转成进程内状态供入口诊断。
///
/// `record_first_start_at()` 的错误不向外传播，所以引导记录失败不会阻断 CLI 启动；调用者可读取
/// `FIRST_START_STATUS_ENV` 区分首次创建、已存在和写入失败。
fn record_first_start() {
    match record_first_start_at(&first_start_path()) {
        Ok(FirstStartRecordResult::Created) => set_first_start_status("created"),
        Ok(FirstStartRecordResult::Existing) => set_first_start_status("existing"),
        Err(error) => {
            set_first_start_status(&format!("error:{error}"));
        }
    }
}
// 这些启动钩子在当前版本均为空实现；调用顺序存在不等于相关能力已经配置。
fn configure_mtls() {}
fn configure_proxy() {}
fn preconnect_api() {}
// 当前上游代理/Windows shell 初始化钩子为空，不能把它们当成已实施的隔离或平台适配。
async fn init_upstream_proxy() {}
fn set_shell_windows() {}

/// 写入首次启动记录时区分“本调用创建”与“文件已由本地/并发流程创建”。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstStartRecordResult {
    /// 当前调用通过 create-new 成功创建记录。
    Created,
    /// 文件已存在，或并发竞争中由另一调用先创建。
    Existing,
}

/// 解析首次启动记录位置；本函数只读环境变量，不创建目录或读写文件。
///
/// 非空 `KIANA_FIRST_START_FILE` 优先，其次使用 `kiana_home_dir()/first-start.json`。返回值可以是相对路径，
/// 此处不规范化路径或检查权限；实际创建发生在 `record_first_start_at()`。
pub fn first_start_path() -> PathBuf {
    // 显式路径只用于本地引导记录；空值回退到 KIANA_HOME，避免把空字符串当成当前目录
    // 下的隐藏状态文件。
    if let Ok(path) = std::env::var(FIRST_START_FILE_ENV) {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    kiana_home_dir().join("first-start.json")
}

/// 读取首次启动 JSON 状态。
///
/// 文件不存在返回 `Ok(None)`；存在但无法读取或不是合法 JSON 时返回错误，避免把损坏的
/// onboarding 记录静默当作“从未启动”。返回值是原始 JSON；本函数只解析 JSON 语法，不验证
/// `FIRST_START_SCHEMA` 或 onboarding 字段语义，具体字段演进由后续消费者处理。
pub fn load_first_start_state() -> Result<Option<Value>> {
    let path = first_start_path();
    if !path.is_file() {
        return Ok(None);
    }
    let value = std::fs::read_to_string(path)?;
    Ok(Some(serde_json::from_str(&value)?))
}

/// 在指定路径并发安全地创建首次启动 JSON 记录。
///
/// 【调用者】`record_first_start()` 使用环境变量解析后的路径调用。
/// 【核心流程】已有路径直接返回 Existing；否则准备父目录/权限、生成首次启动时间与 onboarding 默认项，
/// 再用 `create_new` 让并发初始化中只有一个调用成为创建者，最后写入并 `sync_all()`。
/// 【副作用/失败】创建目录、创建文件、写数据、同步并收紧权限；I/O 错误返回给外层后仅记录状态，不阻断 CLI。
/// ⚠ 如果 create_new 成功后写入、sync 或 chmod 失败，当前函数不会删除不完整文件；后续调用会因路径已存在
/// 而返回 Existing，`load_first_start_state()` 则可能持续读到损坏数据。只记录现状，不在注释任务中修改。
fn record_first_start_at(path: &Path) -> Result<FirstStartRecordResult> {
    if path.exists() {
        return Ok(FirstStartRecordResult::Existing);
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
        set_owner_only_permissions(parent)?;
    }
    let created_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let payload = json!({
        "schema": FIRST_START_SCHEMA,
        "created_at_unix": created_at_unix,
        "onboarding": {
            "status": "pending",
            "recommended_checks": [
                "kiana auth status --json",
                "kiana doctor --json",
                "kiana config init"
            ]
        }
    });
    // create_new 让并发初始化中的胜者只有一个；AlreadyExists 被视为正常的 Existing，
    // 不覆盖先写入者的 onboarding 状态。
    let mut file = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Ok(FirstStartRecordResult::Existing);
        }
        Err(error) => return Err(error.into()),
    };
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(&payload)?)?;
    file.sync_all()?;
    set_owner_only_permissions(path)?;
    Ok(FirstStartRecordResult::Created)
}

/// 把首次启动记录的本次处理结果写入环境变量；它是启动期状态提示，不是持久化账本。
fn set_first_start_status(status: &str) {
    std::env::set_var(FIRST_START_STATUS_ENV, status);
}

/// 一次远程设置请求解析后的三种协议结果；网络/I/O 细节留在 `fetch_remote_settings()`。
enum RemoteSettingsFetch {
    /// 有新的设置正文；checksum 可用于后续条件请求，但可能缺失。
    Settings {
        settings: Value,
        checksum: Option<String>,
    },
    /// 服务端声明客户端所持版本未变化，应尝试继续使用本地 cache。
    NotModified,
    /// 服务端无内容或明确返回空设置，需要移除本地 cache。
    Empty,
}

/// 读取远程设置地址：优先 `KIANA_REMOTE_SETTINGS_URL`，其次兼容 `CLAUDE_REMOTE_SETTINGS_URL`。
///
/// 这里只做空值过滤，不验证 URL scheme、host 或 TLS；真正请求在 `fetch_remote_settings()`。
fn remote_settings_endpoint() -> Option<String> {
    std::env::var("KIANA_REMOTE_SETTINGS_URL")
        .or_else(|_| std::env::var("CLAUDE_REMOTE_SETTINGS_URL"))
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// 解析远程设置缓存文件位置；环境变量优先，否则落到 Kiana home 下的 `remote-settings.json`。
///
/// 本函数只选择字符串路径，不验证其存在性、类型、owner 或读写权限；读写错误由缓存 helper 返回/记录。
fn remote_settings_cache_path() -> PathBuf {
    if let Ok(path) = std::env::var(REMOTE_SETTINGS_CACHE_FILE_ENV) {
        let path = path.trim();
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    kiana_home_dir().join("remote-settings.json")
}

/// 解析用户级 Kiana 数据目录：`KIANA_HOME` → `$HOME/.kiana` → 系统临时目录下的 `kiana`。
///
/// 环境变量首尾空白会被裁掉；这里只生成路径，不创建目录、不要求它是绝对路径。
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

/// 向配置 URL 请求远程设置，并把状态码解码为 Settings/NotModified/Empty。
///
/// 【调用者】`init_remote_settings()`；网络错误和拒绝最终由它决定是否沿用本地缓存。
/// 【请求】10 秒总超时；优先使用非空 `ANTHROPIC_API_KEY`，仅当该环境变量不存在时才尝试
/// `CLAUDE_ACCESS_TOKEN`；缓存 checksum 存在时放进 `If-None-Match` 请求头。
/// 【响应】200 需含 object 形式的 `settings`（或顶层 object）；204/404 视为 Empty，304 视为未变化，
/// 401/403 和其他状态都返回错误。此函数不写 cache，也不把错误升级为 CLI 启动失败。
/// ⚠ endpoint 来自环境变量且本函数不强制 HTTPS/host allowlist；若凭据环境变量已设置，会把凭据发送到
/// 配置的 endpoint。该地址必须来自可信配置来源。本提示只解释当前信任边界，不增加网络限制。
async fn fetch_remote_settings(
    endpoint: &str,
    cached_checksum: Option<String>,
) -> Result<RemoteSettingsFetch> {
    // 远程设置请求有固定超时，并只从环境中读取已有认证材料。响应必须是 JSON 对象，
    // 其他形状拒绝缓存，避免后续配置加载把任意标量解释成合法设置。
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

/// 若缓存路径是普通文件，就把它登记为下游配置加载路径并设置来源状态。
///
/// 返回 true 只说明路径检查通过并完成环境变量投影；这里没有解析/校验 JSON 内容。
fn apply_remote_settings_cache_if_present(path: &Path, status: &str) -> bool {
    if path.is_file() {
        apply_remote_settings_cache(path, status);
        true
    } else {
        false
    }
}

/// 将同一路径同时登记为远程设置正文与 cache 路径，并记录此次选择的来源状态。
///
/// 这里只修改当前进程环境，真正读取配置由后续消费者完成；该状态不等于配置已被所有组件成功应用。
fn apply_remote_settings_cache(path: &Path, status: &str) {
    std::env::set_var(REMOTE_SETTINGS_FILE_ENV, path);
    std::env::set_var(REMOTE_SETTINGS_CACHE_FILE_ENV, path);
    set_remote_settings_status(status);
}

/// 设置远程配置加载诊断状态；它是环境变量投影，不写 EventLog 或用户状态文件。
fn set_remote_settings_status(status: &str) {
    std::env::set_var(REMOTE_SETTINGS_STATUS_ENV, status);
}

/// 把新的设置 object 与可选 checksum 分开写入本地 cache。
///
/// 【调用者】只在收到远程 Settings 时由 `init_remote_settings()` 调用。
/// 【副作用/失败】直接创建父目录并覆盖 settings 文件，收紧权限；有 checksum 时再写 meta 文件。任一步骤
/// 失败都会返回错误，但前面已经成功写入的文件不会自动回滚。它不是原子双文件事务。
/// ⚠ 响应没有 checksum 且磁盘上已有旧 meta 文件时，当前实现不会删除旧 meta；下一次请求可能仍使用
/// 旧 checksum 发送 `If-None-Match`，使缓存正文与条件请求标记不同步。此处只标记行为，不修改写入规则。
fn save_remote_settings_cache(path: &Path, settings: &Value, checksum: Option<&str>) -> Result<()> {
    // 设置主体与 checksum 元数据分开保存，便于 If-None-Match 只使用明确的校验值；文件
    // 写入后收紧权限，降低本地缓存泄露凭据或组织策略的风险。
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

/// 尽力删除设置正文及 checksum sidecar，并清除指向该缓存路径的当前环境变量。
///
/// 删除失败会被忽略，调用方仍把逻辑状态标记为 empty；读端因此还必须校验文件/策略，不能只依赖此状态。
/// ⚠ 当前只在 `REMOTE_SETTINGS_FILE_ENV` 恰好等于被清理路径时移除该变量，没有清除
/// `REMOTE_SETTINGS_CACHE_FILE_ENV`；后者可能继续指向已删除的缓存路径，后续重新解析 cache path 时仍会选中它。
/// 这里只记录当前行为，不改业务逻辑。
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

/// 尝试读取 checksum sidecar；不存在、I/O 错误、JSON 损坏或字段缺失时都降级成 None。
///
/// 因此返回 None 不区分“从未缓存”与“缓存元数据不可读”；调用方会退化为无条件 GET。
fn remote_settings_cached_checksum(path: &Path) -> Option<String> {
    let value = std::fs::read_to_string(remote_settings_meta_path(path)).ok()?;
    let value: Value = serde_json::from_str(&value).ok()?;
    value
        .get("checksum")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 由正文路径派生 sidecar 路径，例如 `remote-settings.json` → `remote-settings.meta.json`。
fn remote_settings_meta_path(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        "{}.meta.json",
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("remote-settings")
    ))
}

/// 注册由入口进程默认拥有的 Team 与 LSP 清理回调。
///
/// 只在第一次调用时登记；按当前函数体，执行时先清理 session teams，再关闭 LSP clients。注册不立即执行，
/// 实际调用由 `run_cleanup_handlers()` 或 signal shutdown 路径触发。
fn register_cleanup_handlers() {
    // one-time flag 先置位，避免并发 init 重复登记；cleanup 的实际执行发生在退出路径。
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

/// 按配置尽力创建 scratchpad，并把最终目录路径发布给同一进程中的后续工具。
///
/// 【输入】`KIANA_DISABLE_SCRATCHPAD` 开关与可选 `KIANA_SCRATCHPAD_DIR`。
/// 【输出/副作用】关闭时不操作；成功时设置 `KIANA_SCRATCHPAD_DIR`；目录或权限失败只写 stderr 并继续启动。
/// 这里复用 `ensure_scratchpad_dir()`，不参与 capability 授权或 workspace 写入范围判定。
async fn ensure_scratchpad() {
    // 这是尽力准备项：关闭开关直接跳过；目录/权限失败只记 stderr，不让 CLI 启动失败。
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

/// 等待进程退出信号，并把平台信号映射为 CLI 约定的进程退出码。
///
/// Unix 上 Ctrl-C/SIGINT → 130、SIGTERM → 143、SIGHUP → 129；非 Unix 仅等待 Ctrl-C 并返回 130。
/// 返回值传给 `graceful_shutdown()`，在那里先跑清理处理器再 `process::exit()`。
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

/// 除非 `KIANA_DISABLE_SCRATCHPAD` 是已识别的 truthy 值，否则允许初始化 scratchpad。
fn scratchpad_enabled() -> bool {
    !env_truthy("KIANA_DISABLE_SCRATCHPAD")
}

/// 创建本次进程使用的 scratchpad 目录并尝试收紧权限。
///
/// 【输入/输出】内部调用 `scratchpad_dir()` 解析路径；成功返回实际路径供 `ensure_scratchpad()` 写入环境变量。
/// 【失败】创建目录或设置 Unix 权限失败返回 anyhow error；上层只记录错误，不中断 CLI。
fn ensure_scratchpad_dir() -> Result<PathBuf> {
    let path = scratchpad_dir();
    std::fs::create_dir_all(&path)?;
    set_owner_only_permissions(&path)?;
    Ok(path)
}

/// 解析 scratchpad 路径：显式环境变量优先，否则用临时根目录、清理过的 cwd 和进程 ID 隔离。
///
/// 环境变量值在此直接转成 PathBuf，不检查非空/绝对路径；默认 cwd 获取失败时用 `.`，后续创建失败由上层记录。
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

/// 把 cwd 转为可安全拼入单个目录名的有限 ASCII 分量。
///
/// 分隔符/非允许字符改为 `_`，压缩重复下划线，裁到 96 个字符；这避免原始路径分隔符扩展目录层级，
/// 但不是哈希或唯一 ID，不同 cwd 可能得到相同分量。
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

/// 判断布尔环境开关；只把 `1/true/yes/on`（忽略大小写）视为真，不裁掉值两端空格。
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

/// 尽力把给定路径权限收紧为仅 owner 可访问的 Unix `0700`。
///
/// Unix 上不区分文件/目录，因此 regular file 也会带 owner execute 位；非 Unix 平台当前不改权限。
/// 此 helper 不检查路径当前类型，也不证明文件系统会跨平台按同一语义强制访问控制。
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

/// 清空测试使用的进程内 handler/一次性闸门状态。
///
/// 调用方必须让其他会读写这些静态值的测试静止；此函数本身不获取专用测试锁，也不会中止此前 spawn 的
/// signal listener。它只重置 cleanup/default/signal 标志，不重置遥测闸门和 ID 计数器。
#[cfg(test)]
fn reset_for_tests() {
    cleanup_handlers().lock().unwrap().clear();
    CLEANUP_RAN.store(false, Ordering::SeqCst);
    DEFAULT_CLEANUP_REGISTERED.store(false, Ordering::SeqCst);
    SIGNAL_HANDLERS_REGISTERED.store(false, Ordering::SeqCst);
}

/// 本模块的单元测试：验证清理器、scratchpad、首次启动记录及远程设置缓存/HTTP overlay。
///
/// 多数测试通过 `kiana-entrypoints::test_support::env_lock()` 串行访问进程环境；该锁只协调遵守约定的
/// 测试。`clear_remote_settings_env()` 直接移除值而不保存/恢复原值，因此所有调用点都应持有该锁。
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// 生成还不存在的临时测试目录候选名；真正创建由各个测试按需完成。
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

    /// 删除本模块测试会读取的 remote settings、首次启动及 provider 配置环境变量。
    ///
    /// 该 helper 不会恢复调用前的进程值；调用者必须先拿 `test_support::env_lock()`，避免与其他遵守同一
    /// 锁约定的环境变量测试交错。
    fn clear_remote_settings_env() {
        for key in [
            "KIANA_DISABLE_REMOTE_SETTINGS",
            "KIANA_REMOTE_SETTINGS_URL",
            "CLAUDE_REMOTE_SETTINGS_URL",
            REMOTE_SETTINGS_FILE_ENV,
            REMOTE_SETTINGS_CACHE_FILE_ENV,
            REMOTE_SETTINGS_STATUS_ENV,
            FIRST_START_FILE_ENV,
            FIRST_START_STATUS_ENV,
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

    /// 验证清理 handler 按注册/注销状态运行，且整个进程最多执行一次 cleanup pass。
    /// ⚠ 本测试直接调用 `reset_for_tests()`，但没有拿其他 init 测试使用的 `env_lock`；reset 会清空共享
    /// handler 注册表，可能与持锁的其他测试并发交错。此处只记录测试隔离风险，不改测试同步方式。
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

    /// 验证自定义 scratchpad 路径被创建，Unix 上目录权限收紧到 owner-only `0700`。
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

    /// 验证关闭开关的 truthy 值会使 scratchpad 准备逻辑跳过目录创建。
    #[test]
    fn scratchpad_can_be_disabled_by_env() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var("KIANA_DISABLE_SCRATCHPAD", "true");
        assert!(!scratchpad_enabled());
        std::env::remove_var("KIANA_DISABLE_SCRATCHPAD");
    }

    /// 验证首次调用创建 pending onboarding 记录，后续调用保留已有用户状态而不覆盖。
    #[test]
    fn init_records_first_start_once_under_kiana_home() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        clear_remote_settings_env();
        let root = temp_root("first-start");
        std::env::set_var("KIANA_HOME", &root);

        record_first_start();

        let path = root.join("first-start.json");
        assert_eq!(first_start_path(), path);
        assert_eq!(
            std::env::var(FIRST_START_STATUS_ENV).ok().as_deref(),
            Some("created")
        );
        let value = load_first_start_state().unwrap().unwrap();
        assert_eq!(value["schema"], FIRST_START_SCHEMA);
        assert_eq!(value["onboarding"]["status"], "pending");

        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&json!({
                "schema": FIRST_START_SCHEMA,
                "created_at_unix": 1,
                "onboarding": { "status": "complete" }
            }))
            .unwrap(),
        )
        .unwrap();
        record_first_start();

        assert_eq!(
            std::env::var(FIRST_START_STATUS_ENV).ok().as_deref(),
            Some("existing")
        );
        let value = load_first_start_state().unwrap().unwrap();
        assert_eq!(value["onboarding"]["status"], "complete");

        clear_remote_settings_env();
        let _ = std::fs::remove_dir_all(root);
    }

    /// 验证首次启动写入失败只记录状态，不把该错误升级为入口初始化失败。
    #[test]
    fn first_start_write_error_is_recorded_without_failing_init() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        clear_remote_settings_env();
        std::env::remove_var(FIRST_START_STATUS_ENV);
        let root = temp_root("first-start-error");
        let blocking_parent = root.join("blocking-parent");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&blocking_parent, b"not a directory").unwrap();
        std::env::set_var(
            FIRST_START_FILE_ENV,
            blocking_parent.join("first-start.json"),
        );

        record_first_start();

        assert!(std::env::var(FIRST_START_STATUS_ENV)
            .unwrap()
            .starts_with("error:"));

        std::env::remove_var(FIRST_START_FILE_ENV);
        std::env::remove_var(FIRST_START_STATUS_ENV);
        let _ = std::fs::remove_dir_all(root);
    }

    /// 没有远程 endpoint 时验证已有 cache 被投影给配置加载器并用于建立 overlay。
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

    /// 通过 loopback mock 返回一个 200 JSON 响应，核对 cache/meta 写入和下游配置 overlay。
    /// 该 mock 只接受一次请求，不覆盖 If-None-Match、认证拒绝、超时或 cache 回退分支。
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

    /// 启动一次性 loopback HTTP stub 并返回 endpoint；只读取一个请求并固定返回 200 JSON。
    ///
    /// 该 helper 不校验请求头/凭据，也没有循环 accept 或超时清理；测试退出时 listener task 随 runtime 收尾。
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
