//! # 文件职责
//!
//! 本模块实现 Chrome 原生消息（Native Messaging）Host 一侧的 stdio 帧收发，并在 Unix 平台建立本地 socket，
//! 把浏览器扩展消息转发给连接进来的 MCP 客户端。它与 `lib.rs` 中的 stdio JSON-RPC MCP server
//! 是两条不同的传输入口：Native Host 负责扩展桥接，浏览器操作仍由另一侧 MCP client 发起。
//!
//! ```text
//! Chrome 扩展
//!   -> Native Messaging（二进制长度帧）: 4 字节 little-endian 长度 + JSON
//!   -> 本模块解析 ping/status 或转发 tool_response/notification
//!   <-> Unix domain socket（本机进程间 socket）上的 MCP 客户端（仅 Unix 实现）
//! ```
//!
//! `kiana-entrypoints` 的 `--chrome-native-host` 命令进入 `run_native_host_stdio()`。本模块不经过
//! DaemonHost、ControlPlane 或 CapabilityBroker；socket 接口和扩展消息不能视为 Kiana 主路径授权结果。
//! Windows/其他非 Unix 编译分支不会启动 socket listener。

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(unix)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, Mutex};

/// Native Host `get_status` 响应展示的硬编码协议实现版本字符串，不会从 Cargo package version 派生。
const VERSION: &str = "1.0.0";
/// 单帧 JSON payload 的最大字节数：1 MiB；长度前缀本身不计入 payload 上限。
const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// 使用进程 stdin/stdout 启动 Native Host 桥接。
///
/// Chrome Native Messaging 约定从 stdin 读取二进制长度帧，并把响应写到 stdout；诊断信息不能随意
/// println 到 stdout，否则会破坏帧格式。本函数把标准 I/O 交给可注入 reader/writer 的通用循环。
pub async fn run_native_host_stdio() -> Result<()> {
    serve_native_host(tokio::io::stdin(), tokio::io::stdout()).await
}

/// 持续读取扩展发来的 Native Messaging 帧，并将其交给 `ChromeNativeHost` 处理。
///
/// 【输入/输出】reader 提供 4 字节长度前缀和 UTF-8 JSON；writer 接收编码后的响应帧。
///
/// 【核心流程】创建共享 Host -> Unix 上启动 socket listener -> 循环读一帧并解析/路由 -> EOF 时返回。
/// socket guard 在函数退出时释放，读帧、解析后的写回或建 socket 失败会作为 Err 返回。
/// 非 Unix 平台的 listener 是空操作，因此本循环仍可处理 ping/status，但没有本机 MCP socket 对端。
pub async fn serve_native_host<R, W>(mut reader: R, writer: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let host = ChromeNativeHost::new(writer);
    let _socket_guard = host.start_socket_server().await?;

    while let Some(message) = read_chrome_message(&mut reader).await? {
        host.handle_message(&message).await?;
    }

    Ok(())
}

struct ChromeNativeHost<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    /// Native Messaging stdout writer；多个 async task 共享，写完整帧时用锁避免字节交错。
    writer: Arc<Mutex<W>>,
    /// 已连接 MCP socket 客户端的有界发送通道，键是本 listener 分配的本地 client ID。
    clients: Arc<Mutex<HashMap<usize, mpsc::Sender<Vec<u8>>>>>,
    #[cfg(unix)]
    /// 为并发接受的本机 socket 连接分配不同 ID。
    next_client_id: Arc<AtomicUsize>,
}

