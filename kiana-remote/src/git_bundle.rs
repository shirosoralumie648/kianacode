use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::process::Command;
use uuid::Uuid;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const FILES_API_BETA: &str = "files-api-2025-04-14,oauth-2025-04-20";
const DEFAULT_BUNDLE_MAX_BYTES: u64 = 100 * 1024 * 1024;
const MAX_FILE_SIZE_BYTES: usize = 500 * 1024 * 1024;
const FILE_UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone)]
pub struct FilesApiConfig {
    pub oauth_token: String,
    pub base_url: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct GitBundleOptions {
    pub cwd: Option<PathBuf>,
    pub max_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleScope {
    All,
    Head,
    Squashed,
}

impl BundleScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Head => "head",
            Self::Squashed => "squashed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleFailReason {
    GitError,
    TooLarge,
    EmptyRepo,
}

impl BundleFailReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GitError => "git_error",
            Self::TooLarge => "too_large",
            Self::EmptyRepo => "empty_repo",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleUploadResult {
    Success {
        file_id: String,
        bundle_size_bytes: u64,
        scope: BundleScope,
        has_wip: bool,
    },
    Failure {
        error: String,
        fail_reason: Option<BundleFailReason>,
    },
}

impl BundleUploadResult {
    pub fn file_id(&self) -> Option<&str> {
        match self {
            Self::Success { file_id, .. } => Some(file_id),
            Self::Failure { .. } => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum GitBundleError {
    #[error("{0}")]
    Message(String),
    #[error("git bundle IO failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("git bundle upload failed: {0}")]
    Request(#[from] reqwest::Error),
}

#[derive(Debug)]
struct GitOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

#[derive(Debug)]
enum BundleCreateResult {
    Ok {
        scope: BundleScope,
    },
    Err {
        error: String,
        fail_reason: BundleFailReason,
    },
}

pub async fn create_and_upload_git_bundle(
    config: &FilesApiConfig,
    options: GitBundleOptions,
) -> Result<BundleUploadResult, GitBundleError> {
    let workdir = options.cwd.unwrap_or(std::env::current_dir()?);
    let Some(git_root) = find_git_root(&workdir).await? else {
        return Ok(BundleUploadResult::Failure {
            error: "Not in a git repository".to_string(),
            fail_reason: Some(BundleFailReason::GitError),
        });
    };

    cleanup_seed_refs(&git_root).await;

    let ref_check = run_git(&git_root, &["for-each-ref", "--count=1", "refs/"]).await?;
    if ref_check.code == 0 && ref_check.stdout.trim().is_empty() {
        return Ok(BundleUploadResult::Failure {
            error: "Repository has no commits yet".to_string(),
            fail_reason: Some(BundleFailReason::EmptyRepo),
        });
    }

    let stash_result = run_git(&git_root, &["stash", "create"]).await?;
    let wip_stash_sha = if stash_result.code == 0 {
        stash_result.stdout.trim().to_string()
    } else {
        String::new()
    };
    let has_wip = !wip_stash_sha.is_empty();
    if has_wip {
        let _ = run_git(
            &git_root,
            &["update-ref", "refs/seed/stash", &wip_stash_sha],
        )
        .await;
    }

    let bundle_path =
        std::env::temp_dir().join(format!("kiana-ccr-seed-{}.bundle", Uuid::new_v4()));
    let max_bytes = options.max_bytes.unwrap_or(DEFAULT_BUNDLE_MAX_BYTES);
    let result = async {
        let bundle = bundle_with_fallback(&git_root, &bundle_path, max_bytes, has_wip).await?;
        let scope = match bundle {
            BundleCreateResult::Ok { scope, .. } => scope,
            BundleCreateResult::Err { error, fail_reason } => {
                return Ok(BundleUploadResult::Failure {
                    error,
                    fail_reason: Some(fail_reason),
                });
            }
        };

        match upload_file(&bundle_path, "_source_seed.bundle", config).await? {
            UploadResult::Success { file_id, size, .. } => Ok(BundleUploadResult::Success {
                file_id,
                bundle_size_bytes: size as u64,
                scope,
                has_wip,
            }),
            UploadResult::Failure { error, .. } => Ok(BundleUploadResult::Failure {
                error,
                fail_reason: None,
            }),
        }
    }
    .await;

    let _ = tokio::fs::remove_file(&bundle_path).await;
    cleanup_seed_refs(&git_root).await;

    result
}

async fn bundle_with_fallback(
    git_root: &Path,
    bundle_path: &Path,
    max_bytes: u64,
    has_stash: bool,
) -> Result<BundleCreateResult, GitBundleError> {
    let mut all_args = vec![
        "bundle".to_string(),
        "create".to_string(),
        bundle_path.to_string_lossy().to_string(),
        "--all".to_string(),
    ];
    if has_stash {
        all_args.push("refs/seed/stash".to_string());
    }
    let all_result = run_git_owned(git_root, all_args).await?;
    if all_result.code != 0 {
        return Ok(BundleCreateResult::Err {
            error: format!(
                "git bundle create --all failed ({}): {}",
                all_result.code,
                truncate_error(&all_result.stderr)
            ),
            fail_reason: BundleFailReason::GitError,
        });
    }
    let all_size = tokio::fs::metadata(bundle_path).await?.len();
    if all_size <= max_bytes {
        return Ok(BundleCreateResult::Ok {
            scope: BundleScope::All,
        });
    }

    let mut head_args = vec![
        "bundle".to_string(),
        "create".to_string(),
        bundle_path.to_string_lossy().to_string(),
        "HEAD".to_string(),
    ];
    if has_stash {
        head_args.push("refs/seed/stash".to_string());
    }
    let head_result = run_git_owned(git_root, head_args).await?;
    if head_result.code != 0 {
        return Ok(BundleCreateResult::Err {
            error: format!(
                "git bundle create HEAD failed ({}): {}",
                head_result.code,
                truncate_error(&head_result.stderr)
            ),
            fail_reason: BundleFailReason::GitError,
        });
    }
    let head_size = tokio::fs::metadata(bundle_path).await?.len();
    if head_size <= max_bytes {
        return Ok(BundleCreateResult::Ok {
            scope: BundleScope::Head,
        });
    }

    let tree_ref = if has_stash {
        "refs/seed/stash^{tree}"
    } else {
        "HEAD^{tree}"
    };
    let commit_tree = run_git(git_root, &["commit-tree", tree_ref, "-m", "seed"]).await?;
    if commit_tree.code != 0 {
        return Ok(BundleCreateResult::Err {
            error: format!(
                "git commit-tree failed ({}): {}",
                commit_tree.code,
                truncate_error(&commit_tree.stderr)
            ),
            fail_reason: BundleFailReason::GitError,
        });
    }
    let squashed_sha = commit_tree.stdout.trim().to_string();
    let _ = run_git(git_root, &["update-ref", "refs/seed/root", &squashed_sha]).await;
    let squash_args = vec![
        "bundle".to_string(),
        "create".to_string(),
        bundle_path.to_string_lossy().to_string(),
        "refs/seed/root".to_string(),
    ];
    let squash_result = run_git_owned(git_root, squash_args).await?;
    if squash_result.code != 0 {
        return Ok(BundleCreateResult::Err {
            error: format!(
                "git bundle create refs/seed/root failed ({}): {}",
                squash_result.code,
                truncate_error(&squash_result.stderr)
            ),
            fail_reason: BundleFailReason::GitError,
        });
    }
    let squash_size = tokio::fs::metadata(bundle_path).await?.len();
    if squash_size <= max_bytes {
        return Ok(BundleCreateResult::Ok {
            scope: BundleScope::Squashed,
        });
    }

    Ok(BundleCreateResult::Err {
        error: "Repo is too large to bundle. Please setup GitHub on https://claude.ai/code"
            .to_string(),
        fail_reason: BundleFailReason::TooLarge,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadResult {
    Success {
        path: String,
        file_id: String,
        size: usize,
    },
    Failure {
        path: String,
        error: String,
    },
}

pub async fn upload_file(
    file_path: &Path,
    relative_path: &str,
    config: &FilesApiConfig,
) -> Result<UploadResult, GitBundleError> {
    let content = match tokio::fs::read(file_path).await {
        Ok(content) => content,
        Err(error) => {
            return Ok(UploadResult::Failure {
                path: relative_path.to_string(),
                error: error.to_string(),
            });
        }
    };
    let file_size = content.len();
    if file_size > MAX_FILE_SIZE_BYTES {
        return Ok(UploadResult::Failure {
            path: relative_path.to_string(),
            error: format!(
                "File exceeds maximum size of {MAX_FILE_SIZE_BYTES} bytes (actual: {file_size})"
            ),
        });
    }

    let boundary = format!("----FormBoundary{}", Uuid::new_v4());
    let filename = Path::new(relative_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(relative_path);
    let mut body = Vec::new();
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&content);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"purpose\"\r\n\r\nuser_data\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    let base_url = normalized_base_url(&config.base_url)?;
    let url = format!("{base_url}/v1/files");
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(&config.oauth_token)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", FILES_API_BETA)
        .header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .header("content-length", body.len().to_string())
        .timeout(FILE_UPLOAD_TIMEOUT)
        .body(body)
        .send()
        .await?;

    if response.status() == reqwest::StatusCode::OK
        || response.status() == reqwest::StatusCode::CREATED
    {
        let value = response.json::<Value>().await?;
        let Some(file_id) = value.get("id").and_then(Value::as_str) else {
            return Ok(UploadResult::Failure {
                path: relative_path.to_string(),
                error: "Upload succeeded but no file ID returned".to_string(),
            });
        };
        return Ok(UploadResult::Success {
            path: relative_path.to_string(),
            file_id: file_id.to_string(),
            size: file_size,
        });
    }

    let error = match response.status() {
        reqwest::StatusCode::UNAUTHORIZED => {
            "Authentication failed: invalid or missing API key".to_string()
        }
        reqwest::StatusCode::FORBIDDEN => "Access denied for upload".to_string(),
        reqwest::StatusCode::PAYLOAD_TOO_LARGE => "File too large for upload".to_string(),
        status => format!("status {status}"),
    };
    Ok(UploadResult::Failure {
        path: relative_path.to_string(),
        error,
    })
}

async fn find_git_root(workdir: &Path) -> Result<Option<PathBuf>, GitBundleError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workdir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .await?;
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(path)))
    }
}

async fn cleanup_seed_refs(git_root: &Path) {
    for reference in ["refs/seed/stash", "refs/seed/root"] {
        let _ = run_git(git_root, &["update-ref", "-d", reference]).await;
    }
}

async fn run_git(git_root: &Path, args: &[&str]) -> Result<GitOutput, GitBundleError> {
    let args = args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    run_git_owned(git_root, args).await
}

async fn run_git_owned(git_root: &Path, args: Vec<String>) -> Result<GitOutput, GitBundleError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(git_root)
        .args(args)
        .output()
        .await?;
    Ok(GitOutput {
        code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn normalized_base_url(api_base_url: &str) -> Result<String, GitBundleError> {
    let base_url = api_base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        Err(GitBundleError::Message(
            "Files API base URL is empty".to_string(),
        ))
    } else {
        Ok(base_url)
    }
}

fn truncate_error(error: &str) -> String {
    error.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        anthropic_version: Option<String>,
        anthropic_beta: Option<String>,
        content_type: Option<String>,
        content_length: Option<usize>,
        body: Vec<u8>,
    }

    #[tokio::test]
    async fn upload_file_posts_reference_multipart_shape_and_headers() {
        let temp_dir = make_temp_dir("kiana-upload-test");
        let file_path = temp_dir.join("seed.bundle");
        tokio::fs::write(&file_path, b"bundle-bytes").await.unwrap();
        let (base_url, request) =
            spawn_mock_files_api_server(201, serde_json::json!({ "id": "file-1" }).to_string())
                .await;

        let upload = upload_file(
            &file_path,
            "_source_seed.bundle",
            &FilesApiConfig {
                oauth_token: "access-token".to_string(),
                base_url,
                session_id: "session-1".to_string(),
            },
        )
        .await
        .unwrap();

        assert_eq!(
            upload,
            UploadResult::Success {
                path: "_source_seed.bundle".to_string(),
                file_id: "file-1".to_string(),
                size: "bundle-bytes".len(),
            }
        );
        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/files");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(
            request.anthropic_version.as_deref(),
            Some(ANTHROPIC_VERSION)
        );
        assert_eq!(request.anthropic_beta.as_deref(), Some(FILES_API_BETA));
        assert_eq!(request.content_length, Some(request.body.len()));
        let content_type = request.content_type.unwrap();
        assert!(content_type.starts_with("multipart/form-data; boundary=----FormBoundary"));
        let body = String::from_utf8_lossy(&request.body);
        assert!(body.contains("name=\"file\"; filename=\"_source_seed.bundle\""));
        assert!(body.contains("Content-Type: application/octet-stream"));
        assert!(body.contains("bundle-bytes"));
        assert!(body.contains("name=\"purpose\""));
        assert!(body.contains("user_data"));

        let _ = std::fs::remove_dir_all(temp_dir);
    }

    #[tokio::test]
    async fn create_and_upload_git_bundle_captures_wip_and_cleans_seed_refs() {
        let repo = make_temp_dir("kiana-git-bundle-test");
        run_command(&repo, &["init"]).await;
        run_command(&repo, &["config", "user.email", "kiana@example.com"]).await;
        run_command(&repo, &["config", "user.name", "Kiana"]).await;
        tokio::fs::write(repo.join("tracked.txt"), "initial\n")
            .await
            .unwrap();
        run_command(&repo, &["add", "tracked.txt"]).await;
        run_command(&repo, &["commit", "-m", "initial"]).await;
        tokio::fs::write(repo.join("tracked.txt"), "changed\n")
            .await
            .unwrap();

        let (base_url, request) =
            spawn_mock_files_api_server(201, serde_json::json!({ "id": "file-1" }).to_string())
                .await;

        let result = create_and_upload_git_bundle(
            &FilesApiConfig {
                oauth_token: "access-token".to_string(),
                base_url,
                session_id: "session-1".to_string(),
            },
            GitBundleOptions {
                cwd: Some(repo.clone()),
                max_bytes: Some(DEFAULT_BUNDLE_MAX_BYTES),
            },
        )
        .await
        .unwrap();

        let BundleUploadResult::Success {
            file_id,
            bundle_size_bytes,
            scope,
            has_wip,
        } = result
        else {
            panic!("expected successful bundle upload");
        };
        assert_eq!(file_id, "file-1");
        assert!(bundle_size_bytes > 0);
        assert_eq!(scope, BundleScope::All);
        assert!(has_wip);

        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.path, "/v1/files");
        assert!(String::from_utf8_lossy(&request.body).contains("_source_seed.bundle"));
        assert_seed_ref_absent(&repo, "refs/seed/stash").await;
        assert_seed_ref_absent(&repo, "refs/seed/root").await;

        let _ = std::fs::remove_dir_all(repo);
    }

    #[tokio::test]
    async fn create_and_upload_git_bundle_reports_empty_repo_without_upload() {
        let repo = make_temp_dir("kiana-empty-bundle-test");
        run_command(&repo, &["init"]).await;
        let (base_url, request) =
            spawn_mock_files_api_server(201, serde_json::json!({ "id": "file-1" }).to_string())
                .await;

        let result = create_and_upload_git_bundle(
            &FilesApiConfig {
                oauth_token: "access-token".to_string(),
                base_url,
                session_id: "session-1".to_string(),
            },
            GitBundleOptions {
                cwd: Some(repo.clone()),
                max_bytes: None,
            },
        )
        .await
        .unwrap();

        assert_eq!(
            result,
            BundleUploadResult::Failure {
                error: "Repository has no commits yet".to_string(),
                fail_reason: Some(BundleFailReason::EmptyRepo),
            }
        );
        assert!(request.lock().unwrap().is_none());

        let _ = std::fs::remove_dir_all(repo);
    }

    async fn assert_seed_ref_absent(repo: &Path, reference: &str) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["show-ref", "--verify", reference])
            .output()
            .await
            .unwrap();
        assert!(!output.status.success(), "{reference} should be absent");
    }

