use std::{
    collections::{HashMap, HashSet},
    env,
    fs,
    path::{Path, PathBuf},
};

use quote::ToTokens;
use syn::{
    Attribute,
    Expr,
    ExprLit,
    ExprMethodCall,
    File,
    FnArg,
    GenericArgument,
    Item,
    ItemEnum,
    ItemFn,
    ItemStruct,
    Lit,
    LitStr,
    PatType,
    Path as SynPath,
    PathArguments,
    ReturnType,
    Type,
    visit::Visit,
    Fields,
    punctuated::Punctuated,
    Token,
};

#[derive(Debug, Clone)]
struct RouteEntry {
    method: String,
    path: String,
    source: String,
    request: String,
    response: String,
}

#[derive(Debug, Clone)]
struct EntityEntry {
    entity: String,
    table: String,
    columns: Vec<EntityColumnEntry>,
}

#[derive(Debug, Clone)]
struct EntityColumnEntry {
    name: String,
    rust_type: String,
    attributes: Vec<String>,
}

#[derive(Debug, Clone)]
struct HandlerInfo {
    request: String,
    response: String,
}

#[derive(Debug, Clone)]
struct FieldDoc {
    name: String,
    ty: String,
}

#[derive(Debug, Clone)]
struct TypeDoc {
    fields: Vec<FieldDoc>,
}

#[derive(Debug, Default)]
struct TypeRegistry {
    docs: HashMap<String, TypeDoc>,
}

#[derive(Debug, Default)]
struct FieldSeaOrmAttrs {
    column_name: Option<String>,
    primary_key: bool,
    unique: bool,
    unique_key: bool,
    indexed: bool,
    nullable: bool,
}

#[derive(Debug, Clone, Copy)]
enum ExtractorKind {
    Json,
    Query,
    Path,
}

impl ExtractorKind {
    fn label(self) -> &'static str {
        match self {
            ExtractorKind::Json => "json",
            ExtractorKind::Query => "query",
            ExtractorKind::Path => "path",
        }
    }
}

impl TypeDoc {
    fn render(&self) -> String {
        if self.fields.is_empty() {
            return "{}".to_string();
        }
        let mut parts = Vec::new();
        for field in &self.fields {
            parts.push(format!("\"{}\": {}", field.name, field.ty));
        }
        format!("{{ {} }}", parts.join(", "))
    }
}

#[derive(Debug, Clone)]
struct RouteHandler {
    method: String,
    handler: Option<String>,
}

struct RouteVisitor<'a> {
    source: String,
    handlers: &'a HashMap<String, HandlerInfo>,
    routes: Vec<RouteEntry>,
}

impl<'a, 'ast> Visit<'ast> for RouteVisitor<'a> {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method_name = node.method.to_string();
        if method_name == "route" {
            let path = node.args.first().and_then(extract_string_literal);
            if let Some(path) = path {
                let mut handlers = node
                    .args
                    .iter()
                    .nth(1)
                    .map(extract_route_handlers)
                    .unwrap_or_default();
                if handlers.is_empty() {
                    let methods = node
                        .args
                        .iter()
                        .nth(1)
                        .map(extract_methods)
                        .unwrap_or_default();
                    handlers = methods
                        .into_iter()
                        .map(|method| RouteHandler {
                            method,
                            handler: None,
                        })
                        .collect();
                }
                if handlers.is_empty() {
                    handlers.push(RouteHandler {
                        method: "ROUTE".to_string(),
                        handler: None,
                    });
                }
                for handler in handlers {
                    let (request, response) = handler
                        .handler
                        .as_ref()
                        .and_then(|name| self.handlers.get(name))
                        .map(|info| (info.request.clone(), info.response.clone()))
                        .unwrap_or_else(|| ("Unknown".to_string(), "Unknown".to_string()));
                    self.routes.push(RouteEntry {
                        method: handler.method,
                        path: path.clone(),
                        source: self.source.clone(),
                        request,
                        response,
                    });
                }
            } else {
                println!(
                    "cargo:warning=Skipping non-literal route path in {}",
                    self.source
                );
            }
        } else if method_name == "route_service" {
            if let Some(path) = node.args.first().and_then(extract_string_literal) {
                self.routes.push(RouteEntry {
                    method: "SERVICE".to_string(),
                    path,
                    source: self.source.clone(),
                    request: "N/A".to_string(),
                    response: "N/A".to_string(),
                });
            } else {
                println!(
                    "cargo:warning=Skipping non-literal route_service path in {}",
                    self.source
                );
            }
        }

