use std::{
    env,
    fs,
    path::{Path, PathBuf},
};

use syn::{Expr, ExprLit, ExprMethodCall, File, Lit, visit::Visit};

#[derive(Debug, Clone)]
struct RouteEntry {
    method: String,
    path: String,
    source: String,
}

struct RouteVisitor {
    source: String,
    routes: Vec<RouteEntry>,
}

impl<'ast> Visit<'ast> for RouteVisitor {
    fn visit_expr_method_call(&mut self, node: &'ast ExprMethodCall) {
        let method_name = node.method.to_string();
        if method_name == "route" {
            let path = node.args.first().and_then(extract_string_literal);
            let methods = node
                .args
                .iter()
                .nth(1)
                .map(extract_methods)
                .unwrap_or_default();
            if let Some(path) = path {
                let methods = if methods.is_empty() {
                    vec!["ROUTE".to_string()]
                } else {
                    methods
                };
                for method in methods {
                    self.routes.push(RouteEntry {
                        method,
                        path: path.clone(),
                        source: self.source.clone(),
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

fn parse_file(path: &Path, manifest_dir: &Path) -> Vec<RouteEntry> {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", path.display(), err));
    let parsed: File = syn::parse_file(&content)
        .unwrap_or_else(|err| panic!("failed to parse {}: {}", path.display(), err));
    let source = path
        .strip_prefix(manifest_dir)
        .unwrap_or(path)
        .display()
        .to_string();
    let mut visitor = RouteVisitor {
        source,
        routes: Vec::new(),
    };
    visitor.visit_file(&parsed);
    visitor.routes
}

fn collect_route_files(routes_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let entries = fs::read_dir(routes_dir)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", routes_dir.display(), err));
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| panic!("failed to read dir entry: {}", err));
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    files.sort();
    files
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR");
    let manifest_path = Path::new(&manifest_dir);
    let routes_dir = manifest_path.join("src/routes");

    let files = collect_route_files(&routes_dir);
    for file in &files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let mut routes = Vec::new();
    for file in files {
        routes.extend(parse_file(&file, manifest_path));
    }

    routes.sort_by(|a, b| a.path.cmp(&b.path).then(a.method.cmp(&b.method)));

    let out_dir = env::var("OUT_DIR").expect("missing OUT_DIR");
    let out_path = Path::new(&out_dir).join("routes_generated.rs");
    let mut output = String::from("pub static ROUTES: &[RouteInfo] = &[\n");
    for route in routes {
        output.push_str(&format!(
            "    RouteInfo {{ method: \"{}\", path: \"{}\", source: \"{}\" }},\n",
            escape_rust_string(&route.method),
            escape_rust_string(&route.path),
            escape_rust_string(&route.source)
        ));
    }
    output.push_str("];\n");

    fs::write(&out_path, output)
        .unwrap_or_else(|err| panic!("failed to write {}: {}", out_path.display(), err));
}
