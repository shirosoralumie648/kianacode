//! Harness 的 Linux `bwrap` 沙箱启动计划。
//!
//! 本模块只负责把已确定的沙箱档位转换为 `bubblewrap` 参数和受限环境变量；它不执行
//! 命令、不判断模型请求是否获批，也不产生授权。真正的能力请求仍要先经过
//! `ControlPlane` 与 capability broker。本模块因此是 daemon 在调用 runner 前的一个
//! 收敛边界，而不是可被入口层绕过的第二条执行循环。
//!
//! 参数设计参考了 Codex 的公开 Apache-2.0 实现中的会话隔离、能力清空和核心环境变量
//! 筛选做法，但这里的文件系统规则以 Kiana 自己的项目根目录为权威：先将宿主根目录
//! 只读绑定，再只读或可写地重新绑定获准的项目根目录。`reference/` 仅是审计输入，
//! 不会在运行时加载为实现。

use kiana_ports::PortError;
use kiana_runner_protocol::{DEFAULT_HARNESS_SANDBOX, HARNESS_SANDBOX_WORKSPACE_WRITE};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Receipt 和子进程环境中标识当前实际采用的 Linux 沙箱后端名称。
///
/// 这个常量描述实现选择，不表示宿主机一定安装了该程序；安装检查由
/// [`bwrap_plan`] 中的路径解析完成，缺失时返回结构化失败而非退化为非沙箱执行。
pub const SANDBOX_BACKEND: &str = "bwrap";
const ENV_BWRAP: &str = "KIANA_BWRAP";
const NETWORK_DISABLED_ENV: &str = "KIANA_SANDBOX_NETWORK_DISABLED";

/// 可从宿主继承的最小 Unix 核心环境变量白名单。
///
/// 白名单比黑名单更容易审计：未列出的变量默认不会传给沙箱。随后仍会按名称排除
/// 可能承载凭据的变量，并删除临时目录变量以保证它们指向沙箱内的 `/tmp`。
const UNIX_CORE_ENV_VARS: &[&str] = &[
    "PATH", "SHELL", "TMPDIR", "TEMP", "TMP", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "LOGNAME",
    "USER",
];

const KIANA_LAUNCH_ENV_VARS: &[&str] = &[
    "KIANA_HARNESS_SCRIPT",
    "KIANA_SYSTEM_PROMPT",
    "KIANA_APPEND_SYSTEM_PROMPT",
];

/// 已解析、但尚未启动的 `bubblewrap` 进程计划。
///
/// 调用方应把 [`program`](Self::program) 作为可执行文件、按顺序传入
/// [`args`](Self::args)。参数末尾的 `--` 是 bwrap 与被执行命令的边界；计划本身没有
/// 副作用，因此可以在测试和审批路径中检查其内容。
#[derive(Debug, Clone)]
pub struct BwrapPlan {
    /// 已规范化且确认为普通文件的 `bubblewrap` 可执行文件路径。
    pub program: PathBuf,
    /// 传给 `bubblewrap` 的完整参数，不含最终由调用者附加的待执行命令。
    pub args: Vec<OsString>,
}

/// 为某次 harness shell 调用构造 fail-closed 的 `bubblewrap` 启动计划。
///
/// `project_root` 与 `workdir` 都会先规范化，以消除符号链接和相对路径造成的目录逃逸；
/// 工作目录必须位于项目根目录之内。当前只支持只读与项目内可写两种 runner 协议定义的
/// 档位。未知档位、缺失 bwrap、不可用目录或目录越界都会返回 [`PortError::Failed`]，
/// 调用者不得据此回退到未沙箱化进程。
///
/// 返回值固定包含新会话、父进程退出联动、网络命名空间隔离、能力清空和显式环境清空。
/// 它不保证内核、bubblewrap 版本或宿主挂载策略没有漏洞，因此只能说明本进程请求了
/// 这些限制，不能把它当作现实世界副作用已被完全隔离的证据。
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
            // `/tmp` 被挂成新的 tmpfs 后，需要重新发布项目根；这样位于
            // `/tmp/...` 的工作区不会消失，同时保持对项目内容的只读约束。
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

/// 从宿主环境产生可传入沙箱的最小环境变量集合。
///
/// 处理顺序很重要：先保留核心白名单，再删除凭据模式和 Kiana 启动控制变量，最后覆盖
/// 临时目录并附加沙箱标记。名称中包含 `KEY`、`SECRET` 或 `TOKEN` 的变量会被拒绝；
/// 这是防御性启发式，不能证明其他名称的变量不含敏感信息，因此新敏感变量不应依赖
/// 该函数自动安全地传递。
///
/// `KIANA_SANDBOX_NETWORK_DISABLED=1` 是对子进程的声明和审计线索；实际网络隔离依赖
/// 上层计划中的 `--unshare-all` 以及宿主内核行为。
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
    // 显式配置优先于 PATH 搜索，便于受控部署固定二进制来源；两条路径都会
    // canonicalize，避免把不存在或目录目标误当成可执行后端。
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