        syn::visit::visit_expr_method_call(self, node);
    }
}

fn extract_string_literal(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Str(value),
            ..
        }) => Some(value.value()),
        Expr::Paren(expr) => extract_string_literal(&expr.expr),
        Expr::Reference(expr) => extract_string_literal(&expr.expr),
        _ => None,
    }
}

fn extract_route_handlers(expr: &Expr) -> Vec<RouteHandler> {
    let mut handlers = Vec::new();
    collect_route_handlers(expr, &mut handlers);
    handlers.reverse();
    handlers
}

fn collect_route_handlers(expr: &Expr, out: &mut Vec<RouteHandler>) {
    match expr {
        Expr::Call(call) => {
            if let Some(method) = method_from_expr(&call.func) {
                let handler = call.args.first().and_then(extract_handler_ident);
                out.push(RouteHandler {
                    method: method.to_string(),
                    handler,
                });
            }
        }
        Expr::MethodCall(method_call) => {
            if let Some(method) = normalize_method(&method_call.method.to_string()) {
                let handler = method_call.args.first().and_then(extract_handler_ident);
                out.push(RouteHandler {
                    method: method.to_string(),
                    handler,
                });
            }
            collect_route_handlers(&method_call.receiver, out);
        }
        Expr::Paren(expr) => collect_route_handlers(&expr.expr, out),
        Expr::Reference(expr) => collect_route_handlers(&expr.expr, out),
        _ => {}
    }
}

fn extract_handler_ident(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => path.path.segments.last().map(|seg| seg.ident.to_string()),
        Expr::Paren(expr) => extract_handler_ident(&expr.expr),
        Expr::Reference(expr) => extract_handler_ident(&expr.expr),
        _ => None,
    }
}

fn method_from_expr(expr: &Expr) -> Option<&'static str> {
    match expr {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .and_then(|segment| normalize_method(&segment.ident.to_string())),
        Expr::Paren(expr) => method_from_expr(&expr.expr),
        Expr::Reference(expr) => method_from_expr(&expr.expr),
        _ => None,
    }
}

fn extract_methods(expr: &Expr) -> Vec<String> {
    let mut names = Vec::new();
    collect_method_names(expr, &mut names);
    names.reverse();

    let mut methods = Vec::new();
    for name in names {
        if let Some(method) = normalize_method(&name) {
            if !methods.iter().any(|existing| existing == method) {
                methods.push(method.to_string());
            }
        }
    }

    methods
}

fn collect_method_names(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Call(call) => match &*call.func {
            Expr::Path(path) => {
                if let Some(segment) = path.path.segments.last() {
                    out.push(segment.ident.to_string());
                }
            }
            Expr::MethodCall(method_call) => {
                out.push(method_call.method.to_string());
                collect_method_names(&method_call.receiver, out);
            }
            Expr::Paren(expr) => collect_method_names(&expr.expr, out),
            Expr::Reference(expr) => collect_method_names(&expr.expr, out),
            _ => {}
        },
        Expr::MethodCall(method_call) => {
            out.push(method_call.method.to_string());
            collect_method_names(&method_call.receiver, out);
        }
        Expr::Paren(expr) => collect_method_names(&expr.expr, out),
        Expr::Reference(expr) => collect_method_names(&expr.expr, out),
        _ => {}
    }
}

fn normalize_method(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "get" => Some("GET"),
        "post" => Some("POST"),
        "put" => Some("PUT"),
        "delete" => Some("DELETE"),
        "patch" => Some("PATCH"),
        "head" => Some("HEAD"),
        "options" => Some("OPTIONS"),
        "trace" => Some("TRACE"),
        "connect" => Some("CONNECT"),
        "any" => Some("ANY"),
        _ => None,
    }
}

