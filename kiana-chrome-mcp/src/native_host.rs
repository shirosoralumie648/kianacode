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

const VERSION: &str = "1.0.0";
const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

pub async fn run_native_host_stdio() -> Result<()> {
    serve_native_host(tokio::io::stdin(), tokio::io::stdout()).await
}

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
    writer: Arc<Mutex<W>>,
    clients: Arc<Mutex<HashMap<usize, mpsc::Sender<Vec<u8>>>>>,
    #[cfg(unix)]
    next_client_id: Arc<AtomicUsize>,
}

impl<W> ChromeNativeHost<W>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    fn new(writer: W) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
            clients: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(unix)]
            next_client_id: Arc::new(AtomicUsize::new(1)),
        }
    }

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

    async fn send_chrome_json(&self, value: &Value) -> Result<()> {
        let frame = encode_chrome_message(value)?;
        let mut writer = self.writer.lock().await;
        writer.write_all(&frame).await?;
        writer.flush().await?;
        Ok(())
    }

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
    async fn start_socket_server(&self) -> Result<NativeHostSocketGuard> {
        use tokio::net::UnixListener;

        let socket_dir = socket_dir();
        if socket_dir.exists() && !socket_dir.is_dir() {
            std::fs::remove_file(&socket_dir)?;
        }
        std::fs::create_dir_all(&socket_dir)?;
        set_unix_permissions(&socket_dir, 0o700)?;

        cleanup_stale_sockets(&socket_dir);
        let socket_path = socket_dir.join(format!("{}.sock", std::process::id()));
        if socket_path.exists() {
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
                let id = next_client_id.fetch_add(1, Ordering::SeqCst);
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
    async fn start_socket_server(&self) -> Result<NativeHostSocketGuard> {
        Ok(NativeHostSocketGuard {
            path: None,
            task: None,
        })
    }
}

struct NativeHostSocketGuard {
    path: Option<PathBuf>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for NativeHostSocketGuard {
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

    clients.lock().await.remove(&id);
    let _ = send_chrome_json_to_writer(&writer, &json!({ "type": "mcp_disconnected" })).await;
    write_task.abort();
}

#[cfg(unix)]
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

    let length = u32::from_le_bytes(length_bytes) as usize;
    if length == 0 || length > MAX_MESSAGE_SIZE {
        return Err(anyhow!("invalid Chrome native message length: {length}"));
    }

    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload).await?;
    Ok(Some(String::from_utf8(payload)?))
}

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

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(unix)]
fn socket_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("KIANA_CHROME_NATIVE_HOST_SOCKET_DIR") {
        return PathBuf::from(path);
    }
    PathBuf::from(format!("/tmp/claude-mcp-browser-bridge-{}", username()))
}

#[cfg(unix)]
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
fn process_is_running(pid: i32) -> bool {
    std::path::Path::new("/proc").join(pid.to_string()).exists()
}

#[cfg(unix)]
fn set_unix_permissions(path: &std::path::Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions)?;
    Ok(())
}

#[cfg(unix)]
fn username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(test)]
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
