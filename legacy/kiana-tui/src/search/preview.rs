// Search preview component with live context display
// Implements Phase 5 Feature 31: Search Preview
// Reference: Telescope.nvim preview functionality

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Configuration for search preview
#[derive(Debug, Clone)]
pub struct PreviewConfig {
    /// Number of context lines to show before match
    pub context_before: usize,
    /// Number of context lines to show after match
    pub context_after: usize,
    /// Maximum preview height
    pub max_preview_height: usize,
    /// Enable syntax highlighting
    pub enable_highlighting: bool,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            context_before: 3,
            context_after: 3,
            max_preview_height: 20,
            enable_highlighting: true,
        }
    }
}

/// A match within a line
#[derive(Debug, Clone)]
pub struct LineMatch {
    /// Line number (0-indexed)
    pub line_number: usize,
    /// Line content
    pub content: String,
    /// Character positions of matches in this line
    pub match_positions: Vec<usize>,
}

/// Preview content for a search result
#[derive(Debug, Clone)]
pub struct SearchPreview {
    /// Lines to display (with context)
    pub lines: Vec<PreviewLine>,
    /// Index of the primary match line in the lines vector
    pub match_line_index: usize,
}

/// A line in the preview
#[derive(Debug, Clone)]
pub struct PreviewLine {
    /// Line number in original content
    pub line_number: usize,
    /// Line content
    pub content: String,
    /// Match positions (if this is a match line)
    pub match_positions: Option<Vec<usize>>,
    /// Whether this is the primary match line
    pub is_primary: bool,
}

/// Search preview renderer
pub struct SearchPreviewRenderer {
    config: PreviewConfig,
}

impl SearchPreviewRenderer {
    /// Create a new search preview renderer
    pub fn new(config: PreviewConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn default_config() -> Self {
        Self {
            config: PreviewConfig::default(),
        }
    }

    /// Generate preview for a line match
    pub fn generate_preview(&self, content: &str, line_match: &LineMatch) -> SearchPreview {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let match_line = line_match.line_number;

        // Calculate range
        let start_line = match_line.saturating_sub(self.config.context_before);
        let end_line = (match_line + self.config.context_after + 1).min(total_lines);

        // Build preview lines
        let mut preview_lines = Vec::new();
        let mut match_index = 0;

        for (idx, line_num) in (start_line..end_line).enumerate() {
            if line_num >= total_lines {
                break;
            }

            let is_match = line_num == match_line;
            if is_match {
                match_index = idx;
            }

            preview_lines.push(PreviewLine {
                line_number: line_num,
                content: lines[line_num].to_string(),
                match_positions: if is_match {
                    Some(line_match.match_positions.clone())
                } else {
                    None
                },
                is_primary: is_match,
            });
        }

        SearchPreview {
            lines: preview_lines,
            match_line_index: match_index,
        }
    }

    /// Render preview in a frame area
    pub fn render(&self, frame: &mut Frame, area: Rect, preview: &SearchPreview) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Preview ")
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Render preview lines
        let display_lines: Vec<Line> = preview
            .lines
            .iter()
            .map(|pline| self.render_preview_line(pline))
            .collect();

        let paragraph = Paragraph::new(display_lines);
        frame.render_widget(paragraph, inner);
    }

    /// Render a single preview line with highlighting
    fn render_preview_line(&self, line: &PreviewLine) -> Line<'static> {
        let line_num_str = format!("{:>4} │ ", line.line_number + 1);

        let mut spans = vec![Span::styled(
            line_num_str,
            if line.is_primary {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        )];

        // Highlight matches if present
        if let Some(positions) = &line.match_positions {
            spans.extend(self.highlight_matches(&line.content, positions));
        } else {
            // Context line - style differently
            spans.push(Span::styled(
                line.content.clone(),
                if line.is_primary {
                    Style::default()
                } else {
                    Style::default().fg(Color::Gray)
                },
            ));
        }

        Line::from(spans)
    }

