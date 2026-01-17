use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, patch, post},
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, EntityTrait, IdenStatic, Iterable, Order, PrimaryKeyToColumn,
    Select, TryIntoModel,
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    db::dao::DaoBase, error::AppError, services::crud_service::CrudService, state::AppState,
};

type DaoOf<S> = <S as CrudService>::Dao;
type EntityOf<S> = <DaoOf<S> as DaoBase>::Entity;
type ModelOf<S> = <EntityOf<S> as EntityTrait>::Model;
type ActiveModelOf<S> = <EntityOf<S> as EntityTrait>::ActiveModel;
type ColumnOf<S> = <EntityOf<S> as EntityTrait>::Column;
type ServiceOf<R> = <R as CrudRouter>::Service;
type RouterModel<R> = ModelOf<ServiceOf<R>>;

#[derive(Clone, serde::Deserialize)]
pub struct ListQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
}

const INVALID_PAYLOAD_MESSAGE: &str = "Invalid payload";

#[allow(async_fn_in_trait)]
pub trait CrudRouter
where
    ActiveModelOf<Self::Service>: ActiveModelTrait + TryIntoModel<ModelOf<Self::Service>>,
    ModelOf<Self::Service>: for<'de> serde::Deserialize<'de> + serde::Serialize,
    ModelOf<Self::Service>: sea_orm::IntoActiveModel<ActiveModelOf<Self::Service>>,
    ColumnOf<Self::Service>: Iterable,
{
    type Service: CrudService + Clone + Send + Sync + 'static;

    fn service(state: &AppState) -> Self::Service;
    fn base_path() -> &'static str;

    fn list_default_page_size() -> u64 {
        25
    }

    fn list_order() -> Option<(ColumnOf<Self::Service>, Order)> {
        None
    }

    fn list_apply(
        _query: &ListQuery,
        select: Select<EntityOf<Self::Service>>,
    ) -> Select<EntityOf<Self::Service>> {
        select
    }

    fn build_create(payload: Value) -> Result<ActiveModelOf<Self::Service>, AppError> {
        let mut active = <ActiveModelOf<Self::Service> as ActiveModelTrait>::default();
        active
            .set_from_json(payload)
            .map_err(|_| AppError::new(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_MESSAGE))?;
        Ok(active)
    }

    fn build_update(payload: Value) -> Result<ActiveModelOf<Self::Service>, AppError> {
        let mut active = <ActiveModelOf<Self::Service> as ActiveModelTrait>::default();
        active
            .set_from_json(payload)
            .map_err(|_| AppError::new(StatusCode::BAD_REQUEST, INVALID_PAYLOAD_MESSAGE))?;
        Ok(active)
    }

    fn apply_patch(active: &mut ActiveModelOf<Self::Service>, patch: ActiveModelOf<Self::Service>) {
        let primary_keys: Vec<&'static str> =
            <EntityOf<Self::Service> as EntityTrait>::PrimaryKey::iter()
                .map(|pk| pk.into_column().as_str())
                .collect();

        for col in ColumnOf::<Self::Service>::iter() {
            if primary_keys.iter().any(|pk| *pk == col.as_str()) {
                continue;
            }
            match patch.get(col) {
                ActiveValue::Set(value) | ActiveValue::Unchanged(value) => active.set(col, value),
                ActiveValue::NotSet => {}
            }
        }
    }

    fn router(state: Arc<AppState>) -> Router
    where
        Self: Sized + Send + Sync + 'static,
    {
        router_for::<Self>(state)
    }
}

pub fn router_for<R>(state: Arc<AppState>) -> Router
where
    R: CrudRouter + Send + Sync + 'static,
{
    let base = R::base_path();
    Router::new()
        .route(
            base,
            post(|State(state): State<Arc<AppState>>, Json(payload)| async move {
                let service = R::service(&state);
                let active = R::build_create(payload)?;
                let model: RouterModel<R> = service.create(active).await?;
                Ok::<_, AppError>((StatusCode::CREATED, Json(model)))
            }),
        )
        .route(
            base,
            get(|State(state): State<Arc<AppState>>, Query(query): Query<ListQuery>| async move {
                let service = R::service(&state);
                let page = query.page.unwrap_or(1);
                let page_size = query.page_size.unwrap_or_else(R::list_default_page_size);
                let response = service
                    .find(page, page_size, R::list_order(), |select| {
                        R::list_apply(&query, select)
                    })
                    .await?;
                Ok::<_, AppError>(Json(response))
            }),
        )
        .route(
            &format!("{}/{{id}}", base),
            get(|State(state): State<Arc<AppState>>, Path(id): Path<Uuid>| async move {
                let service = R::service(&state);
                let model: RouterModel<R> = service.find_by_id(id).await?;
                Ok::<_, AppError>(Json(model))
            }),
        )
        .route(
            &format!("{}/{{id}}", base),
            patch(
                |State(state): State<Arc<AppState>>,
                 Path(id): Path<Uuid>,
                 Json(payload)| async move {
                let service = R::service(&state);
                let patch = R::build_update(payload)?;
                let model: RouterModel<R> = service
                    .update(id, move |active| R::apply_patch(active, patch))
                    .await?;
                Ok::<_, AppError>(Json(model))
            }),
        )
        .route(
            &format!("{}/{{id}}", base),
            delete(|State(state): State<Arc<AppState>>, Path(id): Path<Uuid>| async move {
                let service = R::service(&state);
                service.delete(id).await?;
                Ok::<_, AppError>(StatusCode::NO_CONTENT)
            }),
        )
        .with_state(state)
}
