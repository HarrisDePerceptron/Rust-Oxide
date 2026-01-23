use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult,
    IntoActiveModel, Order, PrimaryKeyTrait, QueryFilter, QueryOrder, QuerySelect, Select,
};
use sea_orm::sea_query::{Expr, ExprTrait, LikeExpr};
use uuid::Uuid;

use super::base_traits::{HasCreatedAtColumn, HasIdActiveModel, TimestampedActiveModel};
use super::error::{DaoLayerError, DaoResult};

#[derive(Debug, serde::Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub has_next: bool,
    pub total: Option<u64>,
}

#[derive(Debug, Clone)]
pub enum FilterOp {
    Eq(sea_orm::sea_query::Value),
    Compare {
        op: CompareOp,
        value: sea_orm::sea_query::Value,
    },
    Like { pattern: String, escape: char },
    Between {
        min: sea_orm::sea_query::Value,
        max: sea_orm::sea_query::Value,
    },
}

#[derive(Debug, Clone, Copy)]
pub enum CompareOp {
    Lt,
    Lte,
    Gt,
    Gte,
}

#[derive(Debug, Clone)]
pub struct ColumnFilter<C> {
    pub column: C,
    pub op: FilterOp,
}

pub struct DaoPager<D, F>
where
    D: DaoBase,
    F: Fn(Select<D::Entity>) -> Select<D::Entity> + Clone + Send,
{
    dao: D,
    page: u64,
    page_size: u64,
    order: Option<(<D::Entity as EntityTrait>::Column, Order)>,
    apply: F,
    done: bool,
}

impl<D, F> DaoPager<D, F>
where
    D: DaoBase,
    F: Fn(Select<D::Entity>) -> Select<D::Entity> + Clone + Send,
    <D::Entity as EntityTrait>::Column: Clone,
{
    pub async fn next_page(
        &mut self,
    ) -> DaoResult<Option<PaginatedResponse<<D::Entity as EntityTrait>::Model>>> {
        if self.done {
            return Ok(None);
        }

        let response = self
            .dao
            .find(
                self.page,
                self.page_size,
                self.order.clone(),
                self.apply.clone(),
            )
            .await?;

        if !response.has_next {
            self.done = true;
        }
        self.page = self.page.saturating_add(1);

        Ok(Some(response))
    }
}

#[async_trait::async_trait]
pub trait DaoBase: Clone + Send + Sync + Sized
where
    <Self::Entity as EntityTrait>::Model:
        FromQueryResult + IntoActiveModel<<Self::Entity as EntityTrait>::ActiveModel> + Send + Sync,
    <Self::Entity as EntityTrait>::ActiveModel:
        ActiveModelTrait<Entity = Self::Entity> + HasIdActiveModel + TimestampedActiveModel + Send,
    <<Self::Entity as EntityTrait>::PrimaryKey as PrimaryKeyTrait>::ValueType:
        From<Uuid> + Send + Sync,
    Self::Entity: HasCreatedAtColumn,
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
        order: Option<(<Self::Entity as EntityTrait>::Column, Order)>,
        apply: impl FnOnce(Select<Self::Entity>) -> Select<Self::Entity> + Send,
    ) -> DaoResult<PaginatedResponse<<Self::Entity as EntityTrait>::Model>> {
        if page == 0 || page_size == 0 || page_size > Self::MAX_PAGE_SIZE {
            return Err(DaoLayerError::InvalidPagination { page, page_size });
        }

        let base = Self::Entity::find();
        let filtered = apply(base);
        let ordered = match order {
            Some((column, order)) => filtered.order_by(column, order),
            None => filtered.order_by_desc(Self::Entity::created_at_column()),
        };
        let fetch_size = page_size.saturating_add(1);
        let offset = page.saturating_sub(1).saturating_mul(page_size);
        let mut data = ordered
            .limit(fetch_size)
            .offset(offset)
            .all(self.db())
            .await
            .map_err(DaoLayerError::Db)?;

        let has_next = data.len() > page_size as usize;
        if has_next {
            data.truncate(page_size as usize);
        }

        Ok(PaginatedResponse {
            data,
            page,
            page_size,
            has_next,
            total: None,
        })
    }

    async fn find_with_filters(
        &self,
        page: u64,
        page_size: u64,
        order: Option<(<Self::Entity as EntityTrait>::Column, Order)>,
        filters: &[ColumnFilter<<Self::Entity as EntityTrait>::Column>],
        apply: impl FnOnce(Select<Self::Entity>) -> Select<Self::Entity> + Send,
    ) -> DaoResult<PaginatedResponse<<Self::Entity as EntityTrait>::Model>>
    where
        <Self::Entity as EntityTrait>::Column: Clone,
    {
        if page == 0 || page_size == 0 || page_size > Self::MAX_PAGE_SIZE {
            return Err(DaoLayerError::InvalidPagination { page, page_size });
        }

        let base = Self::Entity::find();
        let filtered = apply(base);
        let filtered = filters.iter().fold(filtered, |select, filter| match &filter.op {
            FilterOp::Eq(value) => select.filter(filter.column.clone().eq(value.clone())),
            FilterOp::Compare { op, value } => {
                let expr = Expr::col(filter.column.clone());
                let value = Expr::val(value.clone());
                let expr = match op {
                    CompareOp::Lt => expr.lt(value),
                    CompareOp::Lte => expr.lte(value),
                    CompareOp::Gt => expr.gt(value),
                    CompareOp::Gte => expr.gte(value),
                };
                select.filter(expr)
            }
            FilterOp::Like { pattern, escape } => select.filter(
                Expr::col(filter.column.clone())
                    .like(LikeExpr::new(pattern).escape(*escape)),
            ),
            FilterOp::Between { min, max } => select.filter(
                Expr::col(filter.column.clone())
                    .between(Expr::val(min.clone()), Expr::val(max.clone())),
            ),
        });
        let ordered = match order {
            Some((column, order)) => filtered.order_by(column, order),
            None => filtered.order_by_desc(Self::Entity::created_at_column()),
        };
        let fetch_size = page_size.saturating_add(1);
        let offset = page.saturating_sub(1).saturating_mul(page_size);
        let mut data = ordered
            .limit(fetch_size)
            .offset(offset)
            .all(self.db())
            .await
            .map_err(DaoLayerError::Db)?;

        let has_next = data.len() > page_size as usize;
        if has_next {
            data.truncate(page_size as usize);
        }

        Ok(PaginatedResponse {
            data,
            page,
            page_size,
            has_next,
            total: None,
        })
    }

    fn find_iter<F>(
        &self,
        page_size: Option<u64>,
        order: Option<(<Self::Entity as EntityTrait>::Column, Order)>,
        apply: F,
    ) -> DaoPager<Self, F>
    where
        Self: Clone,
        F: Fn(Select<Self::Entity>) -> Select<Self::Entity> + Clone + Send,
        <Self::Entity as EntityTrait>::Column: Clone,
    {
        DaoPager {
            dao: self.clone(),
            page: 1,
            page_size: page_size.unwrap_or(Self::MAX_PAGE_SIZE),
            order,
            apply,
            done: false,
        }
    }

    async fn update<F>(&self, id: Uuid, apply: F) -> DaoResult<<Self::Entity as EntityTrait>::Model>
    where
        F: for<'a> FnOnce(&'a mut <Self::Entity as EntityTrait>::ActiveModel) + Send,
    {
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
