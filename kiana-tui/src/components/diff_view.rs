use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, StatefulWidget, Widget},
};

/// Diff display mode
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DiffMode {
    /// Traditional unified diff format (like git diff)
    Unified,
    /// Side-by-side comparison
    SideBySide,
}

/// A single line in a diff
#[derive(Clone, Debug, PartialEq)]
pub enum DiffLine {
    /// Unchanged context line
    Context {
        content: String,
        old_line: usize,
        new_line: usize,
    },
    /// Line added in new version
    Addition { content: String, new_line: usize },
    /// Line deleted from old version
    Deletion { content: String, old_line: usize },
    /// Line modified (stored as delete + add pair)
    Modified {
        old_content: String,
        new_content: String,
        old_line: usize,
        new_line: usize,
    },
}

impl DiffLine {
    pub fn content(&self) -> &str {
        match self {
            DiffLine::Context { content, .. } => content,
            DiffLine::Addition { content, .. } => content,
            DiffLine::Deletion { content, .. } => content,
            DiffLine::Modified { new_content, .. } => new_content,
        }
    }

    pub fn old_line_number(&self) -> Option<usize> {
        match self {
            DiffLine::Context { old_line, .. } => Some(*old_line),
            DiffLine::Deletion { old_line, .. } => Some(*old_line),
            DiffLine::Modified { old_line, .. } => Some(*old_line),
            DiffLine::Addition { .. } => None,
        }
    }

    pub fn new_line_number(&self) -> Option<usize> {
        match self {
            DiffLine::Context { new_line, .. } => Some(*new_line),
            DiffLine::Addition { new_line, .. } => Some(*new_line),
            DiffLine::Modified { new_line, .. } => Some(*new_line),
            DiffLine::Deletion { .. } => None,
        }
    }
}

/// A hunk represents a contiguous block of changes
#[derive(Clone, Debug)]
pub struct DiffHunk {
    /// Starting line in old file
    pub old_start: usize,
    /// Number of lines in old file
    pub old_count: usize,
    /// Starting line in new file
    pub new_start: usize,
    /// Number of lines in new file
    pub new_count: usize,
    /// Lines in this hunk
    pub lines: Vec<DiffLine>,
    /// Optional hunk header context (like function name)
    pub context: Option<String>,
}

impl DiffHunk {
    pub fn header(&self) -> String {
        let ctx = self
            .context
            .as_ref()
            .map(|c| format!(" {}", c))
            .unwrap_or_default();
        format!(
            "@@ -{},{} +{},{} @@{}",
            self.old_start, self.old_count, self.new_start, self.new_count, ctx
        )
    }

    pub fn additions(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| matches!(l, DiffLine::Addition { .. } | DiffLine::Modified { .. }))
            .count()
    }

    pub fn deletions(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| matches!(l, DiffLine::Deletion { .. } | DiffLine::Modified { .. }))
            .count()
    }
}

/// State for the diff view
#[derive(Clone, Debug)]
pub struct DiffViewState {
    /// All hunks in the diff
    hunks: Vec<DiffHunk>,
    /// Current selected line index (across all hunks)
    current_line: usize,
    /// Current hunk index
    current_hunk: usize,
    /// Vertical scroll offset
    scroll_offset: usize,
    /// Total number of renderable lines (including hunk headers)
    total_lines: usize,
}

impl DiffViewState {
    pub fn new(hunks: Vec<DiffHunk>) -> Self {
        let total_lines = hunks.iter().map(|h| h.lines.len() + 1).sum(); // +1 for header
        Self {
            hunks,
            current_line: 0,
            current_hunk: 0,
            scroll_offset: 0,
            total_lines,
        }
    }

    pub fn hunks(&self) -> &[DiffHunk] {
        &self.hunks
    }

    pub fn current_line(&self) -> usize {
        self.current_line
    }

