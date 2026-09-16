use std::path::{Component, Path};

pub const EXCLUSIVE_PATH_LOCK: &str = "*";

pub fn builder_lock_paths(path_allow: &[String]) -> Vec<String> {
    let mut paths: Vec<String> = path_allow
        .iter()
        .filter_map(|path| normalize_role_path(path))
        .collect();
    paths.sort();
    paths.dedup();
    if paths.is_empty() {
        vec![EXCLUSIVE_PATH_LOCK.to_owned()]
    } else {
        paths
    }
}

pub fn path_locks_conflict(left: &str, right: &str) -> bool {
    if left == EXCLUSIVE_PATH_LOCK || right == EXCLUSIVE_PATH_LOCK {
        return true;
    }
    left == right
        || left.starts_with(&format!("{right}/"))
        || right.starts_with(&format!("{left}/"))
}

pub fn allow_list_covers(path_allow: &[String], path: &str) -> bool {
    let Some(path) = normalize_role_path(path) else {
        return false;
    };
    if path_allow
        .iter()
        .any(|allow| allow.trim() == "." || allow.trim() == "*")
    {
        return true;
    }
    path_allow.iter().any(|allow| {
        let allow = allow.trim().trim_matches('/');
        !allow.is_empty() && (path == allow || path.starts_with(&format!("{allow}/")))
    })
}

/// Normalize and authorize one relative path against a server-owned allow-list.
///
/// This is the shared lexical containment boundary used by policy, Cell, workspace and daemon
/// adapters. Filesystem adapters must still perform their own no-follow/symlink/hardlink checks
/// after this pure check; a successful result never grants an effect by itself.
pub fn enforce_path_containment(path_allow: &[String], path: &str) -> Result<String, &'static str> {
    let normalized = normalize_role_path(path).ok_or("path_not_relative")?;
    if allow_list_covers(path_allow, &normalized) {
        Ok(normalized)
    } else {
        Err("path_outside_scope")
    }
}

/// Shared lexical root check for already-resolved filesystem paths.
pub fn enforce_root_containment<'a>(
    root: &'a std::path::Path,
    candidate: &'a std::path::Path,
) -> Result<(), &'static str> {
    if candidate == root || candidate.starts_with(root) {
        Ok(())
    } else {
        Err("path_outside_project")
    }
}

pub fn normalize_role_path(path: &str) -> Option<String> {
    let path = path.trim().replace('\\', "/");
    if path.is_empty() {
        return None;
    }
    if Path::new(&path).is_absolute() {
        return None;
    }
    if path == "." {
        return Some(".".to_owned());
    }
    let mut parts = Vec::new();
    for component in Path::new(&path).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}
