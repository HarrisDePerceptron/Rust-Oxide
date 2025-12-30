#[allow(unused_imports)]
pub mod prelude {
    pub use super::refresh_token::Entity as RefreshToken;
    pub use super::user::Entity as User;
}

pub mod user {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: Uuid,
        pub email: String,
        pub password_hash: String,
        pub role: String,
        pub created_at: DateTimeWithTimeZone,
        pub updated_at: DateTimeWithTimeZone,
        pub last_login_at: Option<DateTimeWithTimeZone>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(has_many = "super::refresh_token::Entity")]
        RefreshToken,
    }

    impl Related<super::refresh_token::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::RefreshToken.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod refresh_token {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "refresh_tokens")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: Uuid,
        pub token: String,
        pub user_id: Uuid,
        pub expires_at: DateTimeWithTimeZone,
        pub created_at: DateTimeWithTimeZone,
        pub revoked: bool,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(
            belongs_to = "super::user::Entity",
            from = "Column::UserId",
            to = "super::user::Column::Id",
            on_delete = "Cascade"
        )]
        User,
    }

    impl Related<super::user::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::User.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}
