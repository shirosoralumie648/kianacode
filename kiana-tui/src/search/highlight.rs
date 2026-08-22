use super::regex::RegexMatch;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// 高亮配置
#[derive(Debug, Clone)]
pub struct HighlightConfig {
    /// 匹配文本的样式
    pub match_style: Style,
    /// 捕获组的样式
    pub capture_style: Style,
    /// 普通文本的样式
    pub normal_style: Style,
    /// 显示上下文行数
    pub context_lines: usize,
}

impl Default for HighlightConfig {
    fn default() -> Self {
        Self {
            match_style: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            capture_style: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            normal_style: Style::default(),
            context_lines: 2,
        }
    }
}

/// 高亮的文本片段
#[derive(Debug, Clone)]
pub struct HighlightedSpan {
    pub text: String,
    pub style: Style,
    pub is_match: bool,
    pub is_capture: bool,
}

/// 高亮的行
#[derive(Debug, Clone)]
pub struct HighlightedLine {
    pub line_number: usize,
    pub spans: Vec<HighlightedSpan>,
    pub is_match_line: bool,
}

/// 正则搜索结果高亮器
pub struct RegexHighlighter {
    config: HighlightConfig,
}

impl RegexHighlighter {
    /// 创建新的高亮器
    pub fn new(config: HighlightConfig) -> Self {
        Self { config }
    }

    /// 使用默认配置创建高亮器
    pub fn default_config() -> Self {
        Self::new(HighlightConfig::default())
    }

    /// 高亮单个匹配在文本中的位置
    pub fn highlight_match(&self, text: &str, regex_match: &RegexMatch) -> Vec<HighlightedSpan> {
        let mut spans = Vec::new();
        let mut last_end = 0;

        // 先高亮完整匹配
        let match_start = regex_match.start;
        let match_end = regex_match.end;

        // 匹配前的文本
        if match_start > last_end {
            spans.push(HighlightedSpan {
                text: text[last_end..match_start].to_string(),
                style: self.config.normal_style,
                is_match: false,
                is_capture: false,
            });
        }

        // 如果有捕获组，高亮捕获组
        if regex_match.captures.len() > 1 {
            // 跳过索引0（完整匹配）
            let mut capture_positions: Vec<_> = regex_match.captures[1..]
                .iter()
                .map(|c| (c.start, c.end, c))
                .collect();
            capture_positions.sort_by_key(|&(start, _, _)| start);

            let mut pos = match_start;
            for (cap_start, cap_end, capture) in capture_positions {
                // 捕获组之前的文本（匹配但不是捕获组）
                if cap_start > pos {
                    spans.push(HighlightedSpan {
                        text: text[pos..cap_start].to_string(),
                        style: self.config.match_style,
                        is_match: true,
                        is_capture: false,
                    });
                }

                // 捕获组文本
                spans.push(HighlightedSpan {
                    text: capture.text.clone(),
                    style: self.config.capture_style,
                    is_match: true,
                    is_capture: true,
                });

                pos = cap_end;
            }

            // 最后一个捕获组之后到匹配结束的文本
            if pos < match_end {
                spans.push(HighlightedSpan {
                    text: text[pos..match_end].to_string(),
                    style: self.config.match_style,
                    is_match: true,
                    is_capture: false,
                });
            }
        } else {
            // 没有捕获组，直接高亮整个匹配
            spans.push(HighlightedSpan {
                text: text[match_start..match_end].to_string(),
                style: self.config.match_style,
                is_match: true,
                is_capture: false,
            });
        }

        last_end = match_end;

        // 匹配后的文本
        if last_end < text.len() {
            spans.push(HighlightedSpan {
                text: text[last_end..].to_string(),
                style: self.config.normal_style,
                is_match: false,
                is_capture: false,
            });
        }

        spans
    }

    /// 高亮多个匹配
    pub fn highlight_matches(&self, text: &str, matches: &[RegexMatch]) -> Vec<HighlightedSpan> {
        if matches.is_empty() {
            return vec![HighlightedSpan {
                text: text.to_string(),
                style: self.config.normal_style,
                is_match: false,
                is_capture: false,
            }];
        }

        let mut spans = Vec::new();
        let mut last_end = 0;

        for regex_match in matches {
            let match_start = regex_match.start;
            let match_end = regex_match.end;

            // 匹配前的文本
            if match_start > last_end {
                spans.push(HighlightedSpan {
                    text: text[last_end..match_start].to_string(),
                    style: self.config.normal_style,
                    is_match: false,
                    is_capture: false,
                });
            }

            // 高亮匹配（包括捕获组）
            let match_spans =
                self.highlight_match_internal(text, regex_match, match_start, match_end);
            spans.extend(match_spans);

            last_end = match_end;
        }

        // 最后一个匹配后的文本
        if last_end < text.len() {
            spans.push(HighlightedSpan {
                text: text[last_end..].to_string(),
                style: self.config.normal_style,
                is_match: false,
                is_capture: false,
            });
        }

        spans
    }

