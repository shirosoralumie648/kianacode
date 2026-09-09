use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::syntax::{HighlightOptions, HighlightTheme, SyntaxHighlighter};

#[derive(Debug, Clone, PartialEq)]
enum BlockType {
    Normal,
    CodeBlock(String), // language
    Table,
    Blockquote(usize), // nesting level
}

/// Renders markdown text into styled ratatui Lines
/// Supports: headers, code blocks, **bold**, *italic*, `inline code`, links, lists, tables, blockquotes
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    render_markdown_with_theme(text, HighlightTheme::Dark)
}

/// Renders markdown text with a specific theme
pub fn render_markdown_with_theme(text: &str, theme: HighlightTheme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut block_type = BlockType::Normal;
    let mut table_rows: Vec<String> = Vec::new();
    let mut in_table = false;
    let mut code_buffer = String::new();
    let mut code_lang = String::new();

    // Lazy initialize highlighter
    let mut highlighter: Option<SyntaxHighlighter> = None;

    for line in text.lines() {
        // Handle code blocks
        if line.trim_start().starts_with("```") {
            match block_type {
                BlockType::CodeBlock(_) => {
                    // End of code block - highlight accumulated code
                    if !code_buffer.is_empty() {
                        if highlighter.is_none() {
                            let mut hl = SyntaxHighlighter::new();
                            hl.set_theme(theme);
                            highlighter = Some(hl);
                        }

                        if let Some(ref hl) = highlighter {
                            let options = HighlightOptions {
                                show_line_numbers: false,
                                theme,
                                ..Default::default()
                            };

                            let highlighted = hl.highlight(&code_buffer, &code_lang, &options);
                            for highlighted_line in highlighted {
                                let mut spans = vec![Span::raw("  ")];
                                spans.extend(highlighted_line.spans);
                                lines.push(Line::from(spans));
                            }
                        }

                        code_buffer.clear();
                    }
                    block_type = BlockType::Normal;
                }
                _ => {
                    // Start of code block
                    let lang = line.trim_start().trim_start_matches('`').trim().to_string();
                    code_lang = lang.clone();
                    code_buffer.clear();
                    block_type = BlockType::CodeBlock(lang);
                }
            }
            continue;
        }

        match &block_type {
            BlockType::CodeBlock(_) => {
                // Accumulate code lines
                if !code_buffer.is_empty() {
                    code_buffer.push('\n');
                }
                code_buffer.push_str(line);
            }
            _ => {
                // Check if entering/continuing table
                if is_table_row(line) {
                    if !in_table {
                        in_table = true;
                        table_rows.clear();
                    }
                    table_rows.push(line.to_string());
                } else {
                    // If we were in a table, render it now
                    if in_table {
                        lines.extend(render_table(&table_rows));
                        table_rows.clear();
                        in_table = false;
                    }

                    // Parse block-level elements
                    if let Some(rendered) = parse_block_element(line) {
                        lines.push(rendered);
                    }
                }
            }
        }
    }

    // Handle remaining table
    if in_table {
        lines.extend(render_table(&table_rows));
    }

    // Handle unclosed code block
    if let BlockType::CodeBlock(_) = block_type {
        if !code_buffer.is_empty() {
            if highlighter.is_none() {
                let mut hl = SyntaxHighlighter::new();
                hl.set_theme(theme);
                highlighter = Some(hl);
            }

            if let Some(ref hl) = highlighter {
                let options = HighlightOptions {
                    show_line_numbers: false,
                    theme,
                    ..Default::default()
                };

                let highlighted = hl.highlight(&code_buffer, &code_lang, &options);
                for highlighted_line in highlighted {
                    let mut spans = vec![Span::raw("  ")];
                    spans.extend(highlighted_line.spans);
                    lines.push(Line::from(spans));
                }
            }
        }
    }

    lines
}

/// Parse block-level elements (headers, blockquotes, lists, normal text)
fn parse_block_element(line: &str) -> Option<Line<'static>> {
    let trimmed = line.trim_start();

    // Headers
    if trimmed.starts_with('#') {
        return Some(parse_header(line));
    }

    // Blockquotes
    if trimmed.starts_with('>') {
        return Some(parse_blockquote(line));
    }

    // Normal text with inline markdown
    Some(parse_inline_markdown(line))
}