    async fn run_command(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn make_temp_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    async fn spawn_mock_files_api_server(
        status: u16,
        response_body: String,
    ) -> (String, Arc<StdMutex<Option<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let request = Arc::new(StdMutex::new(None));
        let shared_request = request.clone();

        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if let Ok(recorded) = read_http_request(&mut stream).await {
                *shared_request.lock().unwrap() = Some(recorded);
            }
            let _ = write_http_response(&mut stream, status, &response_body).await;
        });

        (format!("http://{}", address), request)
    }

    async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<RecordedRequest> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 4096];
        let header_end = loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed before headers",
                ));
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(index) = find_header_end(&buffer) {
                break index;
            }
        };

        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let mut lines = header_text.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();
        let mut authorization = None;
        let mut anthropic_version = None;
        let mut anthropic_beta = None;
        let mut content_type = None;
        let mut content_length = 0_usize;

        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value);
            } else if name.eq_ignore_ascii_case("anthropic-version") {
                anthropic_version = Some(value);
            } else if name.eq_ignore_ascii_case("anthropic-beta") {
                anthropic_beta = Some(value);
            } else if name.eq_ignore_ascii_case("content-type") {
                content_type = Some(value);
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or_default();
            }
        }

        let body_start = header_end + 4;
        while buffer.len() < body_start + content_length {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        let body = buffer[body_start..buffer.len().min(body_start + content_length)].to_vec();

        Ok(RecordedRequest {
            method,
            path,
            authorization,
            anthropic_version,
            anthropic_beta,
            content_type,
            content_length: Some(content_length),
            body,
        })
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: u16,
        body: &str,
    ) -> std::io::Result<()> {
        let status_text = match status {
            200 => "OK",
            201 => "Created",
            401 => "Unauthorized",
            403 => "Forbidden",
            413 => "Payload Too Large",
            500 => "Internal Server Error",
            _ => "Status",
        };
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    status_text,
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .await?;
        stream.flush().await
    }
}
