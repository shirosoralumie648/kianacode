use std::ffi::OsString;
#[cfg(windows)]
use std::path::{Path, PathBuf};

pub(crate) fn preferred_bash_program() -> OsString {
    if let Some(configured) = configured_bash_program() {
        return configured;
    }

    #[cfg(windows)]
    if let Some(git_bash) = windows_git_bash_program() {
        return git_bash.into_os_string();
    }

    OsString::from("bash")
}

fn configured_bash_program() -> Option<OsString> {
    std::env::var_os("KIANA_BASH_PATH").filter(|value| !value.is_empty())
}

#[cfg(windows)]
fn windows_git_bash_program() -> Option<PathBuf> {
    for candidate in [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files\Git\usr\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
    ] {
        let path = Path::new(candidate);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }

    find_bash_on_path().filter(|path| !is_windows_system_bash(path))
}

#[cfg(windows)]
fn find_bash_on_path() -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).find_map(|dir| {
        for name in ["bash.exe", "bash"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    })
}

#[cfg(windows)]
fn is_windows_system_bash(path: &Path) -> bool {
    let Ok(canonical) = path.canonicalize() else {
        return false;
    };
    canonical
        .to_string_lossy()
        .eq_ignore_ascii_case(r"C:\Windows\System32\bash.exe")
}
