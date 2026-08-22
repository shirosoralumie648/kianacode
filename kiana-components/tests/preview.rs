// Integration tests for preview system

use kiana_components::preview::*;
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_file_type_detection_rust() {
    let path = std::path::Path::new("test.rs");
    let file_type = FileTypeDetector::detect(path);
    assert!(matches!(file_type, FileType::Text { .. }));
}

#[test]
fn test_file_type_detection_markdown() {
    let path = std::path::Path::new("README.md");
    let file_type = FileTypeDetector::detect(path);
    assert_eq!(file_type, FileType::Markdown);
}

#[test]
fn test_file_type_detection_binary() {
    let path = std::path::Path::new("image.png");
    let file_type = FileTypeDetector::detect(path);
    assert_eq!(file_type, FileType::Image);
}

#[test]
fn test_text_preview_simple() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(b"Line 1\nLine 2\nLine 3\n")?;

    let previewer = TextPreviewer::default();
    let lines = previewer.preview(temp_file.path(), None)?;

    assert_eq!(lines.len(), 3);
    Ok(())
}

#[test]
fn test_text_preview_with_syntax_highlighting() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(b"fn main() {\n    println!(\"Hello, world!\");\n}\n")?;

    let previewer = TextPreviewer::default();
    let lines = previewer.preview(temp_file.path(), Some("rust"))?;

    assert_eq!(lines.len(), 3);
    Ok(())
}

#[test]
fn test_binary_preview() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(&[0x00, 0x01, 0x02, 0x03, 0xFF, 0xFE, 0xFD, 0xFC])?;

    let previewer = BinaryPreviewer::default();
    let lines = previewer.preview(temp_file.path())?;

    assert!(!lines.is_empty());
    // Should have header + blank line + at least one hex line
    assert!(lines.len() >= 3);
    Ok(())
}

#[test]
fn test_markdown_preview() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(b"# Title\n\nParagraph\n\n## Section\n")?;

    let previewer = MarkdownPreviewer::default();
    let lines = previewer.preview(temp_file.path())?;

    assert_eq!(lines.len(), 5);
    Ok(())
}

#[test]
fn test_preview_pane_load_text() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(b"Test content\n")?;

    let mut pane = PreviewPane::new();
    pane.load_file(temp_file.path())?;

    assert!(pane.file_path().is_some());
    Ok(())
}

#[test]
fn test_preview_pane_scrolling() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    let content = (0..50).map(|i| format!("Line {}\n", i)).collect::<String>();
    temp_file.write_all(content.as_bytes())?;

    let mut pane = PreviewPane::new();
    pane.load_file(temp_file.path())?;

    // Test scrolling
    assert_eq!(pane.scroll_offset(), 0);

    pane.scroll_down(10);
    assert_eq!(pane.scroll_offset(), 10);

    pane.scroll_up(5);
    assert_eq!(pane.scroll_offset(), 5);

    pane.scroll_to_top();
    assert_eq!(pane.scroll_offset(), 0);

    pane.scroll_to_bottom();
    assert!(pane.scroll_offset() > 0);

    Ok(())
}

#[test]
fn test_preview_pane_toggle_features() -> std::io::Result<()> {
    let mut pane = PreviewPane::new();

    // Test toggle line numbers
    pane.toggle_line_numbers();
    // Note: actual line number display is tested in text previewer tests

    // Test toggle wrap mode
    pane.toggle_wrap_mode();
    pane.toggle_wrap_mode();

    // Test toggle raw markdown
    pane.toggle_raw_markdown();
    pane.toggle_raw_markdown();

    Ok(())
}

#[test]
fn test_preview_pane_clear() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(b"content")?;

    let mut pane = PreviewPane::new();
    pane.load_file(temp_file.path())?;

    assert!(pane.file_path().is_some());

    pane.clear();
    assert!(pane.file_path().is_none());

    Ok(())
}

#[test]
fn test_large_file_handling() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    // Create a large file (2MB of text)
    let large_content = "x".repeat(2 * 1024 * 1024);
    temp_file.write_all(large_content.as_bytes())?;

    let previewer = TextPreviewer::new(100, 1024 * 1024); // 1MB limit
    let lines = previewer.preview(temp_file.path(), None)?;

    // Should show warning about large file
    assert!(!lines.is_empty());
    Ok(())
}

#[test]
fn test_empty_file_handling() -> std::io::Result<()> {
    let temp_file = NamedTempFile::new()?;

    let mut pane = PreviewPane::new();
    pane.load_file(temp_file.path())?;

    // Should handle empty file gracefully
    assert!(pane.file_path().is_some());
    Ok(())
}

#[test]
fn test_content_type_detection() {
    // Test text content
    let text = b"Hello world\nSecond line\n";
    let file_type = FileTypeDetector::detect_from_content(text);
    assert!(matches!(file_type, FileType::Text { .. }));

    // Test PNG header
    let png = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let file_type = FileTypeDetector::detect_from_content(png);
    assert_eq!(file_type, FileType::Image);

    // Test JPEG header
    let jpeg = &[0xFF, 0xD8, 0xFF, 0xE0];
    let file_type = FileTypeDetector::detect_from_content(jpeg);
    assert_eq!(file_type, FileType::Image);
}

#[test]
fn test_line_numbers() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    temp_file.write_all(b"Line 1\nLine 2\nLine 3\n")?;

    let previewer = TextPreviewer::default();
    let lines = previewer.preview(temp_file.path(), None)?;
    let numbered = TextPreviewer::add_line_numbers(lines);

    assert_eq!(numbered.len(), 3);

    // Check first line has line number
    let first_text: String = numbered[0]
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(first_text.contains("1 │"));

    Ok(())
}

#[test]
fn test_multiple_language_support() -> std::io::Result<()> {
    let languages = vec![
        ("rust", "fn main() {}"),
        ("python", "def main():\n    pass"),
        ("javascript", "function main() {}"),
        ("json", r#"{"key": "value"}"#),
    ];

    for (lang, code) in languages {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(code.as_bytes())?;

        let previewer = TextPreviewer::default();
        let result = previewer.preview(temp_file.path(), Some(lang));
        assert!(result.is_ok(), "Failed to preview {} code", lang);
    }

    Ok(())
}
