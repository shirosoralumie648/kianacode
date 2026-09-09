use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Available slash commands
const COMMANDS: &[&str] = &[
    "/help",
    "/review",
    "/fix",
    "/test",
    "/clear",
    "/explain",
    "/refactor",
    "/docs",
    "/sessions",
    "/new",
    "/fork",
    "/switch",
];

/// Directory listing cache entry
struct CacheEntry {
    entries: Vec<String>,
    timestamp: Instant,
}

/// Completion engine with caching
pub struct CompletionEngine {
    dir_cache: HashMap<PathBuf, CacheEntry>,
    cache_duration: Duration,
}

impl CompletionEngine {
    pub fn new() -> Self {
        Self {
            dir_cache: HashMap::new(),
            cache_duration: Duration::from_millis(100),
        }
    }

    /// Get completions for the current input at cursor position
    pub fn get_completions(&mut self, input: &str, cursor_pos: usize) -> Vec<String> {
        let input = &input[..cursor_pos.min(input.len())];

        // Command completion (starts with /)
        if input.starts_with('/') {
            return self.complete_command(input);
        }

        // Check if we're in a command argument position
        if let Some(space_idx) = input.rfind(' ') {
            let before_cursor = &input[..space_idx];
            let arg_part = &input[space_idx + 1..];

            // Check if this looks like a command with file argument
            if before_cursor.starts_with('/') {
                let cmd = before_cursor.split_whitespace().next().unwrap_or("");
                // Commands that take file/directory arguments
                if matches!(cmd, "/review" | "/fix" | "/test" | "/explain" | "/refactor") {
                    return self.complete_path(arg_part);
                }
            }
        }

        Vec::new()
    }

    /// Complete slash commands
    fn complete_command(&self, input: &str) -> Vec<String> {
        COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(input))
            .map(|s| s.to_string())
            .collect()
    }

    /// Complete file/directory paths
    fn complete_path(&mut self, partial: &str) -> Vec<String> {
        let (dir_part, file_part) = if partial.is_empty() {
            (".", "")
        } else if partial.ends_with('/') || partial.ends_with(std::path::MAIN_SEPARATOR) {
            (partial, "")
        } else {
            // Split into directory and file parts
            match partial.rfind(|c| c == '/' || c == std::path::MAIN_SEPARATOR) {
                Some(idx) => (&partial[..=idx], &partial[idx + 1..]),
                None => (".", partial),
            }
        };

        let dir_path = PathBuf::from(dir_part);
        let entries = self.get_directory_entries(&dir_path);

        // Filter entries that match the file part
        entries
            .into_iter()
            .filter(|entry| entry.starts_with(file_part))
            .map(|entry| {
                if dir_part == "." && !partial.starts_with("./") {
                    entry
                } else {
                    format!("{}{}", dir_part, entry)
                }
            })
            .collect()
    }

    /// Get directory entries with caching
    fn get_directory_entries(&mut self, dir: &Path) -> Vec<String> {
        let now = Instant::now();

        // Check cache
        if let Some(entry) = self.dir_cache.get(dir) {
            if now.duration_since(entry.timestamp) < self.cache_duration {
                return entry.entries.clone();
            }
        }

        // Read directory
        let mut entries = Vec::new();
        if let Ok(read_dir) = fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                if let Ok(file_name) = entry.file_name().into_string() {
                    // Add trailing slash for directories
                    let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    if is_dir {
                        entries.push(format!("{}/", file_name));
                    } else {
                        entries.push(file_name);
                    }
                }
            }
        }

        entries.sort();

        // Update cache
        self.dir_cache.insert(
            dir.to_path_buf(),
            CacheEntry {
                entries: entries.clone(),
                timestamp: now,
            },
        );

        entries
    }

    /// Apply a completion to the input buffer
    pub fn apply_completion(input: &str, cursor_pos: usize, completion: &str) -> (String, usize) {
        let before_cursor = &input[..cursor_pos.min(input.len())];
        let after_cursor = &input[cursor_pos.min(input.len())..];

        // Find the start of the token to replace
        let token_start = if before_cursor.starts_with('/') && !before_cursor.contains(' ') {
            // Completing a command
            0
        } else if let Some(space_idx) = before_cursor.rfind(' ') {
            // Completing after a space (file argument)
            space_idx + 1
        } else {
            0
        };

        let new_input = format!("{}{}{}", &input[..token_start], completion, after_cursor);
        let new_cursor_pos = token_start + completion.len();

        (new_input, new_cursor_pos)
    }

    /// Clear the directory cache
    pub fn clear_cache(&mut self) {
        self.dir_cache.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_completion() {
        let mut engine = CompletionEngine::new();
        let completions = engine.get_completions("/re", 3);
        assert!(completions.contains(&"/review".to_string()));
        assert!(completions.contains(&"/refactor".to_string()));
    }

    #[test]
    fn test_apply_completion() {
        let (result, cursor) = CompletionEngine::apply_completion("/re", 3, "/review");
        assert_eq!(result, "/review");
        assert_eq!(cursor, 7);

        let (result, cursor) =
            CompletionEngine::apply_completion("/review src/ma", 14, "src/main.rs");
        assert_eq!(result, "/review src/main.rs");
        assert_eq!(cursor, 19);
    }
}
