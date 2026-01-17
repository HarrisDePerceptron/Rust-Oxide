use base_entity_derive::base_entity;
use sea_orm::entity::prelude::*;

#[base_entity]
#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(unique)]
    pub email: String,
    pub password_hash: String,
    pub role: String,
    pub last_login_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(has_many)]
    pub refresh_tokens: HasMany<super::refresh_token::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
