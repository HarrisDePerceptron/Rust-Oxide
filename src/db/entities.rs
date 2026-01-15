#[allow(unused_imports)]
pub mod prelude {
    pub use super::refresh_token::Entity as RefreshToken;
    pub use super::user::Entity as User;
}

pub mod user {
    use sea_orm::entity::prelude::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: Uuid,
        #[sea_orm(unique)]
        pub email: String,
        pub password_hash: String,
        pub role: String,
        pub created_at: DateTimeWithTimeZone,
        pub updated_at: DateTimeWithTimeZone,
        pub last_login_at: Option<DateTimeWithTimeZone>,
        #[sea_orm(has_many)]
        pub refresh_tokens: HasMany<super::refresh_token::Entity>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod refresh_token {
    use sea_orm::entity::prelude::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "refresh_tokens")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: Uuid,
        #[sea_orm(unique)]
        pub token: String,
        #[sea_orm(indexed)]
        pub user_id: Uuid,
        pub expires_at: DateTimeWithTimeZone,
        pub created_at: DateTimeWithTimeZone,
        pub revoked: bool,
        #[sea_orm(belongs_to, from = "user_id", to = "id", on_delete = "Cascade")]
        pub user: HasOne<super::user::Entity>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}