fn escape_rust_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn parse_rust_file(path: &Path) -> File {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", path.display(), err));
    syn::parse_file(&content)
        .unwrap_or_else(|err| panic!("failed to parse {}: {}", path.display(), err))
}

fn module_path_for_file(path: &Path, src_dir: &Path) -> String {
    let relative = path.strip_prefix(src_dir).unwrap_or(path);
    let mut parts = Vec::new();
    for component in relative.components() {
        if let std::path::Component::Normal(part) = component {
            let part = part.to_string_lossy();
            if part.ends_with(".rs") {
                let stem = Path::new(part.as_ref())
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                if stem != "mod" && stem != "lib" && stem != "main" {
                    parts.push(stem.to_string());
                }
            } else {
                parts.push(part.to_string());
            }
        }
    }
    parts.join("::")
}

fn collect_rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files_inner(root, &mut files);
    files.sort();
    files
}

fn collect_rust_files_inner(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", dir.display(), err));
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| panic!("failed to read dir entry: {}", err));
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files_inner(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

fn has_serde_derive(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let paths = attr.parse_args_with(Punctuated::<SynPath, Token![,]>::parse_terminated);
        if let Ok(paths) = paths {
            for path in paths {
                if let Some(segment) = path.segments.last() {
                    let ident = segment.ident.to_string();
                    if ident == "Serialize" || ident == "Deserialize" {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn has_derive_entity_model(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let paths = attr.parse_args_with(Punctuated::<SynPath, Token![,]>::parse_terminated);
        if let Ok(paths) = paths {
            for path in paths {
                if let Some(segment) = path.segments.last() {
                    if segment.ident == "DeriveEntityModel" {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn extract_table_name(attrs: &[Attribute]) -> Option<String> {
    for attr in attrs {
        if !attr.path().is_ident("sea_orm") {
            continue;
        }
        let mut table_name = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table_name") {
                let value: LitStr = meta.value()?.parse()?;
                table_name = Some(value.value());
            }
            Ok(())
        });
        if table_name.is_some() {
            return table_name;
        }
    }
    None
}

fn parse_field_sea_orm_attrs(attrs: &[Attribute]) -> FieldSeaOrmAttrs {
    let mut out = FieldSeaOrmAttrs::default();
    for attr in attrs {
        if !attr.path().is_ident("sea_orm") {
            continue;
        }
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("column_name") {
                let value: LitStr = meta.value()?.parse()?;
                out.column_name = Some(value.value());
            } else if meta.path.is_ident("primary_key") {
                out.primary_key = true;
            } else if meta.path.is_ident("unique") {
                out.unique = true;
            } else if meta.path.is_ident("unique_key") {
                out.unique_key = true;
            } else if meta.path.is_ident("indexed") {
                out.indexed = true;
            } else if meta.path.is_ident("nullable") {
                out.nullable = true;
            }
            Ok(())
        });
    }
    out
}

fn to_pascal_case(value: &str) -> String {
    let mut out = String::new();
    for part in value.split('_') {
        if part.is_empty() {
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            for ch in chars {
                out.extend(ch.to_lowercase());
            }
        }
    }
    out
}

fn column_variant_from_field(field_name: &str) -> String {
    let stripped = field_name.strip_prefix("r#").unwrap_or(field_name);
    to_pascal_case(stripped)
}

fn add_attribute(attrs: &mut Vec<String>, value: &str) {
    if !attrs.iter().any(|item| item == value) {
        attrs.push(value.to_string());
    }
}

fn extract_column_variant(value: &str) -> Option<String> {
    value.rsplit("::").next().map(|name| name.to_string())
}

fn collect_fk_columns(items: &[Item]) -> HashSet<String> {
    let mut columns = HashSet::new();
    for item in items {
        let Item::Enum(item_enum) = item else {
            continue;
        };
        if item_enum.ident != "Relation" {
            continue;
        }
        collect_fk_columns_from_enum(item_enum, &mut columns);
    }
    columns
}

fn collect_fk_columns_from_enum(item_enum: &ItemEnum, out: &mut HashSet<String>) {
    for variant in &item_enum.variants {
        for attr in &variant.attrs {
            if !attr.path().is_ident("sea_orm") {
                continue;
            }
            let mut belongs_to = false;
            let mut from_value: Option<String> = None;
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("belongs_to") {
                    belongs_to = true;
                } else if meta.path.is_ident("from") {
                    let value: LitStr = meta.value()?.parse()?;
                    from_value = Some(value.value());
                }
                Ok(())
            });
            if belongs_to {
                if let Some(value) = from_value.as_deref() {
                    if let Some(column) = extract_column_variant(value) {
                        out.insert(column);
                    }
                }
            }
        }
    }
}

fn collect_entity_entries(items: &[Item], module_path: &str, out: &mut Vec<EntityEntry>) {
    let fk_columns = collect_fk_columns(items);
    for item in items {
        match item {
            Item::Struct(item_struct) => {
                if has_derive_entity_model(&item_struct.attrs) {
                    if let Some(entity) =
                        build_entity_entry(item_struct, module_path, &fk_columns)
                    {
                        out.push(entity);
                    }
                }
            }
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    let nested_path = if module_path.is_empty() {
                        item_mod.ident.to_string()
                    } else {
                        format!("{}::{}", module_path, item_mod.ident)
                    };
                    collect_entity_entries(nested, &nested_path, out);
                }
            }
            _ => {}
        }
    }
}

fn build_entity_entry(
    item_struct: &ItemStruct,
    module_path: &str,
    fk_columns: &HashSet<String>,
) -> Option<EntityEntry> {
    let entity = module_path
        .split("::")
        .filter(|part| !part.is_empty())
        .last()
        .map(|value| value.to_string())?;
    let table = extract_table_name(&item_struct.attrs).unwrap_or_else(|| entity.clone());
    let columns = build_entity_columns(item_struct, fk_columns);
    Some(EntityEntry {
        entity,
        table,
        columns,
    })
}

fn build_entity_columns(
    item_struct: &ItemStruct,
    fk_columns: &HashSet<String>,
) -> Vec<EntityColumnEntry> {
    let mut columns = Vec::new();
    let Fields::Named(named) = &item_struct.fields else {
        return columns;
    };
    for field in &named.named {
        let mut name = field
            .ident
            .as_ref()
            .map(|ident| ident.to_string())
            .unwrap_or_else(|| "field".to_string());
        if let Some(stripped) = name.strip_prefix("r#") {
            name = stripped.to_string();
        }
        let attrs = parse_field_sea_orm_attrs(&field.attrs);
        let rust_type = type_to_string(&field.ty);
        let column_variant = column_variant_from_field(&name);
        let column_name = attrs.column_name.clone().unwrap_or_else(|| name.clone());

        let mut attributes = Vec::new();
        if attrs.primary_key {
            add_attribute(&mut attributes, "Primary Key");
        }
        if fk_columns.contains(&column_variant) {
            add_attribute(&mut attributes, "Foreign Key");
        }
        if attrs.unique || attrs.unique_key {
            add_attribute(&mut attributes, "Unique");
        }
        if attrs.indexed {
            add_attribute(&mut attributes, "Indexed");
        }
        if attrs.nullable || is_option_type(&field.ty) {
            add_attribute(&mut attributes, "Nullable");
        }

        columns.push(EntityColumnEntry {
            name: column_name,
            rust_type,
            attributes,
        });
    }
    columns
}

fn build_struct_doc(item_struct: &ItemStruct) -> TypeDoc {
    let mut fields = Vec::new();
    match &item_struct.fields {
        Fields::Named(named) => {
            for field in &named.named {
                let mut name = field
                    .ident
                    .as_ref()
                    .map(|ident| ident.to_string())
                    .unwrap_or_else(|| "field".to_string());
                if let Some(stripped) = name.strip_prefix("r#") {
                    name = stripped.to_string();
                }
                let ty = type_to_string(&field.ty);
                fields.push(FieldDoc { name, ty });
            }
        }
        Fields::Unnamed(unnamed) => {
            for (index, field) in unnamed.unnamed.iter().enumerate() {
                let name = index.to_string();
                let ty = type_to_string(&field.ty);
                fields.push(FieldDoc { name, ty });
            }
        }
        Fields::Unit => {}
    }
    TypeDoc { fields }
}

fn register_type_doc(
    registry: &mut TypeRegistry,
    module_path: &str,
    name: &str,
    doc: TypeDoc,
) {
    registry.docs.entry(name.to_string()).or_insert_with(|| doc.clone());
    if !module_path.is_empty() {
        let qualified = format!("{}::{}", module_path, name);
        registry.docs.entry(qualified).or_insert(doc);
    }
}

fn collect_type_docs(file: &File, module_path: &str, registry: &mut TypeRegistry) {
    for item in &file.items {
        if let Item::Struct(item_struct) = item {
            if has_serde_derive(&item_struct.attrs) {
                let doc = build_struct_doc(item_struct);
                let name = item_struct.ident.to_string();
                register_type_doc(registry, module_path, &name, doc);
            }
        }
    }
}

fn type_to_string(ty: &Type) -> String {
    compact_type_string(ty.to_token_stream().to_string())
}

fn compact_type_string(mut value: String) -> String {
    for (from, to) in [
        (" :: ", "::"),
        (" < ", "<"),
        (" >", ">"),
        (" , ", ", "),
        (" & ", "&"),
        ("& '", "&'"),
        (" * ", "*"),
        (" ( ", "("),
        (" ) ", ")"),
        (" [ ", "["),
        (" ] ", "]"),
    ] {
        value = value.replace(from, to);
    }
    while value.contains("  ") {
        value = value.replace("  ", " ");
    }
    value
}

fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

fn type_path_parts(ty: &Type) -> (Option<String>, Option<String>) {
    match ty {
        Type::Path(type_path) => {
            let mut segments = Vec::new();
            for segment in &type_path.path.segments {
                segments.push(segment.ident.to_string());
            }
            if segments.is_empty() {
                return (None, None);
            }
            let full = segments.join("::");
            let last = segments.last().cloned();
            (Some(full), last)
        }
        Type::Reference(reference) => type_path_parts(&reference.elem),
        Type::Paren(paren) => type_path_parts(&paren.elem),
        _ => (None, None),
    }
}

fn resolve_type_doc<'a>(
    registry: &'a TypeRegistry,
    ty: &Type,
    module_path: &str,
) -> Option<&'a TypeDoc> {
    let (full_path, last_ident) = type_path_parts(ty);
    if let Some(full_path) = full_path {
        if let Some(doc) = registry.docs.get(&full_path) {
            return Some(doc);
        }
        if let Some(stripped) = full_path.strip_prefix("crate::") {
            if let Some(doc) = registry.docs.get(stripped) {
                return Some(doc);
            }
        }
    }
    if let Some(last_ident) = last_ident {
        if !module_path.is_empty() {
            let scoped = format!("{}::{}", module_path, last_ident);
            if let Some(doc) = registry.docs.get(&scoped) {
                return Some(doc);
            }
        }
        if let Some(doc) = registry.docs.get(&last_ident) {
            return Some(doc);
        }
    }
    None
}

