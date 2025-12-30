use sea_orm_migration::prelude::*;

mod m20251230_create_refresh_tokens;
mod m20251230_create_users;
mod m20251231_add_last_login_to_users;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251230_create_users::Migration),
            Box::new(m20251230_create_refresh_tokens::Migration),
            Box::new(m20251231_add_last_login_to_users::Migration),
        ]
    }
}
