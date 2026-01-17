use base_entity_derive::base_entity;
use sea_orm::entity::prelude::*;

#[base_entity]
#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "refresh_tokens")]
pub struct Model {
    #[sea_orm(unique)]
    pub token: String,
    #[sea_orm(indexed)]
    pub user_id: Uuid,
    pub expires_at: DateTimeWithTimeZone,
    pub revoked: bool,
    #[sea_orm(belongs_to, from = "user_id", to = "id", on_delete = "Cascade")]
    pub user: HasOne<super::user::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