    pub fn current_hunk(&self) -> usize {
        self.current_hunk
    }

    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        if self.current_line + 1 < self.total_lines {
            self.current_line += 1;
            self.update_current_hunk();
        }
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if self.current_line > 0 {
            self.current_line -= 1;
            self.update_current_hunk();
        }
    }

    /// Jump to next hunk
    pub fn next_hunk(&mut self) {
        if self.current_hunk + 1 < self.hunks.len() {
            self.current_hunk += 1;
            // Jump to the first line of the next hunk
            self.current_line = self.get_hunk_start_line(self.current_hunk);
        }
    }

    /// Jump to previous hunk
    pub fn prev_hunk(&mut self) {
        if self.current_hunk > 0 {
            self.current_hunk -= 1;
            // Jump to the first line of the previous hunk
            self.current_line = self.get_hunk_start_line(self.current_hunk);
        }
    }

    /// Update current hunk based on current line
    fn update_current_hunk(&mut self) {
        let mut line_count = 0;
        for (idx, hunk) in self.hunks.iter().enumerate() {
            let hunk_total = hunk.lines.len() + 1; // +1 for header
            if self.current_line < line_count + hunk_total {
                self.current_hunk = idx;
                return;
            }
            line_count += hunk_total;
        }
    }

    /// Get the starting line index for a hunk
    fn get_hunk_start_line(&self, hunk_idx: usize) -> usize {
        let mut line_count = 0;
        for (idx, hunk) in self.hunks.iter().enumerate() {
            if idx == hunk_idx {
                return line_count;
            }
            line_count += hunk.lines.len() + 1; // +1 for header
        }
        line_count
    }

    /// Set scroll offset to ensure current line is visible
    pub fn ensure_visible(&mut self, viewport_height: usize) {
        if self.current_line < self.scroll_offset {
            self.scroll_offset = self.current_line;
        } else if self.current_line >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.current_line - viewport_height + 1;
        }
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }
}

impl Default for DiffViewState {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Diff view widget
pub struct DiffView {
    mode: DiffMode,
    show_line_numbers: bool,
    context_lines: usize,
    title: Option<String>,
    block: Option<Block<'static>>,
}

impl DiffView {
    pub fn new() -> Self {
        Self {
            mode: DiffMode::Unified,
            show_line_numbers: true,
            context_lines: 3,
            title: None,
            block: None,
        }
    }

    pub fn mode(mut self, mode: DiffMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn show_line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    pub fn context_lines(mut self, lines: usize) -> Self {
        self.context_lines = lines;
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn block(mut self, block: Block<'static>) -> Self {
        self.block = Some(block);
        self
    }

    fn render_unified(&self, area: Rect, buf: &mut Buffer, state: &DiffViewState) {
        let inner = self
            .block
            .as_ref()
            .map(|b| {
                let inner = b.inner(area);
                b.clone().render(area, buf);
                inner
            })
            .unwrap_or(area);

        let mut y = inner.y;
        let max_y = inner.y + inner.height;
        let mut line_idx = 0;

        for (hunk_idx, hunk) in state.hunks().iter().enumerate() {
            // Render hunk header
            if line_idx >= state.scroll_offset() && y < max_y {
                let header_style = Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD);

                let is_current_hunk = hunk_idx == state.current_hunk();
                let header_text = if is_current_hunk {
                    format!("▶ {}", hunk.header())
                } else {
                    format!("  {}", hunk.header())
                };

                buf.set_string(inner.x, y, header_text, header_style);
                y += 1;
            }
            line_idx += 1;

            // Render hunk lines
            for (_local_idx, line) in hunk.lines.iter().enumerate() {
                if line_idx >= state.scroll_offset() && y < max_y {
                    self.render_unified_line(
                        inner,
                        buf,
                        line,
                        inner.x,
                        y,
                        line_idx == state.current_line(),
                    );
                    y += 1;
                }
                line_idx += 1;

                if y >= max_y {
                    break;
                }
            }

            if y >= max_y {
                break;
            }
        }
    }

    fn render_unified_line(
        &self,
        area: Rect,
        buf: &mut Buffer,
        line: &DiffLine,
        x: u16,
        y: u16,
        is_selected: bool,
    ) {
        let (prefix, content, style) = match line {
            DiffLine::Context { content, .. } => {
                (" ", content.as_str(), Style::default().fg(Color::Gray))
            }
            DiffLine::Addition { content, .. } => {
                ("+", content.as_str(), Style::default().fg(Color::Green))
            }
            DiffLine::Deletion { content, .. } => {
                ("-", content.as_str(), Style::default().fg(Color::Red))
            }
            DiffLine::Modified { new_content, .. } => (
                "~",
                new_content.as_str(),
                Style::default().fg(Color::Yellow),
            ),
        };

        let style = if is_selected {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        };

        let mut x_offset = x;

        // Line numbers
        if self.show_line_numbers {
            let old_num = line
                .old_line_number()
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());
            let new_num = line
                .new_line_number()
                .map(|n| format!("{:>4}", n))
                .unwrap_or_else(|| "    ".to_string());

            let num_style = Style::default().fg(Color::DarkGray);
            buf.set_string(x_offset, y, &old_num, num_style);
            x_offset += 5;
            buf.set_string(x_offset, y, &new_num, num_style);
            x_offset += 5;
        }

        // Prefix and content
        buf.set_string(x_offset, y, prefix, style);
        x_offset += 2;

        let max_width = (area.width as usize).saturating_sub((x_offset - x) as usize);
        let display_content = if content.len() > max_width {
            &content[..max_width]
        } else {
            content
        };
        buf.set_string(x_offset, y, display_content, style);
    }

