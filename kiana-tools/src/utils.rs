// Utility functions for tools

use std::path::{Path, PathBuf};

pub fn expand_path(path: &str) -> PathBuf {
    if path.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

pub fn to_relative_path(path: &str, base: &str) -> String {
    let path = Path::new(path);
    let base = Path::new(base);

    if let Ok(relative) = path.strip_prefix(base) {
        relative.display().to_string()
    } else {
        path.display().to_string()
    }
}

pub fn add_line_numbers(content: &str, start_line: usize) -> String {
    content
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{}\t{}", start_line + i, line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_file_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let size = bytes as f64;
    if size < KB {
        format!("{} B", bytes)
    } else if size < MB {
        format!("{:.2} KB", size / KB)
    } else if size < GB {
        format!("{:.2} MB", size / MB)
    } else {
        format!("{:.2} GB", size / GB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1024), "1.00 KB");
        assert_eq!(format_file_size(1048576), "1.00 MB");
    }

    #[test]
    fn test_add_line_numbers() {
        let content = "line 1\nline 2\nline 3";
        let result = add_line_numbers(content, 1);
        assert!(result.contains("1\tline 1"));
        assert!(result.contains("2\tline 2"));
        assert!(result.contains("3\tline 3"));
    }
}
