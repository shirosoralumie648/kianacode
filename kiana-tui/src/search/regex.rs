use regex::{Regex, RegexBuilder};
use std::fmt;

/// 正则表达式搜索错误类型
#[derive(Debug, Clone)]
pub struct RegexError {
    pub message: String,
    pub position: Option<usize>,
    pub suggestion: Option<String>,
}

impl fmt::Display for RegexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "正则表达式错误: {}", self.message)?;
        if let Some(pos) = self.position {
            write!(f, " (位置: {})", pos)?;
        }
        if let Some(ref suggestion) = self.suggestion {
            write!(f, "\n建议: {}", suggestion)?;
        }
        Ok(())
    }
}

impl std::error::Error for RegexError {}

/// 捕获组信息
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    /// 捕获组名称（命名捕获组）
    pub name: Option<String>,
    /// 捕获组索引
    pub index: usize,
    /// 捕获的文本
    pub text: String,
    /// 在原文本中的起始位置
    pub start: usize,
    /// 在原文本中的结束位置
    pub end: usize,
}

/// 正则匹配结果
#[derive(Debug, Clone, PartialEq)]
pub struct RegexMatch {
    /// 匹配的完整文本
    pub text: String,
    /// 匹配的起始位置
    pub start: usize,
    /// 匹配的结束位置
    pub end: usize,
    /// 匹配所在的行号（0-based）
    pub line: usize,
    /// 匹配所在行的列号（0-based）
    pub column: usize,
    /// 捕获组（包括完整匹配 index=0）
    pub captures: Vec<Capture>,
}

/// 正则表达式搜索引擎
#[derive(Debug)]
pub struct RegexSearch {
    /// 原始模式字符串
    pattern: String,
    /// 编译后的正则表达式
    regex: Option<Regex>,
    /// 是否区分大小写
    case_sensitive: bool,
    /// 是否多行模式
    multiline: bool,
    /// 是否启用 dot-matches-newline
    dot_matches_newline: bool,
    /// 编译错误（如果有）
    error: Option<RegexError>,
}

impl RegexSearch {
    /// 创建新的正则搜索实例
    pub fn new(pattern: &str) -> Result<Self, RegexError> {
        let mut search = Self {
            pattern: pattern.to_string(),
            regex: None,
            case_sensitive: true,
            multiline: false,
            dot_matches_newline: false,
            error: None,
        };
        search.compile()?;
        Ok(search)
    }

    /// 设置是否区分大小写
    pub fn case_sensitive(mut self, enabled: bool) -> Result<Self, RegexError> {
        self.case_sensitive = enabled;
        self.compile()?;
        Ok(self)
    }

    /// 设置多行模式（^ 和 $ 匹配行边界）
    pub fn multiline(mut self, enabled: bool) -> Result<Self, RegexError> {
        self.multiline = enabled;
        self.compile()?;
        Ok(self)
    }

    /// 设置 dot-matches-newline 模式（. 匹配换行符）
    pub fn dot_matches_newline(mut self, enabled: bool) -> Result<Self, RegexError> {
        self.dot_matches_newline = enabled;
        self.compile()?;
        Ok(self)
    }

    /// 编译正则表达式
    fn compile(&mut self) -> Result<(), RegexError> {
        let result = RegexBuilder::new(&self.pattern)
            .case_insensitive(!self.case_sensitive)
            .multi_line(self.multiline)
            .dot_matches_new_line(self.dot_matches_newline)
            .build();

        match result {
            Ok(regex) => {
                self.regex = Some(regex);
                self.error = None;
                Ok(())
            }
            Err(e) => {
                let error = Self::parse_regex_error(&e);
                self.error = Some(error.clone());
                Err(error)
            }
        }
    }

    /// 解析正则表达式错误并提供友好的错误消息
    fn parse_regex_error(error: &regex::Error) -> RegexError {
        let message = error.to_string();
        let mut reg_error = RegexError {
            message: message.clone(),
            position: None,
            suggestion: None,
        };

        // 尝试提供有用的建议
        if message.contains("unclosed") || message.contains("unterminated") {
            reg_error.suggestion = Some("检查括号、方括号或花括号是否正确闭合".to_string());
        } else if message.contains("invalid escape") {
            reg_error.suggestion =
                Some("检查转义序列是否有效，常见的有 \\d \\w \\s \\n \\t 等".to_string());
        } else if message.contains("look-around") {
            reg_error.suggestion = Some("Rust regex 不支持 look-around assertions".to_string());
        } else if message.contains("backreference") {
            reg_error.suggestion = Some("Rust regex 不支持反向引用，考虑使用捕获组".to_string());
        }

        reg_error
    }

