//! Toast notification system for displaying temporary messages
//!
//! This module provides a non-blocking notification system that displays
//! temporary messages in the top-right corner of the screen. Toasts auto-dismiss
//! after a configurable duration and can be manually dismissed.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use std::time::{Duration, Instant};

/// Severity level for toast notifications
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastLevel {
    /// Informational message
    Info,
    /// Success confirmation
    Success,
    /// Warning message
    Warning,
    /// Error message
    Error,
}

impl ToastLevel {
    /// Returns the color associated with this toast level
    pub fn color(&self) -> Color {
        match self {
            Self::Info => Color::Blue,
            Self::Success => Color::Green,
            Self::Warning => Color::Yellow,
            Self::Error => Color::Red,
        }
    }

    /// Returns the icon character for this toast level
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Info => "ℹ",
            Self::Success => "✓",
            Self::Warning => "⚠",
            Self::Error => "✗",
        }
    }
}

/// A single toast notification
#[derive(Debug, Clone)]
pub struct Toast {
    /// Unique identifier for this toast
    pub id: usize,
    /// Message to display
    pub message: String,
    /// Severity level
    pub level: ToastLevel,
    /// When the toast was created
    pub created_at: Instant,
    /// How long the toast should be displayed
    pub duration: Duration,
}

impl Toast {
    /// Creates a new toast with default duration (3 seconds)
    pub fn new(id: usize, message: String, level: ToastLevel) -> Self {
        Self {
            id,
            message,
            level,
            created_at: Instant::now(),
            duration: Duration::from_secs(3),
        }
    }

    /// Sets a custom duration for this toast
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Checks if this toast has expired
    pub fn is_expired(&self) -> bool {
        Instant::now().duration_since(self.created_at) >= self.duration
    }
}

/// Manages multiple toast notifications
pub struct ToastManager {
    toasts: Vec<Toast>,
    next_id: usize,
    max_toasts: usize,
}

impl ToastManager {
    /// Creates a new toast manager
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 0,
            max_toasts: 10,
        }
    }

    /// Adds a new toast notification with default duration
    pub fn add(&mut self, message: String, level: ToastLevel) -> usize {
        let id = self.next_id;
        self.next_id += 1;

        let toast = Toast::new(id, message, level);
        self.toasts.push(toast);

        // Limit total toasts
        if self.toasts.len() > self.max_toasts {
            self.toasts.remove(0);
        }

        id
    }

    /// Adds a new toast notification with custom duration
    pub fn add_with_duration(
        &mut self,
        message: String,
        level: ToastLevel,
        duration: Duration,
    ) -> usize {
        let id = self.next_id;
        self.next_id += 1;

        let toast = Toast::new(id, message, level).with_duration(duration);
        self.toasts.push(toast);

        if self.toasts.len() > self.max_toasts {
            self.toasts.remove(0);
        }

        id
    }

    /// Manually dismisses a toast by ID
    pub fn dismiss(&mut self, id: usize) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Removes all expired toasts
    pub fn prune_expired(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    /// Clears all toasts
    pub fn clear(&mut self) {
        self.toasts.clear();
    }

    /// Returns a reference to all current toasts
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }

    /// Renders all toasts in the given frame area
    pub fn render(&self, f: &mut Frame, area: Rect) {
        let toast_width = 40;
        let toast_height = 3;
        let margin_right = 2;
        let margin_top = 1;

        for (i, toast) in self.toasts.iter().enumerate() {
            let y = area.y + margin_top + (i as u16 * toast_height);
            let x = area.right().saturating_sub(toast_width + margin_right);

            // Don't render if off-screen
            if y + toast_height > area.bottom() {
                break;
            }

            let toast_area = Rect {
                x,
                y,
                width: toast_width,
                height: toast_height,
            };

            self.render_toast(f, toast_area, toast);
        }
    }

    /// Renders a single toast
    fn render_toast(&self, f: &mut Frame, area: Rect, toast: &Toast) {
        // Clear background
        f.render_widget(Clear, area);

        let color = toast.level.color();
        let icon = toast.level.icon();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(color));

        let inner = block.inner(area);

        // Truncate message if too long
        let max_len = (inner.width as usize).saturating_sub(3);
        let message = if toast.message.len() > max_len {
            format!("{}...", &toast.message[..max_len.saturating_sub(3)])
        } else {
            toast.message.clone()
        };

        let text = Line::from(vec![
            Span::styled(
                icon,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::raw(message),
        ]);

        let paragraph = Paragraph::new(text).block(block);

        f.render_widget(paragraph, area);
    }
}

