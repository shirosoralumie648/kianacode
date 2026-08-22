use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use kiana_components::log_viewer::LogViewer;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 设置终端
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut viewer = LogViewer::new(1000);

    // 模拟日志流
    let log_messages = vec![
        "INFO: Application started",
        "DEBUG: Loading configuration from /etc/app/config.toml",
        "INFO: Configuration loaded successfully",
        "WARN: Using default settings for missing 'cache_size' config",
        "INFO: Connecting to database at localhost:5432",
        "ERROR: Database connection failed: timeout after 5s",
        "INFO: Retrying connection (attempt 2/3)...",
        "INFO: Connected to database successfully",
        "DEBUG: Running database migrations",
        "INFO: Migration 001_initial_schema.sql applied",
        "INFO: Migration 002_add_users.sql applied",
        "DEBUG: Registering HTTP routes",
        "INFO: Server listening on 0.0.0.0:8080",
        "INFO: Ready to accept connections",
        "DEBUG: Received request: GET /api/health",
        "DEBUG: Response: 200 OK (2ms)",
        "INFO: User login: alice@example.com",
        "WARN: Rate limit approaching for IP 192.168.1.100",
        "ERROR: Failed to process payment: card declined",
        "INFO: Cache hit rate: 87.3%",
        "TRACE: GC completed in 15ms",
        "DEBUG: Connection pool: 12/20 active",
        "FATAL: Out of memory - shutting down gracefully",
    ];

    let mut msg_idx = 0;
    let mut last_update = std::time::Instant::now();

    loop {
        // 模拟新日志到达 (每500ms一条)
        if last_update.elapsed() > Duration::from_millis(500) && msg_idx < log_messages.len() {
            viewer.add_line(log_messages[msg_idx]);
            msg_idx += 1;
            last_update = std::time::Instant::now();
        }

        terminal.draw(|f| {
            let area = f.area();
            viewer.render(f, area);
        })?;

        // 处理输入
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        // Ctrl+C 清空缓冲区
                        viewer.handle_key(key);
                    }
                    _ => {
                        viewer.handle_key(key);
                    }
                }
            }
        }
    }

    // 恢复终端
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    println!("\n=== Demo Completed ===");
    println!("Key bindings used:");
    println!("  ↑/k, ↓/j  - Scroll up/down");
    println!("  PgUp/PgDn - Scroll page");
    println!("  Home/g, End/G - Jump to top/bottom");
    println!("  Space     - Pause/Resume");
    println!("  a         - Toggle auto-scroll");
    println!("  w         - Toggle line wrap");
    println!("  t         - Toggle timestamps");
    println!("  1-6       - Toggle log level filters");
    println!("  Ctrl+C    - Clear buffer");
    println!("  q         - Quit");

    Ok(())
}