    /// Highlight match positions in a line
    fn highlight_matches(&self, text: &str, positions: &[usize]) -> Vec<Span<'static>> {
        if positions.is_empty() {
            return vec![Span::raw(text.to_string())];
        }

        let mut spans = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut last_pos = 0;

        for &pos in positions {
            if pos >= chars.len() {
                continue;
            }

            // Add text before match
            if pos > last_pos {
                let normal_text: String = chars[last_pos..pos].iter().collect();
                spans.push(Span::raw(normal_text));
            }

            // Add highlighted match character
            let match_char: String = chars[pos..pos + 1].iter().collect();
            spans.push(Span::styled(
                match_char,
                Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ));

            last_pos = pos + 1;
        }

        // Add remaining text
        if last_pos < chars.len() {
            let remaining: String = chars[last_pos..].iter().collect();
            spans.push(Span::raw(remaining));
        }

        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preview_config_default() {
        let config = PreviewConfig::default();
        assert_eq!(config.context_before, 3);
        assert_eq!(config.context_after, 3);
    }

    #[test]
    fn test_generate_preview_basic() {
        let renderer = SearchPreviewRenderer::default_config();
        let content = "line 0\nline 1\nline 2\nline 3\nline 4\nline 5\nline 6";

        let line_match = LineMatch {
            line_number: 3,
            content: "line 3".to_string(),
            match_positions: vec![0, 1, 2, 3],
        };

        let preview = renderer.generate_preview(content, &line_match);

        // Should have context_before (3) + match (1) + context_after (3) = 7 lines
        assert_eq!(preview.lines.len(), 7);
        assert_eq!(preview.match_line_index, 3);
        assert!(preview.lines[3].is_primary);
    }

    #[test]
    fn test_generate_preview_at_start() {
        let renderer = SearchPreviewRenderer::default_config();
        let content = "line 0\nline 1\nline 2";

        let line_match = LineMatch {
            line_number: 0,
            content: "line 0".to_string(),
            match_positions: vec![0],
        };

        let preview = renderer.generate_preview(content, &line_match);

        // Should start at line 0 (no lines before)
        assert_eq!(preview.lines[0].line_number, 0);
        assert_eq!(preview.match_line_index, 0);
    }

    #[test]
    fn test_generate_preview_at_end() {
        let renderer = SearchPreviewRenderer::default_config();
        let content = "line 0\nline 1\nline 2";

        let line_match = LineMatch {
            line_number: 2,
            content: "line 2".to_string(),
            match_positions: vec![0],
        };

        let preview = renderer.generate_preview(content, &line_match);

        // Should include context before but stop at end
        assert!(preview.lines.last().unwrap().line_number <= 2);
    }

    #[test]
    fn test_highlight_matches() {
        let renderer = SearchPreviewRenderer::default_config();
        let text = "hello world";
        let positions = vec![0, 6]; // 'h' and 'w'

        let spans = renderer.highlight_matches(text, &positions);

        // Should have: "h" (highlighted) + "ello " + "w" (highlighted) + "orld"
        assert!(spans.len() >= 3);
    }

    #[test]
    fn test_highlight_matches_empty() {
        let renderer = SearchPreviewRenderer::default_config();
        let text = "hello world";
        let positions = vec![];

        let spans = renderer.highlight_matches(text, &positions);

        // Should return single span with full text
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_preview_line_with_matches() {
        let line = PreviewLine {
            line_number: 5,
            content: "test line".to_string(),
            match_positions: Some(vec![0, 1]),
            is_primary: true,
        };

        let renderer = SearchPreviewRenderer::default_config();
        let rendered = renderer.render_preview_line(&line);

        // Should have line number and content
        assert!(!rendered.spans.is_empty());
    }

    #[test]
    fn test_preview_line_context() {
        let line = PreviewLine {
            line_number: 3,
            content: "context line".to_string(),
            match_positions: None,
            is_primary: false,
        };

        let renderer = SearchPreviewRenderer::default_config();
        let rendered = renderer.render_preview_line(&line);

        // Should render as context (no highlighting)
        assert!(!rendered.spans.is_empty());
    }
}