fn is_serde_json_value(ty: &Type) -> bool {
    matches!(type_path_parts(ty).0.as_deref(), Some("serde_json::Value"))
}

fn extract_generic_inner<'a>(ty: &'a Type, ident: &str) -> Option<&'a Type> {
    match ty {
        Type::Path(type_path) => {
            let last = type_path.path.segments.last()?;
            if last.ident != ident {
                return None;
            }
            if let PathArguments::AngleBracketed(args) = &last.arguments {
                for arg in &args.args {
                    if let GenericArgument::Type(inner) = arg {
                        return Some(inner);
                    }
                }
            }
            None
        }
        Type::Reference(reference) => extract_generic_inner(&reference.elem, ident),
        Type::Paren(paren) => extract_generic_inner(&paren.elem, ident),
        _ => None,
    }
}

fn is_option_type(ty: &Type) -> bool {
    extract_generic_inner(ty, "Option").is_some()
}

fn extract_generic_types<'a>(ty: &'a Type, ident: &str) -> Vec<&'a Type> {
    match ty {
        Type::Path(type_path) => {
            let last = match type_path.path.segments.last() {
                Some(segment) => segment,
                None => return Vec::new(),
            };
            if last.ident != ident {
                return Vec::new();
            }
            if let PathArguments::AngleBracketed(args) = &last.arguments {
                let mut out = Vec::new();
                for arg in &args.args {
                    if let GenericArgument::Type(inner) = arg {
                        out.push(inner);
                    }
                }
                return out;
            }
            Vec::new()
        }
        Type::Reference(reference) => extract_generic_types(&reference.elem, ident),
        Type::Paren(paren) => extract_generic_types(&paren.elem, ident),
        _ => Vec::new(),
    }
}

