// Preview pane demo - interactive file browser with preview

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use kiana_components::preview::PreviewPane;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Terminal,
};
use std::io;
use std::path::PathBuf;

struct App {
    files: Vec<PathBuf>,
    list_state: ListState,
    preview_pane: PreviewPane,
    current_dir: PathBuf,
}

impl App {
    fn new() -> io::Result<Self> {
        let current_dir = std::env::current_dir()?;
        let mut app = Self {
            files: Vec::new(),
            list_state: ListState::default(),
            preview_pane: PreviewPane::new(),
            current_dir: current_dir.clone(),
        };
        app.load_directory(&current_dir)?;
        Ok(app)
    }

    fn load_directory(&mut self, dir: &PathBuf) -> io::Result<()> {
        self.files.clear();
        self.current_dir = dir.clone();

        // Add parent directory entry
        if let Some(parent) = dir.parent() {
            self.files.push(parent.to_path_buf());
        }

        // Read directory entries
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .collect();

        // Sort: directories first, then files
        entries.sort_by(|a, b| {
            let a_is_dir = a.is_dir();
            let b_is_dir = b.is_dir();
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.file_name().cmp(&b.file_name()),
            }
        });

        self.files.extend(entries);

        // Select first item
        if !self.files.is_empty() {
            self.list_state.select(Some(0));
            self.update_preview();
        }

        Ok(())
    }

    fn update_preview(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if let Some(path) = self.files.get(selected) {
                if path.is_file() {
                    let _ = self.preview_pane.load_file(path);
                } else {
                    self.preview_pane.clear();
                }
            }
        }
    }

    fn next(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= self.files.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.update_preview();
    }

    fn previous(&mut self) {
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.files.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.update_preview();
    }

    fn enter(&mut self) -> io::Result<()> {
        if let Some(selected) = self.list_state.selected() {
            if let Some(path) = self.files.get(selected) {
                if path.is_dir() {
                    self.load_directory(&path.clone())?;
                }
            }
        }
        Ok(())
    }
}

fn main() -> io::Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new()?;

    // Run app
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }

    Ok(())
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
                .split(f.size());

            // File list
            let items: Vec<ListItem> = app
                .files
                .iter()
                .map(|path| {
                    let display_name =
                        if path == &app.current_dir.parent().unwrap_or(&app.current_dir) {
                            "..".to_string()
                        } else {
                            path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("?")
                                .to_string()
                        };

                    let prefix = if path.is_dir() { "📁 " } else { "📄 " };
                    let style = if path.is_dir() {
                        Style::default().fg(Color::Cyan)
                    } else {
                        Style::default()
                    };

                    ListItem::new(Line::from(vec![
                        Span::raw(prefix),
                        Span::styled(display_name, style),
                    ]))
                })
                .collect();

            let list = List::new(items)
                .block(
                    Block::default().borders(Borders::ALL).title(Span::styled(
                        format!(" {} ", app.current_dir.display()),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    )),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol(">> ");

            f.render_stateful_widget(list, chunks[0], &mut app.list_state);

            // Preview pane
            app.preview_pane.render(chunks[1], f.buffer_mut());
        })?;

        // Handle input
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match handle_key_event(key, app)? {
                    ControlFlow::Continue => {}
                    ControlFlow::Break => break,
                }
            }
        }
    }

    Ok(())
}

enum ControlFlow {
    Continue,
    Break,
}

fn handle_key_event(key: KeyEvent, app: &mut App) -> io::Result<ControlFlow> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(ControlFlow::Break),
        KeyCode::Down | KeyCode::Char('j') => app.next(),
        KeyCode::Up | KeyCode::Char('k') => app.previous(),
        KeyCode::Enter => app.enter()?,
        KeyCode::Char('n') => app.preview_pane.toggle_line_numbers(),
        KeyCode::Char('w') => app.preview_pane.toggle_wrap_mode(),
        KeyCode::Char('r') => app.preview_pane.toggle_raw_markdown(),
        KeyCode::Char('g') => app.preview_pane.scroll_to_top(),
        KeyCode::Char('G') => app.preview_pane.scroll_to_bottom(),
        KeyCode::PageDown => app.preview_pane.scroll_down(10),
        KeyCode::PageUp => app.preview_pane.scroll_up(10),
        KeyCode::Char('d') => app.preview_pane.scroll_down(5),
        KeyCode::Char('u') => app.preview_pane.scroll_up(5),
        _ => {}
    }
    Ok(ControlFlow::Continue)
}
