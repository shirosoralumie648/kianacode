//! Codex-style Linux confinement for harness `shell.exec`.
//!
//! Derived from OpenAI Codex (Apache-2.0):
//! - `codex-rs/linux-sandbox` bubblewrap flags (`--new-session`, `--cap-drop ALL`)
//! - `codex-rs/core/spawn.rs` `env_clear` plus
//!   `codex-rs/protocol/shell_environment.rs` Core inherit + default
//!   `*KEY*`/`*SECRET*`/`*TOKEN*` excludes
//!
//! Filesystem plan keeps Kiana's `--ro-bind / /` + optional writable project
//! bind. Copied into the daemon; `reference/` is audit-only.

use kiana_ports::PortError;
use kiana_runner_protocol::{DEFAULT_HARNESS_SANDBOX, HARNESS_SANDBOX_WORKSPACE_WRITE};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const SANDBOX_BACKEND: &str = "bwrap";
const ENV_BWRAP: &str = "KIANA_BWRAP";
const NETWORK_DISABLED_ENV: &str = "KIANA_SANDBOX_NETWORK_DISABLED";

/// Codex `UNIX_CORE_ENV_VARS` for `ShellEnvironmentPolicyInherit::Core`.
const UNIX_CORE_ENV_VARS: &[&str] = &[
    "PATH", "SHELL", "TMPDIR", "TEMP", "TMP", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "LOGNAME",
    "USER",
];

const KIANA_LAUNCH_ENV_VARS: &[&str] = &[
    "KIANA_HARNESS_SCRIPT",
    "KIANA_SYSTEM_PROMPT",
    "KIANA_APPEND_SYSTEM_PROMPT",
];

#[derive(Debug, Clone)]
pub struct BwrapPlan {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

pub fn bwrap_plan(
    project_root: &Path,
    workdir: &Path,
    sandbox: &str,
) -> Result<BwrapPlan, PortError> {
    let bwrap = find_bwrap()?;
    let project_root = canonicalize_dir(project_root, "harness_project_root_invalid")?;
    let workdir = canonicalize_dir(workdir, "harness_workdir_invalid")?;
    if !workdir.starts_with(&project_root) {
        return Err(PortError::Failed(
            "harness_workdir_outside_project".to_owned(),
        ));
    }

    let mut args = vec![
        OsString::from("--new-session"),
        OsString::from("--die-with-parent"),
        OsString::from("--unshare-all"),
        OsString::from("--ro-bind"),
        OsString::from("/"),
        OsString::from("/"),
        OsString::from("--dev"),
        OsString::from("/dev"),
        OsString::from("--proc"),
        OsString::from("/proc"),
        OsString::from("--tmpfs"),
        OsString::from("/tmp"),
    ];

    match sandbox {
        DEFAULT_HARNESS_SANDBOX => {
            // Re-publish the project after `/tmp` tmpfs so `/tmp/...` workspaces
            // remain visible and stay read-only.
            args.extend([
                OsString::from("--ro-bind"),
                project_root.as_os_str().to_os_string(),
                project_root.as_os_str().to_os_string(),
            ]);
        }
        HARNESS_SANDBOX_WORKSPACE_WRITE => {
            args.extend([
                OsString::from("--bind"),
                project_root.as_os_str().to_os_string(),
                project_root.as_os_str().to_os_string(),
            ]);
        }
        other => {
            return Err(PortError::Failed(format!("sandbox_unsupported:{other}")));
        }
    }

    args.extend([
        OsString::from("--chdir"),
        workdir.as_os_str().to_os_string(),
        OsString::from("--cap-drop"),
        OsString::from("ALL"),
        OsString::from("--clearenv"),
    ]);
    for (key, value) in sandbox_env(std::env::vars(), sandbox) {
        args.extend([
            OsString::from("--setenv"),
            OsString::from(key),
            OsString::from(value),
        ]);
    }
    args.push(OsString::from("--"));

    Ok(BwrapPlan {
        program: bwrap,
        args,
    })
}

/// Codex Core inherit + default excludes, then Kiana sandbox markers.
pub fn sandbox_env(
    inherited: impl IntoIterator<Item = (String, String)>,
    sandbox: &str,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = inherited
        .into_iter()
        .filter(|(name, _)| is_unix_core_env(name))
        .filter(|(name, _)| !is_default_excluded_env(name))
        .filter(|(name, _)| !is_kiana_launch_env(name))
        .collect();
    env.retain(|(name, _)| {
        !name.eq_ignore_ascii_case("TMPDIR")
            && !name.eq_ignore_ascii_case("TEMP")
            && !name.eq_ignore_ascii_case("TMP")
    });
    env.push(("TMPDIR".to_owned(), "/tmp".to_owned()));
    env.push(("KIANA_SANDBOX".to_owned(), sandbox.to_owned()));
    env.push((
        "KIANA_SANDBOX_BACKEND".to_owned(),
        SANDBOX_BACKEND.to_owned(),
    ));
    env.push((NETWORK_DISABLED_ENV.to_owned(), "1".to_owned()));
    env
}

fn is_unix_core_env(name: &str) -> bool {
    UNIX_CORE_ENV_VARS
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(name))
}

fn is_default_excluded_env(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.contains("KEY") || upper.contains("SECRET") || upper.contains("TOKEN")
}

fn is_kiana_launch_env(name: &str) -> bool {
    KIANA_LAUNCH_ENV_VARS
        .iter()
        .any(|restricted| restricted.eq_ignore_ascii_case(name))
}