fn describe_type(ty: &Type, module_path: &str, registry: &TypeRegistry) -> String {
    if is_unit_type(ty) {
        return "None".to_string();
    }
    if let Some(inner) = extract_generic_inner(ty, "Option") {
        return format!("Option<{}>", describe_type(inner, module_path, registry));
    }
    if let Some(inner) = extract_generic_inner(ty, "Vec") {
        return format!("Vec<{}>", describe_type(inner, module_path, registry));
    }
    if let Some(inner) = extract_generic_inner(ty, "Arc") {
        return format!("Arc<{}>", describe_type(inner, module_path, registry));
    }
    if let Some(doc) = resolve_type_doc(registry, ty, module_path) {
        return doc.render();
    }
    if is_serde_json_value(ty) {
        return "JSON".to_string();
    }
    type_to_string(ty)
}

fn extract_request_extractor<'a>(ty: &'a Type) -> Option<(ExtractorKind, &'a Type)> {
    if let Some(inner) = extract_generic_inner(ty, "Json") {
        return Some((ExtractorKind::Json, inner));
    }
    if let Some(inner) = extract_generic_inner(ty, "Query") {
        return Some((ExtractorKind::Query, inner));
    }
    if let Some(inner) = extract_generic_inner(ty, "Path") {
        return Some((ExtractorKind::Path, inner));
    }
    None
}

