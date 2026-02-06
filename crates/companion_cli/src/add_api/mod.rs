use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use crate::cli::AddApiArgs;

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

const ENTITY_TEMPLATE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/templates/entity.rs.tmpl"
));
const DAO_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/dao.rs.tmpl"));
const SERVICE_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/service.rs.tmpl"));
const ROUTE_TEMPLATE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/route.rs.tmpl"));

pub fn run(args: AddApiArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to resolve current directory")?;
    let (project_root, server_root) = resolve_roots(&cwd)?;
    let src_root = server_root.join("src");
    if !src_root.exists() {
        bail!(
            "unable to locate server crate at {}",
            server_root.display()
        );
    }

    let name = args.name.trim();
    if name.is_empty() {
        bail!("resource name is required");
    }

    let entity = to_snake_case(name);
    if entity == "_" {
        bail!("resource name is invalid after normalization");
    }
    validate_ident(&entity, "entity name")?;
    let entity_pascal = to_pascal_case(&entity);

    let plural = args
        .plural
        .as_deref()
        .map(to_snake_case)
        .unwrap_or_else(|| pluralize(&entity));
    if plural == "_" {
        bail!("plural name is invalid after normalization");
    }
    validate_ident(&plural, "plural name")?;

    let table = args
        .table
        .as_deref()
        .map(to_snake_case)
        .unwrap_or_else(|| plural.clone());
    if table == "_" {
        bail!("table name is invalid after normalization");
    }
    validate_ident(&table, "table name")?;

    let default_base_name = if args.plural.is_some() {
        plural.clone()
    } else {
        entity.clone()
    };
    let default_base = format!("/{default_base_name}").replace('_', "-");
    let base_path = normalize_base_path(args.base_path.as_deref().unwrap_or(&default_base));
    if base_path.is_empty() {
        bail!("base path cannot be empty");
    }

    let fields = parse_fields(args.fields.as_deref())?;
    let rendered_fields = render_fields(&fields);

    let dao = format!("{entity_pascal}Dao");
    let service = format!("{entity_pascal}Service");
    let service_module = format!("{entity}_service");
    let route_module = entity.clone();

    let auth_enabled = !args.no_auth;
    let auth_imports = if auth_enabled {
        "use axum::middleware;\nuse crate::middleware::jwt_auth;\n"
    } else {
        ""
    };
    let auth_layer = if auth_enabled {
        "    let auth_layer = middleware::from_fn_with_state(state.clone(), jwt_auth);\n    let router = router.route_layer(auth_layer);\n"
    } else {
        ""
    };

    let mut vars = HashMap::new();
    vars.insert("entity".to_string(), entity.clone());
    vars.insert("Entity".to_string(), entity_pascal.clone());
    vars.insert("entity_plural".to_string(), plural.clone());
    vars.insert("table".to_string(), table.clone());
    vars.insert("base_path".to_string(), escape_rust_string(&base_path));
    vars.insert("Dao".to_string(), dao.clone());
    vars.insert("Service".to_string(), service.clone());
    vars.insert("service_module".to_string(), service_module.clone());
    vars.insert("route_module".to_string(), route_module.clone());
    vars.insert("fields".to_string(), rendered_fields);
    vars.insert("auth_imports".to_string(), auth_imports.to_string());
    vars.insert("auth_layer".to_string(), auth_layer.to_string());

    let entity_contents = render_template(ENTITY_TEMPLATE, &vars)?;
    let dao_contents = render_template(DAO_TEMPLATE, &vars)?;
    let service_contents = render_template(SERVICE_TEMPLATE, &vars)?;
    let route_contents = render_template(ROUTE_TEMPLATE, &vars)?;

    let entity_path = src_root.join("db/entities").join(format!("{entity}.rs"));
    let dao_path = src_root.join("db/dao").join(format!("{entity}_dao.rs"));
    let service_path = src_root.join("services").join(format!("{entity}_service.rs"));
    let route_path = src_root.join("routes/api").join(format!("{entity}.rs"));

    let new_files = [entity_path.clone(), dao_path.clone(), service_path.clone(), route_path.clone()];
    for path in &new_files {
        if path.exists() && !args.force {
            bail!("file already exists (use --force to overwrite): {}", path.display());
        }
    }

    let entities_mod_path = src_root.join("db/entities/mod.rs");
    let dao_mod_path = src_root.join("db/dao/mod.rs");
    let services_mod_path = src_root.join("services/mod.rs");
    let routes_mod_path = src_root.join("routes/api/mod.rs");

    let entities_mod = fs::read_to_string(&entities_mod_path)
        .with_context(|| format!("failed to read {}", entities_mod_path.display()))?;
    let (entities_mod_updated, entities_mod_changed) =
        update_entities_mod(&entities_mod, &entity, &entity_pascal)?;

    let dao_mod = fs::read_to_string(&dao_mod_path)
        .with_context(|| format!("failed to read {}", dao_mod_path.display()))?;
    let (dao_mod_updated, dao_mod_changed) =
        update_dao_mod(&dao_mod, &entity, &dao)?;

    let services_mod = fs::read_to_string(&services_mod_path)
        .with_context(|| format!("failed to read {}", services_mod_path.display()))?;
    let (services_mod_updated, services_mod_changed) =
        update_services_mod(&services_mod, &service_module)?;

    let routes_mod = fs::read_to_string(&routes_mod_path)
        .with_context(|| format!("failed to read {}", routes_mod_path.display()))?;
    let (routes_mod_updated, routes_mod_changed) =
        update_routes_mod(&routes_mod, &route_module)?;

    if args.dry_run {
        println!("Dry run: would create files:");
        for path in &new_files {
            println!("  {}", path.display());
        }
        println!("Dry run: would update files:");
        if entities_mod_changed {
            println!("  {}", entities_mod_path.display());
        }
        if dao_mod_changed {
            println!("  {}", dao_mod_path.display());
        }
        if services_mod_changed {
            println!("  {}", services_mod_path.display());
        }
        if routes_mod_changed {
            println!("  {}", routes_mod_path.display());
        }
        return Ok(());
    }

    write_file(&entity_path, &entity_contents)?;
    write_file(&dao_path, &dao_contents)?;
    write_file(&service_path, &service_contents)?;
    write_file(&route_path, &route_contents)?;

    if entities_mod_changed {
        write_file(&entities_mod_path, &entities_mod_updated)?;
    }
    if dao_mod_changed {
        write_file(&dao_mod_path, &dao_mod_updated)?;
    }
    if services_mod_changed {
        write_file(&services_mod_path, &services_mod_updated)?;
    }
    if routes_mod_changed {
        write_file(&routes_mod_path, &routes_mod_updated)?;
    }

    let registry_path = registry_path(&server_root);
    let mut registry = load_registry(&registry_path)?;
    if let Some(idx) = registry
        .apis
        .iter()
        .position(|entry| entry.name == name || entry.entity == entity)
    {
        if !args.force {
            bail!("API '{name}' is already registered (use --force to overwrite)");
        }
        registry.apis.remove(idx);
    }

    let files = HashMap::from([
        (
            registry_relative_path(&project_root, &entity_path),
            hash_str(&entity_contents),
        ),
        (
            registry_relative_path(&project_root, &dao_path),
            hash_str(&dao_contents),
        ),
        (
            registry_relative_path(&project_root, &service_path),
            hash_str(&service_contents),
        ),
        (
            registry_relative_path(&project_root, &route_path),
            hash_str(&route_contents),
        ),
    ]);

    let mut mod_edits = HashMap::new();
    let mut entities_edits = Vec::new();
    let entities_pre = format!("    pub use super::{entity}::Entity as {entity_pascal};");
    if !line_exists(&entities_mod, &entities_pre) {
        entities_edits.push(entities_pre);
    }
    let entities_mod_line = format!("pub mod {entity};");
    if !line_exists(&entities_mod, &entities_mod_line) {
        entities_edits.push(entities_mod_line);
    }
    if !entities_edits.is_empty() {
        mod_edits.insert(
            registry_relative_path(&project_root, &entities_mod_path),
            entities_edits,
        );
    }

    let mut dao_edits = Vec::new();
    let dao_mod_line = format!("pub mod {entity}_dao;");
    if !line_exists(&dao_mod, &dao_mod_line) {
        dao_edits.push(dao_mod_line);
    }
    let dao_use_line = format!("pub use {entity}_dao::{dao};");
    if !line_exists(&dao_mod, &dao_use_line) {
        dao_edits.push(dao_use_line);
    }
    if !dao_edits.is_empty() {
        mod_edits.insert(
            registry_relative_path(&project_root, &dao_mod_path),
            dao_edits,
        );
    }

    let mut services_edits = Vec::new();
    let services_line = format!("pub mod {service_module};");
    if !line_exists(&services_mod, &services_line) {
        services_edits.push(services_line);
    }
    if !services_edits.is_empty() {
        mod_edits.insert(
            registry_relative_path(&project_root, &services_mod_path),
            services_edits,
        );
    }

    let mut routes_edits = Vec::new();
    let routes_mod_line = format!("pub mod {route_module};");
    if !line_exists(&routes_mod, &routes_mod_line) {
        routes_edits.push(routes_mod_line);
    }
    let routes_merge_line = format!("        .merge({route_module}::router(state.clone()))");
    if !line_exists(&routes_mod, &routes_merge_line) {
        routes_edits.push(routes_merge_line);
    }
    if !routes_edits.is_empty() {
        mod_edits.insert(
            registry_relative_path(&project_root, &routes_mod_path),
            routes_edits,
        );
    }

    let dao_context_method = format!("    pub fn {entity}(&self) -> {dao} {{");
    let dao_context_method = if line_exists(&dao_mod, &dao_context_method) {
        String::new()
    } else {
        dao_context_method
    };
    registry.apis.push(ApiEntry {
        name: name.to_string(),
        entity: entity.clone(),
        plural: plural.clone(),
        base_path: base_path.clone(),
        files,
        mod_edits,
        dao_context_method,
    });
    save_registry(&registry_path, &registry)?;

    println!(
        "Added CRUD API for {entity_pascal} at {}",
        route_path.display()
    );
    Ok(())
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))
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

