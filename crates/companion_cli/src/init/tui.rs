use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    prelude::*,
    widgets::{Paragraph, Wrap},
};
use tempfile::TempDir;

use crate::cli::{DEFAULT_PORT, InitArgs};

use super::default_db_url_for;

const SPINNER_FRAMES: &[&str] = &["-", "\\", "|", "/"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    ProjectName,
    Port,
    Database,
    DatabaseUrl,
    Auth,
    OutputDir,
    Summary,
    Cloning,
}

impl Step {
    fn index(self) -> usize {
        match self {
            Step::ProjectName => 1,
            Step::Port => 2,
            Step::Database => 3,
            Step::DatabaseUrl => 4,
            Step::Auth => 5,
            Step::OutputDir => 6,
            Step::Summary => 7,
            Step::Cloning => 8,
        }
    }

    fn total() -> usize {
        8
    }

    fn title(self) -> &'static str {
        match self {
            Step::ProjectName => "Project name",
            Step::Port => "Port",
            Step::Database => "Database",
            Step::DatabaseUrl => "Database URL",
            Step::Auth => "Auth",
            Step::OutputDir => "Output directory",
            Step::Summary => "Summary",
            Step::Cloning => "Cloning",
        }
    }

    fn next(self) -> Self {
        match self {
            Step::ProjectName => Step::Port,
            Step::Port => Step::Database,
            Step::Database => Step::DatabaseUrl,
            Step::DatabaseUrl => Step::Auth,
            Step::Auth => Step::OutputDir,
            Step::OutputDir => Step::Summary,
            Step::Summary => Step::Cloning,
            Step::Cloning => Step::Cloning,
        }
    }

    fn prev(self) -> Self {
        match self {
            Step::ProjectName => Step::ProjectName,
            Step::Port => Step::ProjectName,
            Step::Database => Step::Port,
            Step::DatabaseUrl => Step::Database,
            Step::Auth => Step::DatabaseUrl,
            Step::OutputDir => Step::Auth,
            Step::Summary => Step::OutputDir,
            Step::Cloning => Step::Summary,
        }
    }
}

struct UiState {
    step: Step,
    name: String,
    out_dir: String,
    port: String,
    input: String,
    error: Option<String>,
    db_index: usize,
    auth_index: usize,
    cursor: usize,
    db_url: String,
    db_url_source: DbUrlSource,
    repo: String,
    temp_dir: Option<TempDir>,
    repo_dir: Option<PathBuf>,
    clone_started_at: Option<Instant>,
    clone_error: Option<String>,
    spinner_index: usize,
    clone_cancel_tx: Option<Sender<()>>,
    clone_result_rx: Option<Receiver<Result<(), String>>>,
    clone_cancel_requested: bool,
}

impl UiState {
    fn step_title(&self) -> String {
        format!(
            "Step {} of {}  |  {}",
            self.step.index(),
            Step::total(),
            self.step.title()
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum DbUrlSource {
    Env,
    Default,
    User,
}

struct TuiCleanup;

pub(super) enum TuiOutcome {
    Completed {
        args: InitArgs,
        temp_dir: TempDir,
        repo_dir: PathBuf,
    },
    Aborted,
}

impl Drop for TuiCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
    }
}

pub(super) fn run_tui(args: InitArgs, repo: String) -> Result<TuiOutcome> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    execute!(stdout, EnterAlternateScreen, cursor::Hide).context("failed to init tui")?;
    let _cleanup = TuiCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    terminal.clear().ok();