    /// 在文本中查找第一个匹配
    pub fn find(&self, text: &str) -> Option<RegexMatch> {
        let regex = self.regex.as_ref()?;
        let captures = regex.captures(text)?;

        Some(Self::build_match(text, captures, 0))
    }

    /// 在文本中查找所有匹配
    pub fn search(&self, text: &str) -> Vec<RegexMatch> {
        let regex = match &self.regex {
            Some(r) => r,
            None => return vec![],
        };

        regex
            .captures_iter(text)
            .enumerate()
            .map(|(idx, captures)| Self::build_match(text, captures, idx))
            .collect()
    }

    /// 按行搜索文本
    pub fn search_lines(&self, lines: &[&str]) -> Vec<RegexMatch> {
        let regex = match &self.regex {
            Some(r) => r,
            None => return vec![],
        };

        let mut results = Vec::new();
        let mut byte_offset = 0;

        for (line_idx, line) in lines.iter().enumerate() {
            for captures in regex.captures_iter(line) {
                let mut match_result = Self::build_match(line, captures, 0);
                match_result.line = line_idx;
                match_result.start += byte_offset;
                match_result.end += byte_offset;

                // 更新捕获组的绝对位置
                for capture in &mut match_result.captures {
                    capture.start += byte_offset;
                    capture.end += byte_offset;
                }

                results.push(match_result);
            }
            byte_offset += line.len() + 1; // +1 for newline
        }

        results
    }

    /// 构建匹配结果
    fn build_match(text: &str, captures: regex::Captures, _index: usize) -> RegexMatch {
        let full_match = captures.get(0).unwrap();
        let start = full_match.start();
        let end = full_match.end();
        let matched_text = full_match.as_str().to_string();

        // 计算行号和列号
        let before_match = &text[..start];
        let line = before_match.chars().filter(|&c| c == '\n').count();
        let column = before_match
            .lines()
            .last()
            .map(|l| l.len())
            .unwrap_or(start);

        // 提取所有捕获组
        let mut capture_list = Vec::new();

        for (idx, cap) in captures.iter().enumerate() {
            if let Some(m) = cap {
                let name = captures
                    .iter()
                    .enumerate()
                    .find(|(i, _)| *i == idx)
                    .and_then(|(_, _)| {
                        // 尝试获取命名捕获组的名称
                        // 注意：regex crate 不直接提供按索引获取名称的方法
                        // 这里我们先返回 None，命名捕获组会在后续优化中实现
                        None
                    });

                capture_list.push(Capture {
                    name,
                    index: idx,
                    text: m.as_str().to_string(),
                    start: m.start(),
                    end: m.end(),
                });
            }
        }

        RegexMatch {
            text: matched_text,
            start,
            end,
            line,
            column,
            captures: capture_list,
        }
    }

    /// 替换第一个匹配
    pub fn replace(&self, text: &str, replacement: &str) -> String {
        match &self.regex {
            Some(regex) => regex.replace(text, replacement).to_string(),
            None => text.to_string(),
        }
    }

    /// 替换所有匹配
    pub fn replace_all(&self, text: &str, replacement: &str) -> String {
        match &self.regex {
            Some(regex) => regex.replace_all(text, replacement).to_string(),
            None => text.to_string(),
        }
    }

    /// 获取模式字符串
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// 检查是否有编译错误
    pub fn has_error(&self) -> bool {
        self.error.is_some()
    }