fn registry_path(server_root: &Path) -> PathBuf {
    server_root.join(".scaffold/apis.json")
}

fn load_registry(path: &Path) -> Result<Registry> {
    if !path.exists() {
        return Ok(Registry {
            version: 1,
            apis: Vec::new(),
        });
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let registry: Registry = serde_json::from_str(&contents)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(registry)
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

fn registry_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn render_template(template: &str, vars: &HashMap<String, String>) -> Result<String> {
    let mut output = String::with_capacity(template.len() + 128);
    let bytes = template.as_bytes();
    let mut i = 0;
    let mut missing = HashSet::new();

    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let start = i + 2;
            let mut end = start;
            while end + 1 < bytes.len() && !(bytes[end] == b'}' && bytes[end + 1] == b'}') {
                end += 1;
            }
            if end + 1 >= bytes.len() {
                bail!("template has unclosed placeholder");
            }
            let key = template[start..end].trim();
            if key.is_empty() {
                bail!("template has empty placeholder");
            }
            match vars.get(key) {
                Some(value) => output.push_str(value),
                None => {
                    missing.insert(key.to_string());
                }
            }
            i = end + 2;
            continue;
        }
        output.push(bytes[i] as char);
        i += 1;
    }

    if !missing.is_empty() {
        let mut keys: Vec<_> = missing.into_iter().collect();
        keys.sort();
        bail!("template placeholders missing values: {}", keys.join(", "));
    }
    Ok(output)
}