/// Parse headers (H1-H6)
fn parse_header(line: &str) -> Line<'static> {
    let trimmed = line.trim_start();
    let mut level = 0;

    for ch in trimmed.chars() {
        if ch == '#' && level < 6 {
            level += 1;
        } else {
            break;
        }
    }

    if level == 0 {
        return parse_inline_markdown(line);
    }

    let content = trimmed[level..].trim_start();
    let (style, prefix) = match level {
        1 => (
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            "█ ",
        ),
        2 => (
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            "▓ ",
        ),
        3 => (
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            "▒ ",
        ),
        4 => (
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
            "░ ",
        ),
        5 => (
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
            "· ",
        ),
        _ => (
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD | Modifier::DIM),
            "  ",
        ),
    };

    let mut spans = vec![Span::styled(prefix.to_string(), style)];
    spans.extend(parse_inline_markdown(content).spans);

    Line::from(
        spans
            .into_iter()
            .map(|span| {
                if span.style == Style::default() {
                    Span::styled(span.content, style)
                } else {
                    // Merge with header style
                    Span::styled(span.content, span.style.patch(style))
                }
            })
            .collect::<Vec<_>>(),
    )
}

/// Parse blockquotes
fn parse_blockquote(line: &str) -> Line<'static> {
    let trimmed = line.trim_start();
    let mut level: usize = 0;
    let mut chars = trimmed.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if ch == '>' {
            level += 1;
            chars.next();
            // Skip optional space after >
            if chars.peek() == Some(&' ') {
                chars.next();
            }
        } else {
            break;
        }
    }

    let content: String = chars.collect();
    let indent = "  ".repeat(level.saturating_sub(1));
    let bar = "│ ";

    let mut spans = vec![
        Span::raw(indent),
        Span::styled(bar.to_string(), Style::default().fg(Color::Gray)),
    ];

    // Apply italic and gray to blockquote content
    let content_spans = parse_inline_markdown(&content).spans;
    for span in content_spans {
        spans.push(Span::styled(
            span.content,
            span.style.fg(Color::Gray).add_modifier(Modifier::ITALIC),
        ));
    }

    Line::from(spans)
}

/// Parse inline markdown: **bold**, *italic*, `code`, [links](url), and lists
fn parse_inline_markdown(line: &str) -> Line<'static> {
    let mut spans = Vec::new();

    // Handle list items
    let (_indent, content) = if line.trim_start().starts_with("- ")
        || line.trim_start().starts_with("* ")
    {
        let indent_count = line.len() - line.trim_start().len();
        let indent = " ".repeat(indent_count);
        spans.push(Span::raw(indent));
        spans.push(Span::styled("• ", Style::default().fg(Color::Cyan)));
        (indent_count + 2, &line.trim_start()[2..])
    } else if let Some(stripped) = line.trim_start().strip_prefix(|c: char| c.is_ascii_digit()) {
        if stripped.starts_with(". ") {
            let indent_count = line.len() - line.trim_start().len();
            let indent = " ".repeat(indent_count);
            let num_part = &line.trim_start()[..line.trim_start().len() - stripped.len()];
            spans.push(Span::raw(indent));
            spans.push(Span::styled(
                format!("{}. ", num_part),
                Style::default().fg(Color::Cyan),
            ));
            (indent_count + num_part.len() + 2, &stripped[2..])
        } else {
            (0, line)
        }
    } else {
        (0, line)
    };

    // Parse inline elements: bold, italic, code, links
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    let mut current_text = String::new();
    let current_style = Style::default();

    while i < chars.len() {
        // Check for links [text](url)
        if chars[i] == '[' {
            if let Some((link_text, url, end_pos)) = parse_link(&chars[i..]) {
                if !current_text.is_empty() {
                    spans.push(Span::styled(current_text.clone(), current_style));
                    current_text.clear();
                }
                // Display as: text (url)
                spans.push(Span::styled(
                    link_text,
                    Style::default()
                        .fg(Color::Blue)
                        .add_modifier(Modifier::UNDERLINED),
                ));
                spans.push(Span::styled(
                    format!(" ({})", url),
                    Style::default().fg(Color::DarkGray),
                ));
                i += end_pos;
                continue;
            }
        }

        // Check for inline code `code`
        if chars[i] == '`' {
            if let Some(end) = find_closing_delimiter(&chars[i + 1..], "`") {
                if !current_text.is_empty() {
                    spans.push(Span::styled(current_text.clone(), current_style));
                    current_text.clear();
                }
                let code_text: String = chars[i + 1..i + 1 + end].iter().collect();
                spans.push(Span::styled(
                    code_text,
                    Style::default().fg(Color::Green).bg(Color::Rgb(40, 40, 40)),
                ));
                i += end + 2; // Skip past closing `
                continue;
            }
        }

        // Check for bold **text**
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }

            if let Some(end) = find_closing_delimiter(&chars[i + 2..], "**") {
                let bold_text: String = chars[i + 2..i + 2 + end].iter().collect();
                spans.push(Span::styled(
                    bold_text,
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                i += end + 4;
                continue;
            }
        }
        // Check for italic *text*
        else if chars[i] == '*' {
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }

            if let Some(end) = find_closing_delimiter(&chars[i + 1..], "*") {
                let italic_text: String = chars[i + 1..i + 1 + end].iter().collect();
                spans.push(Span::styled(
                    italic_text,
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
                i += end + 2;
                continue;
            }
        }

        current_text.push(chars[i]);
        i += 1;
    }

    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    if spans.is_empty() {
        spans.push(Span::raw(""));
    }

    Line::from(spans)
}

