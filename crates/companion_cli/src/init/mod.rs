mod tui;

use std::{
    collections::HashMap,
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{bail, Context, Result};
use tempfile::TempDir;
use walkdir::WalkDir;

use crate::cli::{InitArgs, DEFAULT_DB, DEFAULT_PORT};
use self::tui::{run_tui, TuiOutcome};

const DEFAULT_REPLACE_FROM: &str = "rust_oxide";
const DEFAULT_TEMPLATE_REPO: &str = "https://github.com/HarrisDePerceptron/Rust-Oxide.git";
const ENV_TEMPLATE_REPO: &str = "SAMPLE_SERVER_TEMPLATE_REPO";
const TEMPLATE_SUBDIR: &str = "crates/server";
const BASE_ENTITY_SUBDIR: &str = "crates/base_entity_derive";

pub fn run(mut args: InitArgs) -> Result<()> {
    let interactive = !args.non_interactive && io::stdout().is_terminal();
    let mut temp_dir: Option<TempDir> = None;
    let mut repo_dir: Option<PathBuf> = None;
    let repo = resolve_repo(args.repo.clone())?;

    if interactive {
        match run_tui(args, repo.clone())? {
            TuiOutcome::Completed {
                args: updated,
                temp_dir: cloned_temp_dir,
                repo_dir: cloned_repo_dir,
            } => {
                args = updated;
                temp_dir = Some(cloned_temp_dir);
                repo_dir = Some(cloned_repo_dir);
            }
            TuiOutcome::Aborted => {
                println!("Did not create project.");
                return Ok(());
            }
        }
    }

    let name = match args.name.take() {
        Some(name) => name,
        None => bail!("project name is required in --non-interactive mode"),
    };

    normalize_db(&args.db)?;

    if args.port.is_none() {
        if let Ok(env_port) = std::env::var("PORT") {
            if let Ok(parsed) = env_port.trim().parse::<u16>() {
                args.port = Some(parsed);
            } else {
                bail!("PORT must be a valid u16");
            }
        }
    }

    if args.port.is_none() {
        args.port = Some(DEFAULT_PORT);
    }

    if let Some(port) = args.port {
        if port == 0 {
            bail!("PORT must be between 1 and 65535");
        }
        args.port = Some(port);
    }

    if args.database_url.is_none() {
        if let Ok(env_url) = std::env::var("DATABASE_URL") {
            if !env_url.trim().is_empty() {
                args.database_url = Some(env_url);
            }
        }
    }

    if args.database_url.is_none() {
        args.database_url = Some(default_db_url_for(&name, &args.db));
    }

    if !args.auth {
        eprintln!("note: auth toggling is not implemented yet; auth remains enabled");
    }

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

    let repo_dir = match repo_dir {
        Some(repo_dir) => repo_dir,
        None => {
            let temp = TempDir::new().context("failed to create temp directory")?;
            let repo_dir = temp.path().join("repo");
            clone_repo(&repo, &repo_dir)?;
            temp_dir = Some(temp);
            repo_dir
        }
    };

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

    if let Some(database_url) = args.database_url.as_ref() {
        let env_dest = out_dir.join(".env");
        if env_dest.exists() {
            append_or_replace_env(&env_dest, "DATABASE_URL", database_url)?;
        }
    }

    if let Some(port) = args.port.as_ref() {
        let env_dest = out_dir.join(".env");
        if env_dest.exists() {
            append_or_replace_env(&env_dest, "PORT", &port.to_string())?;
        }
    }

    println!("Created project at {}", out_dir.display());
    println!("Next steps:");
    println!("  cd {}", out_dir.display());
    println!("  cargo run");

    let _temp_guard = temp_dir;
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

fn append_or_replace_env(path: &Path, key: &str, value: &str) -> Result<()> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let mut values: HashMap<String, String> = HashMap::new();
    let mut order: Vec<String> = Vec::new();

    for item in dotenvy::from_read_iter(file) {
        let (k, v) = item.context("failed to parse .env")?;
        if !values.contains_key(&k) {
            order.push(k.clone());
        }
        values.insert(k, v);
    }

    if !values.contains_key(key) {
        order.push(key.to_string());
    }
    values.insert(key.to_string(), value.to_string());

    let mut output = String::new();
    for k in order {
        if let Some(v) = values.get(&k) {
            let formatted = format_env_value(v);
            output.push_str(&format!("{k}={formatted}\n"));
        }
    }

    fs::write(path, output).with_context(|| format!("failed to write {}", path.display()))
}

fn format_env_value(value: &str) -> String {
    if value.chars().any(|ch| ch.is_whitespace() || ch == '#') {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        value.to_string()
    }
}

pub(super) fn default_db_url_for(name: &str, label: &str) -> String {
    let base = derive_crate_name(name);
    match label {
        "postgres" => format!("postgres://postgres:postgres@localhost:5432/{base}"),
        "mysql" => format!("mysql://root:password@localhost:3306/{base}"),
        "sqlite" => format!("sqlite://./{base}.db"),
        _ => format!("postgres://postgres:postgres@localhost:5432/{base}"),
    }
}