    fn render_side_by_side(&self, area: Rect, buf: &mut Buffer, state: &DiffViewState) {
        let inner = self
            .block
            .as_ref()
            .map(|b| {
                let inner = b.inner(area);
                b.clone().render(area, buf);
                inner
            })
            .unwrap_or(area);

        // Split area into left (old) and right (new)
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(inner);

        let left_area = chunks[0];
        let right_area = chunks[1];

        let mut y = inner.y;
        let max_y = inner.y + inner.height;
        let mut line_idx = 0;

        for (_hunk_idx, hunk) in state.hunks().iter().enumerate() {
            // Render hunk header
            if line_idx >= state.scroll_offset() && y < max_y {
                let header_style = Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD);

                let header_text = hunk.header();
                buf.set_string(left_area.x, y, &header_text, header_style);
                y += 1;
            }
            line_idx += 1;

            // Render hunk lines
            for line in &hunk.lines {
                if line_idx >= state.scroll_offset() && y < max_y {
                    self.render_side_by_side_line(
                        left_area,
                        right_area,
                        buf,
                        line,
                        y,
                        line_idx == state.current_line(),
                    );
                    y += 1;
                }
                line_idx += 1;

                if y >= max_y {
                    break;
                }
            }

            if y >= max_y {
                break;
            }
        }
    }

    fn render_side_by_side_line(
        &self,
        left_area: Rect,
        right_area: Rect,
        buf: &mut Buffer,
        line: &DiffLine,
        y: u16,
        is_selected: bool,
    ) {
        match line {
            DiffLine::Context {
                content,
                old_line,
                new_line,
            } => {
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().fg(Color::Gray)
                };

                if self.show_line_numbers {
                    buf.set_string(
                        left_area.x,
                        y,
                        &format!("{:>4} ", old_line),
                        Style::default().fg(Color::DarkGray),
                    );
                    buf.set_string(
                        right_area.x,
                        y,
                        &format!("{:>4} ", new_line),
                        Style::default().fg(Color::DarkGray),
                    );
                }

                let x_offset = if self.show_line_numbers { 5 } else { 0 };
                buf.set_string(left_area.x + x_offset, y, content, style);
                buf.set_string(right_area.x + x_offset, y, content, style);
            }
            DiffLine::Addition { content, new_line } => {
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().fg(Color::Green)
                };

                if self.show_line_numbers {
                    buf.set_string(
                        right_area.x,
                        y,
                        &format!("{:>4} ", new_line),
                        Style::default().fg(Color::DarkGray),
                    );
                }

                let x_offset = if self.show_line_numbers { 5 } else { 0 };
                buf.set_string(right_area.x + x_offset, y, &format!("+{}", content), style);
            }
            DiffLine::Deletion { content, old_line } => {
                let style = if is_selected {
                    Style::default()
                        .fg(Color::Red)
                        .add_modifier(Modifier::REVERSED)
                } else {
                    Style::default().fg(Color::Red)
                };

                if self.show_line_numbers {
                    buf.set_string(
                        left_area.x,
                        y,
                        &format!("{:>4} ", old_line),
                        Style::default().fg(Color::DarkGray),
                    );
                }

                let x_offset = if self.show_line_numbers { 5 } else { 0 };
                buf.set_string(left_area.x + x_offset, y, &format!("-{}", content), style);
            }
            DiffLine::Modified {
                old_content,
                new_content,
                old_line,
                new_line,
            } => {
                let style = if is_selected {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };

                let old_style = style.fg(Color::Red);
                let new_style = style.fg(Color::Green);

                if self.show_line_numbers {
                    buf.set_string(
                        left_area.x,
                        y,
                        &format!("{:>4} ", old_line),
                        Style::default().fg(Color::DarkGray),
                    );
                    buf.set_string(
                        right_area.x,
                        y,
                        &format!("{:>4} ", new_line),
                        Style::default().fg(Color::DarkGray),
                    );
                }

                let x_offset = if self.show_line_numbers { 5 } else { 0 };
                buf.set_string(
                    left_area.x + x_offset,
                    y,
                    &format!("-{}", old_content),
                    old_style,
                );
                buf.set_string(
                    right_area.x + x_offset,
                    y,
                    &format!("+{}", new_content),
                    new_style,
                );
            }
        }
    }
}

