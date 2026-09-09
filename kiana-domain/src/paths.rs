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
