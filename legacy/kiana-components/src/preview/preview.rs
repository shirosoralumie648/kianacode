// Main preview pane component

use super::{BinaryPreviewer, FileType, FileTypeDetector, MarkdownPreviewer, TextPreviewer};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use std::io;
use std::path::{Path, PathBuf};

/// Wrap mode for text display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    /// Wrap long lines
    Wrap,
    /// Truncate long lines
    Truncate,
}

/// Content of the preview pane
#[derive(Debug, Clone)]
pub enum PreviewContent {
    /// Text content with styled lines
    Text(Vec<Line<'static>>),
    /// Binary content (already formatted as hex dump)
    Binary(Vec<Line<'static>>),
    /// Markdown content (rendered)
    Markdown(Vec<Line<'static>>),
    /// Error message
    Error(String),
    /// Empty/no file loaded
    Empty,
}

/// Preview pane component for displaying file contents
pub struct PreviewPane {
    /// Current file path
    file_path: Option<PathBuf>,
    /// Preview content
    content: PreviewContent,
    /// Vertical scroll offset
    scroll_offset: usize,
    /// Show line numbers
    show_line_numbers: bool,
    /// Wrap mode
    wrap_mode: WrapMode,
    /// Show raw markdown (vs rendered)
    show_raw_markdown: bool,
    /// Previewers
    text_previewer: TextPreviewer,
    binary_previewer: BinaryPreviewer,
    markdown_previewer: MarkdownPreviewer,
}

impl Default for PreviewPane {
    fn default() -> Self {
        Self {
            file_path: None,
            content: PreviewContent::Empty,
            scroll_offset: 0,
            show_line_numbers: false,
            wrap_mode: WrapMode::Truncate,
            show_raw_markdown: false,
            text_previewer: TextPreviewer::default(),
            binary_previewer: BinaryPreviewer::default(),
            markdown_previewer: MarkdownPreviewer::default(),
        }
    }
}

impl PreviewPane {
    /// Create a new preview pane
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a file for preview
    pub fn load_file(&mut self, path: &Path) -> io::Result<()> {
        self.file_path = Some(path.to_path_buf());
        self.scroll_offset = 0;

        // Detect file type
        let file_type = FileTypeDetector::detect(path);

        // Load content based on type
        self.content = match file_type {
            FileType::Text { language } => {
                match self.text_previewer.preview(path, language.as_deref()) {
                    Ok(mut lines) => {
                        if self.show_line_numbers {
                            lines = TextPreviewer::add_line_numbers(lines);
                        }
                        PreviewContent::Text(lines)
                    }
                    Err(e) => PreviewContent::Error(format!("Failed to load text: {}", e)),
                }
            }
            FileType::Markdown => {
                let result = if self.show_raw_markdown {
                    self.markdown_previewer.preview_raw(path)
                } else {
                    self.markdown_previewer.preview(path)
                };
                match result {
                    Ok(lines) => PreviewContent::Markdown(lines),
                    Err(e) => PreviewContent::Error(format!("Failed to load markdown: {}", e)),
                }
            }
            FileType::Binary => match self.binary_previewer.preview(path) {
                Ok(lines) => PreviewContent::Binary(lines),
                Err(e) => PreviewContent::Error(format!("Failed to load binary: {}", e)),
            },
            FileType::Image => PreviewContent::Error(
                "Image preview not yet supported (ASCII art coming soon)".to_string(),
            ),
            FileType::Archive => {
                PreviewContent::Error("Archive preview not yet supported".to_string())
            }
            FileType::Unknown => {
                // Try to detect from content
                match std::fs::read(path) {
                    Ok(bytes) => {
                        let detected = FileTypeDetector::detect_from_content(&bytes);
                        match detected {
                            FileType::Text { .. } => {
                                match self.text_previewer.preview(path, None) {
                                    Ok(mut lines) => {
                                        if self.show_line_numbers {
                                            lines = TextPreviewer::add_line_numbers(lines);
                                        }
                                        PreviewContent::Text(lines)
                                    }
                                    Err(e) => {
                                        PreviewContent::Error(format!("Failed to load: {}", e))
                                    }
                                }
                            }
                            _ => match self.binary_previewer.preview(path) {
                                Ok(lines) => PreviewContent::Binary(lines),
                                Err(e) => PreviewContent::Error(format!("Failed to load: {}", e)),
                            },
                        }
                    }
                    Err(e) => PreviewContent::Error(format!("Failed to read file: {}", e)),
                }
            }
        };

        Ok(())
    }

    /// Clear the preview
    pub fn clear(&mut self) {
        self.file_path = None;
        self.content = PreviewContent::Empty;
        self.scroll_offset = 0;
    }

    /// Scroll up by n lines
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    /// Scroll down by n lines
    pub fn scroll_down(&mut self, n: usize) {
        let max_offset = self.max_scroll_offset();
        self.scroll_offset = (self.scroll_offset + n).min(max_offset);
    }

    /// Jump to top
    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
    }

    /// Jump to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll_offset();
    }

    /// Toggle line numbers
    pub fn toggle_line_numbers(&mut self) {
        self.show_line_numbers = !self.show_line_numbers;
        // Reload file to apply line numbers
        if let Some(path) = self.file_path.clone() {
            let _ = self.load_file(&path);
        }
    }

    /// Toggle wrap mode
    pub fn toggle_wrap_mode(&mut self) {
        self.wrap_mode = match self.wrap_mode {
            WrapMode::Wrap => WrapMode::Truncate,
            WrapMode::Truncate => WrapMode::Wrap,
        };
    }

    /// Toggle raw markdown mode
    pub fn toggle_raw_markdown(&mut self) {
        self.show_raw_markdown = !self.show_raw_markdown;
        // Reload if currently viewing markdown
        if let Some(path) = self.file_path.clone() {
            if matches!(self.content, PreviewContent::Markdown(_)) {
                let _ = self.load_file(&path);
            }
        }
    }

    /// Get maximum scroll offset
    fn max_scroll_offset(&self) -> usize {
        let total_lines = match &self.content {
            PreviewContent::Text(lines)
            | PreviewContent::Binary(lines)
            | PreviewContent::Markdown(lines) => lines.len(),
            _ => 0,
        };
        total_lines.saturating_sub(1)
    }

    /// Get current file path
    pub fn file_path(&self) -> Option<&Path> {
        self.file_path.as_deref()
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Render the preview pane
    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let title = if let Some(path) = &self.file_path {
            format!(" {} ", path.display())
        } else {
            " Preview ".to_string()
        };

        let block = Block::default().borders(Borders::ALL).title(Span::styled(
            title,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

        let inner = block.inner(area);
        block.render(area, buf);

        // Render content
        match &self.content {
            PreviewContent::Text(lines)
            | PreviewContent::Binary(lines)
            | PreviewContent::Markdown(lines) => {
                let visible_lines: Vec<Line> = lines
                    .iter()
                    .skip(self.scroll_offset)
                    .take(inner.height as usize)
                    .cloned()
                    .collect();

                let paragraph = Paragraph::new(visible_lines);
                paragraph.render(inner, buf);
            }
            PreviewContent::Error(msg) => {
                let error_line = Line::from(vec![
                    Span::styled("Error: ", Style::default().fg(Color::Red)),
                    Span::raw(msg.clone()),
                ]);
                let paragraph = Paragraph::new(vec![error_line]);
                paragraph.render(inner, buf);
            }
            PreviewContent::Empty => {
                let empty_line = Line::from(Span::styled(
                    "No file selected",
                    Style::default().fg(Color::DarkGray),
                ));
                let paragraph = Paragraph::new(vec![empty_line]);
                paragraph.render(inner, buf);
            }
        }

        // Render scroll indicator
        self.render_scroll_indicator(area, buf);
    }

    /// Render scroll indicator in the bottom-right corner
    fn render_scroll_indicator(&self, area: Rect, buf: &mut Buffer) {
        let total_lines = match &self.content {
            PreviewContent::Text(lines)
            | PreviewContent::Binary(lines)
            | PreviewContent::Markdown(lines) => lines.len(),
            _ => 0,
        };

        if total_lines == 0 {
            return;
        }

        let visible_height = area.height.saturating_sub(2) as usize; // Account for borders
        if total_lines <= visible_height {
            return; // No scrolling needed
        }

        let indicator = format!(
            " {}/{} ",
            self.scroll_offset.min(total_lines.saturating_sub(1)) + 1,
            total_lines
        );

        let x = area.right().saturating_sub(indicator.len() as u16 + 1);
        let y = area.bottom().saturating_sub(1);

        for (i, ch) in indicator.chars().enumerate() {
            if let Some(cell) = buf.cell_mut((x + i as u16, y)) {
                cell.set_char(ch)
                    .set_style(Style::default().fg(Color::DarkGray));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_new_preview_pane() {
        let pane = PreviewPane::new();
        assert!(matches!(pane.content, PreviewContent::Empty));
        assert_eq!(pane.scroll_offset, 0);
    }

    #[test]
    fn test_load_text_file() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"Hello\nWorld")?;

        let mut pane = PreviewPane::new();
        pane.load_file(temp_file.path())?;

        assert!(matches!(pane.content, PreviewContent::Text(_)));
        Ok(())
    }

    #[test]
    fn test_scroll_operations() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        let content = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        temp_file.write_all(content.as_bytes())?;

        let mut pane = PreviewPane::new();
        pane.load_file(temp_file.path())?;

        pane.scroll_down(5);
        assert_eq!(pane.scroll_offset, 5);

        pane.scroll_up(3);
        assert_eq!(pane.scroll_offset, 2);

        pane.scroll_to_top();
        assert_eq!(pane.scroll_offset, 0);

        pane.scroll_to_bottom();
        assert!(pane.scroll_offset > 0);

        Ok(())
    }

    #[test]
    fn test_toggle_line_numbers() {
        let mut pane = PreviewPane::new();
        assert!(!pane.show_line_numbers);

        pane.toggle_line_numbers();
        assert!(pane.show_line_numbers);

        pane.toggle_line_numbers();
        assert!(!pane.show_line_numbers);
    }

    #[test]
    fn test_toggle_wrap_mode() {
        let mut pane = PreviewPane::new();
        assert_eq!(pane.wrap_mode, WrapMode::Truncate);

        pane.toggle_wrap_mode();
        assert_eq!(pane.wrap_mode, WrapMode::Wrap);

        pane.toggle_wrap_mode();
        assert_eq!(pane.wrap_mode, WrapMode::Truncate);
    }

    #[test]
    fn test_clear() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"content")?;

        let mut pane = PreviewPane::new();
        pane.load_file(temp_file.path())?;
        assert!(pane.file_path.is_some());

        pane.clear();
        assert!(pane.file_path.is_none());
        assert!(matches!(pane.content, PreviewContent::Empty));

        Ok(())
    }

    #[test]
    fn test_load_binary_file() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(&[0xFF, 0xD8, 0xFF, 0xE0])?; // JPEG header

        let mut pane = PreviewPane::new();
        pane.load_file(temp_file.path())?;

        // Should detect as image/binary
        assert!(matches!(
            pane.content,
            PreviewContent::Binary(_) | PreviewContent::Error(_)
        ));
        Ok(())
    }
}