    let mut state = UiState {
        step: Step::ProjectName,
        name: args.name.clone().unwrap_or_default(),
        out_dir: args
            .out
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        port: resolve_default_port(args.port),
        input: String::new(),
        error: None,
        db_index: first_enabled_index(DB_OPTIONS),
        auth_index: first_enabled_index(AUTH_OPTIONS),
        cursor: 0,
        db_url: args.database_url.clone().unwrap_or_default(),
        db_url_source: if args.database_url.is_some() {
            DbUrlSource::User
        } else {
            DbUrlSource::Default
        },
        repo,
        temp_dir: None,
        repo_dir: None,
        clone_started_at: None,
        clone_error: None,
        spinner_index: 0,
        clone_cancel_tx: None,
        clone_result_rx: None,
        clone_cancel_requested: false,
    };
    sync_input(&mut state);

    loop {
        if state.step == Step::Cloning {
            tick_clone(&mut state)?;
            if let Some(result) = poll_clone_result(&mut state)? {
                match result {
                    Ok(()) => {
                        let init_args = build_args(&state, &args);
                        let temp_dir = state
                            .temp_dir
                            .take()
                            .context("missing temp dir after clone")?;
                        let repo_dir = state
                            .repo_dir
                            .clone()
                            .context("missing repo dir after clone")?;
                        return Ok(TuiOutcome::Completed {
                            args: init_args,
                            temp_dir,
                            repo_dir,
                        });
                    }
                    Err(err) => {
                        state.clone_error = Some(err);
                    }
                }
            }
        }

        terminal.draw(|frame| draw_ui(frame, &state))?;

        if event::poll(Duration::from_millis(120))? {
            if let Event::Key(key) = event::read()? {
                if handle_key(&mut state, key)? {
                    break;
                }
            }
        }
    }

    Ok(TuiOutcome::Aborted)
}

fn build_args(state: &UiState, args: &InitArgs) -> InitArgs {
    InitArgs {
        name: Some(state.name.clone()),
        out: Some(PathBuf::from(state.out_dir.clone())),
        db: DB_OPTIONS[state.db_index].label.to_string(),
        auth: AUTH_OPTIONS[state.auth_index].enabled,
        database_url: if state.db_url.is_empty() {
            None
        } else {
            Some(state.db_url.clone())
        },
        port: Some(parse_port(&state.port).unwrap_or(DEFAULT_PORT)),
        repo: args.repo.clone(),
        force: args.force,
        non_interactive: args.non_interactive,
    }
}

fn handle_key(state: &mut UiState, key: KeyEvent) -> Result<bool> {
    if state.step == Step::Cloning {
        if state.clone_error.is_some() {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => {
                    return Ok(true);
                }
                _ => return Ok(false),
            }
        }

        if let KeyCode::Esc = key.code {
            request_cancel(state);
        }
        return Ok(false);
    }

    match key.code {
        KeyCode::Esc => {
            return Ok(true);
        }
        KeyCode::Char('b') | KeyCode::Char('B') if state.step != Step::ProjectName => {
            state.step = state.step.prev();
            sync_input(state);
        }
        KeyCode::Up => handle_choice_delta(state, -1),
        KeyCode::Down => handle_choice_delta(state, 1),
        KeyCode::Enter => {
            if apply_step(state)? {
                return Ok(true);
            }
        }
        _ => handle_text_input(state, key),
    }

    Ok(false)
}

fn handle_choice_delta(state: &mut UiState, delta: isize) {
    match state.step {
        Step::Database => {
            let next = adjust_choice_index(state.db_index, DB_OPTIONS, delta);
            if next != state.db_index {
                state.db_index = next;
                if matches!(state.db_url_source, DbUrlSource::Default) {
                    state.db_url =
                        default_db_url_for(&state.name, DB_OPTIONS[state.db_index].label);
                }
            }
        }
        Step::Auth => {
            state.auth_index = adjust_choice_index(state.auth_index, AUTH_OPTIONS, delta);
        }
        _ => {}
    }
}

fn adjust_choice_index(current: usize, options: &[ChoiceOption], delta: isize) -> usize {
    if options.is_empty() {
        return current;
    }
    let mut idx = current;
    for _ in 0..options.len() {
        idx = shift_index(idx, options.len(), delta);
        if options[idx].enabled {
            return idx;
        }
    }
    current
}