    /// 内部方法：高亮单个匹配（用于 highlight_matches）
    fn highlight_match_internal(
        &self,
        text: &str,
        regex_match: &RegexMatch,
        match_start: usize,
        match_end: usize,
    ) -> Vec<HighlightedSpan> {
        let mut spans = Vec::new();

        // 如果有捕获组，高亮捕获组
        if regex_match.captures.len() > 1 {
            let mut capture_positions: Vec<_> = regex_match.captures[1..]
                .iter()
                .map(|c| (c.start, c.end, c))
                .collect();
            capture_positions.sort_by_key(|&(start, _, _)| start);

            let mut pos = match_start;
            for (cap_start, cap_end, capture) in capture_positions {
                // 捕获组之前的文本
                if cap_start > pos {
                    spans.push(HighlightedSpan {
                        text: text[pos..cap_start].to_string(),
                        style: self.config.match_style,
                        is_match: true,
                        is_capture: false,
                    });
                }

                // 捕获组文本
                spans.push(HighlightedSpan {
                    text: capture.text.clone(),
                    style: self.config.capture_style,
                    is_match: true,
                    is_capture: true,
                });

                pos = cap_end;
            }

            // 最后一个捕获组之后的文本
            if pos < match_end {
                spans.push(HighlightedSpan {
                    text: text[pos..match_end].to_string(),
                    style: self.config.match_style,
                    is_match: true,
                    is_capture: false,
                });
            }
        } else {
            // 没有捕获组，直接高亮整个匹配
            spans.push(HighlightedSpan {
                text: text[match_start..match_end].to_string(),
                style: self.config.match_style,
                is_match: true,
                is_capture: false,
            });
        }

        spans
    }

    /// 高亮文本行（带上下文）
    pub fn highlight_lines(&self, lines: &[&str], matches: &[RegexMatch]) -> Vec<HighlightedLine> {
        let mut result = Vec::new();
        let match_lines: std::collections::HashSet<usize> =
            matches.iter().map(|m| m.line).collect();

        // 计算每行的起始位置
        let mut line_offsets = vec![0];
        for line in lines.iter() {
            let last_offset = *line_offsets.last().unwrap();
            line_offsets.push(last_offset + line.len() + 1); // +1 for newline
        }

        for (line_idx, line) in lines.iter().enumerate() {
            let is_match_line = match_lines.contains(&line_idx);

            // 获取该行的所有匹配，并调整位置为相对于行起始
            let line_offset = line_offsets[line_idx];
            let mut line_matches: Vec<RegexMatch> = matches
                .iter()
                .filter(|m| m.line == line_idx)
                .map(|m| {
                    let mut adjusted = m.clone();
                    adjusted.start = adjusted.start.saturating_sub(line_offset);
                    adjusted.end = adjusted.end.saturating_sub(line_offset);
                    // 调整捕获组位置
                    for cap in &mut adjusted.captures {
                        cap.start = cap.start.saturating_sub(line_offset);
                        cap.end = cap.end.saturating_sub(line_offset);
                    }
                    adjusted
                })
                .collect();

            let spans = if !line_matches.is_empty() {
                self.highlight_matches(line, &line_matches)
            } else {
                vec![HighlightedSpan {
                    text: line.to_string(),
                    style: self.config.normal_style,
                    is_match: false,
                    is_capture: false,
                }]
            };

            result.push(HighlightedLine {
                line_number: line_idx,
                spans,
                is_match_line,
            });
        }

        result
    }

    /// 转换为 ratatui 的 Line
    pub fn to_ratatui_line(&self, highlighted: &HighlightedLine) -> Line<'static> {
        let spans: Vec<Span> = highlighted
            .spans
            .iter()
            .map(|hs| Span::styled(hs.text.clone(), hs.style))
            .collect();

        Line::from(spans)
    }

    /// 转换多个高亮行为 ratatui Lines
    pub fn to_ratatui_lines(&self, highlighted: &[HighlightedLine]) -> Vec<Line<'static>> {
        highlighted
            .iter()
            .map(|hl| self.to_ratatui_line(hl))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::regex::RegexSearch;

    #[test]
    fn test_highlight_simple_match() {
        let highlighter = RegexHighlighter::default_config();
        let search = RegexSearch::new(r"\d+").unwrap();
        let text = "There are 123 apples";
        let matches = search.search(text);

        assert_eq!(matches.len(), 1);
        let spans = highlighter.highlight_match(text, &matches[0]);

        assert_eq!(spans.len(), 3); // before, match, after
        assert_eq!(spans[0].text, "There are ");
        assert_eq!(spans[1].text, "123");
        assert_eq!(spans[2].text, " apples");
        assert!(spans[1].is_match);
    }

    #[test]
    fn test_highlight_with_captures() {
        let highlighter = RegexHighlighter::default_config();
        let search = RegexSearch::new(r"(\d+)-(\d+)").unwrap();
        let text = "Call 555-1234";
        let matches = search.search(text);

        assert_eq!(matches.len(), 1);
        let spans = highlighter.highlight_match(text, &matches[0]);

        // "Call " + "555" (capture) + "-" (match) + "1234" (capture)
        assert!(spans.len() >= 2);
        assert_eq!(spans[0].text, "Call ");
        assert!(!spans[0].is_match);
    }

    #[test]
    fn test_highlight_multiple_matches() {
        let highlighter = RegexHighlighter::default_config();
        let search = RegexSearch::new(r"\d+").unwrap();
        let text = "I have 5 apples and 3 oranges";
        let matches = search.search(text);

        assert_eq!(matches.len(), 2);
        let spans = highlighter.highlight_matches(text, &matches);

        // Should have segments: "I have ", "5", " apples and ", "3", " oranges"
        assert!(spans.len() >= 5);
    }

    #[test]
    fn test_highlight_lines() {
        let highlighter = RegexHighlighter::default_config();
        let search = RegexSearch::new(r"\d+").unwrap();
        let lines = vec!["line one", "line two with 123", "line three"];
        let matches = search.search_lines(&lines);

        let highlighted = highlighter.highlight_lines(&lines, &matches);

        assert_eq!(highlighted.len(), 3);
        assert!(!highlighted[0].is_match_line);
        assert!(highlighted[1].is_match_line);
        assert!(!highlighted[2].is_match_line);
    }
}