impl StatefulWidget for DiffView {
    type State = DiffViewState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        match self.mode {
            DiffMode::Unified => self.render_unified(area, buf, state),
            DiffMode::SideBySide => self.render_side_by_side(area, buf, state),
        }
    }
}

impl Default for DiffView {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute diff between two texts
pub fn compute_diff(old_text: &str, new_text: &str, context_lines: usize) -> Vec<DiffHunk> {
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();

    // Use Myers diff algorithm
    let diff = myers_diff(&old_lines, &new_lines);

    // Group into hunks with context
    group_into_hunks(diff, &old_lines, &new_lines, context_lines)
}

/// Myers diff algorithm implementation
fn myers_diff<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<DiffOp<'a>> {
    let n = old.len();
    let m = new.len();
    let max = n + m;

    let mut v: Vec<isize> = vec![0; 2 * max + 1];
    let mut trace: Vec<Vec<isize>> = Vec::new();

    for d in 0..=max {
        trace.push(v.clone());

        let mut k = -(d as isize);
        while k <= d as isize {
            let mut x = if k == -(d as isize)
                || (k != d as isize
                    && v[(max as isize + k - 1) as usize] < v[(max as isize + k + 1) as usize])
            {
                v[(max as isize + k + 1) as usize]
            } else {
                v[(max as isize + k - 1) as usize] + 1
            };

            let mut y = x - k;

            while x < n as isize && y < m as isize && old[x as usize] == new[y as usize] {
                x += 1;
                y += 1;
            }

            v[(max as isize + k) as usize] = x;

            if x >= n as isize && y >= m as isize {
                return backtrack(&trace, old, new, max);
            }

            k += 2;
        }
    }

    Vec::new()
}