impl<W> ChromeNativeHost<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    /// 创建桥接状态；还没有客户端，也没有启动后台 listener。
    fn new(writer: W) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
            clients: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(unix)]
            next_client_id: Arc::new(AtomicUsize::new(1)),
        }
    }

    /// 解析一条已解帧的扩展 JSON 消息，并按 `type` 分流。
    ///
    /// ping/status 直接回到扩展；tool_response/notification 移除 `type` 后广播给 socket 客户端；
    /// 未知 type 返回扩展错误。无效 JSON 或缺失字符串 type 是可报告的消息级错误，写响应失败则向上返回。
    async fn handle_message(&self, message_json: &str) -> Result<()> {
        let raw: Value = match serde_json::from_str(message_json) {
            Ok(value) => value,
            Err(_) => {
                self.send_chrome_json(&json!({
                    "type": "error",
                    "error": "Invalid message format"
                }))
                .await?;
                return Ok(());
            }
        };

        let Some(message_type) = raw.get("type").and_then(Value::as_str) else {
            self.send_chrome_json(&json!({
                "type": "error",
                "error": "Invalid message format"
            }))
            .await?;
            return Ok(());
        };

        match message_type {
            "ping" => {
                self.send_chrome_json(&json!({
                    "type": "pong",
                    "timestamp": now_millis()
                }))
                .await?;
            }
            "get_status" => {
                self.send_chrome_json(&json!({
                    "type": "status_response",
                    "native_host_version": VERSION
                }))
                .await?;
            }
            "tool_response" | "notification" => {
                let mut forwarded = raw
                    .as_object()
                    .cloned()
                    .ok_or_else(|| anyhow!("native host message must be an object"))?;
                forwarded.remove("type");
                self.forward_to_mcp_clients(&Value::Object(forwarded)).await;
            }
            other => {
                self.send_chrome_json(&json!({
                    "type": "error",
                    "error": format!("Unknown message type: {other}")
                }))
                .await?;
            }
        }

        Ok(())
    }

    /// 将 JSON 值编码为 Native Messaging 二进制帧并完整写入扩展 stdout。
    ///
    /// 写锁覆盖整个帧和 flush，避免两个并发客户端通知把长度头与 payload 相互穿插。
    async fn send_chrome_json(&self, value: &Value) -> Result<()> {
        let frame = encode_chrome_message(value)?;
        let mut writer = self.writer.lock().await;
        writer.write_all(&frame).await?;
        writer.flush().await?;
        Ok(())
    }

    /// 把一条扩展消息复制到当前所有 MCP 客户端各自的有界队列。
    ///
    /// 先在 map 锁内克隆 sender，再在锁外 await 发送，避免等待慢客户端时禁止其他任务增删连接。
    /// 每个 client 队列容量为 32 帧；发送会受慢接收端背压。已关闭的队列在本轮后从 map 移除，
    /// 编码失败则静默丢弃，函数本身不把转发失败报告给扩展。
    async fn forward_to_mcp_clients(&self, value: &Value) {
        let Ok(frame) = encode_chrome_message(value) else {
            return;
        };
        let senders = {
            let clients = self.clients.lock().await;
            clients
                .iter()
                .map(|(id, sender)| (*id, sender.clone()))
                .collect::<Vec<_>>()
        };

        let mut disconnected = Vec::new();
        for (id, sender) in senders {
            if sender.send(frame.clone()).await.is_err() {
                disconnected.push(id);
            }
        }
        if !disconnected.is_empty() {
            let mut clients = self.clients.lock().await;
            for id in disconnected {
                clients.remove(&id);
            }
        }
    }

    #[cfg(unix)]
    /// 在用户私有目录启动 Unix socket listener，并为它返回生命周期 guard。
    ///
    /// 【核心流程】解析 socket 目录 -> 设置目录权限为 0700 -> 清理判定为陈旧的 `.sock` 文件 ->
    /// 绑定以当前 PID 命名的 socket -> 设置 socket 权限为 0600 -> 后台 accept -> 每个连接创建
    /// 容量为 32 的消息队列和 client task。
    ///
    /// 【并发与副作用】listener task 与当前 Host 共用 writer/clients；连接 ID 用原子计数器分配。
    /// 绑定、目录创建或权限设置失败会让 Native Host 启动失败。drop guard 会 abort listener 并移除
    /// socket 路径，但没有统一记录/中止其已 spawn 的 client task。
    async fn start_socket_server(&self) -> Result<NativeHostSocketGuard> {
        use tokio::net::UnixListener;

        let socket_dir = socket_dir();
        if socket_dir.exists() && !socket_dir.is_dir() {
            std::fs::remove_file(&socket_dir)?;
        }
        std::fs::create_dir_all(&socket_dir)?;
        set_unix_permissions(&socket_dir, 0o700)?;

        // 旧进程退出后可能留下 pathname；清理函数按文件名中的 PID 判断是否仍在运行。
        cleanup_stale_sockets(&socket_dir);
        let socket_path = socket_dir.join(format!("{}.sock", std::process::id()));
        if socket_path.exists() {
            // 即使路径存在也直接移除；此处没有先连接探测该路径是否仍被同进程的另一实例使用。
            std::fs::remove_file(&socket_path)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        set_unix_permissions(&socket_path, 0o600)?;

        let writer = self.writer.clone();
        let clients = self.clients.clone();
        let next_client_id = self.next_client_id.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    break;
                };
                // 多个 accept 后的 task 可能并发分配 ID；原子递增保证每次读取到不同计数值，
                // SeqCst 为这些递增建立单一全序，避免再给计数器加一把 async 锁。
                let id = next_client_id.fetch_add(1, Ordering::SeqCst);
                // 每个连接用独立有界队列，广播端可按断开的 client 清理 sender。
                let (sender, receiver) = mpsc::channel(32);
                clients.lock().await.insert(id, sender);
                let _ =
                    send_chrome_json_to_writer(&writer, &json!({ "type": "mcp_connected" })).await;
                tokio::spawn(handle_mcp_client(
                    id,
                    socket,
                    clients.clone(),
                    writer.clone(),
                    receiver,
                ));
            }
        });

        Ok(NativeHostSocketGuard {
            path: Some(socket_path),
            task: Some(task),
        })
    }

    #[cfg(not(unix))]
    /// 非 Unix 平台当前没有对应 socket 传输实现，返回一个不持有资源的 guard。
    async fn start_socket_server(&self) -> Result<NativeHostSocketGuard> {
        Ok(NativeHostSocketGuard {
            path: None,
            task: None,
        })
    }
}

