use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::cli::RemoveApiArgs;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct Registry {
    version: u32,
    apis: Vec<ApiEntry>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct ApiEntry {
    name: String,
    entity: String,
    plural: String,
    base_path: String,
    files: HashMap<String, String>,
    mod_edits: HashMap<String, Vec<String>>,
    dao_context_method: String,
}

pub fn run(args: RemoveApiArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    let (project_root, server_root) = resolve_roots(&cwd)?;

    let registry_path = registry_path(&server_root);
    let registry_contents = fs::read_to_string(&registry_path)
        .with_context(|| format!("failed to read {}", registry_path.display()))?;
    let mut registry: Registry = serde_json::from_str(&registry_contents)
        .with_context(|| format!("failed to parse {}", registry_path.display()))?;

    let input_name = args.name.trim();
    if input_name.is_empty() {
        bail!("resource name is required");
    }
    let normalized = to_snake_case(input_name);

    let entry_idx = registry
        .apis
        .iter()
        .position(|entry| entry.name == input_name || entry.entity == normalized)
        .ok_or_else(|| anyhow::anyhow!("no registered API found for '{input_name}'"))?;

    let entry = registry.apis[entry_idx].clone();

    let mut missing_files = Vec::new();
    let mut modified_files = Vec::new();
    for (path_str, expected_hash) in &entry.files {
        let path = resolve_registry_path(&project_root, path_str);
        if !path.exists() {
            missing_files.push(path_str.clone());
            continue;
        }
        let data = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let actual_hash = hash_bytes(&data);
        if &actual_hash != expected_hash {
            modified_files.push(path_str.clone());
        }
    }

    let mut missing_mod_lines = Vec::new();
    for (path_str, lines) in &entry.mod_edits {
        if lines.is_empty() {
            continue;
        }
        let path = resolve_registry_path(&project_root, path_str);
        if !path.exists() {
            missing_mod_lines.push(path_str.clone());
            continue;
        }
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        for line in lines {
            if !contents.lines().any(|existing| existing.trim() == line.trim()) {
                missing_mod_lines.push(path_str.clone());
                break;
            }
        }
    }

    if !modified_files.is_empty() && !args.force {
        bail!(
            "refusing to remove: modified files: {}",
            modified_files.join(", ")
        );
    }
    if (!missing_files.is_empty() || !missing_mod_lines.is_empty()) && !(args.force || args.prune) {
        if !missing_files.is_empty() {
            bail!(
                "refusing to remove: missing files: {}",
                missing_files.join(", ")
            );
        }
        if !missing_mod_lines.is_empty() {
            bail!(
                "refusing to remove: expected edits missing in: {}",
                missing_mod_lines.join(", ")
            );
        }
    }

    if args.dry_run {
        println!("Dry run: would remove files:");
        for path in entry.files.keys() {
            println!("  {}", path);
        }
        println!("Dry run: would update files:");
        for path in entry.mod_edits.keys() {
            println!("  {}", path);
        }
        return Ok(());
    }

    for (path_str, _) in &entry.files {
        let path = resolve_registry_path(&project_root, path_str);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }

    for (path_str, lines) in &entry.mod_edits {
        let path = resolve_registry_path(&project_root, path_str);
        if !path.exists() {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let (updated, changed) = remove_lines(&contents, lines);
        if changed {
            fs::write(&path, updated)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
    }

    if !entry.dao_context_method.trim().is_empty() {
        let dao_mod_path = server_root.join("src/db/dao/mod.rs");
        if dao_mod_path.exists() {
            let contents = fs::read_to_string(&dao_mod_path)
                .with_context(|| format!("failed to read {}", dao_mod_path.display()))?;
            let (updated, changed) = remove_method_block(&contents, &entry.dao_context_method)?;
            if changed {
                fs::write(&dao_mod_path, updated)
                    .with_context(|| format!("failed to write {}", dao_mod_path.display()))?;
            } else if !args.force && !args.prune {
                bail!(
                    "failed to locate DaoContext method for removal in {}",
                    dao_mod_path.display()
                );
            }
        }
    }

    registry.apis.remove(entry_idx);
    save_registry(&registry_path, &registry)?;

    println!("Removed API '{}'", entry.name);
    Ok(())
}

fn registry_path(server_root: &Path) -> PathBuf {
    server_root.join(".scaffold/apis.json")
}

fn save_registry(path: &Path, registry: &Registry) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let contents = serde_json::to_string_pretty(registry)
        .context("failed to serialize registry")?;
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))
}

fn resolve_registry_path(root: &Path, stored: &str) -> PathBuf {
    let stored_path = Path::new(stored);
    if stored_path.is_absolute() {
        stored_path.to_path_buf()
    } else {
        root.join(stored)
    }
}

fn resolve_roots(cwd: &Path) -> Result<(PathBuf, PathBuf)> {
    for ancestor in cwd.ancestors() {
        let workspace_server = ancestor.join("crates/server/src");
        if workspace_server.exists() {
            return Ok((ancestor.to_path_buf(), ancestor.join("crates/server")));
        }
        let server_src = ancestor.join("src");
        let server_manifest = ancestor.join("Cargo.toml");
        if server_src.exists() && server_manifest.exists() {
            return Ok((ancestor.to_path_buf(), ancestor.to_path_buf()));
        }
    }
    bail!(
        "unable to locate server root from {}",
        cwd.display()
    )
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    bytes_to_hex(&digest)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

fn remove_lines(contents: &str, lines_to_remove: &[String]) -> (String, bool) {
    let remove_set: Vec<String> = lines_to_remove.iter().map(|l| l.trim().to_string()).collect();
    let mut changed = false;
    let mut out = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if remove_set.iter().any(|needle| needle == trimmed) {
            changed = true;
            continue;
        }
        out.push(line.to_string());
    }
    let mut updated = out.join("\n");
    if contents.ends_with('\n') {
        updated.push('\n');
    }
    (updated, changed)
}

fn remove_method_block(contents: &str, signature_line: &str) -> Result<(String, bool)> {
    let mut lines: Vec<String> = contents.lines().map(|line| line.to_string()).collect();
    let start_idx = lines
        .iter()
        .position(|line| line.trim() == signature_line.trim());
    let Some(start_idx) = start_idx else {
        return Ok((contents.to_string(), false));
    };

    let mut depth = count_braces(&lines[start_idx]);
    let mut idx = start_idx + 1;
    while idx < lines.len() {
        depth += count_braces(&lines[idx]);
        if depth == 0 {
            lines.drain(start_idx..=idx);
            let mut updated = lines.join("\n");
            if contents.ends_with('\n') {
                updated.push('\n');
            }
            return Ok((updated, true));
        }
        idx += 1;
    }
    bail!("failed to locate end of DaoContext method block");
}

fn count_braces(line: &str) -> i32 {
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

fn to_snake_case(input: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    let mut prev_lower_or_digit = false;

    for ch in input.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() {
                if prev_lower_or_digit && !prev_underscore {
                    out.push('_');
                }
                out.push(ch.to_ascii_lowercase());
            } else {
                out.push(ch);
            }
            prev_underscore = false;
            prev_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else {
            if !prev_underscore {
                out.push('_');
                prev_underscore = true;
            }
            prev_lower_or_digit = false;
        }
    }

    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "_".to_string()
    } else {
        trimmed
    }
}
