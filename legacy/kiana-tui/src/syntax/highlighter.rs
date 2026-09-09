use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::{
    easy::HighlightLines,
    parsing::{SyntaxReference, SyntaxSet},
    util::LinesWithEndings,
};

use super::themes::{HighlightTheme, ThemeManager};

/// 语法高亮选项
#[derive(Debug, Clone)]
pub struct HighlightOptions {
    /// 是否显示行号
    pub show_line_numbers: bool,
    /// 行号宽度（字符数）
    pub line_number_width: usize,
    /// 主题
    pub theme: HighlightTheme,
}

impl Default for HighlightOptions {
    fn default() -> Self {
        Self {
            show_line_numbers: true,
            line_number_width: 4,
            theme: HighlightTheme::Dark,
        }
    }
}

/// 语法高亮器
pub struct SyntaxHighlighter {
    syntax_set: SyntaxSet,
    theme_manager: ThemeManager,
}

impl SyntaxHighlighter {
    /// 创建新的语法高亮器
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_manager: ThemeManager::new(),
        }
    }

    /// 设置主题
    pub fn set_theme(&mut self, theme: HighlightTheme) {
        self.theme_manager.set_theme(theme);
    }

    /// 根据语言名称查找语法定义
    fn find_syntax(&self, language: &str) -> Option<&SyntaxReference> {
        // 尝试直接匹配
        if let Some(syntax) = self.syntax_set.find_syntax_by_token(language) {
            return Some(syntax);
        }

        // 尝试别名匹配
        let lang_lower = language.to_lowercase();
        match lang_lower.as_str() {
            "rs" | "rust" => self.syntax_set.find_syntax_by_extension("rs"),
            "py" | "python" => self.syntax_set.find_syntax_by_extension("py"),
            "js" | "javascript" => self.syntax_set.find_syntax_by_extension("js"),
            "ts" | "typescript" => self.syntax_set.find_syntax_by_extension("ts"),
            "json" => self.syntax_set.find_syntax_by_extension("json"),
            "yaml" | "yml" => self.syntax_set.find_syntax_by_extension("yaml"),
            "toml" => self.syntax_set.find_syntax_by_extension("toml"),
            "sh" | "bash" | "shell" => self.syntax_set.find_syntax_by_extension("sh"),
            "md" | "markdown" => self.syntax_set.find_syntax_by_extension("md"),
            "html" => self.syntax_set.find_syntax_by_extension("html"),
            "css" => self.syntax_set.find_syntax_by_extension("css"),
            "sql" => self.syntax_set.find_syntax_by_extension("sql"),
            "c" => self.syntax_set.find_syntax_by_extension("c"),
            "cpp" | "c++" => self.syntax_set.find_syntax_by_extension("cpp"),
            "go" => self.syntax_set.find_syntax_by_extension("go"),
            "java" => self.syntax_set.find_syntax_by_extension("java"),
            "xml" => self.syntax_set.find_syntax_by_extension("xml"),
            _ => None,
        }
    }

    /// 高亮代码并返回 ratatui Lines
    pub fn highlight(
        &self,
        code: &str,
        language: &str,
        options: &HighlightOptions,
    ) -> Vec<Line<'static>> {
        let syntax = match self.find_syntax(language) {
            Some(s) => s,
            None => {
                // 如果找不到语法定义，返回纯文本
                return self.highlight_plain(code, options);
            }
        };

        let theme = self.theme_manager.get_syntect_theme();
        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut lines = Vec::new();

        let line_count = code.lines().count();
        let line_number_width = if options.show_line_numbers {
            options.line_number_width.max(line_count.to_string().len())
        } else {
            0
        };

        for (line_num, line) in LinesWithEndings::from(code).enumerate() {
            let mut spans = Vec::new();

            // 添加行号
            if options.show_line_numbers {
                let line_number = format!("{:>width$} ", line_num + 1, width = line_number_width);
                spans.push(Span::styled(
                    line_number,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ));
            }

            // 高亮代码
            match highlighter.highlight_line(line, &self.syntax_set) {
                Ok(highlighted) => {
                    for (style, text) in highlighted {
                        let fg_color = ThemeManager::syntect_to_ratatui_color(style.foreground);
                        spans.push(Span::styled(
                            text.to_string(),
                            Style::default().fg(fg_color),
                        ));
                    }
                }
                Err(_) => {
                    // 如果高亮失败，使用纯文本
                    spans.push(Span::raw(line.to_string()));
                }
            }

            lines.push(Line::from(spans));
        }

        lines
    }

    /// 高亮为纯文本（fallback）
    fn highlight_plain(&self, code: &str, options: &HighlightOptions) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let line_count = code.lines().count();
        let line_number_width = if options.show_line_numbers {
            options.line_number_width.max(line_count.to_string().len())
        } else {
            0
        };

        for (line_num, line) in code.lines().enumerate() {
            let mut spans = Vec::new();

            if options.show_line_numbers {
                let line_number = format!("{:>width$} ", line_num + 1, width = line_number_width);
                spans.push(Span::styled(
                    line_number,
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                ));
            }

            spans.push(Span::styled(
                line.to_string(),
                Style::default().fg(Color::Gray),
            ));

            lines.push(Line::from(spans));
        }

        lines
    }
}