fn find_bwrap() -> Result<PathBuf, PortError> {
    if let Ok(explicit) = std::env::var(ENV_BWRAP) {
        let explicit = explicit.trim();
        if !explicit.is_empty() {
            return canonicalize_file(Path::new(explicit));
        }
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("bwrap");
        if candidate.is_file() {
            return canonicalize_file(&candidate);
        }
    }
    Err(PortError::Failed("sandbox_unavailable:bwrap".to_owned()))
}

fn canonicalize_dir(path: &Path, error: &str) -> Result<PathBuf, PortError> {
    let resolved = path
        .canonicalize()
        .map_err(|cause| PortError::Failed(format!("{error}:{cause}")))?;
    if !resolved.is_dir() {
        return Err(PortError::Failed(format!("{error}:not_directory")));
    }
    Ok(resolved)
}

fn canonicalize_file(path: &Path) -> Result<PathBuf, PortError> {
    let resolved = path
        .canonicalize()
        .map_err(|cause| PortError::Failed(format!("sandbox_unavailable:bwrap:{cause}")))?;
    if !resolved.is_file() {
        return Err(PortError::Failed("sandbox_unavailable:bwrap".to_owned()));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-bwrap-plan-{stamp}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn plan_args(sandbox: &str) -> (PathBuf, Vec<String>) {
        let root = temp_root();
        let plan = bwrap_plan(&root, &root, sandbox).unwrap();
        let args: Vec<String> = plan
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        (root, args)
    }

    #[test]
    fn read_only_rebinds_project_after_tmpfs() {
        let (root, args) = plan_args("read-only");
        let tmpfs = args.iter().position(|arg| arg == "--tmpfs").unwrap();
        let rebind = args
            .iter()
            .rposition(|arg| arg == root.to_string_lossy().as_ref())
            .unwrap();
        assert!(tmpfs < rebind);
        assert!(args.contains(&"--ro-bind".to_string()));
        assert!(!args.windows(3).any(|window| {
            window[0] == "--bind" && window[1] == root.to_string_lossy().as_ref()
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn workspace_write_binds_project() {
        let (root, args) = plan_args("workspace-write");
        assert!(args.windows(3).any(|window| {
            window[0] == "--bind" && window[1] == root.to_string_lossy().as_ref()
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn danger_full_access_is_rejected() {
        let root = temp_root();
        let error = bwrap_plan(&root, &root, "danger-full-access").unwrap_err();
        assert_eq!(
            error,
            PortError::Failed("sandbox_unsupported:danger-full-access".to_owned())
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_uses_codex_session_cap_drop_and_clearenv() {
        let (root, args) = plan_args("read-only");
        assert_eq!(args.first().map(String::as_str), Some("--new-session"));
        assert!(args.contains(&"--die-with-parent".to_string()));
        assert!(args.contains(&"--cap-drop".to_string()));
        assert!(args.contains(&"--clearenv".to_string()));
        let cap = args.iter().position(|arg| arg == "--cap-drop").unwrap();
        assert_eq!(args.get(cap + 1).map(String::as_str), Some("ALL"));
        let clear = args.iter().position(|arg| arg == "--clearenv").unwrap();
        let sep = args.iter().position(|arg| arg == "--").unwrap();
        assert!(clear < sep);
        assert!(args.windows(3).any(|window| {
            window[0] == "--setenv" && window[1] == "KIANA_SANDBOX" && window[2] == "read-only"
        }));
        assert!(args.windows(3).any(|window| {
            window[0] == "--setenv"
                && window[1] == "KIANA_SANDBOX_NETWORK_DISABLED"
                && window[2] == "1"
        }));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sandbox_env_keeps_core_path_and_strips_secrets() {
        let env = sandbox_env(
            [
                ("PATH".to_owned(), "/bin".to_owned()),
                ("HOME".to_owned(), "/home/kiana".to_owned()),
                ("ANTHROPIC_API_KEY".to_owned(), "secret-key".to_owned()),
                ("GITHUB_TOKEN".to_owned(), "secret-token".to_owned()),
                ("AWS_SECRET_ACCESS_KEY".to_owned(), "secret".to_owned()),
                (
                    "KIANA_HARNESS_SCRIPT".to_owned(),
                    "/tmp/script.json".to_owned(),
                ),
                ("KIANA_SYSTEM_PROMPT".to_owned(), "do not leak".to_owned()),
                ("EDITOR".to_owned(), "vim".to_owned()),
            ],
            "read-only",
        );
        let map: std::collections::BTreeMap<_, _> = env.into_iter().collect();
        assert_eq!(map.get("PATH").map(String::as_str), Some("/bin"));
        assert_eq!(map.get("HOME").map(String::as_str), Some("/home/kiana"));
        assert_eq!(map.get("TMPDIR").map(String::as_str), Some("/tmp"));
        assert_eq!(
            map.get("KIANA_SANDBOX").map(String::as_str),
            Some("read-only")
        );
        assert!(!map.contains_key("ANTHROPIC_API_KEY"));
        assert!(!map.contains_key("GITHUB_TOKEN"));
        assert!(!map.contains_key("AWS_SECRET_ACCESS_KEY"));
        assert!(!map.contains_key("KIANA_HARNESS_SCRIPT"));
        assert!(!map.contains_key("KIANA_SYSTEM_PROMPT"));
        assert!(!map.contains_key("EDITOR"));
    }
}
