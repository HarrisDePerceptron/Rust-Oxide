use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tempfile::TempDir;
use walkdir::WalkDir;

const DEFAULT_DB: &str = "postgres";
const DEFAULT_REF_FALLBACK: &str = "main";
const DEFAULT_REPLACE_FROM: &str = "sample_server";
const ENV_TEMPLATE_REPO: &str = "SAMPLE_SERVER_TEMPLATE_REPO";
const TEMPLATE_SUBDIR: &str = "crates/server";

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
    /// Git ref (tag/branch) to use; defaults to v<cli_version> then main
    #[arg(long, value_name = "ref")]
    r#ref: Option<String>,
    /// Overwrite existing output directory
    #[arg(long)]
    force: bool,
    /// Disable interactive prompts
    #[arg(long)]
    non_interactive: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init(args) | Commands::New(args) => run_init(args),
    }
}

fn run_init(mut args: InitArgs) -> Result<()> {
    let name = match args.name.take() {
        Some(name) => name,
        None if args.non_interactive => bail!("project name is required in --non-interactive mode"),
        None => prompt_string("Project name", None)?,
    };

    if !args.non_interactive {
        println!("Database: postgres (only option currently)");
        println!("Auth: enabled (not configurable yet)");
    }

    normalize_db(&args.db)?;

    if !args.auth {
        eprintln!("note: auth toggling is not implemented yet; auth remains enabled");
    }

    let repo = resolve_repo(args.repo)?;
    let crate_name = derive_crate_name(&name);
    if crate_name != name {
        eprintln!("using crate name '{crate_name}' derived from '{name}'");
    }

    let out_dir = args
        .out
        .clone()
        .unwrap_or_else(|| PathBuf::from(&name));
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
    clone_repo(&repo, args.r#ref.as_deref(), &repo_dir)?;

    let template_dir = repo_dir.join(TEMPLATE_SUBDIR);
    if !template_dir.exists() {
        bail!(
            "template directory not found at {}",
            template_dir.display()
        );
    }

    copy_dir(&template_dir, &out_dir)?;
    replace_in_dir(&out_dir, DEFAULT_REPLACE_FROM, &crate_name)?;

    println!("Created project at {}", out_dir.display());
    println!("Next steps:");
    println!("  cd {}", out_dir.display());
    println!("  cargo run");

    Ok(())
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
    bail!(
        "template repo not provided; pass --repo or set {ENV_TEMPLATE_REPO}"
    )
}

fn clone_repo(repo: &str, ref_spec: Option<&str>, dest: &Path) -> Result<()> {
    if let Some(ref_spec) = ref_spec {
        return try_clone(repo, ref_spec, dest).with_context(|| {
            format!("failed to clone {repo} at ref {ref_spec}")
        });
    }

    let version_tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    if try_clone(repo, &version_tag, dest).is_ok() {
        return Ok(());
    }

    if dest.exists() {
        fs::remove_dir_all(dest)
            .with_context(|| format!("failed to cleanup clone directory {}", dest.display()))?;
    }

    try_clone(repo, DEFAULT_REF_FALLBACK, dest)
        .with_context(|| format!("failed to clone {repo} at {DEFAULT_REF_FALLBACK}"))
}

fn try_clone(repo: &str, ref_spec: &str, dest: &Path) -> Result<()> {
    let output = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "--branch",
            ref_spec,
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
    for entry in WalkDir::new(src).into_iter().filter_entry(|e| !is_ignored(e.path(), src)) {
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
                fs::create_dir_all(parent).with_context(|| {
                    format!("failed to create directory {}", parent.display())
                })?;
            }
            fs::copy(entry.path(), &target).with_context(|| {
                format!("failed to copy {}", entry.path().display())
            })?;
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

fn is_ignored(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    rel.components().any(|comp| {
        let name = comp.as_os_str().to_string_lossy();
        matches!(name.as_ref(), ".git" | "target" | "Cargo.lock")
    })
}

fn prompt_string(prompt: &str, default: Option<&str>) -> Result<String> {
    let mut stdout = io::stdout();
    match default {
        Some(default) => {
            write!(stdout, "{prompt} [{default}]: ")?;
        }
        None => {
            write!(stdout, "{prompt}: ")?;
        }
    }
    stdout.flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();
    if input.is_empty() {
        if let Some(default) = default {
            Ok(default.to_string())
        } else {
            bail!("input is required")
        }
    } else {
        Ok(input.to_string())
    }
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
    let mut out = if out.is_empty() { "app".to_string() } else { out };
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