/// Parse link syntax [text](url)
/// Returns (link_text, url, total_chars_consumed)
fn parse_link(chars: &[char]) -> Option<(String, String, usize)> {
    if chars.is_empty() || chars[0] != '[' {
        return None;
    }

    // Find closing ]
    let text_end = find_closing_delimiter(&chars[1..], "]")?;
    let link_text: String = chars[1..1 + text_end].iter().collect();

    // Check for (
    let paren_start = 1 + text_end + 1;
    if paren_start >= chars.len() || chars[paren_start] != '(' {
        return None;
    }

    // Find closing )
    let url_end = find_closing_delimiter(&chars[paren_start + 1..], ")")?;
    let url: String = chars[paren_start + 1..paren_start + 1 + url_end]
        .iter()
        .collect();

    let total = paren_start + 1 + url_end + 1;
    Some((link_text, url, total))
}

fn find_closing_delimiter(chars: &[char], delimiter: &str) -> Option<usize> {
    let delim_chars: Vec<char> = delimiter.chars().collect();
    let delim_len = delim_chars.len();

    for i in 0..chars.len() {
        if i + delim_len <= chars.len() {
            let slice: String = chars[i..i + delim_len].iter().collect();
            if slice == delimiter {
                return Some(i);
            }
        }
    }
    None
}

/// Check if a line is part of a table
fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    // Table rows should start and end with |, or contain multiple |
    trimmed.starts_with('|') || trimmed.contains('|')
}

/// Render a table from collected rows
fn render_table(rows: &[String]) -> Vec<Line<'static>> {
    if rows.is_empty() {
        return vec![];
    }

    let mut result = Vec::new();

    // Parse all rows into cells
    let parsed_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            row.trim()
                .trim_start_matches('|')
                .trim_end_matches('|')
                .split('|')
                .map(|cell| cell.trim().to_string())
                .collect()
        })
        .collect();

    if parsed_rows.is_empty() {
        return result;
    }

    // Determine column widths
    let mut col_widths = vec![0; parsed_rows[0].len()];
    for row in &parsed_rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_widths.len() {
                col_widths[i] = col_widths[i].max(cell.len());
            }
        }
    }

    // Check if second row is separator
    let has_separator = rows.len() > 1
        && rows[1]
            .trim()
            .chars()
            .all(|c| c == '|' || c == '-' || c == ':' || c.is_whitespace());

    // Render table with borders
    let mut row_iter = parsed_rows.iter().enumerate().peekable();

    // Top border
    result.push(render_table_border(&col_widths, '┌', '┬', '┐'));

    while let Some((idx, row)) = row_iter.next() {
        // Skip separator row
        if idx == 1 && has_separator {
            // Middle separator
            result.push(render_table_border(&col_widths, '├', '┼', '┤'));
            continue;
        }

        // Render data row
        result.push(render_table_row(
            row,
            &col_widths,
            idx == 0 && has_separator,
        ));
    }

    // Bottom border
    result.push(render_table_border(&col_widths, '└', '┴', '┘'));

    result
}

