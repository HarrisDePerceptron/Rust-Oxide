use base_entity_derive::base_entity;
use sea_orm::entity::prelude::*;

#[base_entity]
#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "todo_items")]
pub struct Model {
    #[sea_orm(indexed)]
    pub list_id: Uuid,
    pub description: String,
    #[sea_orm(default_value = false)]
    pub done: bool,
    #[sea_orm(belongs_to, from = "list_id", to = "id", on_delete = "Cascade")]
    pub list: HasOne<super::todo_list::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