fn shift_index(current: usize, max: usize, delta: isize) -> usize {
    let next = current as isize + delta;
    if next < 0 {
        (max - 1) as usize
    } else {
        (next as usize) % max
    }
}

fn first_enabled_index(options: &[ChoiceOption]) -> usize {
    options.iter().position(|opt| opt.enabled).unwrap_or(0)
}

fn apply_step(state: &mut UiState) -> Result<bool> {
    state.error = None;
    match state.step {
        Step::ProjectName => {
            if state.input.trim().is_empty() {
                state.error = Some("Project name is required".to_string());
            } else {
                state.name = state.input.trim().to_string();
                if state.out_dir.trim().is_empty() {
                    state.out_dir = state.name.clone();
                }
                if matches!(state.db_url_source, DbUrlSource::Default) && !state.db_url.is_empty() {
                    state.db_url =
                        default_db_url_for(&state.name, DB_OPTIONS[state.db_index].label);
                }
                state.step = state.step.next();
                sync_input(state);
            }
        }
        Step::Port => match parse_port(&state.input) {
            Ok(port) => {
                state.port = port.to_string();
                state.step = state.step.next();
                sync_input(state);
            }
            Err(err) => {
                state.error = Some(err);
            }
        },
        Step::Database => {
            state.step = state.step.next();
            prepare_db_url(state);
            sync_input(state);
        }
        Step::DatabaseUrl => {
            if state.input.trim().is_empty() {
                state.error = Some("Database URL is required".to_string());
            } else {
                state.db_url = state.input.trim().to_string();
                state.db_url_source = DbUrlSource::User;
                state.step = state.step.next();
                sync_input(state);
            }
        }
        Step::Auth => {
            state.step = state.step.next();
            sync_input(state);
        }
        Step::OutputDir => {
            if state.input.trim().is_empty() {
                state.error = Some("Output directory is required".to_string());
            } else {
                state.out_dir = state.input.trim().to_string();
                state.step = state.step.next();
                sync_input(state);
            }
        }
        Step::Summary => {
            state.step = state.step.next();
            start_clone(state)?;
        }
        Step::Cloning => {}
    }
    Ok(false)
}

fn handle_text_input(state: &mut UiState, key: KeyEvent) {
    match state.step {
        Step::ProjectName | Step::Port | Step::OutputDir | Step::DatabaseUrl => match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.input.clear();
                state.cursor = 0;
            }
            KeyCode::Char(ch) => {
                insert_char(&mut state.input, &mut state.cursor, ch);
            }
            KeyCode::Backspace => {
                remove_before_cursor(&mut state.input, &mut state.cursor);
            }
            KeyCode::Delete => {
                remove_at_cursor(&mut state.input, &mut state.cursor);
            }
            KeyCode::Left => {
                move_cursor_left(&state.input, &mut state.cursor);
            }
            KeyCode::Right => {
                move_cursor_right(&state.input, &mut state.cursor);
            }
            KeyCode::Home => {
                state.cursor = 0;
            }
            KeyCode::End => {
                state.cursor = char_count(&state.input);
            }
            _ => {}
        },
        _ => {}
    }
}

fn sync_input(state: &mut UiState) {
    state.input = match state.step {
        Step::ProjectName => state.name.clone(),
        Step::Port => state.port.clone(),
        Step::DatabaseUrl => state.db_url.clone(),
        Step::OutputDir => state.out_dir.clone(),
        _ => String::new(),
    };
    state.cursor = char_count(&state.input);
}

fn resolve_default_port(port: Option<u16>) -> String {
    if let Some(port) = port {
        return port.to_string();
    }
    if let Ok(env_port) = std::env::var("PORT") {
        if let Ok(parsed) = env_port.trim().parse::<u16>() {
            return parsed.to_string();
        }
    }
    DEFAULT_PORT.to_string()
}