#[derive(Debug, Clone)]
struct FieldSpec {
    name: String,
    ty: String,
    optional: bool,
}

fn parse_fields(input: Option<&str>) -> Result<Vec<FieldSpec>> {
    let Some(raw) = input else {
        return Ok(vec![FieldSpec {
            name: "name".to_string(),
            ty: "String".to_string(),
            optional: false,
        }]);
    };

    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(vec![FieldSpec {
            name: "name".to_string(),
            ty: "String".to_string(),
            optional: false,
        }]);
    }

    let mut seen = HashSet::new();
    let mut fields = Vec::new();
    for chunk in raw.split(',') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        let (name, ty) = chunk
            .split_once(':')
            .ok_or_else(|| anyhow::anyhow!("invalid field spec '{chunk}', expected name:type"))?;
        let name = to_snake_case(name.trim());
        validate_ident(&name, "field name")?;
        if is_reserved_field(&name) {
            bail!("field '{name}' is reserved and is provided by base_entity");
        }
        if !seen.insert(name.clone()) {
            bail!("duplicate field '{name}'");
        }

        let ty = ty.trim();
        if ty.is_empty() {
            bail!("invalid field spec '{chunk}', missing type");
        }
        let (ty, optional) = if let Some(stripped) = ty.strip_suffix('?') {
            (stripped.trim(), true)
        } else {
            (ty, false)
        };
        let rust_ty = map_type(ty)?;

        fields.push(FieldSpec {
            name,
            ty: rust_ty,
            optional,
        });
    }

    if fields.is_empty() {
        return Ok(vec![FieldSpec {
            name: "name".to_string(),
            ty: "String".to_string(),
            optional: false,
        }]);
    }

    Ok(fields)
}