impl Default for ToastManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_toast_creation() {
        let toast = Toast::new(0, "Test message".into(), ToastLevel::Info);
        assert_eq!(toast.message, "Test message");
        assert_eq!(toast.level, ToastLevel::Info);
        assert!(!toast.is_expired());
    }

    #[test]
    fn test_toast_expiry() {
        let toast =
            Toast::new(0, "Test".into(), ToastLevel::Info).with_duration(Duration::from_millis(10));

        assert!(!toast.is_expired());
        sleep(Duration::from_millis(20));
        assert!(toast.is_expired());
    }

    #[test]
    fn test_manager_add() {
        let mut manager = ToastManager::new();
        let id1 = manager.add("Message 1".into(), ToastLevel::Info);
        let id2 = manager.add("Message 2".into(), ToastLevel::Success);

        assert_eq!(manager.toasts().len(), 2);
        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
    }

    #[test]
    fn test_manager_dismiss() {
        let mut manager = ToastManager::new();
        let id = manager.add("Test".into(), ToastLevel::Info);

        assert_eq!(manager.toasts().len(), 1);
        manager.dismiss(id);
        assert_eq!(manager.toasts().len(), 0);
    }

    #[test]
    fn test_manager_prune_expired() {
        let mut manager = ToastManager::new();
        manager.add_with_duration("Short".into(), ToastLevel::Info, Duration::from_millis(10));
        manager.add_with_duration("Long".into(), ToastLevel::Info, Duration::from_secs(10));

        assert_eq!(manager.toasts().len(), 2);
        sleep(Duration::from_millis(20));
        manager.prune_expired();
        assert_eq!(manager.toasts().len(), 1);
    }

    #[test]
    fn test_manager_max_toasts() {
        let mut manager = ToastManager::new();
        manager.max_toasts = 3;

        for i in 0..5 {
            manager.add(format!("Toast {}", i), ToastLevel::Info);
        }

        assert_eq!(manager.toasts().len(), 3);
        // Should keep the last 3
        assert_eq!(manager.toasts()[0].message, "Toast 2");
    }

    #[test]
    fn test_level_colors() {
        assert_eq!(ToastLevel::Info.color(), Color::Blue);
        assert_eq!(ToastLevel::Success.color(), Color::Green);
        assert_eq!(ToastLevel::Warning.color(), Color::Yellow);
        assert_eq!(ToastLevel::Error.color(), Color::Red);
    }

    #[test]
    fn test_level_icons() {
        assert_eq!(ToastLevel::Info.icon(), "ℹ");
        assert_eq!(ToastLevel::Success.icon(), "✓");
        assert_eq!(ToastLevel::Warning.icon(), "⚠");
        assert_eq!(ToastLevel::Error.icon(), "✗");
    }

    #[test]
    fn test_manager_clear() {
        let mut manager = ToastManager::new();
        manager.add("Test 1".into(), ToastLevel::Info);
        manager.add("Test 2".into(), ToastLevel::Success);

        assert_eq!(manager.toasts().len(), 2);
        manager.clear();
        assert_eq!(manager.toasts().len(), 0);
    }

    #[test]
    fn test_toast_with_custom_duration() {
        let toast =
            Toast::new(0, "Test".into(), ToastLevel::Info).with_duration(Duration::from_secs(5));

        assert_eq!(toast.duration, Duration::from_secs(5));
    }

    #[test]
    fn test_manager_default() {
        let manager = ToastManager::default();
        assert_eq!(manager.toasts().len(), 0);
        assert_eq!(manager.max_toasts, 10);
    }

    #[test]
    fn test_dismiss_nonexistent() {
        let mut manager = ToastManager::new();
        manager.add("Test".into(), ToastLevel::Info);

        // Dismissing a non-existent ID should not panic
        manager.dismiss(999);
        assert_eq!(manager.toasts().len(), 1);
    }

    #[test]
    fn test_multiple_levels() {
        let mut manager = ToastManager::new();
        manager.add("Info".into(), ToastLevel::Info);
        manager.add("Success".into(), ToastLevel::Success);
        manager.add("Warning".into(), ToastLevel::Warning);
        manager.add("Error".into(), ToastLevel::Error);

        let toasts = manager.toasts();
        assert_eq!(toasts.len(), 4);
        assert_eq!(toasts[0].level, ToastLevel::Info);
        assert_eq!(toasts[1].level, ToastLevel::Success);
        assert_eq!(toasts[2].level, ToastLevel::Warning);
        assert_eq!(toasts[3].level, ToastLevel::Error);
    }
}
