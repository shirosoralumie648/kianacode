use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use kiana_components::scrollbar::{Scrollbar, ScrollbarOrientation, ScrollbarState};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;

struct App {
    vertical_scroll: usize,
    horizontal_scroll: usize,
    content_lines: Vec<String>,
    max_line_length: usize,
}

impl App {
    fn new() -> Self {
        // Generate sample content
        let content_lines: Vec<String> = (1..=100)
            .map(|i| {
                format!(
                    "Line {:3}: This is a long line of text that demonstrates horizontal scrolling capabilities",
                    i
                )
            })
            .collect();

        let max_line_length = content_lines
            .iter()
            .map(|line| line.len())
            .max()
            .unwrap_or(0);

        Self {
            vertical_scroll: 0,
            horizontal_scroll: 0,
            content_lines,
            max_line_length,
        }
    }

    fn scroll_up(&mut self) {
        self.vertical_scroll = self.vertical_scroll.saturating_sub(1);
    }

    fn scroll_down(&mut self, viewport_height: usize) {
        let max_scroll = self.content_lines.len().saturating_sub(viewport_height);
        self.vertical_scroll = (self.vertical_scroll + 1).min(max_scroll);
    }

    fn scroll_left(&mut self) {
        self.horizontal_scroll = self.horizontal_scroll.saturating_sub(1);
    }

    fn scroll_right(&mut self, viewport_width: usize) {
        let max_scroll = self.max_line_length.saturating_sub(viewport_width);
        self.horizontal_scroll = (self.horizontal_scroll + 1).min(max_scroll);
    }

    fn page_up(&mut self, viewport_height: usize) {
        self.vertical_scroll = self.vertical_scroll.saturating_sub(viewport_height);
    }

    fn page_down(&mut self, viewport_height: usize) {
        let max_scroll = self.content_lines.len().saturating_sub(viewport_height);
        self.vertical_scroll = (self.vertical_scroll + viewport_height).min(max_scroll);
    }
}

fn main() -> Result<(), io::Error> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new();
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Up | KeyCode::Char('k') => {
                    app.scroll_up();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let viewport_height = terminal.size()?.height.saturating_sub(6) as usize;
                    app.scroll_down(viewport_height);
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    app.scroll_left();
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    let viewport_width = terminal.size()?.width.saturating_sub(4) as usize;
                    app.scroll_right(viewport_width);
                }
                KeyCode::PageUp => {
                    let viewport_height = terminal.size()?.height.saturating_sub(6) as usize;
                    app.page_up(viewport_height);
                }
                KeyCode::PageDown => {
                    let viewport_height = terminal.size()?.height.saturating_sub(6) as usize;
                    app.page_down(viewport_height);
                }
                KeyCode::Home => app.vertical_scroll = 0,
                KeyCode::End => {
                    let viewport_height = terminal.size()?.height.saturating_sub(6) as usize;
                    app.vertical_scroll = app.content_lines.len().saturating_sub(viewport_height);
                }
                _ => {}
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let size = f.area();

    // Main layout
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(size);

    // Header
    let header = Paragraph::new("Scrollbar Demo")
        .style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(header, chunks[0]);

    // Content area with scrollbars
    let content_area = chunks[1];
    let content_block = Block::default()
        .borders(Borders::ALL)
        .title("Content (Use arrow keys, Page Up/Down, Home/End to scroll)");
    let inner_area = content_block.inner(content_area);
    f.render_widget(content_block, content_area);

    // Reserve space for scrollbars
    let text_area = Rect {
        x: inner_area.x,
        y: inner_area.y,
        width: inner_area.width.saturating_sub(1),
        height: inner_area.height.saturating_sub(1),
    };

    let v_scrollbar_area = Rect {
        x: inner_area.x + inner_area.width - 1,
        y: inner_area.y,
        width: 1,
        height: inner_area.height.saturating_sub(1),
    };

    let h_scrollbar_area = Rect {
        x: inner_area.x,
        y: inner_area.y + inner_area.height - 1,
        width: inner_area.width.saturating_sub(1),
        height: 1,
    };

    // Render content
    let viewport_height = text_area.height as usize;
    let viewport_width = text_area.width as usize;

    let visible_lines: Vec<Line> = app
        .content_lines
        .iter()
        .skip(app.vertical_scroll)
        .take(viewport_height)
        .map(|line| {
            let visible_part = if app.horizontal_scroll < line.len() {
                &line[app.horizontal_scroll..]
            } else {
                ""
            };
            Line::from(visible_part.to_string())
        })
        .collect();

    let content = Paragraph::new(visible_lines).style(Style::default().fg(Color::White));
    f.render_widget(content, text_area);

    // Render vertical scrollbar
    let v_state = ScrollbarState::new(app.content_lines.len())
        .viewport_size(viewport_height)
        .position(app.vertical_scroll);

    Scrollbar::new(ScrollbarOrientation::Vertical)
        .track_style(Style::default().fg(Color::DarkGray))
        .thumb_style(Style::default().fg(Color::Cyan))
        .render(v_scrollbar_area, f.buffer_mut(), &v_state);

    // Render horizontal scrollbar
    let h_state = ScrollbarState::new(app.max_line_length)
        .viewport_size(viewport_width)
        .position(app.horizontal_scroll);

    Scrollbar::new(ScrollbarOrientation::Horizontal)
        .track_style(Style::default().fg(Color::DarkGray))
        .thumb_style(Style::default().fg(Color::Cyan))
        .render(h_scrollbar_area, f.buffer_mut(), &h_state);

    // Footer
    let footer_text = format!(
        "Position: V:{}/{} H:{}/{} | Press 'q' to quit",
        app.vertical_scroll + 1,
        app.content_lines.len(),
        app.horizontal_scroll + 1,
        app.max_line_length
    );
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Gray))
        .block(Block::default().borders(Borders::ALL));
    f.render_widget(footer, chunks[2]);
}