fn render_fields(fields: &[FieldSpec]) -> String {
    let mut out = String::new();
    for field in fields {
        let ty = if field.optional {
            format!("Option<{}>", field.ty)
        } else {
            field.ty.clone()
        };
        out.push_str(&format!("    pub {}: {},\n", field.name, ty));
    }
    out
}

fn map_type(input: &str) -> Result<String> {
    let normalized = input.trim().to_lowercase();
    let ty = match normalized.as_str() {
        "string" | "text" => "String",
        "bool" | "boolean" => "bool",
        "i32" => "i32",
        "i64" => "i64",
        "u32" => "u32",
        "u64" => "u64",
        "uuid" => "Uuid",
        "datetime" | "timestamp" => "DateTimeWithTimeZone",
        other => bail!("unsupported field type '{other}'"),
    };
    Ok(ty.to_string())
}

fn is_reserved_field(name: &str) -> bool {
    matches!(name, "id" | "created_at" | "updated_at")
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

fn to_pascal_case(input: &str) -> String {
    let mut out = String::new();
    for segment in input.split('_').filter(|s| !s.is_empty()) {
        let mut chars = segment.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            for ch in chars {
                out.push(ch);
            }
        }
    }
    if out.is_empty() {
        "Entity".to_string()
    } else {
        out
    }
}

fn pluralize(input: &str) -> String {
    if input.ends_with('y') && input.len() > 1 {
        let base = &input[..input.len() - 1];
        format!("{base}ies")
    } else if input.ends_with('s')
        || input.ends_with('x')
        || input.ends_with("ch")
        || input.ends_with("sh")
    {
        format!("{input}es")
    } else {
        format!("{input}s")
    }
}

