// Enhanced search overlay with split preview pane
// Implements Phase 5 Feature 31: Search Preview
// Reference: Telescope.nvim

use crate::overlay::{Overlay, OverlayAction};
use crate::search::{calculate_match_score, LineMatch, SearchPreview, SearchPreviewRenderer};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

/// Search result with preview capability
#[derive(Debug, Clone)]
pub struct SearchResultWithPreview {
    /// Display text for the result
    pub text: String,
    /// Match score (higher is better)
    pub score: usize,
    /// Match positions in the text
    pub positions: Vec<usize>,
    /// Original index in source data
    pub index: usize,
    /// Full content for preview (if available)
    pub full_content: Option<String>,
}

impl SearchResultWithPreview {
    /// Create a new search result
    pub fn new(
        text: String,
        score: usize,
        positions: Vec<usize>,
        index: usize,
        full_content: Option<String>,
    ) -> Self {
        Self {
            text,
            score,
            positions,
            index,
            full_content,
        }
    }

    /// Generate line match for preview
    fn to_line_match(&self) -> LineMatch {
        LineMatch {
            line_number: 0, // Will be updated based on actual content
            content: self.text.clone(),
            match_positions: self.positions.clone(),
        }
    }
}

/// Search overlay with live preview
pub struct SearchWithPreviewOverlay {
    /// Current query string
    query: String,
    /// Search results
    results: Vec<SearchResultWithPreview>,
    /// Currently selected result index
    selected_index: usize,
    /// Preview renderer
    preview_renderer: SearchPreviewRenderer,
    /// Show preview pane
    show_preview: bool,
    /// Preview split ratio (0.0 to 1.0, percentage for results pane)
    split_ratio: f32,
}

impl SearchWithPreviewOverlay {
    /// Create a new search overlay with preview
    pub fn new() -> Self {
        Self {
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
            preview_renderer: SearchPreviewRenderer::default_config(),
            show_preview: true,
            split_ratio: 0.4, // 40% results, 60% preview
        }
    }

    /// Get current query
    pub fn query(&self) -> &str {
        &self.query
    }

    /// Get results
    pub fn results(&self) -> &[SearchResultWithPreview] {
        &self.results
    }

    /// Get selected index
    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Check if preview is enabled
    pub fn show_preview(&self) -> bool {
        self.show_preview
    }

    /// Update query and perform search
    pub fn update_query(&mut self, query: String, items: &[(String, Option<String>)]) {
        self.query = query;
        self.perform_search(items);
    }

