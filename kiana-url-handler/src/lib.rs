use rmcp::{
    handler::server::{router::tool::ToolRouter, tool::Parameters},
    model::*,
    schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use url::Url;

type Callback = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegisterSchemeRequest {
    pub scheme: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HandleUrlRequest {
    pub url: String,
}

#[derive(Clone)]
pub struct UrlHandler {
    callback: Arc<Mutex<Option<Callback>>>,
    tool_router: ToolRouter<Self>,
}

#[tool_router(router = tool_router)]
impl UrlHandler {
    pub fn new() -> Self {
        Self {
            callback: Arc::new(Mutex::new(None)),
            tool_router: Self::tool_router(),
        }
    }

    pub async fn register_callback<F>(&self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let mut cb = self.callback.lock().await;
        *cb = Some(Arc::new(callback));
    }

    #[tool(description = "Register a custom URL scheme handler")]
    async fn register_url_scheme(
        &self,
        request: Parameters<RegisterSchemeRequest>,
    ) -> Result<CallToolResult, McpError> {
        self._register_scheme(&request.0.scheme)
            .map_err(|e| McpError::internal_error(e, None))?;
        Ok(CallToolResult::success(vec![Content::text(
            "Registered successfully",
        )]))
    }

    #[tool(description = "Handle an incoming URL")]
    async fn handle_url(
        &self,
        request: Parameters<HandleUrlRequest>,
    ) -> Result<CallToolResult, McpError> {
        if let Err(e) = Url::parse(&request.0.url) {
            return Err(McpError::invalid_request(
                format!("Invalid URL: {}", e),
                None,
            ));
        }

        if let Some(cb) = self.callback.lock().await.as_ref() {
            cb(request.0.url.clone());
        }
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Handled: {}",
            request.0.url
        ))]))
    }

    #[cfg(target_os = "macos")]
    fn _register_scheme(&self, _scheme: &str) -> Result<(), String> {
        use cocoa::appkit::NSApplication;
        use cocoa::base::id;
        use objc::{msg_send, sel, sel_impl};

        unsafe {
            let app: id = msg_send![class!(NSApplication), sharedApplication];
            let _: () = msg_send![app, setAppleEventHandler];
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn _register_scheme(&self, _scheme: &str) -> Result<(), String> {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED)
                .ok()
                .map_err(|e| format!("COM init failed: {}", e))?;
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn _register_scheme(&self, scheme: &str) -> Result<(), String> {
        Self::register_linux_scheme_with_current_exe(scheme).map(|_| ())
    }

    #[cfg(not(target_os = "linux"))]
    fn _linux_registration_status(&self, _scheme: &str) -> Result<String, String> {
        Err("Linux registration status is only available on Linux".to_string())
    }

    #[cfg(target_os = "linux")]
    fn _linux_registration_status(&self, scheme: &str) -> Result<String, String> {
        Self::linux_registration_status(scheme)
    }

    #[cfg(target_os = "linux")]
    pub fn linux_registration_status(scheme: &str) -> Result<String, String> {
        let scheme = Self::validate_scheme(scheme)?;
        let mime = scheme_mime_type(&scheme);
        let output = std::process::Command::new("xdg-mime")
            .args(["query", "default", &mime])
            .output()
            .map_err(|e| format!("failed to run xdg-mime: {}", e))?;
        if !output.status.success() {
            return Err(format!(
                "xdg-mime query failed with status {}",
                output.status
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    fn _register_scheme(&self, _scheme: &str) -> Result<(), String> {
        Err("Platform not supported".to_string())
    }

    #[cfg(target_os = "linux")]
    pub fn register_linux_scheme_with_current_exe(scheme: &str) -> Result<PathBuf, String> {
        let exe = std::env::current_exe()
            .map_err(|e| format!("failed to locate current executable: {}", e))?;
        let command = format!("{} handle %u", quote_exec_arg(&exe.to_string_lossy()));
        Self::register_linux_scheme_with_command(scheme, &command)
    }

    #[cfg(target_os = "linux")]
    pub fn register_linux_scheme_with_command(
        scheme: &str,
        command: &str,
    ) -> Result<PathBuf, String> {
        let dir = Self::linux_applications_dir()?;
        Self::write_linux_desktop_entry(scheme, &command, &dir)?;
        Self::register_linux_mime_default(
            scheme,
            &desktop_file_name(&Self::validate_scheme(scheme)?),
        )?;
        Ok(dir.join(desktop_file_name(&Self::validate_scheme(scheme)?)))
    }

    #[cfg(target_os = "linux")]
    pub fn linux_desktop_entry_path(scheme: &str) -> Result<PathBuf, String> {
        let scheme = Self::validate_scheme(scheme)?;
        Ok(Self::linux_applications_dir()?.join(desktop_file_name(&scheme)))
    }

    pub fn quote_exec_arg(arg: &str) -> String {
        quote_exec_arg(arg)
    }

    pub fn validate_scheme(scheme: &str) -> Result<String, String> {
        let scheme = scheme.trim().to_ascii_lowercase();
        if scheme.is_empty() {
            return Err("scheme cannot be empty".to_string());
        }
        let mut chars = scheme.chars();
        let Some(first) = chars.next() else {
            return Err("scheme cannot be empty".to_string());
        };
        if !first.is_ascii_alphabetic() {
            return Err("scheme must start with an ASCII letter".to_string());
        }
        if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')) {
            return Err(
                "scheme may only contain ASCII letters, digits, '+', '-', or '.'".to_string(),
            );
        }
        Ok(scheme)
    }

    pub fn desktop_entry_contents(scheme: &str, command: &str) -> Result<String, String> {
        let scheme = Self::validate_scheme(scheme)?;
        if command.trim().is_empty() {
            return Err("handler command cannot be empty".to_string());
        }
        Ok(format!(
            "[Desktop Entry]\nType=Application\nName=Kiana URL Handler ({scheme})\nExec={command}\nTerminal=false\nNoDisplay=true\nMimeType={mime};\n",
            scheme = scheme,
            command = command.trim(),
            mime = scheme_mime_type(&scheme)
        ))
    }

    pub fn write_linux_desktop_entry(
        scheme: &str,
        command: &str,
        applications_dir: &Path,
    ) -> Result<PathBuf, String> {
        let scheme = Self::validate_scheme(scheme)?;
        std::fs::create_dir_all(applications_dir)
            .map_err(|e| format!("failed to create {}: {}", applications_dir.display(), e))?;
        let path = applications_dir.join(desktop_file_name(&scheme));
        let contents = Self::desktop_entry_contents(&scheme, command)?;
        std::fs::write(&path, contents)
            .map_err(|e| format!("failed to write {}: {}", path.display(), e))?;
        Ok(path)
    }

    #[cfg(target_os = "linux")]
    fn register_linux_mime_default(scheme: &str, desktop_file: &str) -> Result<(), String> {
        let scheme = Self::validate_scheme(scheme)?;
        let status = std::process::Command::new("xdg-mime")
            .args(["default", desktop_file, &scheme_mime_type(&scheme)])
            .status()
            .map_err(|e| format!("failed to run xdg-mime: {}", e))?;
        if !status.success() {
            return Err(format!("xdg-mime default failed with status {}", status));
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn linux_applications_dir() -> Result<PathBuf, String> {
        if let Ok(path) = std::env::var("XDG_DATA_HOME") {
            return Ok(PathBuf::from(path).join("applications"));
        }
        if let Ok(home) = std::env::var("HOME") {
            return Ok(PathBuf::from(home).join(".local/share/applications"));
        }
        Err("HOME or XDG_DATA_HOME must be set".to_string())
    }
}

fn scheme_mime_type(scheme: &str) -> String {
    format!("x-scheme-handler/{}", scheme)
}

fn desktop_file_name(scheme: &str) -> String {
    format!("kiana-url-handler-{}.desktop", scheme)
}

fn quote_exec_arg(arg: &str) -> String {
    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '-' | '_' | '.' | ':'))
    {
        return arg.to_string();
    }
    format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for UrlHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some("URL handler for custom schemes and deep linking".into()),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::transport::stdio;

    let service = UrlHandler::new().serve(stdio()).await.inspect_err(|e| {
        eprintln!("Error starting server: {}", e);
    })?;
    service.waiting().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_url_validation() {
        let handler = UrlHandler::new();
        let valid_req = Parameters(HandleUrlRequest {
            url: "myapp://test".to_string(),
        });
        let result = handler.handle_url(valid_req).await;
        assert!(result.is_ok());

        let invalid_req = Parameters(HandleUrlRequest {
            url: "invalid url".to_string(),
        });
        let result = handler.handle_url(invalid_req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_callback() {
        let handler = UrlHandler::new();
        let received = Arc::new(Mutex::new(None));
        let received_clone = received.clone();

        handler
            .register_callback(move |url| {
                let received = received_clone.clone();
                tokio::spawn(async move {
                    *received.lock().await = Some(url);
                });
            })
            .await;

        let req = Parameters(HandleUrlRequest {
            url: "myapp://test".to_string(),
        });
        handler.handle_url(req).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        assert_eq!(received.lock().await.as_ref().unwrap(), "myapp://test");
    }

    #[test]
    fn test_scheme_validation() {
        assert_eq!(
            UrlHandler::validate_scheme("Kiana+Test").unwrap(),
            "kiana+test"
        );
        assert!(UrlHandler::validate_scheme("1bad").is_err());
        assert!(UrlHandler::validate_scheme("bad/slash").is_err());
    }

    #[test]
    fn test_desktop_entry_contents() {
        let contents =
            UrlHandler::desktop_entry_contents("kiana", "/usr/bin/kiana-url-handler handle %u")
                .unwrap();
        assert!(contents.contains("Type=Application"));
        assert!(contents.contains("Exec=/usr/bin/kiana-url-handler handle %u"));
        assert!(contents.contains("MimeType=x-scheme-handler/kiana;"));
    }

    #[test]
    fn test_write_linux_desktop_entry() {
        let root = std::env::temp_dir().join(format!(
            "kiana-url-handler-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path =
            UrlHandler::write_linux_desktop_entry("kiana", "/bin/true handle %u", &root).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(path.file_name().unwrap(), "kiana-url-handler-kiana.desktop");
        assert!(contents.contains("Exec=/bin/true handle %u"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_quote_exec_arg_handles_spaces() {
        assert_eq!(
            UrlHandler::quote_exec_arg("/usr/bin/kiana"),
            "/usr/bin/kiana"
        );
        assert_eq!(
            UrlHandler::quote_exec_arg("/tmp/Kiana Code/kiana"),
            "\"/tmp/Kiana Code/kiana\""
        );
    }

    #[test]
    fn test_write_linux_desktop_entry_with_main_kiana_command() {
        let root = std::env::temp_dir().join(format!(
            "kiana-url-handler-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let command = format!(
            "{} url handle %u",
            UrlHandler::quote_exec_arg("/tmp/Kiana Code/kiana")
        );
        let path = UrlHandler::write_linux_desktop_entry("kiana", &command, &root).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("Exec=\"/tmp/Kiana Code/kiana\" url handle %u"));
        let _ = std::fs::remove_dir_all(root);
    }
}