fn format_request(parts: Vec<(ExtractorKind, String)>) -> String {
    if parts.is_empty() {
        return "None".to_string();
    }
    if parts.len() == 1 {
        let (kind, desc) = parts[0].clone();
        return if matches!(kind, ExtractorKind::Json) {
            desc
        } else {
            format!("{}: {}", kind.label(), desc)
        };
    }
    let mut formatted = Vec::new();
    for (kind, desc) in parts {
        formatted.push(format!("{}: {}", kind.label(), desc));
    }
    formatted.join(" | ")
}

fn describe_response_ok(ty: &Type, module_path: &str, registry: &TypeRegistry) -> String {
    if let Some(inner) = extract_generic_inner(ty, "Json") {
        return describe_type(inner, module_path, registry);
    }
    if let Some(inner) = extract_generic_inner(ty, "Html") {
        return format!("Html<{}>", describe_type(inner, module_path, registry));
    }
    if is_unit_type(ty) {
        return "None".to_string();
    }
    describe_type(ty, module_path, registry)
}

fn describe_response_type(ty: &Type, module_path: &str, registry: &TypeRegistry) -> String {
    let result_args = extract_generic_types(ty, "Result");
    if result_args.len() >= 2 {
        let ok_desc = describe_response_ok(result_args[0], module_path, registry);
        let err_desc = describe_type(result_args[1], module_path, registry);
        return format!("ok: {}, err: {}", ok_desc, err_desc);
    }
    describe_response_ok(ty, module_path, registry)
}

fn build_handler_info(
    item_fn: &ItemFn,
    module_path: &str,
    registry: &TypeRegistry,
) -> HandlerInfo {
    let mut request_parts = Vec::new();
    for input in &item_fn.sig.inputs {
        if let FnArg::Typed(PatType { ty, .. }) = input {
            if let Some((kind, inner)) = extract_request_extractor(ty) {
                let desc = describe_type(inner, module_path, registry);
                request_parts.push((kind, desc));
            }
        }
    }
    let request = format_request(request_parts);
    let response = match &item_fn.sig.output {
        ReturnType::Default => "None".to_string(),
        ReturnType::Type(_, ty) => describe_response_type(ty, module_path, registry),
    };
    HandlerInfo { request, response }
}