    /// Perform search with preview support
    fn perform_search(&mut self, items: &[(String, Option<String>)]) {
        self.results.clear();
        self.selected_index = 0;

        if self.query.is_empty() {
            return;
        }

        // Collect matches
        let mut matches: Vec<SearchResultWithPreview> = items
            .iter()
            .enumerate()
            .filter_map(|(index, (text, full_content))| {
                calculate_match_score(text, &self.query).map(|(score, positions)| {
                    SearchResultWithPreview::new(
                        text.clone(),
                        score.max(0) as usize,
                        positions,
                        index,
                        full_content.clone(),
                    )
                })
            })
            .collect();

        // Sort by score (higher is better)
        matches.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.index.cmp(&b.index)));

        // Limit results
        matches.truncate(50);

        self.results = matches;
    }

    /// Move selection up
    pub fn select_previous(&mut self) {
        if !self.results.is_empty() && self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn select_next(&mut self) {
        if !self.results.is_empty() && self.selected_index < self.results.len() - 1 {
            self.selected_index += 1;
        }
    }

    /// Toggle preview pane visibility
    pub fn toggle_preview(&mut self) {
        self.show_preview = !self.show_preview;
    }

    /// Get selected result
    pub fn selected_result(&self) -> Option<&SearchResultWithPreview> {
        self.results.get(self.selected_index)
    }

    /// Generate preview for selected result
    pub fn generate_preview(&self) -> Option<SearchPreview> {
        let result = self.selected_result()?;
        let content = result.full_content.as_ref()?;

        let line_match = result.to_line_match();
        Some(self.preview_renderer.generate_preview(content, &line_match))
    }

    /// Render search input
    fn render_search_input(&self, frame: &mut Frame, area: Rect) {
        let search_text = if self.query.is_empty() {
            Line::from(vec![
                Span::styled("Search: ", Style::default().fg(Color::Yellow)),
                Span::styled("(type to search...)", Style::default().fg(Color::DarkGray)),
            ])
        } else {
            Line::from(vec![
                Span::styled("Search: ", Style::default().fg(Color::Yellow)),
                Span::styled(&self.query, Style::default().fg(Color::White)),
                Span::styled("_", Style::default().fg(Color::Cyan)),
            ])
        };

        let paragraph = Paragraph::new(search_text);
        frame.render_widget(paragraph, area);
    }

    /// Render results list
    fn render_results_list(&self, frame: &mut Frame, area: Rect) {
        if self.results.is_empty() {
            let empty_msg = if self.query.is_empty() {
                "Start typing to search..."
            } else {
                "No matches found"
            };

            let paragraph = Paragraph::new(Line::from(Span::styled(
                empty_msg,
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(paragraph, area);
            return;
        }

        // Create list items
        let items: Vec<ListItem> = self
            .results
            .iter()
            .enumerate()
            .map(|(idx, result)| {
                let is_selected = idx == self.selected_index;

                // Highlight matches
                let styled_text = self.highlight_matches(&result.text, &result.positions);

                let style = if is_selected {
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(styled_text).style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, area);
    }

    /// Render status bar
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status_text = if self.results.is_empty() {
            Line::from(vec![
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" close | "),
                Span::styled(
                    "Ctrl+P",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" toggle preview"),
            ])
        } else {
            Line::from(vec![
                Span::styled(
                    format!("{}/{}", self.selected_index + 1, self.results.len()),
                    Style::default().fg(Color::Cyan),
                ),
                Span::raw(" | "),
                Span::styled(
                    "↑↓",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" navigate | "),
                Span::styled(
                    "Enter",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" select | "),
                Span::styled(
                    "Ctrl+P",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" preview | "),
                Span::styled(
                    "Esc",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(" cancel"),
            ])
        };

        let paragraph = Paragraph::new(status_text);
        frame.render_widget(paragraph, area);
    }

    /// Highlight match positions
    fn highlight_matches(&self, text: &str, positions: &[usize]) -> Line<'static> {
        if positions.is_empty() {
            return Line::from(text.to_string());
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

            // Add highlighted match
            let match_char: String = chars[pos..pos + 1].iter().collect();
            spans.push(Span::styled(
                match_char,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));

            last_pos = pos + 1;
        }

        // Add remaining text
        if last_pos < chars.len() {
            let remaining: String = chars[last_pos..].iter().collect();
            spans.push(Span::raw(remaining));
        }

        Line::from(spans)
    }
}

impl Overlay for SearchWithPreviewOverlay {
    fn render(&self, frame: &mut Frame, area: Rect) {
        // Clear background
        frame.render_widget(Clear, area);

        // Create centered popup (80% width, 80% height)
        let popup_width = (area.width as f32 * 0.8) as u16;
        let popup_height = (area.height as f32 * 0.8) as u16;
        let popup_x = (area.width.saturating_sub(popup_width)) / 2;
        let popup_y = (area.height.saturating_sub(popup_height)) / 2;

        let popup_area = Rect {
            x: area.x + popup_x,
            y: area.y + popup_y,
            width: popup_width,
            height: popup_height,
        };

        // Main border
        let block = Block::default()
            .title(" Search with Preview ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        frame.render_widget(block, popup_area);

        // Inner area
        let inner_area = Rect {
            x: popup_area.x + 1,
            y: popup_area.y + 1,
            width: popup_area.width.saturating_sub(2),
            height: popup_area.height.saturating_sub(2),
        };

        // Layout: search input | content area | status bar
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // Search input
                Constraint::Min(3),    // Content area
                Constraint::Length(1), // Status bar
            ])
            .split(inner_area);

        // Render search input
        self.render_search_input(frame, main_chunks[0]);

        // Content area: split between results and preview
        if self.show_preview && !self.results.is_empty() {
            let content_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage((self.split_ratio * 100.0) as u16),
                    Constraint::Percentage(((1.0 - self.split_ratio) * 100.0) as u16),
                ])
                .split(main_chunks[1]);

            // Render results list
            self.render_results_list(frame, content_chunks[0]);

            // Render preview pane
            if let Some(preview) = self.generate_preview() {
                self.preview_renderer
                    .render(frame, content_chunks[1], &preview);
            } else {
                // No preview available
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(" Preview ")
                    .border_style(Style::default().fg(Color::DarkGray));

                let inner = block.inner(content_chunks[1]);
                frame.render_widget(block, content_chunks[1]);

                let msg = Paragraph::new(Line::from(Span::styled(
                    "No preview available",
                    Style::default().fg(Color::DarkGray),
                )));
                frame.render_widget(msg, inner);
            }
        } else {
            // Full width results (no preview)
            self.render_results_list(frame, main_chunks[1]);
        }

        // Render status bar
        self.render_status_bar(frame, main_chunks[2]);
    }

    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction {
        match key.code {
            KeyCode::Esc => OverlayAction::Close,
            KeyCode::Enter => {
                if let Some(result) = self.selected_result() {
                    OverlayAction::Submit(result.text.clone())
                } else {
                    OverlayAction::Close
                }
            }
            KeyCode::Up => {
                self.select_previous();
                OverlayAction::Continue
            }
            KeyCode::Down => {
                self.select_next();
                OverlayAction::Continue
            }
            KeyCode::Char('p')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.toggle_preview();
                OverlayAction::Continue
            }
            _ => OverlayAction::Continue,
        }
    }

    fn title(&self) -> &str {
        "Search with Preview"
    }
}

