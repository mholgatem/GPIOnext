use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io, path::PathBuf, time::Duration};

mod app;
mod config;
mod constants;
mod ipc_client;
mod init_sys;
mod presets;
mod ui;

use app::App;

fn default_config_path() -> PathBuf {
    PathBuf::from("/opt/gpionext/config/gpionext.json")
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let config_path = parse_config_arg(&args).unwrap_or_else(default_config_path);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(config_path)?;
    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn parse_config_arg(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--config" || arg == "-c" {
            return iter.next().map(PathBuf::from);
        }
        if let Some(val) = arg.strip_prefix("--config=") {
            return Some(PathBuf::from(val));
        }
    }
    None
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| app.render(f))?;

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    // Ctrl-C or 'q' at top level quits
                    if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
                        break;
                    }
                    if app.handle_key(key) {
                        break;
                    }
                }
                Event::Resize(_, _) => {}
                _ => {}
            }
        }

        // 50ms tick for live pin view refresh
        app.tick();

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