fn parse_port(input: &str) -> Result<u16, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Port is required".to_string());
    }
    let parsed = trimmed
        .parse::<u16>()
        .map_err(|_| "Port must be a valid u16".to_string())?;
    if parsed == 0 {
        return Err("Port must be between 1 and 65535".to_string());
    }
    Ok(parsed)
}

fn draw_ui(frame: &mut Frame<'_>, state: &UiState) {
    let size = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(2),
            Constraint::Min(6),
            Constraint::Length(1),
        ])
        .split(size);

    let header_lines = vec![
        Line::from(vec![
            Span::styled(
                "Create Sample Server",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  -  Scaffold a server from a remote template"),
        ]),
        Line::from(""),
        Line::from("Minimal setup. Fast start. Configurable later."),
    ];

    let header = Paragraph::new(header_lines).wrap(Wrap { trim: true });
    frame.render_widget(header, chunks[0]);

    let step_line = Paragraph::new(state.step_title()).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(step_line, chunks[1]);

    let body = match state.step {
        Step::ProjectName => text_input_lines(
            "Project name",
            &state.input,
            state.error.as_deref(),
            state.cursor,
        ),
        Step::Port => text_input_lines("Port", &state.input, state.error.as_deref(), state.cursor),
        Step::Database => choice_lines("Database (use arrows)", DB_OPTIONS, state.db_index),
        Step::DatabaseUrl => {
            let title = match state.db_url_source {
                DbUrlSource::Env => "Database URL (loaded from env)",
                DbUrlSource::Default => "Database URL (default)",
                DbUrlSource::User => "Database URL",
            };
            text_input_lines(title, &state.input, state.error.as_deref(), state.cursor)
        }
        Step::Auth => choice_lines("Auth", AUTH_OPTIONS, state.auth_index),
        Step::OutputDir => text_input_lines(
            "Output directory",
            &state.input,
            state.error.as_deref(),
            state.cursor,
        ),
        Step::Summary => Paragraph::new(vec![
            Line::from("Review"),
            Line::from(format!("Project name: {}", state.name)),
            Line::from(format!("Output dir:   {}", state.out_dir)),
            Line::from(format!("Port:         {}", state.port)),
            Line::from(format!(
                "Database:     {}",
                DB_OPTIONS[state.db_index].label
            )),
            Line::from(format!("Database URL: {}", state.db_url)),
            Line::from(format!(
                "Auth:         {}",
                AUTH_OPTIONS[state.auth_index].label
            )),
            Line::from(""),
            Line::from("Press Enter to generate."),
        ]),
        Step::Cloning => clone_lines(state),
    };

    frame.render_widget(body, chunks[2]);

    let footer_text = match state.step {
        Step::Cloning => {
            if state.clone_error.is_some() {
                "Enter exit  |  Esc exit"
            } else {
                "Esc cancel"
            }
        }
        _ => "Enter next  |  B back  |  Esc cancel  |  Up/Down choose  |  Left/Right move",
    };

    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, chunks[3]);
}

fn clone_lines(state: &UiState) -> Paragraph<'_> {
    let spinner = SPINNER_FRAMES[state.spinner_index % SPINNER_FRAMES.len()];
    let elapsed = state
        .clone_started_at
        .map(|instant| instant.elapsed().as_secs())
        .unwrap_or(0);
    let mut lines = vec![
        Line::from(format!("{spinner} Cloning template...")),
        Line::from(format!("Elapsed: {elapsed}s")),
    ];

    if state.clone_cancel_requested && state.clone_error.is_none() {
        lines.push(Line::from("Cancelling..."));
    }

    if let Some(err) = state.clone_error.as_deref() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            err,
            Style::default().fg(Color::Red),
        )]));
        lines.push(Line::from("Press Enter to exit."));
    }

    Paragraph::new(lines).wrap(Wrap { trim: true })
}

