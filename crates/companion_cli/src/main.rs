use std::{
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
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
use walkdir::WalkDir;

const DEFAULT_DB: &str = "postgres";
const DEFAULT_REPLACE_FROM: &str = "sample_server";
const DEFAULT_TEMPLATE_REPO: &str = "https://github.com/HarrisDePerceptron/Rust-Oxide.git";
const ENV_TEMPLATE_REPO: &str = "SAMPLE_SERVER_TEMPLATE_REPO";
const TEMPLATE_SUBDIR: &str = "crates/server";
const BASE_ENTITY_SUBDIR: &str = "crates/base_entity_derive";

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init(InitArgs),
    New(InitArgs),
}

#[derive(Parser, Clone)]
struct InitArgs {
    /// Project name (used for directory name and crate name derivation)
    name: Option<String>,
    /// Output directory (defaults to ./<name>)
    #[arg(long)]
    out: Option<PathBuf>,
    /// Database choice (only postgres supported for now)
    #[arg(long, default_value = DEFAULT_DB)]
    db: String,
    /// Enable auth (currently always enabled in template)
    #[arg(long, default_value_t = true)]
    auth: bool,
    /// Template repo URL (or set SAMPLE_SERVER_TEMPLATE_REPO)
    #[arg(long)]
    repo: Option<String>,
    /// Overwrite existing output directory
    #[arg(long)]
    force: bool,
    /// Disable interactive prompts
    #[arg(long)]
    non_interactive: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    ProjectName,
    Database,
    Auth,
    OutputDir,
    Summary,
}

impl Step {
    fn index(self) -> usize {
        match self {
            Step::ProjectName => 1,
            Step::Database => 2,
            Step::Auth => 3,
            Step::OutputDir => 4,
            Step::Summary => 5,
        }
    }

    fn total() -> usize {
        5
    }

    fn title(self) -> &'static str {
        match self {
            Step::ProjectName => "Project name",
            Step::Database => "Database",
            Step::Auth => "Auth",
            Step::OutputDir => "Output directory",
            Step::Summary => "Summary",
        }
    }

    fn next(self) -> Self {
        match self {
            Step::ProjectName => Step::Database,
            Step::Database => Step::Auth,
            Step::Auth => Step::OutputDir,
            Step::OutputDir => Step::Summary,
            Step::Summary => Step::Summary,
        }
    }

    fn prev(self) -> Self {
        match self {
            Step::ProjectName => Step::ProjectName,
            Step::Database => Step::ProjectName,
            Step::Auth => Step::Database,
            Step::OutputDir => Step::Auth,
            Step::Summary => Step::OutputDir,
        }
    }
}

struct UiState {
    step: Step,
    name: String,
    out_dir: String,
    input: String,
    error: Option<String>,
    db_index: usize,
    auth_index: usize,
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

struct TuiCleanup;

impl Drop for TuiCleanup {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let mut stdout = io::stdout();
        let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init(args) | Commands::New(args) => run_init(args),
    }
}

