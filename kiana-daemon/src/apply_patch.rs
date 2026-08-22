//! Codex-compatible apply_patch parser/applier owned by the daemon.
//!
//! Markers and hunk grammar are derived from OpenAI Codex (Apache-2.0)
//! `codex-rs/apply-patch`. Copied into Kiana; `reference/` is audit-only.

use kiana_ports::PortError;
use serde_json::{json, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};

const BEGIN_PATCH: &str = "*** Begin Patch";
const END_PATCH: &str = "*** End Patch";
const ADD_FILE: &str = "*** Add File: ";
const DELETE_FILE: &str = "*** Delete File: ";
const UPDATE_FILE: &str = "*** Update File: ";
const MOVE_TO: &str = "*** Move to: ";
const EOF_MARKER: &str = "*** End of File";

#[derive(Debug, Clone, PartialEq, Eq)]
enum Hunk {
    AddFile {
        path: PathBuf,
        contents: String,
    },
    DeleteFile {
        path: PathBuf,
    },
    UpdateFile {
        path: PathBuf,
        move_path: Option<PathBuf>,
        chunks: Vec<UpdateChunk>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateChunk {
    change_context: Option<String>,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

pub fn apply_codex_patch(project_root: &Path, patch: &str) -> Result<Value, PortError> {
    let hunks = parse_patch(patch)?;
    if hunks.is_empty() {
        return Err(failed("apply_patch_empty"));
    }
    let mut changed = Vec::new();
    for hunk in hunks {
        changed.push(apply_hunk(project_root, hunk)?);
    }
    Ok(json!({ "changed": changed }))
}

fn parse_patch(patch: &str) -> Result<Vec<Hunk>, PortError> {
    let normalized = patch.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    if lines.first().map(|line| line.trim()) != Some(BEGIN_PATCH) {
        return Err(failed("apply_patch_missing_begin"));
    }

    let mut hunks = Vec::new();
    let mut index = 1;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed.is_empty() {
            index += 1;
            continue;
        }
        if trimmed == END_PATCH {
            break;
        }
        if let Some(path) = trimmed.strip_prefix(ADD_FILE) {
            let path = PathBuf::from(path.trim());
            index += 1;
            let mut contents = String::new();
            while index < lines.len() {
                let line = lines[index];
                let next = line.trim_start();
                if next.starts_with("*** ") {
                    break;
                }
                let body = line
                    .strip_prefix('+')
                    .ok_or_else(|| failed("apply_patch_add_line_missing_plus"))?;
                contents.push_str(body);
                contents.push('\n');
                index += 1;
            }
            hunks.push(Hunk::AddFile { path, contents });
            continue;
        }
        if let Some(path) = trimmed.strip_prefix(DELETE_FILE) {
            hunks.push(Hunk::DeleteFile {
                path: PathBuf::from(path.trim()),
            });
            index += 1;
            continue;
        }
        if let Some(path) = trimmed.strip_prefix(UPDATE_FILE) {
            let path = PathBuf::from(path.trim());
            index += 1;
            let mut move_path = None;
            if index < lines.len() {
                if let Some(moved) = lines[index].trim().strip_prefix(MOVE_TO) {
                    move_path = Some(PathBuf::from(moved.trim()));
                    index += 1;
                }
            }
            let mut chunks = Vec::new();
            while index < lines.len() {
                let marker = lines[index].trim();
                if marker.starts_with("*** ") && marker != EOF_MARKER {
                    break;
                }
                if marker == "@@" || marker.starts_with("@@ ") {
                    let change_context = marker
                        .strip_prefix("@@ ")
                        .map(str::to_owned)
                        .filter(|value| !value.is_empty());
                    index += 1;
                    let mut chunk = UpdateChunk {
                        change_context,
                        old_lines: Vec::new(),
                        new_lines: Vec::new(),
                    };
                    while index < lines.len() {
                        let line = lines[index];
                        let trimmed_line = line.trim();
                        if trimmed_line == "@@"
                            || trimmed_line.starts_with("@@ ")
                            || (trimmed_line.starts_with("*** ") && trimmed_line != EOF_MARKER)
                        {
                            break;
                        }
                        if trimmed_line == EOF_MARKER {
                            index += 1;
                            break;
                        }
                        if let Some(body) = line.strip_prefix('+') {
                            chunk.new_lines.push(body.to_owned());
                        } else if let Some(body) = line.strip_prefix('-') {
                            chunk.old_lines.push(body.to_owned());
                        } else if let Some(body) = line.strip_prefix(' ') {
                            chunk.old_lines.push(body.to_owned());
                            chunk.new_lines.push(body.to_owned());
                        } else if line.is_empty() {
                            chunk.old_lines.push(String::new());
                            chunk.new_lines.push(String::new());
                        } else {
                            return Err(failed("apply_patch_update_line_invalid"));
                        }
                        index += 1;
                    }
                    chunks.push(chunk);
                    continue;
                }
                if marker.is_empty() {
                    index += 1;
                    continue;
                }
                return Err(failed("apply_patch_update_hunk_invalid"));
            }
            hunks.push(Hunk::UpdateFile {
                path,
                move_path,
                chunks,
            });
            continue;
        }
        return Err(failed("apply_patch_hunk_invalid"));
    }
    Ok(hunks)
}

fn apply_hunk(project_root: &Path, hunk: Hunk) -> Result<Value, PortError> {
    match hunk {
        Hunk::AddFile { path, contents } => {
            let target = confined_new_file(project_root, &path)?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(io_failed)?;
            }
            if target.exists() {
                return Err(failed("apply_patch_add_exists"));
            }
            fs::write(&target, contents).map_err(io_failed)?;
            Ok(json!({ "op": "add", "path": display_relative(project_root, &target) }))
        }
        Hunk::DeleteFile { path } => {
            let target = confined_existing_file(project_root, &path)?;
            fs::remove_file(&target).map_err(io_failed)?;
            Ok(json!({ "op": "delete", "path": display_relative(project_root, &target) }))
        }
        Hunk::UpdateFile {
            path,
            move_path,
            chunks,
        } => {
            let source = confined_existing_file(project_root, &path)?;
            let original = fs::read_to_string(&source).map_err(io_failed)?;
            let updated = apply_chunks(&original, &chunks)?;
            let destination = match move_path {
                Some(moved) => {
                    let destination = confined_new_file(project_root, &moved)?;
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent).map_err(io_failed)?;
                    }
                    fs::write(&destination, updated).map_err(io_failed)?;
                    fs::remove_file(&source).map_err(io_failed)?;
                    destination
                }
                None => {
                    fs::write(&source, updated).map_err(io_failed)?;
                    source
                }
            };
            Ok(json!({
                "op": "update",
                "path": display_relative(project_root, &destination)
            }))
        }
    }
}

