use once_cell::sync::Lazy;
use std::collections::HashSet;

pub static BINARY_EXTENSIONS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".ico", ".webp", ".tiff", ".tif", ".mp4", ".mov",
        ".avi", ".mkv", ".webm", ".wmv", ".flv", ".m4v", ".mpeg", ".mpg", ".mp3", ".wav", ".ogg",
        ".flac", ".aac", ".m4a", ".wma", ".aiff", ".opus", ".zip", ".tar", ".gz", ".bz2", ".7z",
        ".rar", ".xz", ".z", ".tgz", ".iso", ".exe", ".dll", ".so", ".dylib", ".bin", ".o", ".a",
        ".obj", ".lib", ".app", ".msi", ".deb", ".rpm", ".pdf", ".doc", ".docx", ".xls", ".xlsx",
        ".ppt", ".pptx", ".odt", ".ods", ".odp", ".ttf", ".otf", ".woff", ".woff2", ".eot", ".pyc",
        ".pyo", ".class", ".jar", ".war", ".ear", ".node", ".wasm", ".rlib", ".sqlite", ".sqlite3",
        ".db", ".mdb", ".idx", ".psd", ".ai", ".eps", ".sketch", ".fig", ".xd", ".blend", ".3ds",
        ".max", ".swf", ".fla", ".lockb", ".dat", ".data",
    ]
    .into_iter()
    .collect()
});

pub fn has_binary_extension(file_path: &str) -> bool {
    if let Some(pos) = file_path.rfind('.') {
        let ext = &file_path[pos..].to_lowercase();
        BINARY_EXTENSIONS.contains(ext.as_str())
    } else {
        false
    }
}

const BINARY_CHECK_SIZE: usize = 8192;

pub fn is_binary_content(buffer: &[u8]) -> bool {
    let check_size = buffer.len().min(BINARY_CHECK_SIZE);
    let mut non_printable = 0;

    for &byte in &buffer[..check_size] {
        if byte == 0 {
            return true;
        }
        if byte < 32 && byte != 9 && byte != 10 && byte != 13 {
            non_printable += 1;
        }
    }

    non_printable as f64 / check_size as f64 > 0.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_binary_extension() {
        assert!(has_binary_extension("file.png"));
        assert!(has_binary_extension("path/to/file.PDF"));
        assert!(!has_binary_extension("file.txt"));
        assert!(!has_binary_extension("file"));
    }

    #[test]
    fn test_is_binary_content() {
        assert!(is_binary_content(&[0, 1, 2, 3]));
        assert!(!is_binary_content(b"Hello, world!"));
        let mixed: Vec<u8> = (0..100).map(|i| if i % 10 == 0 { 1 } else { 65 }).collect();
        assert!(!is_binary_content(&mixed));
    }
}
