use std::{
    fs,
    path::{Path, PathBuf},
};

use quote::ToTokens;
use syn::{File, GenericArgument, PathArguments, Type};

pub(crate) fn escape_rust_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

pub(crate) fn parse_rust_file(path: &Path) -> File {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", path.display(), err));
    syn::parse_file(&content)
        .unwrap_or_else(|err| panic!("failed to parse {}: {}", path.display(), err))
}

pub(crate) fn module_path_for_file(path: &Path, src_dir: &Path) -> String {
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

pub(crate) fn collect_rust_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_rust_files_inner(root, &mut files);
    files.sort();
    files
}

fn collect_rust_files_inner(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|err| panic!("failed to read {}: {}", dir.display(), err));
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

pub(crate) fn to_pascal_case(value: &str) -> String {
    if !value.contains('_') && !value.contains('-') {
        let has_upper = value.chars().any(|ch| ch.is_uppercase());
        if has_upper {
            return value.to_string();
        }
    }

    let mut out = String::new();
    for part in value.split(|ch| ch == '_' || ch == '-') {
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

pub(crate) fn compact_type_string(mut value: String) -> String {
    for (from, to) in [
        (" :: ", "::"),
        (" < ", "<"),
        (" > ", ">"),
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

pub(crate) fn type_to_string(ty: &Type) -> String {
    compact_type_string(ty.to_token_stream().to_string())
}

pub(crate) fn is_unit_type(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

pub(crate) fn type_path_parts(ty: &Type) -> (Option<String>, Option<String>) {
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

pub(crate) fn extract_generic_inner<'a>(ty: &'a Type, ident: &str) -> Option<&'a Type> {
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

pub(crate) fn is_option_type(ty: &Type) -> bool {
    extract_generic_inner(ty, "Option").is_some()
}

pub(crate) fn extract_generic_types<'a>(ty: &'a Type, ident: &str) -> Vec<&'a Type> {
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

pub(crate) fn normalize_field_name(name: &str) -> String {
    name.strip_prefix("r#").unwrap_or(name).to_string()
}

pub(crate) fn add_attribute(attrs: &mut Vec<String>, value: &str) {
    if !attrs.iter().any(|item| item == value) {
        attrs.push(value.to_string());
    }
}

pub(crate) fn entity_name_from_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(type_path) => {
            let segments: Vec<String> = type_path
                .path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect();
            if segments.is_empty() {
                return None;
            }
            if segments
                .last()
                .map(|value| value == "Entity")
                .unwrap_or(false)
            {
                if segments.len() >= 2 {
                    return Some(segments[segments.len() - 2].clone());
                }
            }
            segments.last().cloned()
        }
        Type::Reference(reference) => entity_name_from_type(&reference.elem),
        Type::Paren(paren) => entity_name_from_type(&paren.elem),
        _ => None,
    }
}
