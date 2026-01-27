use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use syn::{
    punctuated::Punctuated,
    visit::Visit,
    Attribute,
    Block,
    Expr,
    ExprCall,
    ExprLit,
    ExprMethodCall,
    ExprStruct,
    File,
    FnArg,
    ImplItem,
    Item,
    ItemFn,
    ItemImpl,
    ItemStruct,
    Lit,
    Pat,
    PatType,
    Path as SynPath,
    ReturnType,
    Stmt,
    Token,
    Type,
};

use crate::utils::{
    collect_rust_files,
    entity_name_from_type,
    escape_rust_string,
    extract_generic_inner,
    extract_generic_types,
    is_unit_type,
    module_path_for_file,
    parse_rust_file,
    to_pascal_case,
    type_path_parts,
    type_to_string,
};

#[derive(Debug, Clone)]
pub(crate) struct RouteEntry {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) source: String,
    pub(crate) request: String,
    pub(crate) response: String,
    pub(crate) required_headers: String,
    pub(crate) curl: String,
}

#[derive(Debug, Clone)]
struct HandlerInfo {
    request: String,
    response: String,
    auth_required: bool,
    required_headers: String,
}

#[derive(Debug, Clone)]
struct FieldDoc {
    name: String,
    ty: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TypeDoc {
    fields: Vec<FieldDoc>,
}

#[derive(Debug, Default)]
pub(crate) struct TypeRegistry {
    docs: HashMap<String, TypeDoc>,
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

const CURL_BASE_URL_PLACEHOLDER: &str = "{BASE_URL}";

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

