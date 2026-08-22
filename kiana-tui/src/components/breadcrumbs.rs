use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

#[derive(Clone, Debug)]
pub struct BreadcrumbSegment {
    pub label: String,
    pub action: Option<String>, // Action identifier for navigation
}

impl BreadcrumbSegment {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: None,
        }
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }
}

pub struct Breadcrumbs {
    segments: Vec<BreadcrumbSegment>,
    separator: String,
}

impl Breadcrumbs {
    pub fn new() -> Self {
        Self {
            segments: Vec::new(),
            separator: " > ".to_string(),
        }
    }

    pub fn with_separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    pub fn set_path(&mut self, segments: Vec<BreadcrumbSegment>) {
        self.segments = segments;
    }

    pub fn push(&mut self, segment: BreadcrumbSegment) {
        self.segments.push(segment);
    }

    pub fn pop(&mut self) -> Option<BreadcrumbSegment> {
        self.segments.pop()
    }

    pub fn clear(&mut self) {
        self.segments.clear();
    }

    pub fn segments(&self) -> &[BreadcrumbSegment] {
        &self.segments
    }

    /// Calculate total width needed for full breadcrumb trail
    fn calculate_full_width(&self) -> usize {
        if self.segments.is_empty() {
            return 0;
        }

        let separator_width = self.separator.len();
        let labels_width: usize = self.segments.iter().map(|s| s.label.len()).sum();
        let separators_width = separator_width * (self.segments.len() - 1);

        labels_width + separators_width
    }

    /// Truncate breadcrumbs to fit in available width
    fn truncate_segments(&self, available_width: usize) -> Vec<String> {
        if self.segments.is_empty() {
            return Vec::new();
        }

        let full_width = self.calculate_full_width();

        // If everything fits, return all segments
        if full_width <= available_width {
            return self.segments.iter().map(|s| s.label.clone()).collect();
        }

        // Show at least first and last segments
        if self.segments.len() <= 2 {
            return self.segments.iter().map(|s| s.label.clone()).collect();
        }

        let first = &self.segments[0].label;
        let last = &self.segments[self.segments.len() - 1].label;
        let ellipsis = "...";

        // Calculate: first + separator + ellipsis + separator + last
        let separator_width = self.separator.len();
        let min_width =
            first.len() + separator_width + ellipsis.len() + separator_width + last.len();

        // If even min doesn't fit, truncate individual segments
        if min_width > available_width {
            let per_segment = (available_width - separator_width - ellipsis.len()) / 2;
            return vec![truncate_string(first, per_segment), last.to_string()];
        }

        // Try to fit more segments from the end
        let mut result = vec![first.to_string()];
        let mut current_width = first.len();

        // Add as many segments from the end as possible
        let mut i = self.segments.len() - 1;
        let mut from_end = vec![last.to_string()];
        current_width += separator_width + last.len();

        while i > 1 {
            i -= 1;
            let segment_len = self.segments[i].label.len();
            let new_width = current_width + separator_width + segment_len;

            if new_width + separator_width + ellipsis.len() <= available_width {
                from_end.insert(0, self.segments[i].label.clone());
                current_width = new_width;
            } else {
                break;
            }
        }

        // Add ellipsis if needed
        if i > 1 || from_end[0] != self.segments[1].label {
            result.push(ellipsis.to_string());
        }

        result.extend(from_end);
        result
    }

