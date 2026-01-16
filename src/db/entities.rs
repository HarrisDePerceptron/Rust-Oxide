#[allow(unused_imports)]
pub mod prelude {
    pub use super::refresh_token::Entity as RefreshToken;
    pub use super::todo_item::Entity as TodoItem;
    pub use super::todo_list::Entity as TodoList;
    pub use super::user::Entity as User;
}

pub mod user {
    use sea_orm::entity::prelude::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        #[sea_orm(unique)]
        pub email: String,
        pub password_hash: String,
        pub role: String,
        #[sea_orm(default_expr = "Expr::current_timestamp()")]
        pub created_at: DateTimeWithTimeZone,
        #[sea_orm(default_expr = "Expr::current_timestamp()")]
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
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        #[sea_orm(unique)]
        pub token: String,
        #[sea_orm(indexed)]
        pub user_id: Uuid,
        pub expires_at: DateTimeWithTimeZone,
        #[sea_orm(default_expr = "Expr::current_timestamp()")]
        pub created_at: DateTimeWithTimeZone,
        pub revoked: bool,
        #[sea_orm(belongs_to, from = "user_id", to = "id", on_delete = "Cascade")]
        pub user: HasOne<super::user::Entity>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod todo_list {
    use sea_orm::entity::prelude::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "todo_lists")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        pub title: String,
        #[sea_orm(default_expr = "Expr::current_timestamp()")]
        pub created_at: DateTimeWithTimeZone,
        #[sea_orm(default_expr = "Expr::current_timestamp()")]
        pub updated_at: DateTimeWithTimeZone,
        #[sea_orm(has_many)]
        pub items: HasMany<super::todo_item::Entity>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod todo_item {
    use sea_orm::entity::prelude::*;

    #[sea_orm::model]
    #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
    #[sea_orm(table_name = "todo_items")]
    pub struct Model {
        #[sea_orm(primary_key, auto_increment = false)]
        pub id: Uuid,
        #[sea_orm(indexed)]
        pub list_id: Uuid,
        pub description: String,
        #[sea_orm(default_value = false)]
        pub done: bool,
        #[sea_orm(default_expr = "Expr::current_timestamp()")]
        pub created_at: DateTimeWithTimeZone,
        #[sea_orm(default_expr = "Expr::current_timestamp()")]
        pub updated_at: DateTimeWithTimeZone,
        #[sea_orm(belongs_to, from = "list_id", to = "id", on_delete = "Cascade")]
        pub list: HasOne<super::todo_list::Entity>,
    }

    impl ActiveModelBehavior for ActiveModel {}
}