    fn render_with<F>(&self, expand: F) -> String
    where
        F: Fn(&str) -> String,
    {
        if self.fields.is_empty() {
            return "{}".to_string();
        }
        let mut parts = Vec::new();
        for field in &self.fields {
            parts.push(format!("\"{}\": {}", field.name, expand(&field.ty)));
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
    route_bindings: Option<&'a HashMap<String, Vec<RouteHandler>>>,
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
                    if let Some(bindings) = self.route_bindings {
                        if let Some(bound) = node
                            .args
                            .iter()
                            .nth(1)
                            .and_then(|expr| resolve_route_binding(expr, bindings))
                        {
                            handlers = bound;
                        }
                    }
                }
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
                    let (request, response, auth_required, required_headers) = handler
                        .handler
                        .as_ref()
                        .and_then(|name| self.handlers.get(name))
                        .map(|info| {
                            (
                                info.request.clone(),
                                info.response.clone(),
                                info.auth_required,
                                info.required_headers.clone(),
                            )
                        })
                        .unwrap_or_else(|| {
                            (
                                "Unknown".to_string(),
                                "Unknown".to_string(),
                                false,
                                "None".to_string(),
                            )
                        });
                    let curl = build_curl(&handler.method, &path, &request, auth_required);
                    self.routes.push(RouteEntry {
                        method: handler.method,
                        path: path.clone(),
                        source: self.source.clone(),
                        request,
                        response,
                        required_headers,
                        curl,
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
                let curl = build_curl("SERVICE", &path, "N/A", false);
                self.routes.push(RouteEntry {
                    method: "SERVICE".to_string(),
                    path,
                    source: self.source.clone(),
                    request: "N/A".to_string(),
                    response: "N/A".to_string(),
                    required_headers: "None".to_string(),
                    curl,
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

fn resolve_route_binding(
    expr: &Expr,
    bindings: &HashMap<String, Vec<RouteHandler>>,
) -> Option<Vec<RouteHandler>> {
    match expr {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .and_then(|seg| bindings.get(&seg.ident.to_string()).cloned()),
        Expr::Paren(expr) => resolve_route_binding(&expr.expr, bindings),
        Expr::Reference(expr) => resolve_route_binding(&expr.expr, bindings),
        _ => None,
    }
}

#[derive(Debug)]
struct CrudRouterCall {
    base_path: String,
    service: Option<String>,
}

struct CrudRouterVisitor<'a> {
    consts: &'a HashMap<String, String>,
    locals: &'a HashMap<String, String>,
    calls: Vec<CrudRouterCall>,
    unresolved: usize,
}

impl<'a, 'ast> Visit<'ast> for CrudRouterVisitor<'a> {
    fn visit_expr_call(&mut self, node: &'ast ExprCall) {
        if is_crud_api_router_new(&node.func) {
            let base_arg = node.args.iter().nth(1);
            if let Some(base_arg) = base_arg {
                if let Some(base) = extract_string_literal_or_const(base_arg, self.consts) {
                    let service = node
                        .args
                        .first()
                        .and_then(|expr| resolve_service_type(expr, self.locals));
                    self.calls.push(CrudRouterCall {
                        base_path: base,
                        service,
                    });
                } else {
                    self.unresolved += 1;
                }
            } else {
                self.unresolved += 1;
            }
        }

        syn::visit::visit_expr_call(self, node);
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

fn extract_string_literal_or_const(
    expr: &Expr,
    consts: &HashMap<String, String>,
) -> Option<String> {
    if let Some(value) = extract_string_literal(expr) {
        return Some(value);
    }

    match expr {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .and_then(|seg| consts.get(&seg.ident.to_string()).cloned()),
        Expr::Paren(expr) => extract_string_literal_or_const(&expr.expr, consts),
        Expr::Reference(expr) => extract_string_literal_or_const(&expr.expr, consts),
        _ => None,
    }
}

fn resolve_service_type(expr: &Expr, locals: &HashMap<String, String>) -> Option<String> {
    if let Some(ty) = infer_type_from_expr(expr, locals) {
        return Some(ty);
    }

    match expr {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .and_then(|seg| locals.get(&seg.ident.to_string()).cloned()),
        Expr::Paren(expr) => resolve_service_type(&expr.expr, locals),
        Expr::Reference(expr) => resolve_service_type(&expr.expr, locals),
        _ => None,
    }
}

fn infer_type_from_expr(expr: &Expr, locals: &HashMap<String, String>) -> Option<String> {
    match expr {
        Expr::Call(call) => type_from_constructor(&call.func),
        Expr::MethodCall(method_call) => {
            if method_call.method == "clone" {
                return infer_type_from_expr(&method_call.receiver, locals)
                    .or_else(|| resolve_ident_type(&method_call.receiver, locals));
            }
            None
        }
        Expr::Struct(ExprStruct { path, .. }) => type_from_path(path),
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .and_then(|seg| locals.get(&seg.ident.to_string()).cloned()),
        Expr::Paren(expr) => infer_type_from_expr(&expr.expr, locals),
        Expr::Reference(expr) => infer_type_from_expr(&expr.expr, locals),
        _ => None,
    }
}

fn resolve_ident_type(expr: &Expr, locals: &HashMap<String, String>) -> Option<String> {
    let Expr::Path(path) = expr else {
        return None;
    };
    let Some(segment) = path.path.segments.last() else {
        return None;
    };
    locals.get(&segment.ident.to_string()).cloned()
}

fn type_from_constructor(expr: &Expr) -> Option<String> {
    let path = match expr {
        Expr::Path(path) => Some(&path.path),
        Expr::Paren(expr) => match &*expr.expr {
            Expr::Path(path) => Some(&path.path),
            _ => None,
        },
        Expr::Reference(expr) => match &*expr.expr {
            Expr::Path(path) => Some(&path.path),
            _ => None,
        },
        _ => None,
    }?;

    let mut segments = path.segments.iter().map(|seg| seg.ident.to_string());
    let parts: Vec<String> = segments.by_ref().collect();
    if parts.len() < 2 {
        return None;
    }
    let last = parts.last().map(String::as_str);
    let prev = parts.get(parts.len().saturating_sub(2)).map(String::as_str);
    match (prev, last) {
        (Some(type_name), Some(method)) if matches!(method, "new" | "from" | "default") => {
            Some(type_name.to_string())
        }
        _ => None,
    }
}

fn type_from_struct(expr: &ExprStruct) -> Option<String> {
    type_from_path(&expr.path)
}

fn type_from_path(path: &SynPath) -> Option<String> {
    path.segments.last().map(|seg| seg.ident.to_string())
}

fn is_crud_api_router_new(expr: &Expr) -> bool {
    match expr {
        Expr::Path(path) => {
            let segments = &path.path.segments;
            if segments.len() < 2 {
                return false;
            }
            let last = segments.last().map(|seg| seg.ident.to_string());
            let prev = segments.iter().nth_back(1).map(|seg| seg.ident.to_string());
            matches!(
                (prev.as_deref(), last.as_deref()),
                (Some("CrudApiRouter"), Some("new"))
            )
        }
        Expr::Paren(expr) => is_crud_api_router_new(&expr.expr),
        Expr::Reference(expr) => is_crud_api_router_new(&expr.expr),
        _ => false,
    }
}

fn collect_const_string_literals(file: &File) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for item in &file.items {
        if let Item::Const(item_const) = item {
            if let Some(value) = extract_string_literal(item_const.expr.as_ref()) {
                out.insert(item_const.ident.to_string(), value);
            }
        }
    }
    out
}

fn collect_local_types(block: &Block) -> HashMap<String, String> {
    let mut locals = HashMap::new();
    for stmt in &block.stmts {
        if let Stmt::Local(local) = stmt {
            if let Some((name, ty)) = local_binding_type(local, &locals) {
                locals.insert(name, ty);
            }
        }
    }
    locals
}

fn local_binding_type(
    local: &syn::Local,
    locals: &HashMap<String, String>,
) -> Option<(String, String)> {
    let (name, explicit_type) = match &local.pat {
        Pat::Ident(ident) => (ident.ident.to_string(), None),
        Pat::Type(PatType { pat, ty, .. }) => {
            let ident = match pat.as_ref() {
                Pat::Ident(ident) => ident.ident.to_string(),
                _ => return None,
            };
            let ty = type_from_type(ty);
            (ident, ty)
        }
        _ => return None,
    };

    if let Some(ty) = explicit_type {
        return Some((name, ty));
    }

    let init = local.init.as_ref().map(|init| init.expr.as_ref());
    if let Some(expr) = init {
        if let Some(ty) = infer_type_from_expr(expr, locals) {
            return Some((name, ty));
        }
    }
    None
}

fn type_from_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => type_from_path(&path.path),
        Type::Reference(reference) => type_from_type(&reference.elem),
        Type::Paren(paren) => type_from_type(&paren.elem),
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
        Expr::MethodCall(call) => {
            if let Some(method) = normalize_method(&call.method.to_string()) {
                let handler = call.args.first().and_then(extract_handler_ident);
                out.push(RouteHandler {
                    method: method.to_string(),
                    handler,
                });
            }
            collect_route_handlers(&call.receiver, out);
        }
        Expr::Paren(expr) => collect_route_handlers(&expr.expr, out),
        Expr::Reference(expr) => collect_route_handlers(&expr.expr, out),
        _ => {}
    }
}

fn extract_handler_ident(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Expr::Reference(expr) => extract_handler_ident(&expr.expr),
        Expr::Paren(expr) => extract_handler_ident(&expr.expr),
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

fn is_relation_field(ty: &Type) -> bool {
    let (_, last) = type_path_parts(ty);
    matches!(last.as_deref(), Some("HasOne") | Some("HasMany"))
}

fn build_struct_doc(item_struct: &ItemStruct) -> TypeDoc {
    let mut fields = Vec::new();
    match &item_struct.fields {
        syn::Fields::Named(named) => {
            for field in &named.named {
                if is_relation_field(&field.ty) {
                    continue;
                }
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
        syn::Fields::Unnamed(unnamed) => {
            for (index, field) in unnamed.unnamed.iter().enumerate() {
                if is_relation_field(&field.ty) {
                    continue;
                }
                let name = index.to_string();
                let ty = type_to_string(&field.ty);
                fields.push(FieldDoc { name, ty });
            }
        }
        syn::Fields::Unit => {}
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

pub(crate) fn collect_type_docs(file: &File, module_path: &str, registry: &mut TypeRegistry) {
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

fn render_type_doc(
    doc: &TypeDoc,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
    depth: usize,
) -> String {
    doc.render_with(|ty| expand_type_string(ty, registry, context, depth))
}

fn expand_type_string(
    ty: &str,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
    depth: usize,
) -> String {
    let parsed = syn::parse_str::<Type>(ty);
    let Ok(parsed) = parsed else {
        return ty.to_string();
    };
    expand_type(&parsed, registry, context, depth)
}

fn expand_type(
    ty: &Type,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
    depth: usize,
) -> String {
    if let Some(inner) = extract_generic_inner(ty, "HasMany") {
        if let Some(model) = expand_entity_to_model(inner, registry, context, depth) {
            return format!("Vec<{}>", model);
        }
    }
    if let Some(inner) = extract_generic_inner(ty, "HasOne") {
        if let Some(model) = expand_entity_to_model(inner, registry, context, depth) {
            return format!("Option<{}>", model);
        }
    }
    if let Some(model) = expand_entity_to_model(ty, registry, context, depth) {
        return model;
    }
    type_to_string(ty)
}

fn expand_entity_to_model(
    ty: &Type,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
    depth: usize,
) -> Option<String> {
    if depth >= 1 {
        return None;
    }
    let entity = entity_name_from_type(ty)?;
    let model_path = resolve_model_path(&entity, context)?;
    Some(render_model_doc(&model_path, registry, context, depth + 1))
}

fn resolve_model_path(entity: &str, context: &CrudTypeContext) -> Option<String> {
    if let Some(model) = context.entity_to_model.get(entity) {
        return Some(model.clone());
    }
    let pascal = to_pascal_case(entity);
    context.entity_to_model.get(&pascal).cloned()
}

fn render_model_doc(
    model_path: &str,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
    depth: usize,
) -> String {
    if let Some(doc) = registry.docs.get(model_path) {
        return render_type_doc(doc, registry, context, depth);
    }
    model_path.to_string()
}

fn is_serde_json_value(ty: &Type) -> bool {
    matches!(type_path_parts(ty).0.as_deref(), Some("serde_json::Value"))
}

fn describe_type(
    ty: &Type,
    module_path: &str,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
) -> String {
    if is_unit_type(ty) {
        return "None".to_string();
    }
    if let Some(inner) = extract_generic_inner(ty, "Option") {
        return format!(
            "Option<{}>",
            describe_type(inner, module_path, registry, context)
        );
    }
    if let Some(inner) = extract_generic_inner(ty, "Vec") {
        return format!("Vec<{}>", describe_type(inner, module_path, registry, context));
    }
    if let Some(inner) = extract_generic_inner(ty, "Arc") {
        return format!("Arc<{}>", describe_type(inner, module_path, registry, context));
    }
    if let Some(doc) = resolve_type_doc(registry, ty, module_path) {
        return render_type_doc(doc, registry, context, 0);
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

fn is_auth_guard_type(ty: &Type) -> bool {
    let (_, last) = type_path_parts(ty);
    matches!(
        last.as_deref(),
        Some("AuthGuard") | Some("AuthRoleGuard") | Some("Claims")
    )
}

fn build_required_headers(auth_required: bool, has_json_body: bool) -> String {
    let mut headers = Vec::new();
    if auth_required {
        headers.push("Authorization: Bearer $ACCESS_TOKEN");
    }
    if has_json_body {
        headers.push("Content-Type: application/json");
    }
    if headers.is_empty() {
        "None".to_string()
    } else {
        headers.join(" | ")
    }
}

fn build_curl(method: &str, path: &str, request: &str, auth_required: bool) -> String {
    let parts = split_request_parts(request);
    let mut query = None;
    let mut json_body = None;

    for part in parts {
        let trimmed = part.trim();
        if let Some(rest) = trimmed.strip_prefix("query:") {
            if let Some(built) = build_query(rest.trim()) {
                query = Some(built);
            }
            continue;
        }
        if trimmed.starts_with('{') && trimmed.ends_with('}') {
            json_body = Some(build_json_body(trimmed));
        }
    }
    if json_body.is_none() {
        let trimmed = request.trim();
        if !(trimmed.is_empty()
            || trimmed == "None"
            || trimmed == "N/A"
            || trimmed == "Unknown"
            || trimmed.starts_with("path:")
            || trimmed.starts_with("query:"))
        {
            json_body = Some(sample_json_value(trimmed));
        }
    }

    let method = match method {
        "ROUTE" | "SERVICE" => "GET",
        _ => method,
    };

    let mut url = format!("{}{}", CURL_BASE_URL_PLACEHOLDER, path);
    if let Some(query) = query {
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
    }

    let mut cmd = format!("curl -sS -X {} \"{}\"", method, url);
    if auth_required {
        cmd.push_str(" \\\n  -H \"Authorization: Bearer $ACCESS_TOKEN\"");
    }
    if let Some(body) = json_body {
        let escaped = escape_single_quotes(&body);
        cmd.push_str(" \\\n  -H 'content-type: application/json' \\\n  -d '");
        cmd.push_str(&escaped);
        cmd.push('\'');
    }
    cmd
}

fn split_request_parts(request: &str) -> Vec<&str> {
    if request.contains(" | ") {
        request.split(" | ").collect()
    } else {
        vec![request]
    }
}

fn build_query(desc: &str) -> Option<String> {
    let fields = parse_object_fields(desc);
    if fields.is_empty() {
        return None;
    }
    let pairs: Vec<String> = fields
        .into_iter()
        .map(|(key, ty)| format!("{}={}", key, sample_query_value(&ty)))
        .collect();
    Some(pairs.join("&"))
}

fn build_json_body(desc: &str) -> String {
    let fields = parse_object_fields(desc);
    if fields.is_empty() {
        return "{}".to_string();
    }
    let parts: Vec<String> = fields
        .into_iter()
        .map(|(key, ty)| format!("\"{}\": {}", key, sample_json_value(&ty)))
        .collect();
    format!("{{ {} }}", parts.join(", "))
}

fn parse_object_fields(desc: &str) -> Vec<(String, String)> {
    let trimmed = desc.trim();
    if !(trimmed.starts_with('{') && trimmed.ends_with('}')) {
        return Vec::new();
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (idx, ch) in inner.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(inner[start..idx].to_string());
                start = idx + 1;
            }
            _ => {}
        }
    }
    if start < inner.len() {
        parts.push(inner[start..].to_string());
    }

    let mut out = Vec::new();
    for part in parts {
        let piece = part.trim();
        if piece.is_empty() {
            continue;
        }
        let mut splitter = piece.splitn(2, ':');
        let key = splitter.next().unwrap_or("").trim();
        let ty = splitter.next().unwrap_or("").trim();
        if key.is_empty() || ty.is_empty() {
            continue;
        }
        let key = key.trim_matches('"').to_string();
        out.push((key, ty.to_string()));
    }
    out
}

fn sample_json_value(ty: &str) -> String {
    let cleaned = ty.trim();
    if cleaned.starts_with("Option<") {
        return "null".to_string();
    }
    if cleaned.starts_with("Vec<") {
        return "[]".to_string();
    }
    if cleaned.starts_with('{') {
        return "{}".to_string();
    }
    match cleaned {
        "String" | "&str" => "\"string\"".to_string(),
        "bool" => "false".to_string(),
        "Uuid" => "\"00000000-0000-0000-0000-000000000000\"".to_string(),
        "JSON" => "{}".to_string(),
        "i16" | "i32" | "i64" | "isize" | "u16" | "u32" | "u64" | "usize" => "0".to_string(),
        "DateTimeWithTimeZone" => "\"2024-01-01T00:00:00Z\"".to_string(),
        _ => "\"value\"".to_string(),
    }
}

fn sample_query_value(ty: &str) -> String {
    let cleaned = ty.trim();
    if cleaned.starts_with("Option<") {
        return "1".to_string();
    }
    if cleaned.starts_with("Vec<") {
        return "value".to_string();
    }
    if cleaned.starts_with('{') {
        return "value".to_string();
    }
    match cleaned {
        "String" | "&str" => "string".to_string(),
        "bool" => "true".to_string(),
        "Uuid" => "00000000-0000-0000-0000-000000000000".to_string(),
        "JSON" => "value".to_string(),
        "i16" | "i32" | "i64" | "isize" | "u16" | "u32" | "u64" | "usize" => "1".to_string(),
        "DateTimeWithTimeZone" => "2024-01-01T00:00:00Z".to_string(),
        _ => "value".to_string(),
    }
}

fn escape_single_quotes(value: &str) -> String {
    value.replace('\'', "'\\''")
}

fn describe_response_ok(
    ty: &Type,
    module_path: &str,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
) -> String {
    if let Some(inner) = extract_generic_inner(ty, "Json") {
        return describe_type(inner, module_path, registry, context);
    }
    if let Some(inner) = extract_generic_inner(ty, "Html") {
        return format!(
            "Html<{}>",
            describe_type(inner, module_path, registry, context)
        );
    }
    if let Some(inner) = extract_generic_inner(ty, "JsonApiResponse") {
        let data_desc = describe_type(inner, module_path, registry, context);
        return format!(
            "{{ \"status\": u16, \"message\": String, \"data\": {} }}",
            data_desc
        );
    }
    if is_unit_type(ty) {
        return "None".to_string();
    }
    describe_type(ty, module_path, registry, context)
}

fn describe_response_type(
    ty: &Type,
    module_path: &str,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
) -> String {
    if let Some(inner) = extract_generic_inner(ty, "ApiResult") {
        let data_desc = describe_type(inner, module_path, registry, context);
        let ok_desc = format!(
            "{{ \"status\": u16, \"message\": String, \"data\": {} }}",
            data_desc
        );
        return ok_desc;
    }
    let result_args = extract_generic_types(ty, "Result");
    if result_args.len() >= 2 {
        let ok_desc = describe_response_ok(result_args[0], module_path, registry, context);
        let err_desc = describe_type(result_args[1], module_path, registry, context);
        if err_desc == "AppError" {
            return ok_desc;
        }
        return format!("{} | err: {}", ok_desc, err_desc);
    }
    describe_response_ok(ty, module_path, registry, context)
}

fn build_handler_info(
    item_fn: &ItemFn,
    module_path: &str,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
) -> HandlerInfo {
    let mut request_parts = Vec::new();
    let mut auth_required = false;
    let mut has_json_body = false;
    for input in &item_fn.sig.inputs {
        if let FnArg::Typed(PatType { ty, .. }) = input {
            if is_auth_guard_type(ty) {
                auth_required = true;
                continue;
            }
            if let Some((kind, inner)) = extract_request_extractor(ty) {
                let desc = describe_type(inner, module_path, registry, context);
                if matches!(kind, ExtractorKind::Json) {
                    has_json_body = true;
                }
                request_parts.push((kind, desc));
            }
        }
    }
    let request = format_request(request_parts);
    let response = match &item_fn.sig.output {
        ReturnType::Default => "None".to_string(),
        ReturnType::Type(_, ty) => describe_response_type(ty, module_path, registry, context),
    };
    let required_headers = build_required_headers(auth_required, has_json_body);
    HandlerInfo {
        request,
        response,
        auth_required,
        required_headers,
    }
}

fn collect_handlers(
    file: &File,
    module_path: &str,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
) -> HashMap<String, HandlerInfo> {
    let mut handlers = HashMap::new();
    for item in &file.items {
        if let Item::Fn(item_fn) = item {
            handlers.insert(
                item_fn.sig.ident.to_string(),
                build_handler_info(item_fn, module_path, registry, context),
            );
        }
    }
    handlers
}

pub(crate) fn parse_routes_file(
    path: &Path,
    manifest_dir: &Path,
    src_dir: &Path,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
) -> Vec<RouteEntry> {
    let parsed = parse_rust_file(path);
    let module_path = module_path_for_file(path, src_dir);
    let handlers = collect_handlers(&parsed, &module_path, registry, context);
    let source = path
        .strip_prefix(manifest_dir)
        .unwrap_or(path)
        .display()
        .to_string();
    let mut routes = Vec::new();
    for item in &parsed.items {
        match item {
            Item::Fn(item_fn) => {
                let route_bindings = collect_route_bindings(&item_fn.block);
                let mut visitor = RouteVisitor {
                    source: source.clone(),
                    handlers: &handlers,
                    route_bindings: Some(&route_bindings),
                    routes: Vec::new(),
                };
                visitor.visit_block(&item_fn.block);
                routes.extend(visitor.routes);
            }
            Item::Impl(item_impl) => {
                for item in &item_impl.items {
                    let ImplItem::Fn(item_fn) = item else {
                        continue;
                    };
                    let route_bindings = collect_route_bindings(&item_fn.block);
                    let mut visitor = RouteVisitor {
                        source: source.clone(),
                        handlers: &handlers,
                        route_bindings: Some(&route_bindings),
                        routes: Vec::new(),
                    };
                    visitor.visit_block(&item_fn.block);
                    routes.extend(visitor.routes);
                }
            }
            _ => {}
        }
    }
    routes
}

fn collect_route_bindings(block: &Block) -> HashMap<String, Vec<RouteHandler>> {
    let mut out = HashMap::new();
    for stmt in &block.stmts {
        let Stmt::Local(local) = stmt else {
            continue;
        };
        let name = match &local.pat {
            Pat::Ident(ident) => ident.ident.to_string(),
            _ => continue,
        };
        let Some(init) = local.init.as_ref().map(|init| init.expr.as_ref()) else {
            continue;
        };
        let mut handlers = extract_route_handlers(init);
        if handlers.is_empty() {
            let methods = extract_methods(init);
            handlers = methods
                .into_iter()
                .map(|method| RouteHandler {
                    method,
                    handler: None,
                })
                .collect();
        }
        if !handlers.is_empty() {
            out.insert(name, handlers);
        }
    }
    out
}

fn crud_route_entries(
    base: &str,
    source: &str,
    service: Option<&str>,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
) -> Vec<RouteEntry> {
    let id_path = format!("{}/{{id}}", base);
    let model_type = resolve_model_type(service, context);
    let model_desc = model_type
        .as_deref()
        .map(|name| describe_type_name(name, registry, context))
        .unwrap_or_else(|| "JSON".to_string());
    let list_query_desc = describe_type_name("ListQuery", registry, context);
    let list_response = replace_paginated_generic(
        &describe_type_name("PaginatedResponse", registry, context),
        &model_desc,
    );
    let model_response = format!(
        "{{ \"status\": u16, \"message\": String, \"data\": {} }}",
        model_desc
    );
    let list_response = format!(
        "{{ \"status\": u16, \"message\": String, \"data\": {} }}",
        list_response
    );
    let delete_response = "{ \"status\": u16, \"message\": String, \"data\": JSON }".to_string();
    vec![
        RouteEntry {
            method: "POST".to_string(),
            path: base.to_string(),
            source: source.to_string(),
            request: model_desc.clone(),
            response: model_response.clone(),
            required_headers: build_required_headers(false, true),
            curl: build_curl("POST", base, &model_desc, false),
        },
        RouteEntry {
            method: "GET".to_string(),
            path: base.to_string(),
            source: source.to_string(),
            request: format!("query: {}", list_query_desc),
            response: list_response,
            required_headers: "None".to_string(),
            curl: build_curl("GET", base, &format!("query: {}", list_query_desc), false),
        },
        RouteEntry {
            method: "GET".to_string(),
            path: id_path.clone(),
            source: source.to_string(),
            request: "path: Uuid".to_string(),
            response: model_response.clone(),
            required_headers: "None".to_string(),
            curl: build_curl("GET", &id_path, "path: Uuid", false),
        },
        RouteEntry {
            method: "PATCH".to_string(),
            path: id_path.clone(),
            source: source.to_string(),
            request: format!("path: Uuid | {}", model_desc),
            response: model_response.clone(),
            required_headers: build_required_headers(false, true),
            curl: build_curl("PATCH", &id_path, &model_desc, false),
        },
        RouteEntry {
            method: "DELETE".to_string(),
            path: id_path,
            source: source.to_string(),
            request: "path: Uuid".to_string(),
            response: delete_response,
            required_headers: "None".to_string(),
            curl: build_curl("DELETE", &format!("{}/{{id}}", base), "path: Uuid", false),
        },
    ]
}

pub(crate) fn parse_crud_router_routes(
    path: &Path,
    manifest_dir: &Path,
    registry: &TypeRegistry,
    context: &CrudTypeContext,
) -> Vec<RouteEntry> {
    let parsed = parse_rust_file(path);
    let source = path
        .strip_prefix(manifest_dir)
        .unwrap_or(path)
        .display()
        .to_string();
    let consts = collect_const_string_literals(&parsed);
    let mut unresolved = 0;
    let mut calls = Vec::new();

    for item in &parsed.items {
        let Item::Fn(item_fn) = item else {
            continue;
        };
        let locals = collect_local_types(&item_fn.block);
        let mut visitor = CrudRouterVisitor {
            consts: &consts,
            locals: &locals,
            calls: Vec::new(),
            unresolved: 0,
        };
        visitor.visit_block(&item_fn.block);
        unresolved += visitor.unresolved;
        calls.extend(visitor.calls);
    }

    if unresolved > 0 {
        println!(
            "cargo:warning=Skipping non-literal CrudApiRouter base path in {}",
            source
        );
    }

    let mut seen = HashSet::new();
    let mut routes = Vec::new();
    for call in calls {
        if seen.insert(call.base_path.clone()) {
            routes.extend(crud_route_entries(
                &call.base_path,
                &source,
                call.service.as_deref(),
                registry,
                context,
            ));
        }
    }
    routes
}

pub(crate) fn collect_route_files(routes_dir: &Path) -> Vec<PathBuf> {
    collect_rust_files(routes_dir)
}

#[derive(Default)]
pub(crate) struct CrudTypeContext {
    pub(crate) service_to_dao: HashMap<String, String>,
    pub(crate) dao_to_entity: HashMap<String, String>,
    pub(crate) entity_to_model: HashMap<String, String>,
}

pub(crate) fn collect_crud_service_impls(file: &File, out: &mut HashMap<String, String>) {
    for item in &file.items {
        let Item::Impl(item_impl) = item else {
            continue;
        };
        if !impl_uses_trait(item_impl, "CrudService") {
            continue;
        }
        let service = match type_from_type(&item_impl.self_ty) {
            Some(ty) => ty,
            None => continue,
        };
        if let Some(dao) = impl_associated_type(item_impl, "Dao") {
            out.insert(service, dao);
        }
    }
}

pub(crate) fn collect_dao_base_impls(file: &File, out: &mut HashMap<String, String>) {
    for item in &file.items {
        let Item::Impl(item_impl) = item else {
            continue;
        };
        if !impl_uses_trait(item_impl, "DaoBase") {
            continue;
        }
        let dao = match type_from_type(&item_impl.self_ty) {
            Some(ty) => ty,
            None => continue,
        };
        if let Some(entity) = impl_associated_type(item_impl, "Entity") {
            out.insert(dao, entity);
        }
    }
}

fn impl_uses_trait(item_impl: &ItemImpl, trait_name: &str) -> bool {
    let Some((_, path, _)) = &item_impl.trait_ else {
        return false;
    };
    path.segments
        .last()
        .map(|seg| seg.ident == trait_name)
        .unwrap_or(false)
}

fn impl_associated_type(item_impl: &ItemImpl, name: &str) -> Option<String> {
    for item in &item_impl.items {
        let ImplItem::Type(item_type) = item else {
            continue;
        };
        if item_type.ident != name {
            continue;
        }
        if let Some(ty) = type_from_type(&item_type.ty) {
            return Some(ty);
        }
    }
    None
}

pub(crate) fn collect_entity_model_map(entities_dir: &Path, src_dir: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for file in collect_rust_files(entities_dir) {
        let stem = file
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if stem.is_empty() || stem == "mod" {
            continue;
        }
        let alias = to_pascal_case(stem);
        let module_path = module_path_for_file(&file, src_dir);
        let model_path = format!("{}::Model", module_path);
        out.insert(alias, model_path);
    }
    out
}

fn describe_type_name(name: &str, registry: &TypeRegistry, context: &CrudTypeContext) -> String {
    registry
        .docs
        .get(name)
        .map(|doc| render_type_doc(doc, registry, context, 0))
        .unwrap_or_else(|| name.to_string())
}

fn resolve_model_type(service: Option<&str>, context: &CrudTypeContext) -> Option<String> {
    let service = service?;
    let dao = context.service_to_dao.get(service)?;
    let entity = context.dao_to_entity.get(dao)?;
    context.entity_to_model.get(entity).cloned()
}

fn replace_paginated_generic(doc: &str, model_name: &str) -> String {
    let vec_replacement = format!("Vec<{}>", model_name);
    let option_replacement = format!("Option<{}>", model_name);
    doc.replace("Vec<T>", &vec_replacement)
        .replace("Option<T>", &option_replacement)
}

pub(crate) fn write_routes(out_dir: &Path, routes: &[RouteEntry]) {
    let out_path = out_dir.join("routes_generated.rs");
    let mut output = String::from("pub static ROUTES: &[RouteInfo] = &[\n");
    for route in routes {
        output.push_str(&format!(
            "    RouteInfo {{ method: \"{}\", path: \"{}\", source: \"{}\", request: \"{}\", response: \"{}\", required_headers: \"{}\", curl: \"{}\" }},\n",
            escape_rust_string(&route.method),
            escape_rust_string(&route.path),
            escape_rust_string(&route.source),
            escape_rust_string(&route.request),
            escape_rust_string(&route.response),
            escape_rust_string(&route.required_headers),
            escape_rust_string(&route.curl)
        ));
    }
    output.push_str("];\n");

    std::fs::write(&out_path, output)
        .unwrap_or_else(|err| panic!("failed to write {}: {}", out_path.display(), err));
}
