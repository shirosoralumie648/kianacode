// File type detection for preview system

use std::path::Path;

/// Represents different file types that can be previewed
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileType {
    /// Text file with optional language hint for syntax highlighting
    Text { language: Option<String> },
    /// Binary file (hex dump)
    Binary,
    /// Markdown file
    Markdown,
    /// Image file (for future ASCII art support)
    Image,
    /// Archive file (for future listing support)
    Archive,
    /// Unknown or unsupported file type
    Unknown,
}

/// Detects file types based on extension and content
pub struct FileTypeDetector;

impl FileTypeDetector {
    /// Detect file type from path (primarily using extension)
    pub fn detect(path: &Path) -> FileType {
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase());

        match extension.as_deref() {
            // Markdown
            Some("md") | Some("markdown") => FileType::Markdown,

            // Rust
            Some("rs") => FileType::Text {
                language: Some("rust".to_string()),
            },

            // Python
            Some("py") | Some("pyw") => FileType::Text {
                language: Some("python".to_string()),
            },

            // JavaScript/TypeScript
            Some("js") | Some("mjs") | Some("cjs") => FileType::Text {
                language: Some("javascript".to_string()),
            },
            Some("ts") | Some("mts") | Some("cts") => FileType::Text {
                language: Some("typescript".to_string()),
            },
            Some("jsx") => FileType::Text {
                language: Some("jsx".to_string()),
            },
            Some("tsx") => FileType::Text {
                language: Some("tsx".to_string()),
            },

            // Web
            Some("html") | Some("htm") => FileType::Text {
                language: Some("html".to_string()),
            },
            Some("css") => FileType::Text {
                language: Some("css".to_string()),
            },

            // Shell
            Some("sh") | Some("bash") | Some("zsh") => FileType::Text {
                language: Some("bash".to_string()),
            },

            // Configuration
            Some("json") => FileType::Text {
                language: Some("json".to_string()),
            },
            Some("yaml") | Some("yml") => FileType::Text {
                language: Some("yaml".to_string()),
            },
            Some("toml") => FileType::Text {
                language: Some("toml".to_string()),
            },
            Some("xml") => FileType::Text {
                language: Some("xml".to_string()),
            },
            Some("ini") | Some("conf") | Some("cfg") => FileType::Text {
                language: Some("ini".to_string()),
            },

            // C/C++
            Some("c") | Some("h") => FileType::Text {
                language: Some("c".to_string()),
            },
            Some("cpp") | Some("cc") | Some("cxx") | Some("hpp") | Some("hxx") => FileType::Text {
                language: Some("cpp".to_string()),
            },

            // Go
            Some("go") => FileType::Text {
                language: Some("go".to_string()),
            },

            // Java
            Some("java") => FileType::Text {
                language: Some("java".to_string()),
            },

            // Text files
            Some("txt") | Some("log") | Some("text") => FileType::Text { language: None },

            // Images
            Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("bmp") | Some("webp") => {
                FileType::Image
            }

            // Archives
            Some("zip") | Some("tar") | Some("gz") | Some("bz2") | Some("xz") | Some("7z") => {
                FileType::Archive
            }

            // Default to Unknown
            _ => FileType::Unknown,
        }
    }

    /// Detect file type from content (magic bytes)
    pub fn detect_from_content(bytes: &[u8]) -> FileType {
        if bytes.is_empty() {
            return FileType::Unknown;
        }

        // Check for common binary file signatures
        if bytes.len() >= 4 {
            match &bytes[0..4] {
                // PNG
                [0x89, 0x50, 0x4E, 0x47] => return FileType::Image,
                // JPEG
                [0xFF, 0xD8, 0xFF, _] => return FileType::Image,
                // GIF
                [0x47, 0x49, 0x46, 0x38] => return FileType::Image,
                // ZIP
                [0x50, 0x4B, 0x03, 0x04] => return FileType::Archive,
                _ => {}
            }
        }

        // Check if content appears to be text
        if Self::is_likely_text(bytes) {
            FileType::Text { language: None }
        } else {
            FileType::Binary
        }
    }

    /// Heuristic to determine if content is likely text
    fn is_likely_text(bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return true;
        }

        // Sample first 512 bytes
        let sample_size = bytes.len().min(512);
        let sample = &bytes[0..sample_size];

        // Count non-printable characters
        let non_printable = sample
            .iter()
            .filter(|&&b| {
                // Allow common control characters
                b < 32 && b != b'\n' && b != b'\r' && b != b'\t'
            })
            .count();

        // If less than 5% non-printable, consider it text
        (non_printable as f32 / sample_size as f32) < 0.05
    }

    /// Combined detection: try path first, fall back to content
    pub fn detect_with_content(path: &Path, bytes: &[u8]) -> FileType {
        let from_path = Self::detect(path);

        // If path detection is inconclusive, check content
        match from_path {
            FileType::Unknown => Self::detect_from_content(bytes),
            _ => from_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_rust() {
        let path = PathBuf::from("test.rs");
        assert_eq!(
            FileTypeDetector::detect(&path),
            FileType::Text {
                language: Some("rust".to_string())
            }
        );
    }

    #[test]
    fn test_detect_markdown() {
        let path = PathBuf::from("README.md");
        assert_eq!(FileTypeDetector::detect(&path), FileType::Markdown);
    }

    #[test]
    fn test_detect_json() {
        let path = PathBuf::from("config.json");
        assert_eq!(
            FileTypeDetector::detect(&path),
            FileType::Text {
                language: Some("json".to_string())
            }
        );
    }

    #[test]
    fn test_detect_image() {
        let path = PathBuf::from("image.png");
        assert_eq!(FileTypeDetector::detect(&path), FileType::Image);
    }

    #[test]
    fn test_detect_unknown() {
        let path = PathBuf::from("unknown.xyz");
        assert_eq!(FileTypeDetector::detect(&path), FileType::Unknown);
    }

    #[test]
    fn test_detect_from_content_text() {
        let text = b"Hello, world!\nThis is text.";
        assert!(matches!(
            FileTypeDetector::detect_from_content(text),
            FileType::Text { .. }
        ));
    }

    #[test]
    fn test_detect_from_content_binary() {
        let binary = &[0xFF, 0xD8, 0xFF, 0xE0]; // JPEG header
        assert_eq!(
            FileTypeDetector::detect_from_content(binary),
            FileType::Image
        );
    }

    #[test]
    fn test_is_likely_text() {
        let text = b"Normal text content\n";
        assert!(FileTypeDetector::is_likely_text(text));

        let binary = &[0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE];
        assert!(!FileTypeDetector::is_likely_text(binary));
    }
}