/// Render table border line
fn render_table_border(col_widths: &[usize], left: char, mid: char, right: char) -> Line<'static> {
    let mut spans = vec![Span::styled(
        left.to_string(),
        Style::default().fg(Color::Gray),
    )];

    for (i, &width) in col_widths.iter().enumerate() {
        spans.push(Span::styled(
            "─".repeat(width + 2),
            Style::default().fg(Color::Gray),
        ));
        if i < col_widths.len() - 1 {
            spans.push(Span::styled(
                mid.to_string(),
                Style::default().fg(Color::Gray),
            ));
        }
    }

    spans.push(Span::styled(
        right.to_string(),
        Style::default().fg(Color::Gray),
    ));

    Line::from(spans)
}

/// Render table data row
fn render_table_row(cells: &[String], col_widths: &[usize], is_header: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        "│ ".to_string(),
        Style::default().fg(Color::Gray),
    )];

    for (i, cell) in cells.iter().enumerate() {
        let width = if i < col_widths.len() {
            col_widths[i]
        } else {
            cell.len()
        };

        let padded = format!("{:width$}", cell, width = width);
        let style = if is_header {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        spans.push(Span::styled(padded, style));

        if i < cells.len() - 1 {
            spans.push(Span::styled(
                " │ ".to_string(),
                Style::default().fg(Color::Gray),
            ));
        } else {
            spans.push(Span::styled(
                " │".to_string(),
                Style::default().fg(Color::Gray),
            ));
        }
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_code_block() {
        let text = "```rust\nfn main() {}\n```";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_bold() {
        let text = "This is **bold** text";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 1);
        // Check that we have at least 3 spans (text, bold, text)
        assert!(lines[0].spans.len() >= 2);
    }

    #[test]
    fn test_italic() {
        let text = "This is *italic* text";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.len() >= 2);
    }

    #[test]
    fn test_inline_code() {
        let text = "Use `cargo build` to compile";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 1);
        // Should have spans for text before, code, and text after
        assert!(lines[0].spans.len() >= 3);
    }

    #[test]
    fn test_headers() {
        let text = "# Header 1\n## Header 2\n### Header 3";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_link() {
        let text = "Check [documentation](https://example.com) for details";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 1);
        // Should have text, link, url, and more text
        assert!(lines[0].spans.len() >= 3);
    }

    #[test]
    fn test_blockquote() {
        let text = "> This is a quote\n> Second line";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_table() {
        let text = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n| Bob | 25 |";
        let lines = render_markdown(text);
        // Should have: top border, header, separator, 2 data rows, bottom border = 6 lines
        assert!(lines.len() >= 5);
    }

    #[test]
    fn test_unordered_list() {
        let text = "- Item 1\n- Item 2\n* Item 3";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_ordered_list() {
        let text = "1. First\n2. Second\n3. Third";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_mixed_markdown() {
        let text =
            "# Title\n\nThis is **bold** and *italic* with `code`.\n\n- List item\n- Another item";
        let lines = render_markdown(text);
        assert!(lines.len() >= 4);
    }

    #[test]
    fn test_nested_blockquote() {
        let text = "> Level 1\n>> Level 2";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn test_parse_link() {
        let text = "[GitHub](https://github.com)";
        let chars: Vec<char> = text.chars().collect();
        let result = parse_link(&chars);
        assert!(result.is_some());
        let (link_text, url, len) = result.unwrap();
        assert_eq!(link_text, "GitHub");
        assert_eq!(url, "https://github.com");
        assert_eq!(len, text.len());
    }

    #[test]
    fn test_is_table_row() {
        assert!(is_table_row("| A | B |"));
        assert!(is_table_row("|---|---|"));
        assert!(is_table_row("A | B"));
        assert!(!is_table_row(""));
        assert!(!is_table_row("Normal text"));
    }
}