fn normalize_base_path(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn validate_ident(input: &str, label: &str) -> Result<()> {
    let mut chars = input.chars();
    let Some(first) = chars.next() else {
        bail!("{label} cannot be empty");
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        bail!("{label} must start with a letter or underscore");
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        bail!("{label} must be alphanumeric or underscore");
    }
    Ok(())
}

fn escape_rust_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn line_exists(contents: &str, line: &str) -> bool {
    contents
        .lines()
        .any(|existing| existing.trim() == line.trim())
}

fn hash_str(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
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

fn update_entities_mod(
    contents: &str,
    entity: &str,
    entity_pascal: &str,
) -> Result<(String, bool)> {
    let mut lines: Vec<String> = contents.lines().map(|line| line.to_string()).collect();
    let mut changed = false;
    let prelude_line = format!("    pub use super::{entity}::Entity as {entity_pascal};");
    if !lines.iter().any(|line| line.trim() == prelude_line.trim()) {
        insert_in_block(
            &mut lines,
            "pub mod prelude {",
            prelude_line,
            "    ",
        )?;
        changed = true;
    }

    let mod_line = format!("pub mod {entity};");
    if !lines.iter().any(|line| line.trim() == mod_line.trim()) {
        insert_after_last_match(&mut lines, |line| line.trim_start().starts_with("pub mod "), mod_line);
        changed = true;
    }

    Ok((reconstruct(contents, &lines), changed))
}

fn update_dao_mod(contents: &str, entity: &str, dao: &str) -> Result<(String, bool)> {
    let mut lines: Vec<String> = contents.lines().map(|line| line.to_string()).collect();
    let mut changed = false;

    let mod_line = format!("pub mod {entity}_dao;");
    if !lines.iter().any(|line| line.trim() == mod_line.trim()) {
        insert_after_last_match(&mut lines, |line| line.trim_start().starts_with("pub mod "), mod_line);
        changed = true;
    }

    let use_line = format!("pub use {entity}_dao::{dao};");
    if !lines.iter().any(|line| line.trim() == use_line.trim()) {
        insert_after_last_match(&mut lines, |line| line.trim_start().starts_with("pub use "), use_line);
        changed = true;
    }

    let method_line = format!("    pub fn {entity}(&self) -> {dao} {{");
    if !lines.iter().any(|line| line.trim() == method_line.trim()) {
        insert_method_in_impl(&mut lines, "impl DaoContext {", &method_line)?;
        changed = true;
    }

    Ok((reconstruct(contents, &lines), changed))
}

fn update_services_mod(contents: &str, service_module: &str) -> Result<(String, bool)> {
    let mut lines: Vec<String> = contents.lines().map(|line| line.to_string()).collect();
    let mut changed = false;
    let mod_line = format!("pub mod {service_module};");
    if !lines.iter().any(|line| line.trim() == mod_line.trim()) {
        insert_after_last_match(&mut lines, |line| line.trim_start().starts_with("pub mod "), mod_line);
        changed = true;
    }
    Ok((reconstruct(contents, &lines), changed))
}

fn update_routes_mod(contents: &str, route_module: &str) -> Result<(String, bool)> {
    let mut lines: Vec<String> = contents.lines().map(|line| line.to_string()).collect();
    let mut changed = false;

    let mod_line = format!("pub mod {route_module};");
    if !lines.iter().any(|line| line.trim() == mod_line.trim()) {
        insert_after_last_match(&mut lines, |line| line.trim_start().starts_with("pub mod "), mod_line);
        changed = true;
    }

    let merge_line = format!("        .merge({route_module}::router(state.clone()))");
    if !lines.iter().any(|line| line.trim() == merge_line.trim()) {
        if insert_in_block(&mut lines, "fn router", merge_line.clone(), "        ").is_ok() {
            changed = true;
        } else if let Some(idx) = lines
            .iter()
            .position(|line| line.contains(".merge(todo_crud::router"))
        {
            lines.insert(idx + 1, merge_line);
            changed = true;
        } else if let Some(idx) = lines
            .iter()
            .position(|line| line.contains(".merge(auth::router"))
        {
            lines.insert(idx + 1, merge_line);
            changed = true;
        } else if let Some(idx) = lines.iter().position(|line| line.contains("Router::new()")) {
            lines.insert(idx + 1, merge_line);
            changed = true;
        } else {
            bail!("failed to locate router merge chain");
        }
    }

    Ok((reconstruct(contents, &lines), changed))
}

fn insert_in_block(
    lines: &mut Vec<String>,
    block_start: &str,
    new_line: String,
    indent: &str,
) -> Result<()> {
    let start_idx = lines
        .iter()
        .position(|line| line.contains(block_start))
        .ok_or_else(|| anyhow::anyhow!("failed to find block '{block_start}'"))?;

    let mut depth = count_braces(&lines[start_idx]);
    let mut idx = start_idx + 1;
    while idx < lines.len() {
        depth += count_braces(&lines[idx]);
        if depth == 0 {
            lines.insert(idx, format!("{indent}{}", new_line.trim()));
            return Ok(());
        }
        idx += 1;
    }
    bail!("failed to locate end of block '{block_start}'");
}

fn insert_after_last_match(
    lines: &mut Vec<String>,
    predicate: impl Fn(&str) -> bool,
    new_line: String,
) {
    let mut insert_idx = None;
    for (idx, line) in lines.iter().enumerate() {
        if predicate(line) {
            insert_idx = Some(idx + 1);
        }
    }
    match insert_idx {
        Some(idx) => lines.insert(idx, new_line),
        None => lines.push(new_line),
    }
}

fn insert_method_in_impl(
    lines: &mut Vec<String>,
    impl_header: &str,
    method_line: &str,
) -> Result<()> {
    let start_idx = lines
        .iter()
        .position(|line| line.contains(impl_header))
        .ok_or_else(|| anyhow::anyhow!("failed to find impl block '{impl_header}'"))?;

    let mut depth = count_braces(&lines[start_idx]);
    let mut idx = start_idx + 1;
    while idx < lines.len() {
        depth += count_braces(&lines[idx]);
        if depth == 0 {
            lines.insert(idx, method_line.to_string());
            lines.insert(idx + 1, "        DaoBase::new(&self.db)".to_string());
            lines.insert(idx + 2, "    }".to_string());
            return Ok(());
        }
        idx += 1;
    }
    bail!("failed to locate end of impl block '{impl_header}'");
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

fn reconstruct(original: &str, lines: &[String]) -> String {
    let mut out = lines.join("\n");
    if original.ends_with('\n') {
        out.push('\n');
    }
    out
}