fn run_init(mut args: InitArgs) -> Result<()> {
    let interactive = !args.non_interactive && io::stdout().is_terminal();
    if interactive {
        args = run_tui(args)?;
    }

    let name = match args.name.take() {
        Some(name) => name,
        None => bail!("project name is required in --non-interactive mode"),
    };

    normalize_db(&args.db)?;

    if !args.auth {
        eprintln!("note: auth toggling is not implemented yet; auth remains enabled");
    }

    let repo = resolve_repo(args.repo)?;
    let crate_name = derive_crate_name(&name);
    if crate_name != name {
        eprintln!("using crate name '{crate_name}' derived from '{name}'");
    }

    let out_dir = args.out.clone().unwrap_or_else(|| PathBuf::from(&name));
    if out_dir.exists() {
        if args.force {
            if out_dir.is_dir() {
                fs::remove_dir_all(&out_dir).with_context(|| {
                    format!("failed to remove existing directory {}", out_dir.display())
                })?;
            } else {
                fs::remove_file(&out_dir)
                    .with_context(|| format!("failed to remove {}", out_dir.display()))?;
            }
        } else {
            bail!("output directory already exists: {}", out_dir.display());
        }
    }

    let temp = TempDir::new().context("failed to create temp directory")?;
    let repo_dir = temp.path().join("repo");
    clone_repo(&repo, &repo_dir)?;

    let template_dir = repo_dir.join(TEMPLATE_SUBDIR);
    if !template_dir.exists() {
        bail!("template directory not found at {}", template_dir.display());
    }

    copy_dir(&template_dir, &out_dir)?;

    let base_entity_dir = repo_dir.join(BASE_ENTITY_SUBDIR);
    if base_entity_dir.exists() {
        let dest = out_dir.join("crates/base_entity_derive");
        copy_dir(&base_entity_dir, &dest)?;
        let cargo_toml = out_dir.join("Cargo.toml");
        replace_in_file(
            &cargo_toml,
            "path = \"../base_entity_derive\"",
            "path = \"crates/base_entity_derive\"",
        )?;
    }

    let env_source = repo_dir.join(".env");
    if env_source.exists() {
        let env_dest = out_dir.join(".env");
        fs::copy(&env_source, &env_dest).with_context(|| {
            format!(
                "failed to copy .env from {} to {}",
                env_source.display(),
                env_dest.display()
            )
        })?;
    }

    replace_in_dir(&out_dir, DEFAULT_REPLACE_FROM, &crate_name)?;

    println!("Created project at {}", out_dir.display());
    println!("Next steps:");
    println!("  cd {}", out_dir.display());
    println!("  cargo run");

    Ok(())
}

fn run_tui(args: InitArgs) -> Result<InitArgs> {
    let mut stdout = io::stdout();
    enable_raw_mode().context("failed to enable raw mode")?;
    execute!(stdout, EnterAlternateScreen, cursor::Hide).context("failed to init tui")?;
    let _cleanup = TuiCleanup;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    terminal.clear().ok();

    let mut state = UiState {
        step: Step::ProjectName,
        name: args.name.unwrap_or_default(),
        out_dir: args
            .out
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        input: String::new(),
        error: None,
        db_index: 0,
        auth_index: 0,
    };

    loop {
        terminal.draw(|frame| draw_ui(frame, &state))?;

        if event::poll(Duration::from_millis(120))? {
            if let Event::Key(key) = event::read()? {
                if handle_key(&mut state, key)? {
                    break;
                }
            }
        }
    }

    Ok(InitArgs {
        name: Some(state.name),
        out: Some(PathBuf::from(state.out_dir)),
        db: DEFAULT_DB.to_string(),
        auth: true,
        repo: args.repo,
        force: args.force,
        non_interactive: args.non_interactive,
    })
}

fn handle_key(state: &mut UiState, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc => bail!("init cancelled"),
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
            state.db_index = adjust_index(state.db_index, DB_OPTIONS.len(), delta);
        }
        Step::Auth => {
            state.auth_index = adjust_index(state.auth_index, AUTH_OPTIONS.len(), delta);
        }
        _ => {}
    }
}

fn adjust_index(current: usize, max: usize, delta: isize) -> usize {
    if max == 0 {
        return current;
    }
    let next = current as isize + delta;
    if next < 0 {
        (max - 1) as usize
    } else {
        (next as usize) % max
    }
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
                state.step = state.step.next();
                sync_input(state);
            }
        }
        Step::Database => {
            state.step = state.step.next();
            sync_input(state);
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
        Step::Summary => return Ok(true),
    }
    Ok(false)
}

fn handle_text_input(state: &mut UiState, key: KeyEvent) {
    match state.step {
        Step::ProjectName | Step::OutputDir => match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                state.input.clear();
            }
            KeyCode::Char(ch) => {
                state.input.push(ch);
            }
            KeyCode::Backspace => {
                state.input.pop();
            }
            _ => {}
        },
        _ => {}
    }
}

fn sync_input(state: &mut UiState) {
    state.input = match state.step {
        Step::ProjectName => state.name.clone(),
        Step::OutputDir => state.out_dir.clone(),
        _ => String::new(),
    };
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
            true,
        ),
        Step::Database => choice_lines("Database (use arrows)", DB_OPTIONS, state.db_index),
        Step::Auth => choice_lines("Auth", AUTH_OPTIONS, state.auth_index),
        Step::OutputDir => {
            text_input_lines(
                "Output directory",
                &state.input,
                state.error.as_deref(),
                true,
            )
        }
        Step::Summary => Paragraph::new(vec![
            Line::from("Review"),
            Line::from(format!("Project name: {}", state.name)),
            Line::from(format!("Output dir:   {}", state.out_dir)),
            Line::from(format!("Database:     {}", DB_OPTIONS[state.db_index])),
            Line::from(format!("Auth:         {}", AUTH_OPTIONS[state.auth_index])),
            Line::from(""),
            Line::from("Press Enter to generate."),
        ]),
    };

    frame.render_widget(body, chunks[2]);

    let footer = Paragraph::new("Enter next  |  B back  |  Esc cancel  |  Up/Down choose")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, chunks[3]);
}

