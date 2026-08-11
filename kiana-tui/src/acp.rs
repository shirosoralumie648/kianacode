use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum AcpMessage {
    Response(Value),
    Notification(String, Value),
}

pub struct AcpClient {
    _child: Child,
    stdin: ChildStdin,
    request_id: u64,
    message_rx: Receiver<AcpMessage>,
    last_activity: Instant,
}

impl AcpClient {
    pub fn new() -> Result<Self> {
        Self::new_with_retry(3)
    }

    pub fn new_with_retry(max_retries: u32) -> Result<Self> {
        let mut last_error = None;

        for attempt in 0..max_retries {
            if attempt > 0 {
                tracing::warn!("Retrying ACP server spawn (attempt {}/{})", attempt + 1, max_retries);
                thread::sleep(Duration::from_secs(1));
            }

            match Self::try_spawn() {
                Ok(client) => {
                    tracing::info!("ACP server spawned successfully");
                    return Ok(client);
                }
                Err(e) => {
                    tracing::error!("Failed to spawn ACP server (attempt {}): {}", attempt + 1, e);
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Failed to spawn ACP server after {} attempts", max_retries)))
    }

    fn try_spawn() -> Result<Self> {
        let mut child = Command::new("cargo")
            .args([
                "run",
                "-p",
                "kiana-acp-server",
                "--bin",
                "kiana-acp-server",
                "--",
                "--stdio",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn kiana-acp-server")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stdout = BufReader::new(stdout);

        // Spawn a background thread to read messages
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = stdout;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if let Ok(message) = serde_json::from_str::<Value>(&line) {
                            // Check if it's a notification (has method but no id)
                            let acp_msg = if message.get("method").is_some() && message.get("id").is_none() {
                                let method = message["method"].as_str().unwrap_or("unknown").to_string();
                                let params = message.get("params").cloned().unwrap_or(Value::Null);
                                AcpMessage::Notification(method, params)
                            } else {
                                // It's a response
                                AcpMessage::Response(message)
                            };

                            if tx.send(acp_msg).is_err() {
                                break; // Receiver dropped
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            _child: child,
            stdin,
            request_id: 0,
            message_rx: rx,
            last_activity: Instant::now(),
        })
    }

    fn next_id(&mut self) -> String {
        self.request_id += 1;
        self.request_id.to_string()
    }

    fn send_request(&mut self, method: &str, params: Value) -> Result<String> {
        let id = self.next_id();
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        });

        let request_str = serde_json::to_string(&request)?;
        writeln!(self.stdin, "{}", request_str)?;
        self.stdin.flush()?;

        Ok(id)
    }

    fn read_response(&mut self) -> Result<Value> {
        let timeout = Duration::from_secs(30);
        let start = Instant::now();

        // Wait for a response message (not a notification)
        loop {
            // Check for timeout
            if start.elapsed() > timeout {
                anyhow::bail!("Request timeout after 30 seconds");
            }

            match self.message_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(AcpMessage::Response(response)) => {
                    self.last_activity = Instant::now();
                    return Ok(response);
                }
                Ok(AcpMessage::Notification(_, _)) => {
                    // Skip notifications while waiting for response
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Continue waiting
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("ACP connection closed")
                }
            }
        }
    }

    /// Try to read a message without blocking (returns None if no data available)
    pub fn try_read_message(&mut self) -> Result<Option<AcpMessage>> {
        match self.message_rx.try_recv() {
            Ok(msg) => {
                self.last_activity = Instant::now();
                Ok(Some(msg))
            }
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => anyhow::bail!("ACP connection closed"),
        }
    }

    pub fn is_alive(&self) -> bool {
        // Check if we've received any activity in the last 60 seconds
        self.last_activity.elapsed() < Duration::from_secs(60)
    }

    pub fn check_connection(&mut self) -> Result<()> {
        if !self.is_alive() {
            anyhow::bail!("ACP connection appears dead (no activity for 60s)");
        }
        Ok(())
    }

    pub fn initialize_session(&mut self, project_root: &str) -> Result<String> {
        let params = json!({
            "project_root": project_root
        });

        self.send_request("initialize_session", params)?;
        let response = self.read_response()?;

        if let Some(error) = response.get("error") {
            anyhow::bail!("ACP error: {}", error);
        }

        let session_id = response["result"]["session_id"]
            .as_str()
            .context("Missing session_id in response")?
            .to_string();

        Ok(session_id)
    }

    pub fn execute_command(
        &mut self,
        session_id: &str,
        command: &str,
        arguments: Value,
    ) -> Result<Value> {
        let params = json!({
            "session_id": session_id,
            "command": command,
            "arguments": arguments
        });

        self.send_request("execute_command", params)?;
        let response = self.read_response()?;

        if let Some(error) = response.get("error") {
            anyhow::bail!("ACP error: {}", error);
        }

        Ok(response["result"].clone())
    }
}
