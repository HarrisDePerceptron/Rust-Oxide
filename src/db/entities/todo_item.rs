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