fn text_input_lines<'a>(
    title: &'a str,
    input: &'a str,
    error: Option<&'a str>,
    show_cursor: bool,
) -> Paragraph<'a> {
    let cursor = if show_cursor { "_" } else { "" };
    let mut lines = vec![Line::from(title), Line::from(format!("> {input}{cursor}"))];
    if let Some(err) = error {
        lines.push(Line::from(vec![Span::styled(
            err,
            Style::default().fg(Color::Red),
        )]));
    }
    Paragraph::new(lines).wrap(Wrap { trim: true })
}

fn choice_lines<'a>(title: &'a str, options: &'a [&'a str], selected: usize) -> Paragraph<'a> {
    let mut lines = vec![Line::from(title)];
    for (idx, option) in options.iter().enumerate() {
        let marker = if idx == selected { "(*)" } else { "( )" };
        let line = if idx == selected {
            Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Green)),
                Span::raw(" "),
                Span::styled(*option, Style::default().add_modifier(Modifier::BOLD)),
            ])
        } else {
            Line::from(format!("{marker} {option}"))
        };
        lines.push(line);
    }
    Paragraph::new(lines).wrap(Wrap { trim: true })
}


fn normalize_db(db: &str) -> Result<&str> {
    let normalized = db.trim().to_lowercase();
    if normalized == DEFAULT_DB {
        Ok(DEFAULT_DB)
    } else {
        bail!("unsupported database '{db}' (only postgres is available)");
    }
}

fn resolve_repo(repo: Option<String>) -> Result<String> {
    if let Some(repo) = repo {
        return Ok(repo);
    }
    if let Ok(repo) = std::env::var(ENV_TEMPLATE_REPO) {
        if !repo.trim().is_empty() {
            return Ok(repo);
        }
    }
    Ok(DEFAULT_TEMPLATE_REPO.to_string())
}

fn clone_repo(repo: &str, dest: &Path) -> Result<()> {
    let output = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            "master",
            repo,
            dest.to_string_lossy().as_ref(),
        ])
        .output()
        .context("failed to execute git clone")?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git clone failed: {stderr}");
    }
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    for entry in WalkDir::new(src)
        .into_iter()
        .filter_entry(|e| !is_ignored(e.path(), src))
    {
        let entry = entry.context("failed to read directory entry")?;
        let rel = entry
            .path()
            .strip_prefix(src)
            .context("failed to resolve relative path")?;
        let target = dst.join(rel);

        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .with_context(|| format!("failed to create directory {}", target.display()))?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create directory {}", parent.display()))?;
            }
            fs::copy(entry.path(), &target)
                .with_context(|| format!("failed to copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn replace_in_dir(root: &Path, from: &str, to: &str) -> Result<()> {
    for entry in WalkDir::new(root) {
        let entry = entry.context("failed to walk output directory")?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        if contents.contains(from) {
            let updated = contents.replace(from, to);
            fs::write(path, updated)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
    }
    Ok(())
}

fn replace_in_file(path: &Path, from: &str, to: &str) -> Result<()> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let updated = contents.replace(from, to);
    if updated != contents {
        fs::write(path, updated).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

fn is_ignored(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    rel.components().any(|comp| {
        let name = comp.as_os_str().to_string_lossy();
        matches!(name.as_ref(), ".git" | "target" | "Cargo.lock")
    })
}

fn derive_crate_name(input: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in input.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }
    let out = out.trim_matches('_').to_string();
    let mut out = if out.is_empty() {
        "app".to_string()
    } else {
        out
    };
    if out
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
    {
        out = format!("app_{out}");
    }
    out
}

const DB_OPTIONS: &[&str] = &["postgres"];
const AUTH_OPTIONS: &[&str] = &["enabled"];