impl Default for SearchWithPreviewOverlay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_overlay() {
        let overlay = SearchWithPreviewOverlay::new();
        assert_eq!(overlay.query(), "");
        assert_eq!(overlay.results().len(), 0);
        assert!(overlay.show_preview);
    }

    #[test]
    fn test_update_query() {
        let mut overlay = SearchWithPreviewOverlay::new();
        let items = vec![
            (
                "hello world".to_string(),
                Some("hello world\nfull content".to_string()),
            ),
            ("goodbye".to_string(), None),
        ];

        overlay.update_query("hello".to_string(), &items);
        assert_eq!(overlay.query(), "hello");
        assert_eq!(overlay.results().len(), 1);
    }

    #[test]
    fn test_navigation() {
        let mut overlay = SearchWithPreviewOverlay::new();
        let items = vec![
            ("item1".to_string(), None),
            ("item2".to_string(), None),
            ("item3".to_string(), None),
        ];

        overlay.update_query("item".to_string(), &items);
        assert_eq!(overlay.selected_index(), 0);

        overlay.select_next();
        assert_eq!(overlay.selected_index(), 1);

        overlay.select_next();
        assert_eq!(overlay.selected_index(), 2);

        overlay.select_next();
        assert_eq!(overlay.selected_index(), 2); // No overflow

        overlay.select_previous();
        assert_eq!(overlay.selected_index(), 1);
    }

    #[test]
    fn test_toggle_preview() {
        let mut overlay = SearchWithPreviewOverlay::new();
        assert!(overlay.show_preview);

        overlay.toggle_preview();
        assert!(!overlay.show_preview);

        overlay.toggle_preview();
        assert!(overlay.show_preview);
    }

    #[test]
    fn test_generate_preview() {
        let mut overlay = SearchWithPreviewOverlay::new();
        let full_content = "line 1\nline 2\nline 3\nline 4\nline 5";
        let items = vec![("line 2".to_string(), Some(full_content.to_string()))];

        overlay.update_query("line".to_string(), &items);
        let preview = overlay.generate_preview();
        assert!(preview.is_some());
    }

    #[test]
    fn test_selected_result() {
        let mut overlay = SearchWithPreviewOverlay::new();
        let items = vec![("first".to_string(), None), ("second".to_string(), None)];

        overlay.update_query("i".to_string(), &items);
        let selected = overlay.selected_result();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().text, "first");
    }
}
