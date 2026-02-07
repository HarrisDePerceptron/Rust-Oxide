use std::{collections::HashSet, fs, path::Path};

use syn::{Attribute, Fields, Item, ItemEnum, ItemStruct, LitStr, Type};

use crate::utils::{
    add_attribute, entity_name_from_type, escape_rust_string, extract_generic_inner,
    is_option_type, normalize_field_name, to_pascal_case, type_to_string,
};

#[derive(Debug, Clone)]
pub(crate) struct EntityEntry {
    pub(crate) entity: String,
    pub(crate) table: String,
    pub(crate) columns: Vec<EntityColumnEntry>,
}

#[derive(Debug, Clone)]
pub(crate) struct EntityColumnEntry {
    pub(crate) name: String,
    pub(crate) rust_type: String,
    pub(crate) attributes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RelationKind {
    HasMany,
    HasOne,
    BelongsTo,
}

impl RelationKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            RelationKind::HasMany => "has_many",
            RelationKind::HasOne => "has_one",
            RelationKind::BelongsTo => "belongs_to",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct EntityRelationEntry {
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) kind: RelationKind,
    pub(crate) label: String,
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

#[derive(Debug, Clone)]
struct BaseEntityDefaults {
    id: String,
    created_at: String,
    updated_at: String,
}

impl Default for BaseEntityDefaults {
    fn default() -> Self {
        Self {
            id: "id".to_string(),
            created_at: "created_at".to_string(),
            updated_at: "updated_at".to_string(),
        }
    }
}

pub(crate) fn collect_entity_entries(
    items: &[Item],
    module_path: &str,
    out: &mut Vec<EntityEntry>,
) {
    let fk_columns = collect_fk_columns(items);
    for item in items {
        match item {
            Item::Struct(item_struct) => {
                if has_derive_entity_model(&item_struct.attrs) {
                    if let Some(entity) = build_entity_entry(item_struct, module_path, &fk_columns)
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

pub(crate) fn collect_entity_relations(
    items: &[Item],
    module_path: &str,
    out: &mut Vec<EntityRelationEntry>,
) {
    for item in items {
        match item {
            Item::Struct(item_struct) => {
                if has_derive_entity_model(&item_struct.attrs) {
                    out.extend(build_entity_relations(item_struct, module_path));
                }
            }
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    let nested_path = if module_path.is_empty() {
                        item_mod.ident.to_string()
                    } else {
                        format!("{}::{}", module_path, item_mod.ident)
                    };
                    collect_entity_relations(nested, &nested_path, out);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn render_mermaid_er_diagram(
    entities: &[EntityEntry],
    relations: &[EntityRelationEntry],
) -> String {
    let mut output = String::from("erDiagram\n");

    let mut preferred = HashSet::new();
    for relation in relations {
        if relation.kind != RelationKind::BelongsTo {
            preferred.insert((relation.from.clone(), relation.to.clone()));
        }
    }

    for relation in relations {
        if relation.kind == RelationKind::BelongsTo
            && preferred.contains(&(relation.to.clone(), relation.from.clone()))
        {
            continue;
        }
        let from = mermaid_sanitize_word(&relation.from);
        let to = mermaid_sanitize_word(&relation.to);
        let marker = mermaid_relation_marker(relation.kind);
        let label = mermaid_label(&relation.label);
        output.push_str(&format!("  {} {} {} : {}\n", from, marker, to, label));
    }

    for entity in entities {
        let name = mermaid_sanitize_word(&entity.entity);
        output.push_str(&format!("  {} {{\n", name));
        for column in &entity.columns {
            let attributes = mermaid_attribute_suffix(&column.attributes);
            let ty = mermaid_type_name(&column.rust_type);
            let label = mermaid_attribute_name(&column.name);
            output.push_str(&format!("    {} {}{}\n", ty, label, attributes));
        }
        output.push_str("  }\n");
    }

    output
}

pub(crate) fn write_entities(
    out_dir: &Path,
    entities: &[EntityEntry],
    relations: &[EntityRelationEntry],
) {
    let entity_out_path = out_dir.join("entities_generated.rs");
    let mut entity_output = String::from("pub static ENTITIES: &[EntityInfo] = &[\n");
    for entity in entities {
        entity_output.push_str(&format!(
            "    EntityInfo {{ entity: \"{}\", table: \"{}\", column_count: {}, columns: &[\n",
            escape_rust_string(&entity.entity),
            escape_rust_string(&entity.table),
            entity.columns.len()
        ));
        for column in &entity.columns {
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
    entity_output.push_str("pub static RELATIONS: &[EntityRelationInfo] = &[\n");
    for relation in relations {
        entity_output.push_str(&format!(
            "    EntityRelationInfo {{ from: \"{}\", to: \"{}\", kind: \"{}\", label: \"{}\" }},\n",
            escape_rust_string(&relation.from),
            escape_rust_string(&relation.to),
            relation.kind.as_str(),
            escape_rust_string(&relation.label)
        ));
    }
    entity_output.push_str("];\n");
    let mermaid = render_mermaid_er_diagram(entities, relations);
    entity_output.push_str(&format!(
        "pub static ERD_MERMAID: &str = \"{}\";\n",
        escape_rust_string(&mermaid)
    ));

    fs::write(&entity_out_path, entity_output)
        .unwrap_or_else(|err| panic!("failed to write {}: {}", entity_out_path.display(), err));
}

fn has_derive_entity_model(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("derive") {
            continue;
        }
        let paths = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        );
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

fn base_entity_defaults_from_attrs(attrs: &[Attribute]) -> Option<BaseEntityDefaults> {
    let mut config: Option<BaseEntityDefaults> = None;
    for attr in attrs {
        if !attr.path().is_ident("base_entity") {
            continue;
        }
        let defaults = config.get_or_insert_with(BaseEntityDefaults::default);
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("id") {
                let value: LitStr = meta.value()?.parse()?;
                defaults.id = value.value();
            } else if meta.path.is_ident("created_at") {
                let value: LitStr = meta.value()?.parse()?;
                defaults.created_at = value.value();
            } else if meta.path.is_ident("updated_at") {
                let value: LitStr = meta.value()?.parse()?;
                defaults.updated_at = value.value();
            }
            Ok(())
        });
    }
    config
}

fn push_base_entity_column(
    columns: &mut Vec<EntityColumnEntry>,
    existing: &mut HashSet<String>,
    name: &str,
    rust_type: &str,
    primary_key: bool,
) {
    let normalized = normalize_field_name(name);
    if existing.contains(&normalized) {
        return;
    }
    existing.insert(normalized);
    let mut attributes = Vec::new();
    if primary_key {
        add_attribute(&mut attributes, "Primary Key");
    }
    columns.push(EntityColumnEntry {
        name: name.to_string(),
        rust_type: rust_type.to_string(),
        attributes,
    });
}

fn column_variant_from_field(field_name: &str) -> String {
    let stripped = field_name.strip_prefix("r#").unwrap_or(field_name);
    to_pascal_case(stripped)
}

fn field_has_relation_attr(attrs: &[Attribute]) -> bool {
    for attr in attrs {
        if !attr.path().is_ident("sea_orm") {
            continue;
        }
        let mut is_relation = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("has_many")
                || meta.path.is_ident("has_one")
                || meta.path.is_ident("belongs_to")
            {
                is_relation = true;
            }
            Ok(())
        });
        if is_relation {
            return true;
        }
    }
    false
}

fn is_relation_type(ty: &Type) -> bool {
    let (_, last) = crate::utils::type_path_parts(ty);
    if let Some(last) = last.as_deref() {
        if last == "HasOne" || last == "HasMany" {
            return true;
        }
    }
    if let Some(inner) =
        extract_generic_inner(ty, "Option").or_else(|| extract_generic_inner(ty, "Vec"))
    {
        let (_, last) = crate::utils::type_path_parts(inner);
        return matches!(last.as_deref(), Some("Entity"));
    }
    false
}

fn parse_fk_column_variants(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    let parts: Vec<&str> = if trimmed.starts_with('(') && trimmed.ends_with(')') {
        trimmed[1..trimmed.len() - 1]
            .split(',')
            .map(|part| part.trim())
            .collect()
    } else {
        vec![trimmed]
    };
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| part.rsplit("::").next().unwrap_or(part).trim().to_string())
        .collect()
}

fn collect_fk_columns(items: &[Item]) -> HashSet<String> {
    let mut out = HashSet::new();
    for item in items {
        match item {
            Item::Enum(item_enum) => collect_fk_columns_from_enum(item_enum, &mut out),
            Item::Struct(item_struct) => collect_fk_columns_from_model(item_struct, &mut out),
            Item::Mod(item_mod) => {
                if let Some((_, nested)) = &item_mod.content {
                    let nested_fk = collect_fk_columns(nested);
                    out.extend(nested_fk);
                }
            }
            _ => {}
        }
    }
    out
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
                    for column in parse_fk_column_variants(value) {
                        out.insert(column);
                    }
                }
            }
        }
    }
}

fn collect_fk_columns_from_model(item_struct: &ItemStruct, out: &mut HashSet<String>) {
    let Fields::Named(fields) = &item_struct.fields else {
        return;
    };
    for field in &fields.named {
        for column in extract_belongs_to_from_field(&field.attrs) {
            out.insert(column);
        }
    }
}

fn extract_belongs_to_from_field(attrs: &[Attribute]) -> Vec<String> {
    let mut columns = Vec::new();
    for attr in attrs {
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
                columns.extend(parse_fk_column_variants(value));
            }
        }
    }
    columns
}

