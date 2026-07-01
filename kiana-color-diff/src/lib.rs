use serde::{Deserialize, Serialize};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub lines: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntaxTheme {
    pub theme: String,
    pub source: Option<String>,
}

pub struct ColorDiff {
    hunk: Hunk,
    file_path: String,
}

impl ColorDiff {
    pub fn new(hunk: Hunk, _first_line: Option<String>, file_path: String) -> Self {
        Self { hunk, file_path }
    }

    pub fn render(&self, theme_name: &str, _width: usize, _dim: bool) -> Option<Vec<String>> {
        let ps = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();

        let theme = if theme_name.contains("dark") {
            &ts.themes["base16-mocha.dark"]
        } else {
            &ts.themes["InspiredGitHub"]
        };

        let syntax = ps
            .find_syntax_by_extension(
                std::path::Path::new(&self.file_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("txt"),
            )
            .unwrap_or_else(|| ps.find_syntax_plain_text());

        let mut output = Vec::new();
        let max_line_num = self.hunk.old_start.max(self.hunk.new_start)
            + self.hunk.old_lines.max(self.hunk.new_lines);
        let digits = format!("{}", max_line_num).len();

        let mut old_line = self.hunk.old_start;
        let mut new_line = self.hunk.new_start;

        for line in &self.hunk.lines {
            let marker = line.chars().next().unwrap_or(' ');
            let code = &line[1..];

            let line_num = match marker {
                '+' => {
                    let n = new_line;
                    new_line += 1;
                    n
                }
                '-' => {
                    let n = old_line;
                    old_line += 1;
                    n
                }
                _ => {
                    let n = new_line;
                    old_line += 1;
                    new_line += 1;
                    n
                }
            };

            let mut h = HighlightLines::new(syntax, theme);
            let highlighted = if marker == '-' {
                format!("\x1b[2m{}\x1b[22m", code)
            } else {
                let ranges = h.highlight_line(code, &ps).ok()?;
                as_24_bit_terminal_escaped(&ranges[..], false)
            };

            let prefix = match marker {
                '+' => "\x1b[32m+\x1b[0m",
                '-' => "\x1b[31m-\x1b[0m",
                _ => " ",
            };

            let rendered = format!(
                "{} {:>width$} {}",
                prefix,
                line_num,
                highlighted,
                width = digits
            );
            output.push(rendered);
        }

        Some(output)
    }
}

pub struct ColorFile {
    code: String,
    file_path: String,
}

impl ColorFile {
    pub fn new(code: String, file_path: String) -> Self {
        Self { code, file_path }
    }

    pub fn render(&self, theme_name: &str, _width: usize, _dim: bool) -> Option<Vec<String>> {
        let ps = SyntaxSet::load_defaults_newlines();
        let ts = ThemeSet::load_defaults();

        let theme = if theme_name.contains("dark") {
            &ts.themes["base16-mocha.dark"]
        } else {
            &ts.themes["InspiredGitHub"]
        };

        let syntax = ps
            .find_syntax_by_extension(
                std::path::Path::new(&self.file_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("txt"),
            )
            .unwrap_or_else(|| ps.find_syntax_plain_text());

        let lines: Vec<&str> = self.code.lines().collect();
        let digits = format!("{}", lines.len()).len();
        let mut output = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            let mut h = HighlightLines::new(syntax, theme);
            let ranges = h.highlight_line(line, &ps).ok()?;
            let highlighted = as_24_bit_terminal_escaped(&ranges[..], true);
            let rendered = format!(" {:>width$} {}", i + 1, highlighted, width = digits);
            output.push(rendered);
        }

        Some(output)
    }
}

pub fn get_syntax_theme(theme_name: &str) -> SyntaxTheme {
    let theme = if theme_name.contains("dark") {
        "base16-mocha.dark"
    } else {
        "InspiredGitHub"
    };

    SyntaxTheme {
        theme: theme.to_string(),
        source: std::env::var("BAT_THEME").ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_file_render() {
        let code = "fn main() {\n    println!(\"Hello\");\n}".to_string();
        let file = ColorFile::new(code, "test.rs".to_string());
        let output = file.render("dark", 80, false);
        assert!(output.is_some());
        let lines = output.unwrap();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_color_diff_render() {
        let hunk = Hunk {
            old_start: 1,
            old_lines: 1,
            new_start: 1,
            new_lines: 1,
            lines: vec!["-old line".to_string(), "+new line".to_string()],
        };
        let diff = ColorDiff::new(hunk, None, "test.txt".to_string());
        let output = diff.render("dark", 80, false);
        assert!(output.is_some());
    }

    #[test]
    fn test_get_syntax_theme() {
        let theme = get_syntax_theme("dark");
        assert_eq!(theme.theme, "base16-mocha.dark");

        let theme = get_syntax_theme("light");
        assert_eq!(theme.theme, "InspiredGitHub");
    }
}
