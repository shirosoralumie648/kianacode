use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Renders markdown text into styled ratatui Lines
/// Supports: code blocks, **bold**, *italic*, and lists
pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code_block = false;
    let mut code_lang = String::new();

    for line in text.lines() {
        // Check for code block markers
        if line.trim_start().starts_with("```") {
            if in_code_block {
                // End of code block
                in_code_block = false;
                code_lang.clear();
            } else {
                // Start of code block
                in_code_block = true;
                code_lang = line.trim_start().trim_start_matches('`').trim().to_string();
            }
            continue;
        }

        if in_code_block {
            // Render code with highlighting
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    line.to_string(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::DIM),
                ),
            ]));
        } else {
            // Parse inline markdown
            lines.push(parse_inline_markdown(line));
        }
    }

    lines
}

/// Parse inline markdown: **bold**, *italic*, and lists
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

    // Parse bold and italic
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    let mut current_text = String::new();
    let current_style = Style::default();

    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            // Found **
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }

            // Find closing **
            if let Some(end) = find_closing_delimiter(&chars[i + 2..], "**") {
                let bold_text: String = chars[i + 2..i + 2 + end].iter().collect();
                spans.push(Span::styled(
                    bold_text,
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                i += end + 4; // Skip past closing **
                continue;
            }
        } else if chars[i] == '*' {
            // Found single *
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text.clone(), current_style));
                current_text.clear();
            }

            // Find closing *
            if let Some(end) = find_closing_delimiter(&chars[i + 1..], "*") {
                let italic_text: String = chars[i + 1..i + 1 + end].iter().collect();
                spans.push(Span::styled(
                    italic_text,
                    Style::default().add_modifier(Modifier::ITALIC),
                ));
                i += end + 2; // Skip past closing *
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
    }

    #[test]
    fn test_italic() {
        let text = "This is *italic* text";
        let lines = render_markdown(text);
        assert_eq!(lines.len(), 1);
    }
}
