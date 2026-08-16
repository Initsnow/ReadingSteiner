use std::io::{self, Stdout};
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::widgets::{Block, Borders, Paragraph};
use serde_json::Value;

use crate::config::Config;
use crate::control::{self, ControlRequest};
use crate::error::Result;
use crate::models::DaemonStatus;

pub async fn run_tui(cfg: &Config) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, cfg).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn query_value(socket: &std::path::Path, req: &ControlRequest) -> Option<Value> {
    control::send_request(socket, req)
        .await
        .ok()
        .and_then(|r| r.result)
}

async fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>, cfg: &Config) -> Result<()> {
    let socket = cfg.socket_path();
    loop {
        let status = query_value(&socket, &ControlRequest::Status)
            .await
            .and_then(|v| serde_json::from_value::<DaemonStatus>(v).ok())
            .unwrap_or_else(|| DaemonStatus {
                running: false,
                version: env!("CARGO_PKG_VERSION").to_string(),
                sources: 0,
                enabled_sources: 0,
                queue_depth: 0,
                last_tick_at: None,
                engine_health: Default::default(),
            });
        let sources = query_value(&socket, &ControlRequest::ListSources)
            .await
            .unwrap_or_else(|| Value::Array(vec![]));
        let events = query_value(&socket, &ControlRequest::ListEvents { limit: Some(10) })
            .await
            .unwrap_or_else(|| Value::Array(vec![]));

        let sources_text = sources
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.get("id").and_then(|v| v.as_str()))
                    .take(10)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let events_text = events
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|e| {
                        let id = e.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
                        let wp = e
                            .get("watchpoint_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let ct = e.get("change_type").and_then(|v| v.as_str()).unwrap_or("");
                        format!("#{id} {wp} {ct}")
                    })
                    .take(10)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();

        terminal.draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Length(3),
                    Constraint::Min(0),
                ])
                .split(f.area());
            f.render_widget(
                Paragraph::new(format!("ReadingSteiner v{}", status.version))
                    .block(Block::default().borders(Borders::ALL).title("Status")),
                chunks[0],
            );
            f.render_widget(
                Paragraph::new(format!(
                    "running: {} | sources: {} (enabled {}) | queue: {}",
                    status.running, status.sources, status.enabled_sources, status.queue_depth
                ))
                .block(Block::default().borders(Borders::ALL)),
                chunks[1],
            );
            f.render_widget(
                Paragraph::new(format!(
                    "sources: {}",
                    if sources_text.is_empty() {
                        "(none)"
                    } else {
                        &sources_text
                    }
                ))
                .block(Block::default().borders(Borders::ALL).title("Sources")),
                chunks[2],
            );
            f.render_widget(
                Paragraph::new(if events_text.is_empty() {
                    "No change events yet".to_string()
                } else {
                    events_text
                })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Events (q to quit)"),
                ),
                chunks[3],
            );
        })?;

        if event::poll(Duration::from_millis(500))?
            && let Event::Key(key) = event::read()?
            && (key.code == KeyCode::Char('q') || key.code == KeyCode::Esc)
        {
            break;
        }
    }
    Ok(())
}
