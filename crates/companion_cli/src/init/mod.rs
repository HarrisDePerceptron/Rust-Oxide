mod tui;

use std::{
    collections::HashMap,
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use tempfile::TempDir;
use walkdir::WalkDir;

use self::tui::{TuiOutcome, run_tui};
use crate::cli::{DEFAULT_PORT, InitArgs, POSTGRES_DB, SQLITE_DB};

const DEFAULT_REPLACE_FROM: &str = "rust_oxide";
const DEFAULT_TEMPLATE_REPO: &str = "https://github.com/HarrisDePerceptron/Rust-Oxide.git";
const ENV_TEMPLATE_REPO: &str = "SAMPLE_SERVER_TEMPLATE_REPO";
const TEMPLATE_SUBDIR: &str = "crates/server";
const BASE_ENTITY_SUBDIR: &str = "crates/base_entity_derive";
const AUTH_BOOTSTRAP_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/auth_bootstrap.rs.tmpl"
));
const AUTH_MOD_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/auth_mod.rs.tmpl"
));
const AUTH_PROVIDERS_MOD_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/auth_providers_mod.rs.tmpl"
));
const DB_ENTITIES_MOD_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/db_entities_mod.rs.tmpl"
));
const DB_ENTITIES_PRELUDE_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/db_entities_prelude.rs.tmpl"
));
const DB_DAO_MOD_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/db_dao_mod.rs.tmpl"
));
const DB_DAO_CONTEXT_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/db_dao_context.rs.tmpl"
));
const SERVICES_MOD_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/services_mod.rs.tmpl"
));
const SERVICES_CONTEXT_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/services_context.rs.tmpl"
));
const API_MOD_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/routes_api_mod.rs.tmpl"
));
const API_ROUTER_DISABLED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_local_auth/routes_api_router.rs.tmpl"
));
const VIEWS_PUBLIC_NO_DOCS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_docs/views_public.rs.tmpl"
));
const BUILD_DOCS_NO_DOCS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/init/no_docs/build_docs.rs.tmpl"
));