struct NativeHostSocketGuard {
    /// listener bind 成功时记录的 socket 路径；drop 时删除。
    path: Option<PathBuf>,
    /// accept 循环的后台任务句柄；drop 时 abort。
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for NativeHostSocketGuard {
    /// 尝试释放 listener 和 pathname，避免正常退出后残留可见 socket 文件。
    ///
    /// 这是同步 Drop，无法 await 客户端 task 收尾；socket 文件删除错误也被忽略，因此只属于 best-effort
    /// 清理，不表示所有客户端 task 都退出或已连上的 peer 都收到了断开通知。
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(unix)]
/// 维护一个 MCP socket 连接的双向生命周期：MCP 请求转发给扩展，扩展消息由队列送回该 socket。
///
/// read/write halves 被拆开后，写 task 等待 bounded mpsc 队列，主 task 独立读请求帧。收到 EOF、
/// 帧错误或读 socket 出错会进入清理；JSON 无效或缺 method 的单帧会跳过。当前转发只带 method/params，
/// 不保留传入 JSON-RPC request ID，且扩展返回会广播到全部已连客户端，这是本协议桥的路由边界。
async fn handle_mcp_client<W>(
    id: usize,
    socket: tokio::net::UnixStream,
    clients: Arc<Mutex<HashMap<usize, mpsc::Sender<Vec<u8>>>>>,
    writer: Arc<Mutex<W>>,
    mut receiver: mpsc::Receiver<Vec<u8>>,
) where
    W: AsyncWrite + Unpin + Send + 'static,
{
    let (mut read_half, mut write_half) = socket.into_split();
    // 单独任务负责把扩展广播来的帧写向 socket；主循环可继续等待 MCP 请求。
    let write_task = tokio::spawn(async move {
        while let Some(frame) = receiver.recv().await {
            if write_half.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    loop {
        match read_chrome_message(&mut read_half).await {
            Ok(Some(message)) => {
                let Ok(request) = serde_json::from_str::<Value>(&message) else {
                    continue;
                };
                let Some(method) = request.get("method").and_then(Value::as_str) else {
                    continue;
                };
                let params = request.get("params").cloned().unwrap_or(Value::Null);
                let _ = send_chrome_json_to_writer(
                    &writer,
                    &json!({
                        "type": "tool_request",
                        "method": method,
                        "params": params
                    }),
                )
                .await;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    // 先移除 sender，阻止后续广播继续排队，再通知扩展并停止写 task。
    clients.lock().await.remove(&id);
    let _ = send_chrome_json_to_writer(&writer, &json!({ "type": "mcp_disconnected" })).await;
    write_task.abort();
}

#[cfg(unix)]
/// Native Host 内部复用的并发安全帧写入 helper。
async fn send_chrome_json_to_writer<W>(writer: &Arc<Mutex<W>>, value: &Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let frame = encode_chrome_message(value)?;
    let mut writer = writer.lock().await;
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

/// 读取并解码一帧 Native Messaging 输入。
///
/// 先读取 4 字节 little-endian 长度，再拒绝 0 或大于 1 MiB 的 payload，然后读取完整字节并要求 UTF-8。
/// header 读取遇到 UnexpectedEof 会按流结束返回 None；payload 中途 EOF、其他 I/O 错误或非法 UTF-8 返回 Err。
async fn read_chrome_message<R>(reader: &mut R) -> Result<Option<String>>
where
    R: AsyncRead + Unpin,
{
    let mut length_bytes = [0_u8; 4];
    match reader.read_exact(&mut length_bytes).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(error.into()),
    }

    // Native Messaging 帧头固定为无符号 32 位 little-endian 字节数。
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length == 0 || length > MAX_MESSAGE_SIZE {
        return Err(anyhow!("invalid Chrome native message length: {length}"));
    }

    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    Ok(Some(String::from_utf8(payload)?))
}

/// 将 JSON 编码为 Native Messaging 帧，并在分配最终帧前检查 payload 大小。
///
/// 返回布局为 4 字节 little-endian 长度 + JSON UTF-8 bytes；编码失败、空 payload 或超过 1 MiB 返回 Err。
fn encode_chrome_message(value: &Value) -> Result<Vec<u8>> {
    let payload = serde_json::to_vec(value)?;
    if payload.is_empty() || payload.len() > MAX_MESSAGE_SIZE {
        return Err(anyhow!(
            "Chrome native message size {} exceeds allowed range",
            payload.len()
        ));
    }
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// 生成 Unix epoch 毫秒时间戳，供扩展 `pong` 消息显示；系统时钟早于 epoch 时退回 0。
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(unix)]
/// 选择 Unix socket 目录；专用环境变量优先，否则按用户名放在 `/tmp` 下。
fn socket_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("KIANA_CHROME_NATIVE_HOST_SOCKET_DIR") {
        return PathBuf::from(path);
    }
    PathBuf::from(format!("/tmp/claude-mcp-browser-bridge-{}", username()))
}

#[cfg(unix)]
/// 扫描 socket 目录，删除文件名可解析出 PID 且当前被判为未运行的 `.sock` 项。
///
/// ⚠ `process_is_running()` 使用 `/proc/<pid>`，但此函数的 cfg 是所有 Unix；在没有 `/proc` 的 Unix
/// 系统上，存活进程也可能被判为不存在。该 helper 还只根据名字中的 PID 判断，不会尝试连接 socket。
fn cleanup_stale_sockets(socket_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(socket_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("sock") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let Ok(pid) = stem.parse::<i32>() else {
            continue;
        };
        if process_is_running(pid) {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(unix)]
/// 用 Linux `/proc` 目录存在性近似判断 PID 是否存活。
fn process_is_running(pid: i32) -> bool {
    std::path::Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(unix)]
/// 设置目录或 socket 文件的 Unix mode；调用方传入 0700/0600 限制其他本机用户访问。
fn set_unix_permissions(path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
/// 取 socket 默认目录名中的用户名；USER/USERNAME 都无有效值时使用固定 fallback。
fn username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(test)]
/// Native Messaging 帧与消息分流的本地夹具；这些测试不启动真实 Chrome 扩展或产品授权链。
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::Mutex as StdMutex;
    use std::task::{Context, Poll};

    #[derive(Clone, Default)]
    struct SharedWriter {
        bytes: Arc<StdMutex<Vec<u8>>>,
    }

    impl SharedWriter {
        fn bytes(&self) -> Vec<u8> {
            self.bytes.lock().unwrap().clone()
        }
    }

    impl AsyncWrite for SharedWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn encodes_and_reads_native_message_frames() {
        let frame = encode_chrome_message(&json!({ "type": "ping" })).unwrap();
        let mut reader = &frame[..];

        let message = read_chrome_message(&mut reader).await.unwrap().unwrap();

        assert_eq!(
            serde_json::from_str::<Value>(&message).unwrap()["type"],
            "ping"
        );
    }

    #[tokio::test]
    async fn responds_to_ping_and_status() {
        let writer = SharedWriter::default();
        let host = ChromeNativeHost::new(writer.clone());

        host.handle_message(r#"{"type":"ping"}"#).await.unwrap();
        host.handle_message(r#"{"type":"get_status"}"#)
            .await
            .unwrap();

        let responses = decode_all_frames(&writer.bytes());
        assert_eq!(responses[0]["type"], "pong");
        assert!(responses[0]["timestamp"].as_u64().is_some());
        assert_eq!(responses[1]["type"], "status_response");
        assert_eq!(responses[1]["native_host_version"], VERSION);
    }

    #[tokio::test]
    async fn invalid_and_unknown_messages_return_errors() {
        let writer = SharedWriter::default();
        let host = ChromeNativeHost::new(writer.clone());

        host.handle_message("not-json").await.unwrap();
        host.handle_message(r#"{"type":"missing"}"#).await.unwrap();

        let responses = decode_all_frames(&writer.bytes());
        assert_eq!(responses[0]["type"], "error");
        assert_eq!(responses[0]["error"], "Invalid message format");
        assert_eq!(responses[1]["type"], "error");
        assert_eq!(responses[1]["error"], "Unknown message type: missing");
    }

    fn decode_all_frames(bytes: &[u8]) -> Vec<Value> {
        let mut offset = 0;
        let mut values = Vec::new();
        while offset + 4 <= bytes.len() {
            let length = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            let payload = &bytes[offset..offset + length];
            offset += length;
            values.push(serde_json::from_slice(payload).unwrap());
        }
        values
    }
}
