// Markdown file previewer

use ratatui::text::Line;
use std::io;
use std::path::Path;

/// Markdown file previewer (wrapper for consistency)
pub struct MarkdownPreviewer {
    /// Maximum number of lines to read
    pub max_lines: usize,
}

impl Default for MarkdownPreviewer {
    fn default() -> Self {
        Self { max_lines: 1000 }
    }
}

impl MarkdownPreviewer {
    /// Create a new markdown previewer
    pub fn new(max_lines: usize) -> Self {
        Self { max_lines }
    }

    /// Preview a markdown file with rendering
    pub fn preview(&self, path: &Path) -> io::Result<Vec<Line<'static>>> {
        let content = std::fs::read_to_string(path)?;
        // Convert borrowed lines to owned lines
        let lines = self.render_markdown(&content);
        Ok(lines
            .into_iter()
            .map(|line| {
                let spans: Vec<_> = line
                    .spans
                    .into_iter()
                    .map(|span| ratatui::text::Span::styled(span.content.to_string(), span.style))
                    .collect();
                Line::from(spans)
            })
            .collect())
    }

    /// Render markdown content to styled lines
    /// For now, this is a simplified version. Full markdown rendering
    /// would integrate with the existing markdown renderer from Feature 20.
    fn render_markdown(&self, content: &str) -> Vec<Line<'static>> {
        use ratatui::style::{Color, Modifier, Style};
        use ratatui::text::Span;

        let mut lines = Vec::new();
        let mut line_count = 0;

        for line in content.lines() {
            if line_count >= self.max_lines {
                lines.push(Line::from(Span::styled(
                    format!("... ({} more lines)", content.lines().count() - line_count),
                    Style::default().fg(Color::Yellow),
                )));
                break;
            }

            // Simple markdown rendering
            let rendered_line = if line.starts_with("# ") {
                // H1
                Line::from(Span::styled(
                    line[2..].to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::UNDERLINED),
                ))
            } else if line.starts_with("## ") {
                // H2
                Line::from(Span::styled(
                    line[3..].to_string(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("### ") {
                // H3
                Line::from(Span::styled(
                    line[4..].to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("#### ")
                || line.starts_with("##### ")
                || line.starts_with("###### ")
            {
                // H4-H6
                let level = line.chars().take_while(|&c| c == '#').count();
                Line::from(Span::styled(
                    line[level + 1..].to_string(),
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                ))
            } else if line.starts_with("```") {
                // Code fence
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Green),
                ))
            } else if line.starts_with(">") {
                // Blockquote
                Line::from(Span::styled(
                    line.to_string(),
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::ITALIC),
                ))
            } else if line.starts_with("- ") || line.starts_with("* ") || line.starts_with("+ ") {
                // Unordered list
                Line::from(vec![
                    Span::styled("• ".to_string(), Style::default().fg(Color::Blue)),
                    Span::raw(line[2..].to_string()),
                ])
            } else if line.len() > 2
                && line.chars().nth(0).unwrap().is_numeric()
                && line.starts_with(|c: char| c.is_numeric())
                && line.contains(". ")
            {
                // Ordered list (simple detection)
                if let Some(pos) = line.find(". ") {
                    Line::from(vec![
                        Span::styled(
                            line[..pos + 2].to_string(),
                            Style::default().fg(Color::Blue),
                        ),
                        Span::raw(line[pos + 2..].to_string()),
                    ])
                } else {
                    Line::from(line.to_string())
                }
            } else {
                // Regular text
                Line::from(line.to_string())
            };

            lines.push(rendered_line);
            line_count += 1;
        }

        if lines.is_empty() {
            lines.push(Line::from("".to_string()));
        }

        lines
    }

    /// Preview markdown as raw text (no rendering)
    pub fn preview_raw(&self, path: &Path) -> io::Result<Vec<Line<'static>>> {
        let content = std::fs::read_to_string(path)?;
        let mut lines = Vec::new();
        let mut line_count = 0;

        for line in content.lines() {
            if line_count >= self.max_lines {
                break;
            }
            lines.push(Line::from(line.to_string()));
            line_count += 1;
        }

        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_preview_markdown() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"# Title\n\nSome content\n\n## Section")?;

        let previewer = MarkdownPreviewer::default();
        let lines = previewer.preview(temp_file.path())?;

        assert_eq!(lines.len(), 5);
        Ok(())
    }

    #[test]
    fn test_render_headers() {
        let previewer = MarkdownPreviewer::default();
        let content = "# H1\n## H2\n### H3\n#### H4";
        let lines = previewer.render_markdown(content);

        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn test_render_list() {
        let previewer = MarkdownPreviewer::default();
        let content = "- Item 1\n- Item 2\n* Item 3";
        let lines = previewer.render_markdown(content);

        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_render_blockquote() {
        let previewer = MarkdownPreviewer::default();
        let content = "> This is a quote\n> Second line";
        let lines = previewer.render_markdown(content);

        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_preview_raw() -> io::Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"# Title\n**bold**\n*italic*")?;

        let previewer = MarkdownPreviewer::default();
        let lines = previewer.preview_raw(temp_file.path())?;

        // Raw should not render markdown
        assert_eq!(lines.len(), 3);
        Ok(())
    }

    #[test]
    fn test_respects_max_lines() {
        let previewer = MarkdownPreviewer::new(5);
        let content = (0..20)
            .map(|i| format!("Line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let lines = previewer.render_markdown(&content);

        // Should truncate at 5 lines + truncation message
        assert!(lines.len() <= 6);
    }
}
