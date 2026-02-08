use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::utils::escape_rust_string;

#[derive(Debug)]
struct SectionFile {
    path: PathBuf,
    name: String,
    order: u32,
}

pub(crate) fn write_docs_sections(manifest_path: &Path, out_dir: &Path) {
    let sections_dir = manifest_path.join("views/docs/sections");
    println!("cargo:rerun-if-changed={}", sections_dir.display());

    let mut section_files = collect_section_files(&sections_dir);
    if section_files.is_empty() {
        panic!(
            "no docs section files found under {}; add at least one '*.html' file",
            sections_dir.display()
        );
    }

    section_files.sort_by(|left, right| {
        left.order
            .cmp(&right.order)
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut combined_html = String::new();
    for section in section_files {
        println!("cargo:rerun-if-changed={}", section.path.display());
        let section_html = fs::read_to_string(&section.path).unwrap_or_else(|err| {
            panic!(
                "failed to read docs section {}: {}",
                section.path.display(),
                err
            )
        });
        if !combined_html.is_empty() {
            combined_html.push('\n');
        }
        combined_html.push_str(&section_html);
    }

    let output = format!(
        "pub static DOCS_SECTIONS_HTML: &str = \"{}\";\n",
        escape_rust_string(&combined_html)
    );
    let output_path = out_dir.join("docs_sections_generated.rs");
    fs::write(&output_path, output)
        .unwrap_or_else(|err| panic!("failed to write {}: {}", output_path.display(), err));
}

fn collect_section_files(sections_dir: &Path) -> Vec<SectionFile> {
    let entries = fs::read_dir(sections_dir).unwrap_or_else(|err| {
        panic!(
            "failed to read docs sections directory {}: {}",
            sections_dir.display(),
            err
        )
    });

    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.unwrap_or_else(|err| panic!("failed to read docs dir entry: {}", err));
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("html") {
            continue;
        }

        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_else(|| panic!("invalid docs section filename: {}", path.display()))
            .to_string();

        let (order, name) = parse_ordered_name(&file_name).unwrap_or_else(|reason| {
            panic!(
                "invalid docs section filename '{}': {}; expected '<number>_<name>.html'",
                file_name, reason
            )
        });

        files.push(SectionFile { path, name, order });
    }

    files
}

fn parse_ordered_name(file_name: &str) -> Result<(u32, String), &'static str> {
    let stem = file_name
        .strip_suffix(".html")
        .ok_or("file does not end with .html")?;
    if stem.is_empty() {
        return Err("empty filename");
    }

    let split_at = stem
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(idx, _)| idx)
        .unwrap_or(stem.len());

    if split_at == 0 {
        return Err("missing numeric prefix");
    }

    let order = stem[..split_at]
        .parse::<u32>()
        .map_err(|_| "numeric prefix is out of range")?;
    if order == 0 {
        return Err("numeric prefix must be greater than zero");
    }

    let rest = &stem[split_at..];
    let rest = rest.strip_prefix('_').or_else(|| rest.strip_prefix('-'));
    let rest = rest.ok_or("numeric prefix must be followed by '_' or '-'")?;
    if rest.is_empty() {
        return Err("missing section name after numeric prefix");
    }

    Ok((order, rest.to_string()))
}