fn entity_name_from_module_path(module_path: &str) -> Option<String> {
    module_path
        .split("::")
        .filter(|part| !part.is_empty())
        .last()
        .map(|value| value.to_string())
}

fn build_entity_entry(
    item_struct: &ItemStruct,
    module_path: &str,
    fk_columns: &HashSet<String>,
) -> Option<EntityEntry> {
    let entity = entity_name_from_module_path(module_path)?;
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
    let base_entity_defaults = base_entity_defaults_from_attrs(&item_struct.attrs);
    let mut existing_fields: HashSet<String> = named
        .named
        .iter()
        .filter_map(|field| {
            field
                .ident
                .as_ref()
                .map(|ident| normalize_field_name(&ident.to_string()))
        })
        .collect();
    if let Some(defaults) = base_entity_defaults {
        push_base_entity_column(
            &mut columns,
            &mut existing_fields,
            &defaults.id,
            "uuid::Uuid",
            true,
        );
        push_base_entity_column(
            &mut columns,
            &mut existing_fields,
            &defaults.created_at,
            "sea_orm::entity::prelude::DateTimeWithTimeZone",
            false,
        );
        push_base_entity_column(
            &mut columns,
            &mut existing_fields,
            &defaults.updated_at,
            "sea_orm::entity::prelude::DateTimeWithTimeZone",
            false,
        );
    }
    for field in &named.named {
        if field_has_relation_attr(&field.attrs) || is_relation_type(&field.ty) {
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

fn build_entity_relations(item_struct: &ItemStruct, module_path: &str) -> Vec<EntityRelationEntry> {
    let mut relations = Vec::new();
    let Some(entity) = entity_name_from_module_path(module_path) else {
        return relations;
    };
    let Fields::Named(fields) = &item_struct.fields else {
        return relations;
    };
    for field in &fields.named {
        if !field_has_relation_attr(&field.attrs) && !is_relation_type(&field.ty) {
            continue;
        }
        let kind =
            relation_kind_from_attrs(&field.attrs).or_else(|| relation_kind_from_type(&field.ty));
        let Some(kind) = kind else {
            continue;
        };
        let Some(target) = relation_target_entity(&field.ty) else {
            continue;
        };
        let mut label = field
            .ident
            .as_ref()
            .map(|ident| ident.to_string())
            .unwrap_or_else(|| "relation".to_string());
        if let Some(stripped) = label.strip_prefix("r#") {
            label = stripped.to_string();
        }
        relations.push(EntityRelationEntry {
            from: entity.clone(),
            to: target,
            kind,
            label,
        });
    }
    relations
}

fn relation_kind_from_attrs(attrs: &[Attribute]) -> Option<RelationKind> {
    for attr in attrs {
        if !attr.path().is_ident("sea_orm") {
            continue;
        }
        let mut kind = None;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("has_many") {
                kind = Some(RelationKind::HasMany);
            } else if meta.path.is_ident("has_one") {
                kind = Some(RelationKind::HasOne);
            } else if meta.path.is_ident("belongs_to") {
                kind = Some(RelationKind::BelongsTo);
            }
            Ok(())
        });
        if kind.is_some() {
            return kind;
        }
    }
    None
}

fn relation_kind_from_type(ty: &Type) -> Option<RelationKind> {
    if extract_generic_inner(ty, "HasMany").is_some() {
        return Some(RelationKind::HasMany);
    }
    if extract_generic_inner(ty, "HasOne").is_some() {
        return Some(RelationKind::HasOne);
    }
    if extract_generic_inner(ty, "Vec").is_some() {
        return Some(RelationKind::HasMany);
    }
    if extract_generic_inner(ty, "Option").is_some() {
        return Some(RelationKind::HasOne);
    }
    None
}

fn relation_target_entity(ty: &Type) -> Option<String> {
    if let Some(inner) = extract_generic_inner(ty, "HasMany")
        .or_else(|| extract_generic_inner(ty, "HasOne"))
        .or_else(|| extract_generic_inner(ty, "Vec"))
        .or_else(|| extract_generic_inner(ty, "Option"))
    {
        return entity_name_from_type(inner);
    }
    entity_name_from_type(ty)
}

fn mermaid_relation_marker(kind: RelationKind) -> &'static str {
    match kind {
        RelationKind::HasMany => "||--o{",
        RelationKind::HasOne => "||--o|",
        RelationKind::BelongsTo => "}o--||",
    }
}

