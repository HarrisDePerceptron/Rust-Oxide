use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, DatabaseConnection, EntityTrait, FromQueryResult, IntoActiveModel,
    PaginatorTrait, PrimaryKeyTrait, Select,
};
use uuid::Uuid;

use super::base_traits::{HasIdActiveModel, TimestampedActiveModel};
use super::error::{DaoLayerError, DaoResult};

pub trait DaoBase: Clone + Send + Sync + Sized
where
    <Self::Entity as EntityTrait>::Model:
        FromQueryResult + IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel> + Send + Sync,
    <Self::Entity as EntityTrait>::ActiveModel:
        ActiveModelTrait<Entity = Self::Entity> + HasIdActiveModel + TimestampedActiveModel + Send,
    <<Self::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType:
        From<Uuid> + Send + Sync,
{
    type Entity: EntityTrait + Send + Sync;
    const MAX_PAGE_SIZE: u64 = 100;

    fn from_db(db: DatabaseConnection) -> Self;

    fn new(db: &DatabaseConnection) -> Self {
        Self::from_db(db.clone())
    }

    fn db(&self) -> &DatabaseConnection;

    async fn create(
        &self,
        data: impl IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel> + Send,
    ) -> DaoResult<<Self::Entity as EntityTrait>::Model> {
        let now = Utc::now().fixed_offset();
        let mut active = data.into_active_model();
        active.set_id(Uuid::new_v4());
        active.set_created_at(now);
        active.set_updated_at(now);
        active.insert(self.db()).await.map_err(DaoLayerError::Db)
    }

    async fn find_by_id(&self, id: Uuid) -> DaoResult<<Self::Entity as EntityTrait>::Model> {
        let model = Self::Entity::find_by_id(id)
            .one(self.db())
            .await
            .map_err(DaoLayerError::Db)?;

        model.ok_or(DaoLayerError::NotFound {
            entity: std::any::type_name::<Self::Entity>(),
            id,
        })
    }

    async fn find(
        &self,
        page: u64,
        page_size: u64,
        apply: impl FnOnce(Select<Self::Entity>) -> Select<Self::Entity> + Send,
    ) -> DaoResult<Vec<<Self::Entity as EntityTrait>::Model>> {
        if page == 0 || page_size == 0 || page_size > Self::MAX_PAGE_SIZE {
            return Err(DaoLayerError::InvalidPagination { page, page_size });
        }

        let base = Self::Entity::find();
        let filtered = apply(base);
        let page_index = page.saturating_sub(1);

        filtered
            .paginate(self.db(), page_size)
            .fetch_page(page_index)
            .await
            .map_err(DaoLayerError::Db)
    }

    async fn update(
        &self,
        id: Uuid,
        apply: impl FnOnce(&mut <Self::Entity as EntityTrait>::ActiveModel) + Send,
    ) -> DaoResult<<Self::Entity as EntityTrait>::Model> {
        let model = Self::Entity::find_by_id(id)
            .one(self.db())
            .await
            .map_err(DaoLayerError::Db)?
            .ok_or(DaoLayerError::NotFound {
                entity: std::any::type_name::<Self::Entity>(),
                id,
            })?;

        let mut active = model.into_active_model();
        apply(&mut active);
        active.set_updated_at(Utc::now().fixed_offset());

        active.update(self.db()).await.map_err(DaoLayerError::Db)
    }

    async fn delete(&self, id: Uuid) -> DaoResult<Uuid> {
        let result = Self::Entity::delete_by_id(id)
            .exec(self.db())
            .await
            .map_err(DaoLayerError::Db)?;

        if result.rows_affected == 0 {
            return Err(DaoLayerError::NotFound {
                entity: std::any::type_name::<Self::Entity>(),
                id,
            });
        }

        Ok(id)
    }
}
