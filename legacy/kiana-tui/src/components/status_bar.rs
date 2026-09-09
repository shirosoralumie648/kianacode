use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Normal,
    Insert,
    Search,
    Config,
    Overlay,
}

impl AppMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Insert => "INSERT",
            Self::Search => "SEARCH",
            Self::Config => "CONFIG",
            Self::Overlay => "OVERLAY",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            Self::Normal => Color::Blue,
            Self::Insert => Color::Green,
            Self::Search => Color::Yellow,
            Self::Config => Color::Magenta,
            Self::Overlay => Color::Cyan,
        }
    }
}

/// Priority level for keybinding hints
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HintPriority {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

/// Category of keybinding hint
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HintCategory {
    Navigation,
    Action,
    Edit,
    System,
}

#[derive(Debug, Clone)]
pub struct KeyHint {
    pub key: String,
    pub description: String,
    pub priority: HintPriority,
    pub category: HintCategory,
}

impl KeyHint {
    pub fn new(key: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
            priority: HintPriority::Medium,
            category: HintCategory::Action,
        }
    }

    /// Set the priority level for this hint
    pub fn with_priority(mut self, priority: HintPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set the category for this hint
    pub fn with_category(mut self, category: HintCategory) -> Self {
        self.category = category;
        self
    }
}

pub struct StatusBar {
    mode: AppMode,
    status_message: Option<String>,
    key_hints: Vec<KeyHint>,
    macro_recording: Option<char>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self {
            mode: AppMode::Normal,
            status_message: None,
            key_hints: Vec::new(),
            macro_recording: None,
        }
    }

    /// Set macro recording state
    pub fn set_macro_recording(&mut self, register: Option<char>) {
        self.macro_recording = register;
    }

    /// Check if recording a macro
    pub fn is_recording_macro(&self) -> bool {
        self.macro_recording.is_some()
    }

    pub fn set_mode(&mut self, mode: AppMode) {
        self.mode = mode;
    }

    pub fn mode(&self) -> AppMode {
        self.mode
    }

    pub fn set_status(&mut self, message: Option<String>) {
        self.status_message = message;
    }

    pub fn set_key_hints(&mut self, hints: Vec<KeyHint>) {
        self.key_hints = hints;
    }

    /// Set key hints with intelligent filtering based on priority and available width
    pub fn set_key_hints_with_filter(&mut self, hints: Vec<KeyHint>, max_width: u16) {
        self.key_hints = self.filter_hints_by_priority(hints, max_width);
    }

    /// Filter and sort hints based on priority and available width
    /// Returns hints sorted by priority (descending), fitting within the given width
    fn filter_hints_by_priority(&self, hints: Vec<KeyHint>, max_width: u16) -> Vec<KeyHint> {
        let mut sorted = hints;
        // Sort by priority (descending) then by category
        sorted.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.category.cmp(&b.category))
        });

        // Simulate fitting hints within width
        let mut result = Vec::new();
        let mut current_width = 0u16;
        let separator_width = 3u16; // " │ "

        for hint in sorted {
            let hint_width = (hint.key.len() + hint.description.len() + 2) as u16; // +2 for ": "
            let needed = if result.is_empty() {
                hint_width
            } else {
                hint_width + separator_width
            };

            if current_width + needed <= max_width.saturating_sub(10) {
                current_width += needed;
                result.push(hint);
            } else if hint.priority == HintPriority::Critical {
                // Always show critical hints, even if we need to stop after
                result.push(hint);
                break;
            }
        }

        result
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        let left = self.render_left_section();
        let center = self.render_center_section();
        let right = self.render_right_section(area.width);

        // Calculate section widths
        let left_width = left.width() as u16;
        let right_width = right.width() as u16;
        let center_width = area.width.saturating_sub(left_width + right_width + 2);

        // Render left section
        let left_area = Rect {
            x: area.x,
            y: area.y,
            width: left_width.min(area.width),
            height: 1,
        };
        f.render_widget(Paragraph::new(left), left_area);

        // Render center section
        if center_width > 0 {
            let center_area = Rect {
                x: area.x + left_width + 1,
                y: area.y,
                width: center_width,
                height: 1,
            };
            f.render_widget(
                Paragraph::new(center).alignment(Alignment::Center),
                center_area,
            );
        }

        // Render right section
        if right_width > 0 && area.width > left_width + right_width {
            let right_area = Rect {
                x: area.right().saturating_sub(right_width),
                y: area.y,
                width: right_width.min(area.width),
                height: 1,
            };
            f.render_widget(
                Paragraph::new(right).alignment(Alignment::Right),
                right_area,
            );
        }
    }

    fn render_left_section(&self) -> Line<'_> {
        let mode_color = self.mode.color();
        let mode_name = self.mode.display_name();

        let mut spans = vec![Span::styled(
            format!(" {} ", mode_name),
            Style::default()
                .fg(Color::Black)
                .bg(mode_color)
                .add_modifier(Modifier::BOLD),
        )];

        // Add recording indicator if recording
        if let Some(register) = self.macro_recording {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("🔴 recording @{}", register),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }

        Line::from(spans)
    }

    fn render_center_section(&self) -> Line<'_> {
        if let Some(ref message) = self.status_message {
            Line::from(Span::raw(message.clone()))
        } else {
            Line::from(Span::raw("Ready"))
        }
    }

    fn render_right_section(&self, max_width: u16) -> Line<'_> {
        if self.key_hints.is_empty() {
            return Line::from(vec![]);
        }

        let mut spans = Vec::new();
        let mut current_width = 0u16;
        let separator = " │ ";
        let separator_width = separator.len() as u16;

        for (i, hint) in self.key_hints.iter().enumerate() {
            let hint_text = format!("{}: {}", hint.key, hint.description);
            let hint_width = hint_text.len() as u16;

            // Check if adding this hint would exceed max width
            let needed_width = if i > 0 {
                hint_width + separator_width
            } else {
                hint_width
            };

            if current_width + needed_width > max_width.saturating_sub(10) {
                break;
            }

            // Add separator
            if i > 0 {
                spans.push(Span::styled(
                    separator,
                    Style::default().fg(Color::DarkGray),
                ));
                current_width += separator_width;
            }

            // Add key
            spans.push(Span::styled(
                &hint.key,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(": "));
            spans.push(Span::raw(&hint.description));

            current_width += hint_width;
        }

        spans.push(Span::raw(" "));
        Line::from(spans)
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_display_names() {
        assert_eq!(AppMode::Normal.display_name(), "NORMAL");
        assert_eq!(AppMode::Insert.display_name(), "INSERT");
        assert_eq!(AppMode::Search.display_name(), "SEARCH");
        assert_eq!(AppMode::Config.display_name(), "CONFIG");
        assert_eq!(AppMode::Overlay.display_name(), "OVERLAY");
    }

    #[test]
    fn test_mode_colors() {
        assert_eq!(AppMode::Normal.color(), Color::Blue);
        assert_eq!(AppMode::Insert.color(), Color::Green);
        assert_eq!(AppMode::Search.color(), Color::Yellow);
        assert_eq!(AppMode::Config.color(), Color::Magenta);
        assert_eq!(AppMode::Overlay.color(), Color::Cyan);
    }

    #[test]
    fn test_status_bar_creation() {
        let bar = StatusBar::new();
        assert_eq!(bar.mode(), AppMode::Normal);
        assert!(bar.status_message.is_none());
        assert!(bar.key_hints.is_empty());
    }

    #[test]
    fn test_set_mode() {
        let mut bar = StatusBar::new();
        bar.set_mode(AppMode::Insert);
        assert_eq!(bar.mode(), AppMode::Insert);
    }

    #[test]
    fn test_set_status() {
        let mut bar = StatusBar::new();
        bar.set_status(Some("Connected".into()));
        assert_eq!(bar.status_message, Some("Connected".into()));
    }

    #[test]
    fn test_set_key_hints() {
        let mut bar = StatusBar::new();
        let hints = vec![KeyHint::new("?", "Help"), KeyHint::new("q", "Quit")];
        bar.set_key_hints(hints);
        assert_eq!(bar.key_hints.len(), 2);
    }

    #[test]
    fn test_key_hint_creation() {
        let hint = KeyHint::new("Ctrl+P", "Command Palette");
        assert_eq!(hint.key, "Ctrl+P");
        assert_eq!(hint.description, "Command Palette");
    }

    #[test]
    fn test_default_trait() {
        let bar = StatusBar::default();
        assert_eq!(bar.mode(), AppMode::Normal);
    }

    #[test]
    fn test_mode_equality() {
        assert_eq!(AppMode::Normal, AppMode::Normal);
        assert_ne!(AppMode::Normal, AppMode::Insert);
    }

    #[test]
    fn test_render_left_section() {
        let bar = StatusBar::new();
        let line = bar.render_left_section();
        assert!(line.width() > 0);
    }

    #[test]
    fn test_render_center_section_with_message() {
        let mut bar = StatusBar::new();
        bar.set_status(Some("Test Message".into()));
        let line = bar.render_center_section();
        assert!(line.width() > 0);
    }

    #[test]
    fn test_render_center_section_without_message() {
        let bar = StatusBar::new();
        let line = bar.render_center_section();
        assert_eq!(line.width(), 5); // "Ready"
    }

    #[test]
    fn test_render_right_section_empty() {
        let bar = StatusBar::new();
        let line = bar.render_right_section(100);
        assert_eq!(line.width(), 0);
    }

    #[test]
    fn test_render_right_section_with_hints() {
        let mut bar = StatusBar::new();
        bar.set_key_hints(vec![KeyHint::new("q", "Quit"), KeyHint::new("?", "Help")]);
        let line = bar.render_right_section(100);
        assert!(line.width() > 0);
    }

    #[test]
    fn test_render_right_section_narrow_terminal() {
        let mut bar = StatusBar::new();
        bar.set_key_hints(vec![
            KeyHint::new("q", "Quit"),
            KeyHint::new("?", "Help"),
            KeyHint::new("Ctrl+P", "Command Palette"),
        ]);
        let line = bar.render_right_section(20);
        // Should truncate hints to fit
        assert!(line.width() <= 20);
    }

    // Feature 6: Enhanced Keybinding Hints Tests

    #[test]
    fn test_hint_priority_ordering() {
        assert!(HintPriority::Critical > HintPriority::High);
        assert!(HintPriority::High > HintPriority::Medium);
        assert!(HintPriority::Medium > HintPriority::Low);
    }

    #[test]
    fn test_hint_priority_values() {
        assert_eq!(HintPriority::Low as i32, 1);
        assert_eq!(HintPriority::Medium as i32, 2);
        assert_eq!(HintPriority::High as i32, 3);
        assert_eq!(HintPriority::Critical as i32, 4);
    }

    #[test]
    fn test_hint_category_ordering() {
        assert!(HintCategory::Navigation < HintCategory::Action);
        assert!(HintCategory::Action < HintCategory::Edit);
        assert!(HintCategory::Edit < HintCategory::System);
    }

    #[test]
    fn test_hint_with_priority() {
        let hint = KeyHint::new("q", "Quit").with_priority(HintPriority::Critical);
        assert_eq!(hint.priority, HintPriority::Critical);
        assert_eq!(hint.key, "q");
        assert_eq!(hint.description, "Quit");
    }

    #[test]
    fn test_hint_with_category() {
        let hint = KeyHint::new("?", "Help").with_category(HintCategory::System);
        assert_eq!(hint.category, HintCategory::System);
    }

    #[test]
    fn test_hint_with_priority_and_category() {
        let hint = KeyHint::new("Ctrl+C", "Quit")
            .with_priority(HintPriority::Critical)
            .with_category(HintCategory::System);
        assert_eq!(hint.priority, HintPriority::Critical);
        assert_eq!(hint.category, HintCategory::System);
    }

    #[test]
    fn test_hint_default_priority_and_category() {
        let hint = KeyHint::new("x", "Test");
        assert_eq!(hint.priority, HintPriority::Medium);
        assert_eq!(hint.category, HintCategory::Action);
    }

    #[test]
    fn test_filter_hints_by_priority() {
        let bar = StatusBar::new();
        let hints = vec![
            KeyHint::new("a", "Low").with_priority(HintPriority::Low),
            KeyHint::new("b", "Critical").with_priority(HintPriority::Critical),
            KeyHint::new("c", "Medium").with_priority(HintPriority::Medium),
            KeyHint::new("d", "High").with_priority(HintPriority::High),
        ];

        let filtered = bar.filter_hints_by_priority(hints, 100);

        // Should be sorted by priority descending
        assert_eq!(filtered.len(), 4);
        assert_eq!(filtered[0].priority, HintPriority::Critical);
        assert_eq!(filtered[1].priority, HintPriority::High);
        assert_eq!(filtered[2].priority, HintPriority::Medium);
        assert_eq!(filtered[3].priority, HintPriority::Low);
    }

    #[test]
    fn test_filter_hints_narrow_width() {
        let bar = StatusBar::new();
        let hints = vec![
            KeyHint::new("a", "Action").with_priority(HintPriority::Medium),
            KeyHint::new("b", "Critical").with_priority(HintPriority::Critical),
            KeyHint::new("c", "Low").with_priority(HintPriority::Low),
        ];

        let filtered = bar.filter_hints_by_priority(hints, 25);

        // Should prioritize critical hint and fit what we can
        assert!(!filtered.is_empty());
        assert_eq!(filtered[0].priority, HintPriority::Critical);
        // Width is tight, might only fit critical hint
        assert!(filtered.len() <= 2);
    }

    #[test]
    fn test_critical_hint_always_shown() {
        let bar = StatusBar::new();
        let hints = vec![KeyHint::new("verylongkey", "verylongdescription")
            .with_priority(HintPriority::Critical)];

        let filtered = bar.filter_hints_by_priority(hints, 15);

        // Critical hints shown even if width is very tight
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].priority, HintPriority::Critical);
    }

    #[test]
    fn test_filter_hints_stops_at_critical_when_no_space() {
        let bar = StatusBar::new();
        let hints = vec![
            KeyHint::new("a", "First").with_priority(HintPriority::Low),
            KeyHint::new("b", "Second").with_priority(HintPriority::Low),
            KeyHint::new("c", "MustShow").with_priority(HintPriority::Critical),
        ];

        let filtered = bar.filter_hints_by_priority(hints, 20);

        // Should show critical even if it means stopping early
        assert!(!filtered.is_empty());
        let has_critical = filtered
            .iter()
            .any(|h| h.priority == HintPriority::Critical);
        assert!(has_critical);
    }

    #[test]
    fn test_filter_hints_empty_list() {
        let bar = StatusBar::new();
        let hints = vec![];

        let filtered = bar.filter_hints_by_priority(hints, 100);
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_filter_hints_sorts_by_category_when_priority_equal() {
        let bar = StatusBar::new();
        let hints = vec![
            KeyHint::new("s", "System")
                .with_priority(HintPriority::High)
                .with_category(HintCategory::System),
            KeyHint::new("n", "Nav")
                .with_priority(HintPriority::High)
                .with_category(HintCategory::Navigation),
            KeyHint::new("a", "Action")
                .with_priority(HintPriority::High)
                .with_category(HintCategory::Action),
        ];

        let filtered = bar.filter_hints_by_priority(hints, 100);

        assert_eq!(filtered.len(), 3);
        // When priority is equal, should sort by category
        assert_eq!(filtered[0].category, HintCategory::Navigation);
        assert_eq!(filtered[1].category, HintCategory::Action);
        assert_eq!(filtered[2].category, HintCategory::System);
    }

    #[test]
    fn test_set_key_hints_with_filter() {
        let mut bar = StatusBar::new();
        let hints = vec![
            KeyHint::new("a", "Low").with_priority(HintPriority::Low),
            KeyHint::new("b", "Critical").with_priority(HintPriority::Critical),
        ];

        bar.set_key_hints_with_filter(hints, 50);

        // Should have filtered and sorted hints
        assert_eq!(bar.key_hints.len(), 2);
        assert_eq!(bar.key_hints[0].priority, HintPriority::Critical);
    }
}