fn apply_chunks(original: &str, chunks: &[UpdateChunk]) -> Result<String, PortError> {
    let trailing_newline = original.ends_with('\n');
    let mut lines: Vec<String> = original.lines().map(str::to_owned).collect();

    for chunk in chunks {
        let start = locate_chunk(&lines, chunk)?;
        let end = start + chunk.old_lines.len();
        let mut next = Vec::new();
        next.extend_from_slice(&lines[..start]);
        next.extend(chunk.new_lines.iter().cloned());
        next.extend_from_slice(&lines[end..]);
        lines = next;
    }

    if lines.is_empty() {
        return Ok(String::new());
    }
    let mut rendered = lines.join("\n");
    if trailing_newline {
        rendered.push('\n');
    }
    Ok(rendered)
}

fn locate_chunk(lines: &[String], chunk: &UpdateChunk) -> Result<usize, PortError> {
    let search_from = if let Some(context) = &chunk.change_context {
        lines
            .iter()
            .position(|line| line.contains(context))
            .ok_or_else(|| failed("apply_patch_context_missing"))?
    } else {
        0
    };
    if chunk.old_lines.is_empty() {
        return Ok(if chunk.change_context.is_some() {
            search_from + 1
        } else {
            0
        });
    }
    let mut found = None;
    let window = chunk.old_lines.len();
    if search_from + window > lines.len() {
        return Err(failed("apply_patch_hunk_mismatch"));
    }
    for start in search_from..=lines.len() - window {
        if lines[start..start + window] == chunk.old_lines {
            if found.is_some() {
                return Err(failed("apply_patch_hunk_ambiguous"));
            }
            found = Some(start);
        }
    }
    found.ok_or_else(|| failed("apply_patch_hunk_mismatch"))
}

fn confined_existing_file(root: &Path, relative: &Path) -> Result<PathBuf, PortError> {
    let candidate = confined_candidate(root, relative)?;
    let resolved = candidate
        .canonicalize()
        .map_err(|error| failed(format!("apply_patch_path_invalid:{error}")))?;
    ensure_inside(root, &resolved)?;
    if !resolved.is_file() {
        return Err(failed("apply_patch_path_not_file"));
    }
    Ok(resolved)
}

fn confined_new_file(root: &Path, relative: &Path) -> Result<PathBuf, PortError> {
    let candidate = confined_candidate(root, relative)?;
    if candidate.exists() {
        let resolved = candidate
            .canonicalize()
            .map_err(|error| failed(format!("apply_patch_path_invalid:{error}")))?;
        ensure_inside(root, &resolved)?;
        return Ok(resolved);
    }
    if let Some(parent) = candidate.parent() {
        if parent.exists() {
            let resolved_parent = parent
                .canonicalize()
                .map_err(|error| failed(format!("apply_patch_path_invalid:{error}")))?;
            ensure_inside(root, &resolved_parent)?;
        } else {
            ensure_inside(root, parent)?;
        }
    }
    Ok(candidate)
}

fn confined_candidate(root: &Path, relative: &Path) -> Result<PathBuf, PortError> {
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(failed("apply_patch_path_not_relative"));
    }
    if relative.as_os_str().is_empty() {
        return Err(failed("apply_patch_path_required"));
    }
    Ok(root.join(relative))
}

fn ensure_inside(root: &Path, candidate: &Path) -> Result<(), PortError> {
    if !candidate.starts_with(root) {
        return Err(failed("apply_patch_path_outside_project"));
    }
    Ok(())
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn failed(message: impl Into<String>) -> PortError {
    PortError::Failed(message.into())
}

fn io_failed(error: std::io::Error) -> PortError {
    PortError::Failed(format!("apply_patch_io:{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("kiana-apply-patch-{stamp}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn add_file_is_confined_to_project_root() {
        let root = temp_root();
        apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Add File: notes/hello.txt\n+hello\n*** End Patch\n",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("notes/hello.txt")).unwrap(),
            "hello\n"
        );
    }

    #[test]
    fn update_replaces_a_unique_hunk() {
        let root = temp_root();
        fs::write(root.join("file.txt"), "old\nkeep\n").unwrap();
        apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Update File: file.txt\n@@\n-old\n+new\n*** End Patch\n",
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(root.join("file.txt")).unwrap(),
            "new\nkeep\n"
        );
    }

    #[test]
    fn parent_dir_escape_is_rejected() {
        let root = temp_root();
        let error = apply_codex_patch(
            &root,
            "*** Begin Patch\n*** Add File: ../escape.txt\n+nope\n*** End Patch\n",
        )
        .unwrap_err();
        assert_eq!(error, failed("apply_patch_path_not_relative"));
    }
}