pub fn run(mut args: InitArgs) -> Result<()> {
    if args.no_auth_local {
        args.auth_local = false;
    }
    if args.no_todo_example {
        args.todo_example = false;
    }
    if args.no_docs {
        args.docs = false;
    }

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

    args.db = normalize_db(&args.db)?.to_string();

    if args.port.is_none() {
        if let Some(env_port) = first_non_empty_env(&["APP_GENERAL__PORT", "PORT"]) {
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
        if let Some(env_url) = first_non_empty_env(&["APP_DATABASE__URL", "DATABASE_URL"]) {
            args.database_url = Some(env_url);
        }
    }

    if args.database_url.is_none() {
        args.database_url = Some(default_db_url_for(&name, &args.db));
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

    if !args.auth_local {
        disable_local_auth_profile(&out_dir)?;
    }
    if !args.todo_example {
        disable_todo_example_profile(&out_dir)?;
    }
    if !args.docs {
        disable_docs_profile(&out_dir)?;
    }

    apply_database_profile(&out_dir, &args.db)?;

    if let Some(database_url) = args.database_url.as_ref() {
        let env_dest = out_dir.join(".env");
        if env_dest.exists() {
            append_or_replace_env(&env_dest, "APP_DATABASE__URL", database_url)?;
        }
    }

    if let Some(port) = args.port.as_ref() {
        let env_dest = out_dir.join(".env");
        if env_dest.exists() {
            append_or_replace_env(&env_dest, "APP_GENERAL__PORT", &port.to_string())?;
        }
    }

    println!("Created project at {}", out_dir.display());
    println!("Next steps:");
    println!("  cd {}", out_dir.display());
    println!("  cargo run");

    let _temp_guard = temp_dir;
    Ok(())
}

fn normalize_db(db: &str) -> Result<&'static str> {
    let normalized = db.trim().to_lowercase();
    match normalized.as_str() {
        POSTGRES_DB => Ok(POSTGRES_DB),
        SQLITE_DB => Ok(SQLITE_DB),
        _ => bail!("unsupported database '{db}' (supported: sqlite, postgres)"),
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

fn disable_local_auth_profile(root: &Path) -> Result<()> {
    let files_to_remove = [
        "src/auth/providers/local.rs",
        "src/auth/jwt.rs",
        "src/auth/password.rs",
        "src/db/entities/user.rs",
        "src/db/entities/refresh_token.rs",
        "src/db/dao/user_dao.rs",
        "src/db/dao/refresh_token_dao.rs",
        "src/services/user_service.rs",
        "src/routes/api/protected.rs",
        "src/routes/api/admin.rs",
        "tests/auth_flow.rs",
        "tests/mock_routes.rs",
        "tests/todo_routes.rs",
    ];
    for rel in files_to_remove {
        remove_file_if_exists(&root.join(rel))?;
    }

    write_file_if_exists(&root.join("src/auth/bootstrap.rs"), AUTH_BOOTSTRAP_DISABLED)?;
    write_file_if_exists(&root.join("src/auth/mod.rs"), AUTH_MOD_DISABLED)?;
    write_file_if_exists(
        &root.join("src/auth/providers/mod.rs"),
        AUTH_PROVIDERS_MOD_DISABLED,
    )?;
    write_file_if_exists(
        &root.join("src/db/entities/mod.rs"),
        DB_ENTITIES_MOD_DISABLED,
    )?;
    write_file_if_exists(
        &root.join("src/db/entities/prelude.rs"),
        DB_ENTITIES_PRELUDE_DISABLED,
    )?;
    write_file_if_exists(&root.join("src/db/dao/mod.rs"), DB_DAO_MOD_DISABLED)?;
    write_file_if_exists(&root.join("src/db/dao/context.rs"), DB_DAO_CONTEXT_DISABLED)?;
    write_file_if_exists(&root.join("src/services/mod.rs"), SERVICES_MOD_DISABLED)?;
    write_file_if_exists(
        &root.join("src/services/context.rs"),
        SERVICES_CONTEXT_DISABLED,
    )?;
    write_file_if_exists(&root.join("src/routes/api/mod.rs"), API_MOD_DISABLED)?;
    write_file_if_exists(&root.join("src/routes/api/router.rs"), API_ROUTER_DISABLED)?;

    remove_dependency(&root.join("Cargo.toml"), "argon2")?;
    remove_dependency(&root.join("Cargo.toml"), "jsonwebtoken")?;
    remove_dependency(&root.join("Cargo.toml"), "rand")?;

    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn write_file_if_exists(path: &Path, contents: &str) -> Result<()> {
    if path.exists() {
        fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

fn remove_dependency(path: &Path, dep_name: &str) -> Result<()> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut out = String::new();
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(&format!("{dep_name} = ")) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    fs::write(path, out).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn disable_todo_example_profile(root: &Path) -> Result<()> {
    let files_to_remove = [
        "src/routes/api/todo_crud.rs",
        "src/routes/views/todo.rs",
        "src/services/todo_service.rs",
        "src/db/entities/todo_item.rs",
        "src/db/entities/todo_list.rs",
        "src/db/dao/todo_dao.rs",
        "views/todo.html",
        "tests/todo_routes.rs",
        "tests/mock_routes.rs",
    ];
    for rel in files_to_remove {
        remove_file_if_exists(&root.join(rel))?;
    }

    remove_anchor_block_by_href(&root.join("views/base.html"), "/todo/ui")?;

    remove_lines_containing(&root.join("src/routes/api/mod.rs"), &["pub mod todo_crud;"])?;
    remove_lines_containing(
        &root.join("src/routes/api/router.rs"),
        &[".merge(todo_crud::router("],
    )?;
    replace_in_file_if_exists(
        &root.join("src/routes/api/router.rs"),
        "admin, auth, protected, public, todo_crud",
        "admin, auth, protected, public",
    )?;
    replace_in_file_if_exists(
        &root.join("src/routes/api/router.rs"),
        "auth, public, todo_crud",
        "auth, public",
    )?;

    remove_lines_containing(&root.join("src/routes/views/mod.rs"), &["pub mod todo;"])?;
    replace_in_file_if_exists(
        &root.join("src/routes/views/router.rs"),
        "Router::new().merge(public::router()).merge(todo::router())",
        "Router::new().merge(public::router())",
    )?;
    remove_lines_containing(
        &root.join("src/routes/views/router.rs"),
        &["merge(todo::router())"],
    )?;
    replace_in_file_if_exists(
        &root.join("src/routes/views/router.rs"),
        "super::{public, todo};",
        "super::public;",
    )?;

    remove_lines_containing(
        &root.join("src/services/mod.rs"),
        &["pub mod todo_service;"],
    )?;
    replace_in_file_if_exists(
        &root.join("src/services/context.rs"),
        "todo_service::TodoService, ",
        "",
    )?;
    replace_in_file_if_exists(
        &root.join("src/services/context.rs"),
        ", todo_service::TodoService",
        "",
    )?;
    remove_method_block(
        &root.join("src/services/context.rs"),
        "pub fn todo(&self) -> TodoService {",
    )?;

    remove_lines_containing(
        &root.join("src/db/entities/mod.rs"),
        &["pub mod todo_item;", "pub mod todo_list;"],
    )?;
    remove_lines_containing(
        &root.join("src/db/entities/prelude.rs"),
        &[
            "pub use super::todo_item::Entity as TodoItem;",
            "pub use super::todo_list::Entity as TodoList;",
        ],
    )?;

    remove_lines_containing(&root.join("src/db/dao/mod.rs"), &["pub mod todo_dao;"])?;
    remove_lines_containing(
        &root.join("src/db/dao/mod.rs"),
        &["pub use todo_dao::TodoDao;"],
    )?;
    replace_in_file_if_exists(&root.join("src/db/dao/context.rs"), ", TodoDao", "")?;
    remove_method_block(
        &root.join("src/db/dao/context.rs"),
        "pub fn todo(&self) -> TodoDao {",
    )?;

    Ok(())
}

fn disable_docs_profile(root: &Path) -> Result<()> {
    for rel in ["views/docs.html", "crates/server/views/docs.html"] {
        remove_file_if_exists(&root.join(rel))?;
    }
    for rel in ["views/docs", "crates/server/views/docs"] {
        remove_dir_if_exists(&root.join(rel))?;
    }
    for rel in ["views/base.html", "crates/server/views/base.html"] {
        remove_anchor_block_by_href(&root.join(rel), "/docs")?;
    }
    for rel in [
        "src/routes/views/public.rs",
        "crates/server/src/routes/views/public.rs",
    ] {
        write_file_if_exists(&root.join(rel), VIEWS_PUBLIC_NO_DOCS)?;
    }
    for rel in ["build/docs.rs", "crates/server/build/docs.rs"] {
        write_file_if_exists(&root.join(rel), BUILD_DOCS_NO_DOCS)?;
    }
    Ok(())
}

fn remove_lines_containing(path: &Path, needles: &[&str]) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut changed = false;
    let mut out = String::new();
    for line in contents.lines() {
        if needles.iter().any(|needle| line.contains(needle)) {
            changed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if changed {
        fs::write(path, out).with_context(|| format!("failed to write {}", path.display()))?;
    }
    Ok(())
}

fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

fn replace_in_file_if_exists(path: &Path, from: &str, to: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    replace_in_file(path, from, to)
}

fn remove_method_block(path: &Path, signature_fragment: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut lines: Vec<String> = contents.lines().map(|line| line.to_string()).collect();
    let start_idx = lines
        .iter()
        .position(|line| line.contains(signature_fragment));
    let Some(start_idx) = start_idx else {
        return Ok(());
    };

    let mut depth = brace_delta(&lines[start_idx]);
    let mut idx = start_idx + 1;
    while idx < lines.len() {
        depth += brace_delta(&lines[idx]);
        if depth == 0 {
            lines.drain(start_idx..=idx);
            let mut updated = lines.join("\n");
            if contents.ends_with('\n') {
                updated.push('\n');
            }
            fs::write(path, updated)
                .with_context(|| format!("failed to write {}", path.display()))?;
            return Ok(());
        }
        idx += 1;
    }

    bail!("failed to locate end of method block in {}", path.display())
}

fn brace_delta(line: &str) -> i32 {
    let mut count = 0;
    for ch in line.chars() {
        if ch == '{' {
            count += 1;
        } else if ch == '}' {
            count -= 1;
        }
    }
    count
}

fn remove_anchor_block_by_href(path: &Path, href: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }

    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut lines: Vec<String> = contents.lines().map(|line| line.to_string()).collect();

    let href_idx = lines
        .iter()
        .position(|line| line.contains("href=") && line.contains(href));
    let Some(href_idx) = href_idx else {
        return Ok(());
    };

    let mut start = href_idx;
    while start > 0 {
        let line = lines[start].trim();
        if line.starts_with("<a") || line == "<a" {
            break;
        }
        start -= 1;
    }

    let mut end = href_idx;
    while end + 1 < lines.len() {
        let line = lines[end].trim();
        if line.contains("</a>") || line == ">" || line.ends_with('>') {
            break;
        }
        end += 1;
    }

    lines.drain(start..=end);
    let mut updated = lines.join("\n");
    if contents.ends_with('\n') {
        updated.push('\n');
    }
    fs::write(path, updated).with_context(|| format!("failed to write {}", path.display()))?;
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
    let file =
        fs::File::open(path).with_context(|| format!("failed to read {}", path.display()))?;
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
        "sqlite" => format!("sqlite://{base}.db?mode=rwc"),
        "postgres" => format!("postgres://postgres:postgres@localhost:5432/{base}"),
        _ => format!("postgres://postgres:postgres@localhost:5432/{base}"),
    }
}

fn first_non_empty_env(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn apply_database_profile(root: &Path, db: &str) -> Result<()> {
    let cargo_toml = root.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Ok(());
    }

    let contents = fs::read_to_string(&cargo_toml)
        .with_context(|| format!("failed to read {}", cargo_toml.display()))?;
    let updated = rewrite_sea_orm_driver_feature(&contents, db)?;
    if updated != contents {
        fs::write(&cargo_toml, updated)
            .with_context(|| format!("failed to write {}", cargo_toml.display()))?;
    }
    Ok(())
}

fn rewrite_sea_orm_driver_feature(contents: &str, db: &str) -> Result<String> {
    let selected_driver = match db {
        "sqlite" => "sqlx-sqlite",
        "postgres" => "sqlx-postgres",
        other => bail!("unsupported database profile '{other}'"),
    };

    let mut seen = false;
    let mut rewritten = String::with_capacity(contents.len() + 32);

    for line in contents.lines() {
        if line.trim_start().starts_with("sea-orm = {") {
            seen = true;
            let mut line = line.to_string();
            line = line.replace("\"sqlx-postgres\", ", "");
            line = line.replace("\"sqlx-sqlite\", ", "");
            line = line.replace(", \"sqlx-postgres\"", "");
            line = line.replace(", \"sqlx-sqlite\"", "");
            line = line.replace("\"sqlx-postgres\"", "");
            line = line.replace("\"sqlx-sqlite\"", "");

            let marker = "\"with-chrono\"";
            let feature_literal = format!("\"{selected_driver}\", ");
            if !line.contains(selected_driver) {
                if let Some(idx) = line.find(marker) {
                    line.insert_str(idx, &feature_literal);
                } else if let Some(idx) = line.find("features = [") {
                    line.insert_str(idx + "features = [".len(), &feature_literal);
                } else {
                    bail!("failed to find sea-orm features list while applying database profile");
                }
            }

            rewritten.push_str(&line);
            rewritten.push('\n');
            continue;
        }

        rewritten.push_str(line);
        rewritten.push('\n');
    }

    if !seen {
        bail!("failed to find sea-orm dependency while applying database profile");
    }

    Ok(rewritten)
}
