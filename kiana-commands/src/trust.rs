use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_types::{
    has_explicit_project_trust, legacy_project_trust_file_path, project_trust_file_path,
    project_trust_from_app_state, project_trust_id, project_trust_root, read_project_trust,
    remove_project_trust, write_project_trust, ProjectTrust,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const TRUST_STATUS_SCHEMA: &str = "kiana.app-server.trust-status.v1";
const LEGACY_TRUST_REASON: &str = "project_local_trust_is_not_authoritative";

pub struct TrustCommand;

#[async_trait]
impl Command for TrustCommand {
    fn name(&self) -> &str {
        "trust"
    }

    fn description(&self) -> &str {
        "Manage project trust"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = context.args.trim();
        let (command, rest) = split_word(args);
        match command.unwrap_or("status") {
            "" | "status" | "list" => {
                reject_unexpected_rest("trust status", rest)?;
                status(&context)
            }
            "path" => {
                reject_unexpected_rest("trust path", rest)?;
                Ok(CommandResult::text(
                    project_trust_file_path(cwd(&context))
                        .map_err(anyhow::Error::msg)?
                        .display()
                        .to_string(),
                ))
            }
            "json" => {
                reject_unexpected_rest("trust json", rest)?;
                status_json(&context)
            }
            "." | "trust" | "trusted" | "allow" | "enable" => {
                reject_unexpected_rest("trust trust", rest)?;
                set_trust(&context, ProjectTrust::Trusted)
            }
            "untrust" | "untrusted" | "deny" | "disable" => {
                reject_unexpected_rest("trust untrust", rest)?;
                set_trust(&context, ProjectTrust::Untrusted)
            }
            "reset" | "clear" => {
                reject_unexpected_rest("trust reset", rest)?;
                reset_trust(&context)
            }
            "help" | "--help" | "-h" => {
                reject_unexpected_rest("trust help", rest)?;
                Ok(CommandResult::text(usage()))
            }
            other => Err(anyhow!("unknown trust command '{}'\n\n{}", other, usage())),
        }
    }
}

fn status(context: &CommandContext) -> Result<CommandResult> {
    let snapshot = trust_status_snapshot(context);
    let file_path = snapshot
        .file
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    let file_error = snapshot.file.error.as_deref().unwrap_or("none");
    Ok(CommandResult::text(
        [
            "Project trust status".to_string(),
            format!("project_trust: {}", snapshot.project_trust.as_str()),
            format!("project_trusted: {}", snapshot.project_trust.as_bool()),
            format!(
                "allows_project_resources: {}",
                snapshot.project_trust.allows_project_resources()
            ),
            format!("source: {}", snapshot.source),
            format!("project_id: {}", snapshot.project_id),
            format!("project_root: {}", snapshot.project_root.display()),
            format!("file: {file_path}"),
            format!("file_status: {}", snapshot.file.status),
            format!("file_exists: {}", snapshot.file.exists),
            format!("file_error: {file_error}"),
            format!(
                "legacy_project_file: {}",
                snapshot.legacy_project_file.path.display()
            ),
            format!(
                "legacy_project_file_exists: {}",
                snapshot.legacy_project_file.exists
            ),
            "legacy_project_file_ignored: true".to_string(),
            format!(
                "legacy_project_file_reason: {}",
                snapshot.legacy_project_file.reason
            ),
            "usage: kiana trust . | trust | untrust | reset | status".to_string(),
        ]
        .join("\n"),
    ))
}

fn status_json(context: &CommandContext) -> Result<CommandResult> {
    Ok(CommandResult::text(serde_json::to_string_pretty(
        &trust_status_payload(context),
    )?))
}

pub fn trust_status_payload(context: &CommandContext) -> Value {
    let snapshot = trust_status_snapshot(context);
    json!({
        "schema": TRUST_STATUS_SCHEMA,
        "workspace": snapshot.project_root.display().to_string(),
        "project_trust": snapshot.project_trust.as_str(),
        "project_trusted": snapshot.project_trust.as_bool(),
        "allows_project_resources": snapshot.project_trust.allows_project_resources(),
        "source": snapshot.source,
        "project_id": snapshot.project_id,
        "project_root": snapshot.project_root.display().to_string(),
        "file": {
            "path": snapshot.file.path.as_ref().map(|path| path.display().to_string()),
            "status": snapshot.file.status,
            "exists": snapshot.file.exists,
            "error": snapshot.file.error,
        },
        "legacy_project_file": {
            "path": snapshot.legacy_project_file.path.display().to_string(),
            "exists": snapshot.legacy_project_file.exists,
            "ignored": true,
            "reason": snapshot.legacy_project_file.reason,
        }
    })
}

fn set_trust(context: &CommandContext, project_trust: ProjectTrust) -> Result<CommandResult> {
    let cwd = cwd(context);
    let path = write_project_trust(&cwd, project_trust).map_err(anyhow::Error::msg)?;
    Ok(CommandResult::text(format!(
        "Project trust updated\nproject_trust: {}\nfile: {}",
        project_trust.as_str(),
        path.display()
    )))
}

fn reset_trust(context: &CommandContext) -> Result<CommandResult> {
    let cwd = cwd(context);
    remove_project_trust(&cwd).map_err(anyhow::Error::msg)?;
    let snapshot = trust_status_snapshot(context);
    let file = snapshot
        .file
        .path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unavailable".to_string());
    Ok(CommandResult::text(format!(
        "Project trust reset\nproject_trust: {}\nfile: {}",
        snapshot.project_trust.as_str(),
        file
    )))
}

struct TrustStatusSnapshot {
    project_trust: ProjectTrust,
    source: &'static str,
    project_id: String,
    project_root: PathBuf,
    file: TrustFileSnapshot,
    legacy_project_file: LegacyTrustFileSnapshot,
}

struct TrustFileSnapshot {
    path: Option<PathBuf>,
    status: &'static str,
    exists: bool,
    error: Option<String>,
}

struct LegacyTrustFileSnapshot {
    path: PathBuf,
    exists: bool,
    reason: &'static str,
}

fn trust_status_snapshot(context: &CommandContext) -> TrustStatusSnapshot {
    let cwd = cwd(context);
    let project_root = project_trust_root(&cwd);
    let project_id = project_trust_id(&cwd);
    let session_trust = session_project_trust(&context.app_state);

    let (file, user_store_trust) = match project_trust_file_path(&cwd) {
        Ok(path) => {
            let exists = path_exists_no_follow(&path);
            match read_project_trust(&cwd) {
                Ok(Some(trust)) => (
                    TrustFileSnapshot {
                        path: Some(path),
                        status: "found",
                        exists: true,
                        error: None,
                    },
                    Some(trust),
                ),
                Ok(None) => (
                    TrustFileSnapshot {
                        path: Some(path),
                        status: "missing",
                        exists,
                        error: None,
                    },
                    None,
                ),
                Err(error) => (
                    TrustFileSnapshot {
                        path: Some(path),
                        status: "error",
                        exists,
                        error: Some(error),
                    },
                    None,
                ),
            }
        }
        Err(error) => (
            TrustFileSnapshot {
                path: None,
                status: "error",
                exists: false,
                error: Some(error),
            },
            None,
        ),
    };

    let (project_trust, source) = if let Some(trust) = session_trust {
        (trust, "session")
    } else if let Some(trust) = user_store_trust {
        (trust, "user_store")
    } else {
        (ProjectTrust::Unknown, "default")
    };
    let legacy_path = legacy_project_trust_file_path(&cwd);

    TrustStatusSnapshot {
        project_trust,
        source,
        project_id,
        project_root,
        file,
        legacy_project_file: LegacyTrustFileSnapshot {
            exists: path_exists_no_follow(&legacy_path),
            path: legacy_path,
            reason: LEGACY_TRUST_REASON,
        },
    }
}

fn session_project_trust(
    app_state: &std::collections::HashMap<String, Value>,
) -> Option<ProjectTrust> {
    let mut session_state = app_state.clone();
    session_state.remove("cwd");
    session_state.remove("project_root");
    session_state.remove("projectRoot");
    has_explicit_project_trust(&session_state).then(|| project_trust_from_app_state(&session_state))
}

fn path_exists_no_follow(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

fn cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn split_word(value: &str) -> (Option<&str>, &str) {
    let value = value.trim_start();
    if value.is_empty() {
        return (None, "");
    }
    if let Some(index) = value.find(char::is_whitespace) {
        (Some(&value[..index]), value[index..].trim_start())
    } else {
        (Some(value), "")
    }
}

fn reject_unexpected_rest(command: &str, rest: &str) -> Result<()> {
    if rest.trim().is_empty() {
        return Ok(());
    }
    Err(anyhow!("usage: kiana {command}\n\n{}", usage()))
}

fn usage() -> &'static str {
    "Usage:\n  kiana trust .\n  kiana trust [status|list]\n  kiana trust trust\n  kiana trust untrust\n  kiana trust reset\n  kiana trust path\n  kiana trust json"
}

#[cfg(test)]
mod tests {
    use super::TrustCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use kiana_types::{
        legacy_project_trust_file_path, project_trust_file_path, project_trust_from_app_state,
        ProjectTrust,
    };
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::ffi::OsString;
    use std::fs;
    use std::sync::MutexGuard;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TrustTestEnv {
        _guard: MutexGuard<'static, ()>,
        original_kiana_home: Option<OsString>,
        original_home: Option<OsString>,
        original_user_profile: Option<OsString>,
        root: std::path::PathBuf,
        project: std::path::PathBuf,
        kiana_home: std::path::PathBuf,
    }

    impl TrustTestEnv {
        fn new(label: &str) -> Self {
            let guard = env_lock()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "kiana-trust-command-{label}-{}-{unique}",
                std::process::id()
            ));
            let project = root.join("project");
            let kiana_home = root.join("kiana-home");
            fs::create_dir_all(project.join(".git")).unwrap();
            fs::create_dir_all(&kiana_home).unwrap();
            let project = fs::canonicalize(project).unwrap();
            let kiana_home = fs::canonicalize(kiana_home).unwrap();

            let original_kiana_home = std::env::var_os("KIANA_HOME");
            let original_home = std::env::var_os("HOME");
            let original_user_profile = std::env::var_os("USERPROFILE");
            std::env::set_var("KIANA_HOME", &kiana_home);

            Self {
                _guard: guard,
                original_kiana_home,
                original_home,
                original_user_profile,
                root,
                project,
                kiana_home,
            }
        }
    }

    impl Drop for TrustTestEnv {
        fn drop(&mut self) {
            restore_env("KIANA_HOME", &self.original_kiana_home);
            restore_env("HOME", &self.original_home);
            restore_env("USERPROFILE", &self.original_user_profile);
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn restore_env(name: &str, value: &Option<OsString>) {
        if let Some(value) = value {
            std::env::set_var(name, value);
        } else {
            std::env::remove_var(name);
        }
    }

    fn context(args: &str, cwd: &std::path::Path) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([("cwd".to_string(), json!(cwd))]),
        }
    }

    fn context_with_trust(
        args: &str,
        cwd: &std::path::Path,
        project_trusted: Value,
    ) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([
                ("cwd".to_string(), json!(cwd)),
                ("project_trusted".to_string(), project_trusted),
            ]),
        }
    }

    async fn json_status(context: CommandContext) -> Value {
        let result = TrustCommand.execute(context).await.unwrap();
        serde_json::from_str(&result.value).unwrap()
    }

    #[tokio::test]
    async fn unknown_status_exposes_complete_external_store_contract() {
        let env = TrustTestEnv::new("unknown-status");
        let expected_file = project_trust_file_path(&env.project).unwrap();
        let expected_legacy = legacy_project_trust_file_path(&env.project);

        let status = json_status(context("json", &env.project)).await;
        assert_eq!(status["schema"], "kiana.app-server.trust-status.v1");
        assert_eq!(status["workspace"], env.project.display().to_string());
        assert_eq!(status["project_trust"], "unknown");
        assert_eq!(status["project_trusted"], false);
        assert_eq!(status["allows_project_resources"], false);
        assert_eq!(status["source"], "default");
        assert_eq!(status["project_root"], env.project.display().to_string());
        assert_eq!(status["project_id"].as_str().unwrap().len(), 64);
        assert_eq!(status["file"]["path"], expected_file.display().to_string());
        assert_eq!(status["file"]["status"], "missing");
        assert_eq!(status["file"]["exists"], false);
        assert_eq!(status["file"]["error"], Value::Null);
        assert_eq!(
            status["legacy_project_file"]["path"],
            expected_legacy.display().to_string()
        );
        assert_eq!(status["legacy_project_file"]["exists"], false);
        assert_eq!(status["legacy_project_file"]["ignored"], true);
        assert_eq!(
            status["legacy_project_file"]["reason"],
            "project_local_trust_is_not_authoritative"
        );

        let text = TrustCommand
            .execute(context("status", &env.project))
            .await
            .unwrap();
        assert!(text.value.contains("project_trust: unknown"));
        assert!(text.value.contains("allows_project_resources: false"));
        assert!(text.value.contains("legacy_project_file_ignored: true"));

        let path = TrustCommand
            .execute(context("path", &env.project))
            .await
            .unwrap();
        assert_eq!(path.value, expected_file.display().to_string());
        assert!(expected_file.starts_with(&env.kiana_home));
    }

    #[tokio::test]
    async fn trust_untrust_reset_uses_user_store_and_ignores_legacy_claim() {
        let env = TrustTestEnv::new("lifecycle");
        let legacy_path = legacy_project_trust_file_path(&env.project);
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        fs::write(&legacy_path, r#"{"trusted":true}"#).unwrap();

        let initial = json_status(context("json", &env.project)).await;
        assert_eq!(initial["project_trust"], "unknown");
        assert_eq!(initial["legacy_project_file"]["exists"], true);
        assert_eq!(initial["legacy_project_file"]["ignored"], true);

        let trusted = TrustCommand
            .execute(context(".", &env.project))
            .await
            .unwrap();
        assert!(trusted.value.contains("project_trust: trusted"));
        let app_state = HashMap::from([("cwd".to_string(), json!(env.project.clone()))]);
        assert_eq!(
            project_trust_from_app_state(&app_state),
            ProjectTrust::Trusted
        );
        let trusted_status = json_status(context("json", &env.project)).await;
        assert_eq!(trusted_status["source"], "user_store");
        assert_eq!(trusted_status["file"]["status"], "found");
        assert_eq!(trusted_status["file"]["exists"], true);

        let untrusted = TrustCommand
            .execute(context("untrust", &env.project))
            .await
            .unwrap();
        assert!(untrusted.value.contains("project_trust: untrusted"));
        assert_eq!(
            project_trust_from_app_state(&app_state),
            ProjectTrust::Untrusted
        );

        let reset = TrustCommand
            .execute(context("reset", &env.project))
            .await
            .unwrap();
        assert!(reset.value.contains("project_trust: unknown"));
        assert_eq!(
            project_trust_from_app_state(&app_state),
            ProjectTrust::Unknown
        );
        let reset_status = json_status(context("json", &env.project)).await;
        assert_eq!(reset_status["project_trust"], "unknown");
        assert_eq!(reset_status["source"], "default");
        assert_eq!(reset_status["file"]["status"], "missing");
        assert_eq!(reset_status["legacy_project_file"]["exists"], true);
        assert!(legacy_path.is_file());
    }

    #[tokio::test]
    async fn malformed_user_store_record_is_reported_and_fails_closed() {
        let env = TrustTestEnv::new("malformed-record");
        TrustCommand
            .execute(context("trust", &env.project))
            .await
            .unwrap();
        let record_path = project_trust_file_path(&env.project).unwrap();
        fs::write(&record_path, r#"{"schema":"wrong"}"#).unwrap();

        let status = json_status(context("json", &env.project)).await;
        assert_eq!(status["project_trust"], "unknown");
        assert_eq!(status["project_trusted"], false);
        assert_eq!(status["allows_project_resources"], false);
        assert_eq!(status["source"], "default");
        assert_eq!(status["file"]["status"], "error");
        assert_eq!(status["file"]["exists"], true);
        assert!(status["file"]["error"]
            .as_str()
            .is_some_and(|error| error.contains("failed to parse")));
    }

    #[tokio::test]
    async fn valid_session_decision_precedes_user_store_error() {
        let env = TrustTestEnv::new("session-precedence");
        TrustCommand
            .execute(context("trust", &env.project))
            .await
            .unwrap();
        let record_path = project_trust_file_path(&env.project).unwrap();
        fs::write(&record_path, b"not-json").unwrap();

        let status = json_status(context_with_trust("json", &env.project, json!(true))).await;
        assert_eq!(status["project_trust"], "trusted");
        assert_eq!(status["project_trusted"], true);
        assert_eq!(status["allows_project_resources"], true);
        assert_eq!(status["source"], "session");
        assert_eq!(status["file"]["status"], "error");

        let invalid =
            json_status(context_with_trust("json", &env.project, json!("sometimes"))).await;
        assert_eq!(invalid["project_trust"], "unknown");
        assert_eq!(invalid["source"], "default");
    }
}
