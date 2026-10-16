//! 只读 Settings/readiness 页面模型和 Ratatui renderer。
//!
//! 数据由 `kiana-entrypoints/src/tui.rs` 的旧 adapter 汇总后传入；本模块没有配置文件读写或授权判断。
//! `SettingsState` 保存分组、错误和当前滚动位置，`SettingsScreen::draw()` 将它们变成有限可见行。

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph},
    Frame,
};

#[derive(Debug, Clone, PartialEq, Eq)]
/// 设置/readiness 页面的一行展示值；它不携带可执行配置操作。
pub struct SettingsRow {
    /// 左侧字段名。
    pub label: String,
    /// 右侧展示文本。
    pub value: String,
}

impl SettingsRow {
    /// 接受可转换为 String 的字段名和值，创建纯展示行。
    pub fn new(label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 一组相关的只读展示行。
pub struct SettingsSection {
    /// 分组标题。
    pub title: String,
    /// 该组下的内容行。
    pub rows: Vec<SettingsRow>,
}

impl SettingsSection {
    /// 创建分组数据；不读取配置，也不校验 value 的业务含义。
    pub fn new(title: impl Into<String>, rows: Vec<SettingsRow>) -> Self {
        Self {
            title: title.into(),
            rows,
        }
    }
}

#[derive(Debug, Default)]
/// Settings screen 的 UI 快照，由 adapter 写入并由 renderer 读取/校正滚动位置。
pub struct SettingsState {
    /// 外层数据汇总是否尚未完成。
    pub loading: bool,
    /// 已加载的展示分组。
    pub sections: Vec<SettingsSection>,
    /// 页面顶部显示的加载/汇总错误。
    pub errors: Vec<String>,
    /// 当前首个可见行相对于完整渲染列表的偏移。
    pub scroll_offset: usize,
}

impl SettingsState {
    /// 清空旧结果并显示一个等待中的 Readiness 状态。
    pub fn set_loading(&mut self) {
        self.loading = true;
        self.errors.clear();
        self.sections = vec![SettingsSection::new(
            "Readiness",
            vec![SettingsRow::new("status", "loading settings...")],
        )];
        self.scroll_offset = 0;
    }

    /// 用外层汇总的新分组替换旧值，并将滚动回到顶部。
    pub fn set_sections(&mut self, sections: Vec<SettingsSection>) {
        self.loading = false;
        self.errors.clear();
        self.sections = sections;
        self.scroll_offset = 0;
    }

    /// 把页面切换到失败展示，并保留错误原文。
    pub fn set_error(&mut self, error: String) {
        self.loading = false;
        self.errors = vec![error];
        self.sections = vec![SettingsSection::new(
            "Readiness",
            vec![SettingsRow::new("status", "failed")],
        )];
        self.scroll_offset = 0;
    }

    /// 处理 Settings 页面按键；true 表示请求退出此 screen。
    ///
    /// Up/PageUp 每次减少一个行偏移，Down/PageDown 每次增加一个；最终上限由 draw 时根据终端尺寸裁剪。
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => true,
            KeyCode::PageUp | KeyCode::Up => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                false
            }
            KeyCode::PageDown | KeyCode::Down => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
                false
            }
            _ => false,
        }
    }
}

/// Settings 页面 renderer；布局和分页只影响展示，不修改外部配置。
pub struct SettingsScreen;

impl SettingsScreen {
    /// 生成标题、错误、分组和行，再按当前 frame 高度裁剪可见范围。
    ///
    /// 如果窗口缩小导致原 scroll_offset 超出当前最大偏移，本方法会把它改回有效范围，避免显示空白页。
    pub fn draw(frame: &mut Frame, state: &mut SettingsState) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        let mut items = Vec::new();
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Settings and Readiness",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])));
        items.push(ListItem::new(Line::from("")));

        for error in &state.errors {
            items.push(ListItem::new(Line::from(vec![
                Span::styled("error: ", Style::default().fg(Color::Red)),
                Span::raw(error.clone()),
            ])));
        }

        for section in &state.sections {
            items.push(ListItem::new(Line::from(vec![Span::styled(
                section.title.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )])));
            if section.rows.is_empty() {
                items.push(ListItem::new(Line::from("  none")));
            } else {
                for row in &section.rows {
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  {}: ", row.label),
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::raw(row.value.clone()),
                    ])));
                }
            }
            items.push(ListItem::new(Line::from("")));
        }

        // 内容高度扣除上下边框；小窗口用至少一行，避免 take(0) 让列表完全消失。
        let height = chunks[0].height.saturating_sub(2) as usize;
        let max_offset = items.len().saturating_sub(height.max(1));
        if state.scroll_offset > max_offset {
            state.scroll_offset = max_offset;
        }
        let visible = items
            .into_iter()
            .skip(state.scroll_offset)
            .take(height.max(1))
            .collect::<Vec<_>>();

        let title = if state.loading {
            " Settings (loading) "
        } else {
            " Settings "
        };
        let list = List::new(visible).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        );
        frame.render_widget(list, chunks[0]);

        let prompt = Paragraph::new("Press Enter or Esc to return to REPL")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .padding(Padding::horizontal(1)),
            );
        frame.render_widget(prompt, chunks[1]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn render_settings_to_text(state: &mut SettingsState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| SettingsScreen::draw(frame, state))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .fold(String::new(), |mut output, cell| {
                output.push_str(cell.symbol());
                output
            })
    }

    #[test]
    fn settings_state_tracks_loading_success_and_error() {
        let mut state = SettingsState::default();
        state.set_loading();
        assert!(state.loading);
        assert!(state.sections[0].rows[0].value.contains("loading"));

        state.set_sections(vec![SettingsSection::new(
            "Account",
            vec![SettingsRow::new("api_key", "missing")],
        )]);
        assert!(!state.loading);
        assert_eq!(state.sections[0].title, "Account");

        state.set_error("auth status failed".to_string());
        assert!(!state.loading);
        assert_eq!(state.errors, vec!["auth status failed"]);
    }

    #[test]
    fn settings_render_shows_readiness_sections() {
        let mut state = SettingsState::default();
        state.set_sections(vec![
            SettingsSection::new(
                "Account/Auth",
                vec![
                    SettingsRow::new("api_key", "missing"),
                    SettingsRow::new("oauth", "missing"),
                ],
            ),
            SettingsSection::new(
                "Provider/Model",
                vec![SettingsRow::new("anthropic", "missing claude-sonnet-4-6")],
            ),
        ]);

        let rendered = render_settings_to_text(&mut state, 90, 18);

        assert!(rendered.contains("Settings and Readiness"));
        assert!(rendered.contains("Account/Auth"));
        assert!(rendered.contains("api_key"));
        assert!(rendered.contains("Provider/Model"));
        assert!(rendered.contains("anthropic"));
        assert!(rendered.contains("Press Enter or Esc"));
    }
}