fn text_input_lines<'a>(
    title: &'a str,
    input: &'a str,
    error: Option<&'a str>,
    cursor: usize,
) -> Paragraph<'a> {
    let display = render_input_with_cursor(input, cursor);
    let mut lines = vec![Line::from(title), Line::from(format!("> {display}"))];
    if let Some(err) = error {
        lines.push(Line::from(vec![Span::styled(
            err,
            Style::default().fg(Color::Red),
        )]));
    }
    Paragraph::new(lines).wrap(Wrap { trim: true })
}

fn choice_lines<'a>(title: &'a str, options: &'a [ChoiceOption], selected: usize) -> Paragraph<'a> {
    let mut lines = vec![Line::from(title)];
    for (idx, option) in options.iter().enumerate() {
        let marker = if idx == selected { "(*)" } else { "( )" };
        let label = if option.enabled {
            option.label.to_string()
        } else {
            format!("{} (coming soon)", option.label)
        };
        let line = if idx == selected {
            Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Green)),
                Span::raw(" "),
                Span::styled(label, Style::default().add_modifier(Modifier::BOLD)),
            ])
        } else if option.enabled {
            Line::from(format!("{marker} {label}"))
        } else {
            Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::DarkGray)),
                Span::raw(" "),
                Span::styled(label, Style::default().fg(Color::DarkGray)),
            ])
        };
        lines.push(line);
    }
    Paragraph::new(lines).wrap(Wrap { trim: true })
}

fn render_input_with_cursor(input: &str, cursor: usize) -> String {
    let idx = cursor_to_byte(input, cursor);
    let mut rendered = String::with_capacity(input.len() + 1);
    rendered.push_str(&input[..idx]);
    rendered.push('_');
    rendered.push_str(&input[idx..]);
    rendered
}

fn prepare_db_url(state: &mut UiState) {
    if matches!(state.db_url_source, DbUrlSource::User) {
        return;
    }
    if let Ok(env_url) = std::env::var("DATABASE_URL") {
        if !env_url.trim().is_empty() {
            state.db_url = env_url;
            state.db_url_source = DbUrlSource::Env;
            return;
        }
    }
    state.db_url = default_db_url_for(&state.name, DB_OPTIONS[state.db_index].label);
    state.db_url_source = DbUrlSource::Default;
}

fn insert_char(input: &mut String, cursor: &mut usize, ch: char) {
    let idx = cursor_to_byte(input, *cursor);
    let mut updated = String::with_capacity(input.len() + ch.len_utf8());
    updated.push_str(&input[..idx]);
    updated.push(ch);
    updated.push_str(&input[idx..]);
    *input = updated;
    *cursor += 1;
}

fn remove_before_cursor(input: &mut String, cursor: &mut usize) {
    if *cursor == 0 {
        return;
    }
    let end = cursor_to_byte(input, *cursor);
    let start = cursor_to_byte(input, *cursor - 1);
    let mut updated = String::with_capacity(input.len().saturating_sub(end - start));
    updated.push_str(&input[..start]);
    updated.push_str(&input[end..]);
    *input = updated;
    *cursor -= 1;
}

fn remove_at_cursor(input: &mut String, cursor: &mut usize) {
    let count = char_count(input);
    if *cursor >= count {
        return;
    }
    let start = cursor_to_byte(input, *cursor);
    let end = cursor_to_byte(input, *cursor + 1);
    let mut updated = String::with_capacity(input.len().saturating_sub(end - start));
    updated.push_str(&input[..start]);
    updated.push_str(&input[end..]);
    *input = updated;
}

fn move_cursor_left(_input: &str, cursor: &mut usize) {
    if *cursor > 0 {
        *cursor -= 1;
    }
}

fn move_cursor_right(input: &str, cursor: &mut usize) {
    let count = char_count(input);
    if *cursor < count {
        *cursor += 1;
    }
}

