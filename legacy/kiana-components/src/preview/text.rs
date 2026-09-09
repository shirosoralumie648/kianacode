// Text file previewer with syntax highlighting

use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::text::Span;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style as SyntectStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// Text file previewer with optional syntax highlighting
pub struct TextPreviewer {
    /// Maximum number of lines to read
    pub max_lines: usize,
    /// Maximum bytes to read from file
    pub max_bytes: usize,
    /// Syntax highlighting sets (lazy loaded)
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Default for TextPreviewer {
    fn default() -> Self {
        Self {
            max_lines: 1000,
            max_bytes: 1024 * 1024, // 1MB
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }
}

impl TextPreviewer {
    /// Create a new text previewer with custom limits
    pub fn new(max_lines: usize, max_bytes: usize) -> Self {
        Self {
            max_lines,
            max_bytes,
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    /// Preview a text file with optional syntax highlighting
    pub fn preview(&self, path: &Path, language: Option<&str>) -> io::Result<Vec<Line<'static>>> {
        // Check file size first
        let metadata = std::fs::metadata(path)?;
        if metadata.len() as usize > self.max_bytes {
            return self.preview_large_file(path, language);
        }

        // Read entire file for syntax highlighting
        let content = std::fs::read_to_string(path)?;

        if let Some(lang) = language {
            self.preview_with_highlighting(&content, lang)
        } else {
            Ok(self.preview_plain(&content))
        }
    }

    /// Preview file with syntax highlighting
    fn preview_with_highlighting(
        &self,
        content: &str,
        language: &str,
    ) -> io::Result<Vec<Line<'static>>> {
        let syntax = self
            .syntax_set
            .find_syntax_by_token(language)
            .or_else(|| self.syntax_set.find_syntax_by_extension(language))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        let mut lines = Vec::new();
        let mut line_count = 0;

        for line in LinesWithEndings::from(content) {
            if line_count >= self.max_lines {
                lines.push(Line::from(Span::styled(
                    format!(
                        "... ({} more lines truncated)",
                        content.lines().count() - line_count
                    ),
                    Style::default().fg(Color::Yellow),
                )));
                break;
            }

            let ranges: Vec<(SyntectStyle, &str)> = highlighter
                .highlight_line(line, &self.syntax_set)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

            let spans: Vec<Span> = ranges
                .into_iter()
                .map(|(style, text)| {
                    let fg = syntect_to_ratatui_color(style.foreground);
                    Span::styled(text.to_string(), Style::default().fg(fg))
                })
                .collect();

            lines.push(Line::from(spans));
            line_count += 1;
        }

        Ok(lines)
    }

    /// Preview file as plain text (no highlighting)
    fn preview_plain(&self, content: &str) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let mut line_count = 0;

        for line in content.lines() {
            if line_count >= self.max_lines {
                lines.push(Line::from(Span::styled(
                    format!(
                        "... ({} more lines truncated)",
                        content.lines().count() - line_count
                    ),
                    Style::default().fg(Color::Yellow),
                )));
                break;
            }

            lines.push(Line::from(line.to_string()));
            line_count += 1;
        }

        // Handle case where file doesn't end with newline
        if lines.is_empty() {
            lines.push(Line::from(""));
        }

        lines
    }

    /// Preview large file (only read first N lines)
    fn preview_large_file(
        &self,
        path: &Path,
        _language: Option<&str>,
    ) -> io::Result<Vec<Line<'static>>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut lines = Vec::new();

        // Add warning header
        lines.push(Line::from(Span::styled(
            format!("Large file (syntax highlighting disabled)"),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::from(""));

        let mut line_count = 0;
        for line in reader.lines() {
            if line_count >= self.max_lines {
                lines.push(Line::from(Span::styled(
                    format!("... (truncated, showing first {} lines)", self.max_lines),
                    Style::default().fg(Color::Yellow),
                )));
                break;
            }

            let line_content = line?;
            lines.push(Line::from(line_content));
            line_count += 1;
        }

        Ok(lines)
    }

    /// Add line numbers to preview lines
    pub fn add_line_numbers(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
        let num_width = (lines.len()).to_string().len();

        lines
            .into_iter()
            .enumerate()
            .map(|(i, line)| {
                let line_num = format!("{:>width$} │ ", i + 1, width = num_width);
                let mut spans = vec![Span::styled(line_num, Style::default().fg(Color::DarkGray))];
                spans.extend(line.spans.into_iter());
                Line::from(spans)
            })
            .collect()
    }
}

/// Convert syntect color to ratatui color
fn syntect_to_ratatui_color(color: syntect::highlighting::Color) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_preview_plain_text() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"Line 1\nLine 2\nLine 3")?;

        let previewer = TextPreviewer::default();
        let lines = previewer.preview(temp_file.path(), None)?;

        assert_eq!(lines.len(), 3);
        Ok(())
    }

    #[test]
    fn test_preview_with_language() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"fn main() {\n    println!(\"Hello\");\n}")?;

        let previewer = TextPreviewer::default();
        let lines = previewer.preview(temp_file.path(), Some("rust"))?;

        assert_eq!(lines.len(), 3);
        Ok(())
    }

    #[test]
    fn test_preview_respects_max_lines() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        let content = (0..100)
            .map(|i| format!("Line {}\n", i))
            .collect::<String>();
        temp_file.write_all(content.as_bytes())?;

        let previewer = TextPreviewer::new(10, 1024 * 1024);
        let lines = previewer.preview(temp_file.path(), None)?;

        // Should have 10 lines + 1 truncation message
        assert!(lines.len() <= 11);
        Ok(())
    }

    #[test]
    fn test_add_line_numbers() {
        let lines = vec![
            Line::from("First line"),
            Line::from("Second line"),
            Line::from("Third line"),
        ];

        let numbered = TextPreviewer::add_line_numbers(lines);

        assert_eq!(numbered.len(), 3);
        // Check that first line starts with "1 │"
        let first_text: String = numbered[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(first_text.contains("1 │"));
    }

    #[test]
    fn test_preview_large_file() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        // Create a file larger than max_bytes
        let large_content = vec![b'x'; 2 * 1024 * 1024]; // 2MB
        temp_file.write_all(&large_content)?;

        let previewer = TextPreviewer::new(100, 1024 * 1024); // 1MB limit
        let lines = previewer.preview(temp_file.path(), None)?;

        // Should have warning header
        let first_line_text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(first_line_text.contains("Large file"));

        Ok(())
    }

    #[test]
    fn test_empty_file() -> io::Result<()> {
        let temp_file = NamedTempFile::new()?;

        let previewer = TextPreviewer::default();
        let lines = previewer.preview(temp_file.path(), None)?;

        // Empty file should return at least one empty line
        assert!(!lines.is_empty());
        Ok(())
    }
}
