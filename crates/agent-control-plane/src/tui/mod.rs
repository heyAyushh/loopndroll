mod actions;
mod commands;
mod geometry;
mod json_value;
mod model;
mod render;
mod state;
mod tabs;
mod theme;
mod transport;

use std::io;
use std::time::Duration;

use anyhow::Result;
use crossterm::cursor::Show;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, MouseButton, MouseEvent,
    MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use reqwest::Client;
use tokio::sync::mpsc;

use self::commands::execute_command;
use self::render::render;
use self::state::{COMMAND_PREFIX, ServerData, TuiState};
use self::transport::{get_json, tail_events};

const POLL_INTERVAL: Duration = Duration::from_millis(200);
const LOG_CHANNEL_CAPACITY: usize = 100;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchOptions {
    pub thread_id: Option<String>,
}

pub async fn run() -> Result<()> {
    run_with_options(LaunchOptions::default()).await
}

pub async fn run_with_options(options: LaunchOptions) -> Result<()> {
    let client = Client::new();
    let (log_sender, mut log_receiver) = mpsc::channel(LOG_CHANNEL_CAPACITY);
    tokio::spawn(tail_events(client.clone(), log_sender));
    let mut app = load_state(&client).await.unwrap_or_default();
    if let Some(thread_id) = options.thread_id.as_deref() {
        app.focus_thread(thread_id);
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let mut cleanup = TerminalCleanup::active();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_tui(&mut terminal, &client, &mut app, &mut log_receiver).await;

    cleanup.restore(&mut terminal)?;
    result
}

struct TerminalCleanup {
    active: bool,
}

impl TerminalCleanup {
    fn active() -> Self {
        Self { active: true }
    }

    fn restore(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            DisableMouseCapture,
            LeaveAlternateScreen
        )?;
        terminal.show_cursor()?;
        self.active = false;
        Ok(())
    }
}

impl Drop for TerminalCleanup {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, DisableMouseCapture, LeaveAlternateScreen, Show);
    }
}

async fn run_tui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: &Client,
    app: &mut TuiState,
    log_receiver: &mut mpsc::Receiver<String>,
) -> Result<()> {
    loop {
        drain_logs(app, log_receiver);
        terminal.draw(|frame| render(frame, app))?;
        if !event::poll(POLL_INTERVAL)? {
            continue;
        }
        let terminal_event = event::read()?;
        if let Event::Key(key) = terminal_event
            && app.command_mode()
        {
            handle_command_key(client, app, key.code).await?;
            continue;
        }
        match terminal_event {
            Event::Key(key) => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('r') => reload_state(client, app).await?,
                KeyCode::Char(COMMAND_PREFIX) => app.start_command(),
                KeyCode::Left => app.previous_tab(),
                KeyCode::Right => app.next_tab(),
                KeyCode::Up => app.previous_row(),
                KeyCode::Down => app.next_row(),
                _ => {}
            },
            Event::Mouse(mouse) => handle_mouse_event(app, mouse),
            Event::Resize(_, _) => app.set_status_line("resized"),
            _ => {}
        }
    }
}

async fn handle_command_key(client: &Client, app: &mut TuiState, key: KeyCode) -> Result<()> {
    match key {
        KeyCode::Esc => app.cancel_command(),
        KeyCode::Enter => {
            let command = app.command_buffer().to_owned();
            app.cancel_command();
            if let Err(error) = execute_command(client, app, &command).await {
                app.set_status_line(format!("error: {error}"));
            } else {
                reload_state(client, app).await?;
            }
        }
        KeyCode::Backspace => app.pop_command_char(),
        KeyCode::Char(value) => app.push_command_char(value),
        _ => {}
    }
    Ok(())
}

fn handle_mouse_event(app: &mut TuiState, mouse: MouseEvent) {
    app.update_pointer(mouse.column, mouse.row);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => app.activate_pointer_target(),
        MouseEventKind::Down(MouseButton::Right) => app.start_context_command(),
        MouseEventKind::ScrollUp => app.scroll_pointer_target_up(),
        MouseEventKind::ScrollDown => app.scroll_pointer_target_down(),
        MouseEventKind::Drag(MouseButton::Left) => app.drag_pointer_target(),
        MouseEventKind::Moved
        | MouseEventKind::Up(_)
        | MouseEventKind::Down(MouseButton::Middle)
        | MouseEventKind::Drag(MouseButton::Right)
        | MouseEventKind::Drag(MouseButton::Middle)
        | MouseEventKind::ScrollLeft
        | MouseEventKind::ScrollRight => {}
    }
}

async fn load_state(client: &Client) -> Result<TuiState> {
    Ok(TuiState::with_server_data(
        load_server_data(client, true).await?,
    ))
}

async fn reload_state(client: &Client, app: &mut TuiState) -> Result<()> {
    app.replace_server_data(load_server_data(client, false).await?);
    app.set_status_line("refreshed");
    Ok(())
}

async fn load_server_data(client: &Client, include_pairing: bool) -> Result<ServerData> {
    Ok(ServerData {
        status: get_json(client, "/status/control-plane").await?,
        snapshot: get_json(client, "/desktop/snapshot").await?,
        mobile_state: get_json(client, "/desktop/mobile-state").await?,
        connections: get_json(client, "/desktop/connections").await?,
        push_devices: get_json(client, "/desktop/push/devices").await?,
        pairing: if include_pairing {
            Some(get_json(client, "/desktop/pairing").await?)
        } else {
            None
        },
    })
}

fn drain_logs(app: &mut TuiState, log_receiver: &mut mpsc::Receiver<String>) {
    while let Ok(event) = log_receiver.try_recv() {
        app.push_log_event(event);
    }
}
