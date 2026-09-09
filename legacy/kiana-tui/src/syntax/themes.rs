use ratatui::style::Color;
use syntect::highlighting::{Theme, ThemeSet};

/// 高亮主题类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightTheme {
    /// 暗色主题
    Dark,
    /// 亮色主题
    Light,
}

/// 主题管理器
pub struct ThemeManager {
    theme_set: ThemeSet,
    current_theme: HighlightTheme,
}

impl ThemeManager {
    /// 创建新的主题管理器
    pub fn new() -> Self {
        Self {
            theme_set: ThemeSet::load_defaults(),
            current_theme: HighlightTheme::Dark,
        }
    }

    /// 获取当前主题
    pub fn current_theme(&self) -> HighlightTheme {
        self.current_theme
    }

    /// 设置主题
    pub fn set_theme(&mut self, theme: HighlightTheme) {
        self.current_theme = theme;
    }

    /// 获取 syntect 主题
    pub fn get_syntect_theme(&self) -> &Theme {
        match self.current_theme {
            HighlightTheme::Dark => &self.theme_set.themes["base16-ocean.dark"],
            HighlightTheme::Light => &self.theme_set.themes["base16-ocean.light"],
        }
    }

    /// 将 syntect 颜色转换为 ratatui 颜色
    pub fn syntect_to_ratatui_color(color: syntect::highlighting::Color) -> Color {
        Color::Rgb(color.r, color.g, color.b)
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_manager_creation() {
        let manager = ThemeManager::new();
        assert_eq!(manager.current_theme(), HighlightTheme::Dark);
    }

    #[test]
    fn test_theme_switching() {
        let mut manager = ThemeManager::new();
        manager.set_theme(HighlightTheme::Light);
        assert_eq!(manager.current_theme(), HighlightTheme::Light);
    }

    #[test]
    fn test_syntect_theme_access() {
        let manager = ThemeManager::new();
        let theme = manager.get_syntect_theme();
        assert!(!theme.name.is_none());
    }

    #[test]
    fn test_color_conversion() {
        let syntect_color = syntect::highlighting::Color {
            r: 255,
            g: 128,
            b: 64,
            a: 255,
        };
        let ratatui_color = ThemeManager::syntect_to_ratatui_color(syntect_color);
        assert_eq!(ratatui_color, Color::Rgb(255, 128, 64));
    }
}