fn char_count(input: &str) -> usize {
    input.chars().count()
}

fn cursor_to_byte(input: &str, cursor: usize) -> usize {
    if cursor == 0 {
        return 0;
    }
    input
        .char_indices()
        .nth(cursor)
        .map(|(idx, _)| idx)
        .unwrap_or_else(|| input.len())
}

fn start_clone(state: &mut UiState) -> Result<()> {
    let temp_dir = TempDir::new().context("failed to create temp directory")?;
    let repo_dir = temp_dir.path().join("repo");

    let (cancel_tx, cancel_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();

    let repo = state.repo.clone();
    let repo_dir_clone = repo_dir.clone();

    thread::spawn(move || {
        let result = clone_repo_with_cancel(&repo, &repo_dir_clone, cancel_rx);
        let _ = result_tx.send(result);
    });

    state.temp_dir = Some(temp_dir);
    state.repo_dir = Some(repo_dir);
    state.clone_started_at = Some(Instant::now());
    state.clone_error = None;
    state.spinner_index = 0;
    state.clone_cancel_tx = Some(cancel_tx);
    state.clone_result_rx = Some(result_rx);
    state.clone_cancel_requested = false;

    Ok(())
}

fn tick_clone(state: &mut UiState) -> Result<()> {
    if state.clone_error.is_none() {
        state.spinner_index = state.spinner_index.wrapping_add(1);
    }
    Ok(())
}

fn poll_clone_result(state: &mut UiState) -> Result<Option<Result<(), String>>> {
    let rx = match state.clone_result_rx.as_ref() {
        Some(rx) => rx,
        None => return Ok(None),
    };

    match rx.try_recv() {
        Ok(result) => {
            state.clone_result_rx = None;
            Ok(Some(result))
        }
        Err(TryRecvError::Empty) => Ok(None),
        Err(TryRecvError::Disconnected) => {
            state.clone_result_rx = None;
            Ok(Some(Err("git clone failed: channel closed".to_string())))
        }
    }
}

fn request_cancel(state: &mut UiState) {
    if state.clone_cancel_requested {
        return;
    }
    if let Some(tx) = state.clone_cancel_tx.as_ref() {
        let _ = tx.send(());
        state.clone_cancel_requested = true;
    }
}

fn clone_repo_with_cancel(repo: &str, dest: &Path, cancel_rx: Receiver<()>) -> Result<(), String> {
    let mut child = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            "master",
            repo,
            dest.to_string_lossy().as_ref(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("failed to execute git clone: {err}"))?;

    loop {
        if cancel_rx.try_recv().is_ok() {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .map_err(|err| format!("failed to wait for git clone: {err}"))?;
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.trim().is_empty() {
                return Err("git clone cancelled".to_string());
            }
            return Err(format!("git clone cancelled: {stderr}"));
        }

        match child
            .try_wait()
            .map_err(|err| format!("failed to wait for git clone: {err}"))?
        {
            Some(status) => {
                let output = child
                    .wait_with_output()
                    .map_err(|err| format!("failed to read git clone output: {err}"))?;
                if status.success() {
                    return Ok(());
                }
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.trim().is_empty() {
                    return Err("git clone failed".to_string());
                }
                return Err(format!("git clone failed: {stderr}"));
            }
            None => thread::sleep(Duration::from_millis(80)),
        }
    }
}

struct ChoiceOption {
    label: &'static str,
    enabled: bool,
}

const DB_OPTIONS: &[ChoiceOption] = &[
    ChoiceOption {
        label: "postgres",
        enabled: true,
    },
    ChoiceOption {
        label: "mysql",
        enabled: false,
    },
    ChoiceOption {
        label: "sqlite",
        enabled: false,
    },
];

const AUTH_OPTIONS: &[ChoiceOption] = &[ChoiceOption {
    label: "enabled",
    enabled: true,
}];
