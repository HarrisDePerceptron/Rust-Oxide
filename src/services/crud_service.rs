use axum::http::StatusCode;
use sea_orm::{EntityTrait, IntoActiveModel, Order, Select};
use uuid::Uuid;

use crate::db::dao::{DaoBase, DaoLayerError, PaginatedResponse};
use crate::error::AppError;

type CrudEntity<D> = <D as DaoBase>::Entity;
type CrudModel<D> = <CrudEntity<D> as EntityTrait>::Model;
type CrudActiveModel<D> = <CrudEntity<D> as EntityTrait>::ActiveModel;
type CrudColumn<D> = <CrudEntity<D> as EntityTrait>::Column;

#[derive(Clone, Copy)]
pub struct CrudErrors {
    pub create_failed: &'static str,
    pub find_failed: &'static str,
    pub not_found: &'static str,
    pub update_failed: &'static str,
    pub delete_failed: &'static str,
    pub invalid_pagination: &'static str,
}

impl Default for CrudErrors {
    fn default() -> Self {
        Self {
            create_failed: "Create failed",
            find_failed: "Find failed",
            not_found: "Resource not found",
            update_failed: "Update failed",
            delete_failed: "Delete failed",
            invalid_pagination: "Invalid pagination",
        }
    }
}

#[derive(Clone, Copy)]
pub enum CrudOp {
    Create,
    Find,
    List,
    Update,
    Delete,
}

#[allow(async_fn_in_trait)]
pub trait CrudService {
    type Dao: DaoBase;

    fn dao(&self) -> &Self::Dao;
    fn errors(&self) -> CrudErrors {
        CrudErrors::default()
    }

    fn map_error(&self, op: CrudOp, err: DaoLayerError) -> AppError {
        let errors = self.errors();
        match err {
            DaoLayerError::NotFound { .. } => {
                AppError::new(StatusCode::NOT_FOUND, errors.not_found)
            }
            DaoLayerError::InvalidPagination { .. } => {
                AppError::new(StatusCode::BAD_REQUEST, errors.invalid_pagination)
            }
            DaoLayerError::Db(_) => {
                let message = match op {
                    CrudOp::Create => errors.create_failed,
                    CrudOp::Find | CrudOp::List => errors.find_failed,
                    CrudOp::Update => errors.update_failed,
                    CrudOp::Delete => errors.delete_failed,
                };
                AppError::new(StatusCode::INTERNAL_SERVER_ERROR, message)
            }
        }
    }

    async fn create<T>(&self, data: T) -> Result<CrudModel<Self::Dao>, AppError>
    where
        T: IntoActiveModel<CrudActiveModel<Self::Dao>> + Send,
    {
        self.dao()
            .create(data)
            .await
            .map_err(|err| self.map_error(CrudOp::Create, err))
    }

    async fn find_by_id(&self, id: Uuid) -> Result<CrudModel<Self::Dao>, AppError> {
        self.dao()
            .find_by_id(id)
            .await
            .map_err(|err| self.map_error(CrudOp::Find, err))
    }

    async fn find<F>(
        &self,
        page: u64,
        page_size: u64,
        order: Option<(CrudColumn<Self::Dao>, Order)>,
        apply: F,
    ) -> Result<PaginatedResponse<CrudModel<Self::Dao>>, AppError>
    where
        F: FnOnce(Select<CrudEntity<Self::Dao>>) -> Select<CrudEntity<Self::Dao>> + Send,
    {
        self.dao()
            .find(page, page_size, order, apply)
            .await
            .map_err(|err| self.map_error(CrudOp::List, err))
    }

    async fn update<F>(&self, id: Uuid, apply: F) -> Result<CrudModel<Self::Dao>, AppError>
    where
        F: FnOnce(&mut CrudActiveModel<Self::Dao>) + Send,
    {
        self.dao()
            .update(id, apply)
            .await
            .map_err(|err| self.map_error(CrudOp::Update, err))
    }

    async fn delete(&self, id: Uuid) -> Result<(), AppError> {
        self.dao()
            .delete(id)
            .await
            .map(|_| ())
            .map_err(|err| self.map_error(CrudOp::Delete, err))
    }
}