fn mermaid_label(value: &str) -> String {
    let sanitized = value.replace('\n', " ").replace('\r', " ");
    mermaid_sanitize_word(sanitized.trim())
}

fn mermaid_attribute_suffix(attributes: &[String]) -> String {
    if attributes.is_empty() {
        return String::new();
    }
    let mut out = Vec::new();
    for attr in attributes {
        let label = mermaid_label(attr);
        if !label.is_empty() {
            out.push(label);
        }
    }
    if out.is_empty() {
        return String::new();
    }
    format!(" \"{}\"", out.join(", "))
}

fn mermaid_attribute_name(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "field".to_string();
    }
    mermaid_sanitize_word(trimmed)
}

fn mermaid_type_name(value: &str) -> String {
    if let Some(inner) = strip_generic(value, "Option<") {
        return mermaid_type_name(inner);
    }
    if let Some(inner) = strip_generic(value, "Vec<") {
        return format!("{}[]", mermaid_type_name(inner));
    }
    let trimmed = value.trim();
    let trimmed = trimmed.strip_prefix("&").unwrap_or(trimmed);
    let trimmed = trimmed.strip_prefix("crate::").unwrap_or(trimmed);
    let trimmed = trimmed.rsplit("::").next().unwrap_or(trimmed);
    mermaid_sanitize_word(trimmed)
}

fn strip_generic<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let trimmed = value.trim();
    if trimmed.starts_with(prefix) && trimmed.ends_with('>') {
        return Some(&trimmed[prefix.len()..trimmed.len() - 1]);
    }
    None
}

fn mermaid_sanitize_word(value: &str) -> String {
    let mut out = String::new();
    let mut prev_space = false;
    for ch in value.chars() {
        let allowed = ch.is_alphanumeric() || ch == '_';
        if allowed {
            out.push(ch);
            prev_space = false;
        } else if !prev_space {
            out.push('_');
            prev_space = true;
        }
    }
    if out.is_empty() {
        "Unknown".to_string()
    } else {
        out
    }
}
