use base_entity_derive::base_entity;
use sea_orm::entity::prelude::*;

#[base_entity]
#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "todo_lists")]
pub struct Model {
    pub title: String,
    #[sea_orm(has_many)]
    pub items: HasMany<super::todo_item::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
