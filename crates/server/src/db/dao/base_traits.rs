pub trait HasCreatedAtColumn: sea_orm::EntityTrait {
    fn created_at_column() -> Self::Column;
}

pub trait HasIdActiveModel {
    fn set_id(&mut self, id: uuid::Uuid);
}

pub trait TimestampedActiveModel {
    fn set_created_at(&mut self, ts: sea_orm::entity::prelude::DateTimeWithTimeZone);
    fn set_updated_at(&mut self, ts: sea_orm::entity::prelude::DateTimeWithTimeZone);
}