    pub fn render(&self, f: &mut Frame, area: Rect) {
        if self.segments.is_empty() {
            return;
        }

        let available_width = area.width as usize;
        let truncated = self.truncate_segments(available_width);

        let mut spans = Vec::new();

        for (i, label) in truncated.iter().enumerate() {
            // Determine if this is the last segment (current location)
            let is_last = i == truncated.len() - 1;

            let style = if is_last {
                // Current location - highlighted
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if label == "..." {
                // Ellipsis
                Style::default().fg(Color::DarkGray)
            } else {
                // Clickable parent segments
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::UNDERLINED)
            };

            spans.push(Span::styled(label.clone(), style));

            // Add separator except after last segment
            if i < truncated.len() - 1 {
                spans.push(Span::styled(
                    &self.separator,
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }

        let paragraph = Paragraph::new(Line::from(spans));
        f.render_widget(paragraph, area);
    }
}

impl Default for Breadcrumbs {
    fn default() -> Self {
        Self::new()
    }
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        s.chars().take(max_len).collect()
    } else {
        let mut result: String = s.chars().take(max_len - 3).collect();
        result.push_str("...");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_creation() {
        let segment = BreadcrumbSegment::new("Home").with_action("goto_home");

        assert_eq!(segment.label, "Home");
        assert_eq!(segment.action, Some("goto_home".to_string()));
    }

    #[test]
    fn test_breadcrumbs_push_pop() {
        let mut breadcrumbs = Breadcrumbs::new();
        assert_eq!(breadcrumbs.segments().len(), 0);

        breadcrumbs.push(BreadcrumbSegment::new("Home"));
        assert_eq!(breadcrumbs.segments().len(), 1);

        breadcrumbs.push(BreadcrumbSegment::new("Sessions"));
        assert_eq!(breadcrumbs.segments().len(), 2);

        let popped = breadcrumbs.pop();
        assert!(popped.is_some());
        assert_eq!(popped.unwrap().label, "Sessions");
        assert_eq!(breadcrumbs.segments().len(), 1);
    }

    #[test]
    fn test_breadcrumbs_clear() {
        let mut breadcrumbs = Breadcrumbs::new();
        breadcrumbs.push(BreadcrumbSegment::new("Home"));
        breadcrumbs.push(BreadcrumbSegment::new("Sessions"));

        breadcrumbs.clear();
        assert_eq!(breadcrumbs.segments().len(), 0);
    }

    #[test]
    fn test_set_path() {
        let mut breadcrumbs = Breadcrumbs::new();

        let segments = vec![
            BreadcrumbSegment::new("Home"),
            BreadcrumbSegment::new("Sessions"),
            BreadcrumbSegment::new("session-123"),
        ];

        breadcrumbs.set_path(segments);
        assert_eq!(breadcrumbs.segments().len(), 3);
        assert_eq!(breadcrumbs.segments()[0].label, "Home");
        assert_eq!(breadcrumbs.segments()[2].label, "session-123");
    }

    #[test]
    fn test_calculate_full_width() {
        let mut breadcrumbs = Breadcrumbs::new();
        assert_eq!(breadcrumbs.calculate_full_width(), 0);

        breadcrumbs.push(BreadcrumbSegment::new("Home"));
        assert_eq!(breadcrumbs.calculate_full_width(), 4); // "Home"

        breadcrumbs.push(BreadcrumbSegment::new("Sessions"));
        // "Home" (4) + " > " (3) + "Sessions" (8) = 15
        assert_eq!(breadcrumbs.calculate_full_width(), 15);
    }

    #[test]
    fn test_truncate_fits_all() {
        let mut breadcrumbs = Breadcrumbs::new();
        breadcrumbs.push(BreadcrumbSegment::new("Home"));
        breadcrumbs.push(BreadcrumbSegment::new("Sessions"));

        let truncated = breadcrumbs.truncate_segments(50);
        assert_eq!(truncated.len(), 2);
        assert_eq!(truncated[0], "Home");
        assert_eq!(truncated[1], "Sessions");
    }

    #[test]
    fn test_truncate_with_ellipsis() {
        let mut breadcrumbs = Breadcrumbs::new();
        breadcrumbs.push(BreadcrumbSegment::new("Home"));
        breadcrumbs.push(BreadcrumbSegment::new("Sessions"));
        breadcrumbs.push(BreadcrumbSegment::new("session-123"));
        breadcrumbs.push(BreadcrumbSegment::new("Config"));

        // Small width forces truncation
        let truncated = breadcrumbs.truncate_segments(30);

        // Should have first, ellipsis, and last
        assert!(truncated.len() >= 2);
        assert_eq!(truncated[0], "Home");
        assert_eq!(truncated[truncated.len() - 1], "Config");
    }

    #[test]
    fn test_truncate_two_segments() {
        let mut breadcrumbs = Breadcrumbs::new();
        breadcrumbs.push(BreadcrumbSegment::new("Home"));
        breadcrumbs.push(BreadcrumbSegment::new("Sessions"));

        // Even with small width, show both
        let truncated = breadcrumbs.truncate_segments(10);
        assert_eq!(truncated.len(), 2);
    }

    #[test]
    fn test_custom_separator() {
        let breadcrumbs = Breadcrumbs::new().with_separator(" / ");
        assert_eq!(breadcrumbs.separator, " / ");
    }

    #[test]
    fn test_empty_breadcrumbs() {
        let breadcrumbs = Breadcrumbs::new();
        assert_eq!(breadcrumbs.calculate_full_width(), 0);
        assert_eq!(breadcrumbs.truncate_segments(100).len(), 0);
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("Hello", 10), "Hello");
        assert_eq!(truncate_string("Hello World", 8), "Hello...");
        assert_eq!(truncate_string("Hi", 3), "Hi");
        assert_eq!(truncate_string("Hello", 2), "He");
    }

    #[test]
    fn test_segment_without_action() {
        let segment = BreadcrumbSegment::new("Home");
        assert_eq!(segment.label, "Home");
        assert_eq!(segment.action, None);
    }

    #[test]
    fn test_multiple_pops() {
        let mut breadcrumbs = Breadcrumbs::new();
        breadcrumbs.push(BreadcrumbSegment::new("A"));
        breadcrumbs.push(BreadcrumbSegment::new("B"));
        breadcrumbs.push(BreadcrumbSegment::new("C"));

        assert_eq!(breadcrumbs.pop().unwrap().label, "C");
        assert_eq!(breadcrumbs.pop().unwrap().label, "B");
        assert_eq!(breadcrumbs.pop().unwrap().label, "A");
        assert!(breadcrumbs.pop().is_none());
    }
}
