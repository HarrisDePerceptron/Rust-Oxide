#[derive(Debug, Clone, Copy)]
pub struct EntityColumnInfo {
    pub name: &'static str,
    pub rust_type: &'static str,
    pub attributes: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct EntityInfo {
    pub entity: &'static str,
    pub table: &'static str,
    pub column_count: usize,
    pub columns: &'static [EntityColumnInfo],
}

#[derive(Debug, Clone, Copy)]
pub struct EntityRelationInfo {
    pub from: &'static str,
    pub to: &'static str,
    pub kind: &'static str,
    pub label: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/entities_generated.rs"));

pub fn entities() -> &'static [EntityInfo] {
    ENTITIES
}

pub fn relations() -> &'static [EntityRelationInfo] {
    RELATIONS
}

pub fn erd_mermaid() -> &'static str {
    ERD_MERMAID
}