fn collect_handlers(
    file: &File,
    module_path: &str,
    registry: &TypeRegistry,
) -> HashMap<String, HandlerInfo> {
    let mut handlers = HashMap::new();
    for item in &file.items {
        if let Item::Fn(item_fn) = item {
            handlers.insert(
                item_fn.sig.ident.to_string(),
                build_handler_info(item_fn, module_path, registry),
            );
        }
    }
    handlers
}

fn parse_routes_file(
    path: &Path,
    manifest_dir: &Path,
    src_dir: &Path,
    registry: &TypeRegistry,
) -> Vec<RouteEntry> {
    let parsed = parse_rust_file(path);
    let module_path = module_path_for_file(path, src_dir);
    let handlers = collect_handlers(&parsed, &module_path, registry);
    let source = path
        .strip_prefix(manifest_dir)
        .unwrap_or(path)
        .display()
        .to_string();
    let mut visitor = RouteVisitor {
        source,
        handlers: &handlers,
        routes: Vec::new(),
    };
    visitor.visit_file(&parsed);
    visitor.routes
}

fn collect_route_files(routes_dir: &Path) -> Vec<PathBuf> {
    collect_rust_files(routes_dir)
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR");
    let manifest_path = Path::new(&manifest_dir);
    let src_dir = manifest_path.join("src");
    let routes_dir = src_dir.join("routes");

    let src_files = collect_rust_files(&src_dir);
    for file in &src_files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let mut registry = TypeRegistry::default();
    for file in &src_files {
        let parsed = parse_rust_file(file);
        let module_path = module_path_for_file(file, &src_dir);
        collect_type_docs(&parsed, &module_path, &mut registry);
    }

    let mut routes = Vec::new();
    for file in collect_route_files(&routes_dir) {
        routes.extend(parse_routes_file(&file, manifest_path, &src_dir, &registry));
    }

    routes.sort_by(|a, b| a.path.cmp(&b.path).then(a.method.cmp(&b.method)));

    let mut entities = Vec::new();
    for file in &src_files {
        let parsed = parse_rust_file(file);
        let module_path = module_path_for_file(file, &src_dir);
        collect_entity_entries(&parsed.items, &module_path, &mut entities);
    }

    entities.sort_by(|a, b| a.entity.cmp(&b.entity));

    let out_dir = env::var("OUT_DIR").expect("missing OUT_DIR");
    let out_path = Path::new(&out_dir).join("routes_generated.rs");
    let mut output = String::from("pub static ROUTES: &[RouteInfo] = &[\n");
    for route in routes {
        output.push_str(&format!(
            "    RouteInfo {{ method: \"{}\", path: \"{}\", source: \"{}\", request: \"{}\", response: \"{}\" }},\n",
            escape_rust_string(&route.method),
            escape_rust_string(&route.path),
            escape_rust_string(&route.source),
            escape_rust_string(&route.request),
            escape_rust_string(&route.response)
        ));
    }
    output.push_str("];\n");

    fs::write(&out_path, output)
        .unwrap_or_else(|err| panic!("failed to write {}: {}", out_path.display(), err));

    let entity_out_path = Path::new(&out_dir).join("entities_generated.rs");
    let mut entity_output = String::from("pub static ENTITIES: &[EntityInfo] = &[\n");
    for entity in entities {
        entity_output.push_str(&format!(
            "    EntityInfo {{ entity: \"{}\", table: \"{}\", column_count: {}, columns: &[\n",
            escape_rust_string(&entity.entity),
            escape_rust_string(&entity.table),
            entity.columns.len()
        ));
        for column in entity.columns {
            let attributes = if column.attributes.is_empty() {
                "None".to_string()
            } else {
                column.attributes.join(", ")
            };
            entity_output.push_str(&format!(
                "        EntityColumnInfo {{ name: \"{}\", rust_type: \"{}\", attributes: \"{}\" }},\n",
                escape_rust_string(&column.name),
                escape_rust_string(&column.rust_type),
                escape_rust_string(&attributes)
            ));
        }
        entity_output.push_str("    ] },\n");
    }
    entity_output.push_str("];\n");

    fs::write(&entity_out_path, entity_output).unwrap_or_else(|err| {
        panic!(
            "failed to write {}: {}",
            entity_out_path.display(),
            err
        )
    });
}