impl Default for SyntaxHighlighter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlighter_creation() {
        let highlighter = SyntaxHighlighter::new();
        assert!(highlighter.find_syntax("rust").is_some());
    }

    #[test]
    fn test_language_detection() {
        let highlighter = SyntaxHighlighter::new();

        // Test direct matches
        assert!(highlighter.find_syntax("rust").is_some());
        assert!(highlighter.find_syntax("python").is_some());
        assert!(highlighter.find_syntax("javascript").is_some());

        // Test aliases
        assert!(highlighter.find_syntax("rs").is_some());
        assert!(highlighter.find_syntax("py").is_some());
        assert!(highlighter.find_syntax("js").is_some());
    }

    #[test]
    fn test_highlight_rust_code() {
        let highlighter = SyntaxHighlighter::new();
        let code = "fn main() {\n    println!(\"Hello\");\n}";
        let options = HighlightOptions::default();

        let lines = highlighter.highlight(code, "rust", &options);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_highlight_with_line_numbers() {
        let highlighter = SyntaxHighlighter::new();
        let code = "let x = 42;";
        let options = HighlightOptions {
            show_line_numbers: true,
            ..Default::default()
        };

        let lines = highlighter.highlight(code, "rust", &options);
        assert_eq!(lines.len(), 1);
        // Line should contain line number spans
        assert!(!lines[0].spans.is_empty());
    }

    #[test]
    fn test_highlight_without_line_numbers() {
        let highlighter = SyntaxHighlighter::new();
        let code = "let x = 42;";
        let options = HighlightOptions {
            show_line_numbers: false,
            ..Default::default()
        };

        let lines = highlighter.highlight(code, "rust", &options);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_highlight_unknown_language() {
        let highlighter = SyntaxHighlighter::new();
        let code = "some unknown code";
        let options = HighlightOptions::default();

        let lines = highlighter.highlight(code, "unknown", &options);
        // Should fall back to plain text
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_theme_switching() {
        let mut highlighter = SyntaxHighlighter::new();
        highlighter.set_theme(HighlightTheme::Light);
        assert_eq!(
            highlighter.theme_manager.current_theme(),
            HighlightTheme::Light
        );
    }

    #[test]
    fn test_multiple_languages() {
        let highlighter = SyntaxHighlighter::new();
        let options = HighlightOptions::default();

        let rust_code = "fn main() {}";
        let python_code = "def main(): pass";
        let js_code = "function main() {}";

        let rust_lines = highlighter.highlight(rust_code, "rust", &options);
        let python_lines = highlighter.highlight(python_code, "python", &options);
        let js_lines = highlighter.highlight(js_code, "javascript", &options);

        assert_eq!(rust_lines.len(), 1);
        assert_eq!(python_lines.len(), 1);
        assert_eq!(js_lines.len(), 1);
    }
}
