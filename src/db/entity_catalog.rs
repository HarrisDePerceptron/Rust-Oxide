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

include!(concat!(env!("OUT_DIR"), "/entities_generated.rs"));

pub fn entities() -> &'static [EntityInfo] {
    ENTITIES
}
