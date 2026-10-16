//! TUI Doctor/diagnostics 页面状态与渲染。
//!
//! App 进入该页时发出 LoadDoctor；外层旧 TUI runtime 运行诊断命令并通过 `set_output`/`set_error`
//! 回填文本。此模块只整理/显示收到的字符串，不自行执行诊断、探测安装或查询工具注册表。
//! 若有 diagnostic_lines，页面优先展示这些行；只有它们为空时才展示结构化安装字段。

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Padding, Paragraph},
    Frame,
};

/// Doctor 页面当前展示的数据；加载和执行结果由外层 adapter 注入。
#[derive(Debug, Default)]
pub struct DoctorState {
    /// 外层是否正在收集诊断信息。
    pub loading: bool,
    /// 命令返回的逐行诊断文本；非空时绘制器优先显示这部分。
    pub diagnostic_lines: Vec<String>,
    /// 若没有原始诊断行，供结构化摘要展示的版本号。
    pub version: String,
    /// 安装来源分类。
    pub install_type: String,
    /// 当前可执行文件或安装路径。
    pub install_path: String,
    /// 可选包管理器名称。
    pub package_manager: Option<String>,
    /// 外层检测到的自动更新状态文本。
    pub auto_updates: String,
    /// ripgrep 搜索工具是否可用。
    pub ripgrep_working: bool,
    /// 外层汇总的活动工具数量。
    pub active_tools: usize,
    /// 外层汇总的活动 agent 数量。
    pub active_agents: usize,
    /// 需要展示的配置或环境警告。
    pub warnings: Vec<String>,
}

impl DoctorState {
    /// 进入 loading 状态，并放一行占位文本供 screen 即时绘制。
    pub fn set_loading(&mut self) {
        self.loading = true;
        self.diagnostic_lines = vec!["Loading diagnostics...".to_string()];
    }

    /// 接收外层命令 stdout；空/全空白输出会替换为 fallback 提示。
    pub fn set_output(&mut self, output: String) {
        self.loading = false;
        self.diagnostic_lines =
            diagnostic_lines_or_fallback(output, "Doctor completed with no output.");
    }

    /// 结束 loading 并将外层错误显示为单行诊断文本。
    pub fn set_error(&mut self, error: String) {
        self.loading = false;
        self.diagnostic_lines = vec![format!("Doctor failed: {error}")];
    }

    /// Enter 或 Esc 请求返回 REPL；返回 true 由 App 解释为切换 screen。
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        matches!(key.code, KeyCode::Enter | KeyCode::Esc)
    }
}

/// 保留命令输出的行结构并移除每行末尾空白；若没有任何非空内容则返回单条 fallback。
///
/// 空白行在存在其他有效行时仍保留，用来维持诊断输出的分段布局。
fn diagnostic_lines_or_fallback(output: String, fallback: &str) -> Vec<String> {
    let lines = output
        .lines()
        .map(str::trim_end)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if lines.iter().any(|line| !line.trim().is_empty()) {
        lines
    } else {
        vec![fallback.to_string()]
    }
}

/// Doctor 的 Ratatui renderer；只消费 DoctorState，不执行诊断命令。
pub struct DoctorScreen;

impl DoctorScreen {
    /// 按布局把诊断/结构化安装信息、警告和返回提示绘制到当前帧。
    ///
    /// diagnostic_lines 非空时会提前返回，因此结构化 installation/version 等字段不会与命令原文同时出现。
    pub fn draw(frame: &mut Frame, state: &DoctorState) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        let info_area = chunks[0];
        let prompt_area = chunks[1];

        // Build diagnostic lines
        let mut items = Vec::new();

        // Title
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Diagnostics",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])));

        items.push(ListItem::new(Line::from("")));

        if !state.diagnostic_lines.is_empty() {
            for line in &state.diagnostic_lines {
                items.push(ListItem::new(Line::from(line.clone())));
            }
            if !state.warnings.is_empty() {
                items.push(ListItem::new(Line::from("")));
                items.push(ListItem::new(Line::from(vec![Span::styled(
                    "Warnings",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                )])));
                for warning in &state.warnings {
                    items.push(ListItem::new(Line::from(vec![
                        Span::styled("└ ", Style::default().fg(Color::Yellow)),
                        Span::raw(warning.clone()),
                    ])));
                }
            }

            let list = List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Doctor ")
                    .title_style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
            );

            frame.render_widget(list, info_area);

            let prompt = Paragraph::new("Press Enter or Esc to return to REPL")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .padding(Padding::horizontal(1)),
                );
            frame.render_widget(prompt, prompt_area);
            return;
        }

        // Installation info
        items.push(ListItem::new(Line::from(vec![
            Span::raw("└ Currently running: "),
            Span::styled(
                format!("{} ({})", state.install_type, state.version),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ])));

        if let Some(ref pm) = state.package_manager {
            items.push(ListItem::new(Line::from(format!(
                "└ Package manager: {}",
                pm
            ))));
        }

        items.push(ListItem::new(Line::from(format!(
            "└ Path: {}",
            state.install_path
        ))));

        items.push(ListItem::new(Line::from(format!(
            "└ Search: {}",
            if state.ripgrep_working {
                "OK"
            } else {
                "Not working"
            }
        ))));

        items.push(ListItem::new(Line::from("")));

        // Updates
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Updates",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])));
        items.push(ListItem::new(Line::from(format!(
            "└ Auto-updates: {}",
            state.auto_updates
        ))));

        items.push(ListItem::new(Line::from("")));

        // Tools & Agents
        items.push(ListItem::new(Line::from(vec![Span::styled(
            "Environment",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )])));
        items.push(ListItem::new(Line::from(format!(
            "└ Active tools: {}",
            state.active_tools
        ))));
        items.push(ListItem::new(Line::from(format!(
            "└ Active agents: {}",
            state.active_agents
        ))));

        // Warnings
        if !state.warnings.is_empty() {
            items.push(ListItem::new(Line::from("")));
            items.push(ListItem::new(Line::from(vec![Span::styled(
                "Warnings",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )])));
            for warning in &state.warnings {
                items.push(ListItem::new(Line::from(vec![
                    Span::styled("└ ", Style::default().fg(Color::Yellow)),
                    Span::raw(warning.clone()),
                ])));
            }
        }

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Doctor ")
                .title_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
        );

        frame.render_widget(list, info_area);

        // Prompt at the bottom
        let prompt = Paragraph::new("Press Enter or Esc to return to REPL")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .padding(Padding::horizontal(1)),
            );
        frame.render_widget(prompt, prompt_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_state_uses_cli_output_lines() {
        let mut state = DoctorState::default();

        state.set_output("Doctor\ncwd: /tmp/work\napi_key_set: yes\n".to_string());

        assert!(!state.loading);
        assert_eq!(
            state.diagnostic_lines,
            vec![
                "Doctor".to_string(),
                "cwd: /tmp/work".to_string(),
                "api_key_set: yes".to_string()
            ]
        );
    }

    #[test]
    fn doctor_state_reports_empty_output_and_errors() {
        let mut state = DoctorState::default();
        state.set_output(String::new());
        assert_eq!(
            state.diagnostic_lines,
            vec!["Doctor completed with no output."]
        );

        state.set_error("boom".to_string());
        assert_eq!(state.diagnostic_lines, vec!["Doctor failed: boom"]);
    }
}