#[derive(Debug, Clone)]
enum DiffOp<'a> {
    Equal(&'a str),
    Delete(&'a str),
    Insert(&'a str),
}

fn backtrack<'a>(
    trace: &[Vec<isize>],
    old: &[&'a str],
    new: &[&'a str],
    max: usize,
) -> Vec<DiffOp<'a>> {
    let mut x = old.len() as isize;
    let mut y = new.len() as isize;
    let mut ops = Vec::new();

    for (d, v) in trace.iter().enumerate().rev() {
        let k = x - y;
        let prev_k = if k == -(d as isize)
            || (k != d as isize
                && v[(max as isize + k - 1) as usize] < v[(max as isize + k + 1) as usize])
        {
            k + 1
        } else {
            k - 1
        };

        let prev_x = v[(max as isize + prev_k) as usize];
        let prev_y = prev_x - prev_k;

        while x > prev_x && y > prev_y {
            ops.push(DiffOp::Equal(old[(x - 1) as usize]));
            x -= 1;
            y -= 1;
        }

        if d > 0 {
            if x == prev_x {
                ops.push(DiffOp::Insert(new[(y - 1) as usize]));
                y -= 1;
            } else {
                ops.push(DiffOp::Delete(old[(x - 1) as usize]));
                x -= 1;
            }
        }
    }

    ops.reverse();
    ops
}

fn group_into_hunks<'a>(
    ops: Vec<DiffOp<'a>>,
    _old_lines: &[&'a str],
    _new_lines: &[&'a str],
    context_lines: usize,
) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current_hunk: Option<DiffHunk> = None;
    let mut old_line_num = 1;
    let mut new_line_num = 1;
    let mut context_buffer = Vec::new();

    for op in ops {
        match op {
            DiffOp::Equal(content) => {
                context_buffer.push(DiffLine::Context {
                    content: content.to_string(),
                    old_line: old_line_num,
                    new_line: new_line_num,
                });
                old_line_num += 1;
                new_line_num += 1;

                if context_buffer.len() > context_lines * 2 {
                    if let Some(ref mut hunk) = current_hunk {
                        // Add trailing context
                        for _ in 0..context_lines {
                            if let Some(line) = context_buffer.first().cloned() {
                                hunk.lines.push(line);
                                context_buffer.remove(0);
                            }
                        }
                    }

                    if let Some(hunk) = current_hunk.take() {
                        hunks.push(hunk);
                    }

                    context_buffer.clear();
                }
            }
            DiffOp::Insert(content) => {
                if current_hunk.is_none() {
                    let old_start = old_line_num.saturating_sub(context_buffer.len());
                    let new_start = new_line_num.saturating_sub(context_buffer.len());
                    current_hunk = Some(DiffHunk {
                        old_start,
                        old_count: 0,
                        new_start,
                        new_count: 0,
                        lines: context_buffer.clone(),
                        context: None,
                    });
                    context_buffer.clear();
                }

                if let Some(ref mut hunk) = current_hunk {
                    hunk.lines.push(DiffLine::Addition {
                        content: content.to_string(),
                        new_line: new_line_num,
                    });
                    hunk.new_count += 1;
                }

                new_line_num += 1;
            }
            DiffOp::Delete(content) => {
                if current_hunk.is_none() {
                    let old_start = old_line_num.saturating_sub(context_buffer.len());
                    let new_start = new_line_num.saturating_sub(context_buffer.len());
                    current_hunk = Some(DiffHunk {
                        old_start,
                        old_count: 0,
                        new_start,
                        new_count: 0,
                        lines: context_buffer.clone(),
                        context: None,
                    });
                    context_buffer.clear();
                }

                if let Some(ref mut hunk) = current_hunk {
                    hunk.lines.push(DiffLine::Deletion {
                        content: content.to_string(),
                        old_line: old_line_num,
                    });
                    hunk.old_count += 1;
                }

                old_line_num += 1;
            }
        }
    }

    // Finalize last hunk
    if let Some(mut hunk) = current_hunk {
        // Add remaining context
        for line in context_buffer.iter().take(context_lines) {
            hunk.lines.push(line.clone());
        }
        hunks.push(hunk);
    }

    hunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_line_content() {
        let line = DiffLine::Context {
            content: "hello".to_string(),
            old_line: 1,
            new_line: 1,
        };
        assert_eq!(line.content(), "hello");
    }

    #[test]
    fn test_compute_simple_diff() {
        let old = "line1\nline2\nline3";
        let new = "line1\nline2 modified\nline3";

        let hunks = compute_diff(old, new, 1);
        assert!(!hunks.is_empty());
    }

    #[test]
    fn test_hunk_header() {
        let hunk = DiffHunk {
            old_start: 10,
            old_count: 5,
            new_start: 10,
            new_count: 6,
            lines: vec![],
            context: Some("function_name".to_string()),
        };

        assert_eq!(hunk.header(), "@@ -10,5 +10,6 @@ function_name");
    }

    #[test]
    fn test_state_navigation() {
        let hunks = vec![
            DiffHunk {
                old_start: 1,
                old_count: 3,
                new_start: 1,
                new_count: 3,
                lines: vec![
                    DiffLine::Context {
                        content: "a".to_string(),
                        old_line: 1,
                        new_line: 1,
                    },
                    DiffLine::Context {
                        content: "b".to_string(),
                        old_line: 2,
                        new_line: 2,
                    },
                ],
                context: None,
            },
            DiffHunk {
                old_start: 5,
                old_count: 2,
                new_start: 5,
                new_count: 2,
                lines: vec![DiffLine::Addition {
                    content: "new".to_string(),
                    new_line: 5,
                }],
                context: None,
            },
        ];

        let mut state = DiffViewState::new(hunks);
        assert_eq!(state.current_hunk(), 0);

        state.next_hunk();
        assert_eq!(state.current_hunk(), 1);

        state.prev_hunk();
        assert_eq!(state.current_hunk(), 0);
    }
}