    /// 获取编译错误
    pub fn error(&self) -> Option<&RegexError> {
        self.error.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_search() {
        let search = RegexSearch::new(r"\d+").unwrap();
        let matches = search.search("There are 123 apples and 456 oranges");

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].text, "123");
        assert_eq!(matches[1].text, "456");
    }

    #[test]
    fn test_case_sensitive() {
        let search = RegexSearch::new(r"hello").unwrap();
        let matches = search.search("Hello HELLO hello");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].text, "hello");
    }

    #[test]
    fn test_case_insensitive() {
        let search = RegexSearch::new(r"hello")
            .unwrap()
            .case_sensitive(false)
            .unwrap();
        let matches = search.search("Hello HELLO hello");
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_capture_groups() {
        let search = RegexSearch::new(r"(\d{3})-(\d{4})").unwrap();
        let result = search.find("Call me at 555-1234");

        assert!(result.is_some());
        let match_result = result.unwrap();
        assert_eq!(match_result.text, "555-1234");
        assert_eq!(match_result.captures.len(), 3); // 完整匹配 + 2个捕获组
        assert_eq!(match_result.captures[1].text, "555");
        assert_eq!(match_result.captures[2].text, "1234");
    }

    #[test]
    fn test_multiline() {
        let text = "line 1\nline 2\nline 3";
        let search = RegexSearch::new(r"^line \d")
            .unwrap()
            .multiline(true)
            .unwrap();

        let matches = search.search(text);
        assert_eq!(matches.len(), 3);
    }

    #[test]
    fn test_dot_matches_newline() {
        let text = "start\nmiddle\nend";
        let search = RegexSearch::new(r"start.+end")
            .unwrap()
            .dot_matches_newline(true)
            .unwrap();

        let matches = search.search(text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].text, text);
    }

    #[test]
    fn test_find_first() {
        let search = RegexSearch::new(r"\w+").unwrap();
        let result = search.find("hello world");

        assert!(result.is_some());
        let match_result = result.unwrap();
        assert_eq!(match_result.text, "hello");
    }

    #[test]
    fn test_replace() {
        let search = RegexSearch::new(r"\d+").unwrap();
        let result = search.replace("I have 5 apples", "ten");
        assert_eq!(result, "I have ten apples");
    }

    #[test]
    fn test_replace_all() {
        let search = RegexSearch::new(r"\d+").unwrap();
        let result = search.replace_all("I have 5 apples and 3 oranges", "X");
        assert_eq!(result, "I have X apples and X oranges");
    }

    #[test]
    fn test_invalid_regex() {
        let result = RegexSearch::new(r"[unclosed");
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.message.contains("unclosed") || error.message.contains("unterminated"));
        assert!(error.suggestion.is_some());
    }

    #[test]
    fn test_search_lines() {
        let lines = vec!["line one with 123", "line two with 456", "line three"];
        let search = RegexSearch::new(r"\d+").unwrap();
        let matches = search.search_lines(&lines);

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 0);
        assert_eq!(matches[0].text, "123");
        assert_eq!(matches[1].line, 1);
        assert_eq!(matches[1].text, "456");
    }

    #[test]
    fn test_line_and_column() {
        let text = "first line\nsecond line has match\nthird line";
        let search = RegexSearch::new(r"match").unwrap();
        let result = search.find(text);

        assert!(result.is_some());
        let match_result = result.unwrap();
        assert_eq!(match_result.line, 1); // 第二行（0-based）
        assert!(match_result.column > 0);
    }

    #[test]
    fn test_word_boundaries() {
        let search = RegexSearch::new(r"\btest\b").unwrap();
        let text = "test testing tested test";
        let matches = search.search(text);

        assert_eq!(matches.len(), 2); // 只匹配独立的 "test"
        assert_eq!(matches[0].text, "test");
        assert_eq!(matches[1].text, "test");
    }

    #[test]
    fn test_anchors() {
        let search = RegexSearch::new(r"^start").unwrap();
        assert!(search.find("start of line").is_some());
        assert!(search.find("not at start").is_none());

        let search = RegexSearch::new(r"end$").unwrap();
        assert!(search.find("at the end").is_some());
        assert!(search.find("end not there").is_none());
    }

    #[test]
    fn test_empty_pattern() {
        let result = RegexSearch::new("");
        assert!(result.is_ok());
    }

    #[test]
    fn test_unicode() {
        let search = RegexSearch::new(r"[一-龥]+").unwrap();
        let matches = search.search("Hello 世界 World 你好");

        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].text, "世界");
        assert_eq!(matches[1].text, "你好");
    }
}
