use axum::{
    Json, Router,
    extract::{Path, Query, Request},
    extract::rejection::QueryRejection,
    http::StatusCode,
    response::Response,
    routing::{MethodRouter, Route, delete, get, patch, post},
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, EntityTrait, IdenStatic, Iterable, Order, PrimaryKeyToColumn,
    Select, TryIntoModel,
};
use serde_json::Value;
use std::{collections::HashMap, convert::Infallible};
use tower::{Layer, Service, util::BoxCloneSyncServiceLayer};
use uuid::Uuid;

use super::base_router::BaseRouter;
use crate::{
    db::dao::DaoBase,
    error::AppError,
    response::JsonApiResponse,
    services::crud_service::CrudService,
};

type DaoOf<S> = <S as CrudService>::Dao;
type EntityOf<S> = <DaoOf<S> as DaoBase>::Entity;
type ModelOf<S> = <EntityOf<S> as EntityTrait>::Model;
type ActiveModelOf<S> = <EntityOf<S> as EntityTrait>::ActiveModel;
type ColumnOf<S> = <EntityOf<S> as EntityTrait>::Column;

#[derive(Clone, serde::Deserialize)]
pub struct ListQuery {
    pub page: Option<u64>,
    pub page_size: Option<u64>,
    #[serde(flatten, default)]
    pub filters: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Method {
    Create,
    List,
    Get,
    Patch,
    Delete,
}

const DEFAULT_ALLOWED_METHODS: [Method; 5] = [
    Method::Create,
    Method::List,
    Method::Get,
    Method::Patch,
    Method::Delete,
];

const INVALID_PAYLOAD_MESSAGE: &str = "Invalid payload";
const INVALID_QUERY_MESSAGE: &str = "Invalid query";

type MethodLayer = BoxCloneSyncServiceLayer<Route<Infallible>, Request, Response, Infallible>;

pub struct CrudApiRouter<S> {
    service: S,
    base_path: &'static str,
    allowed_methods: Vec<Method>,
    method_middlewares: HashMap<Method, Vec<MethodLayer>>,
}

impl<S> CrudApiRouter<S> {
    pub fn new(service: S, base_path: &'static str) -> Self {
        Self {
            service,
            base_path,
            allowed_methods: DEFAULT_ALLOWED_METHODS.to_vec(),
            method_middlewares: HashMap::new(),
        }
    }

    pub fn set_allowed_methods(mut self, methods: &[Method]) -> Self {
        self.allowed_methods = methods.to_vec();
        self
    }

    pub fn set_method_middleware<L>(mut self, method: Method, layer: L) -> Self
    where
        L: Layer<Route<Infallible>> + Clone + Send + Sync + 'static,
        L::Service: Service<Request, Response = Response, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request>>::Future: Send + 'static,
    {
        self.method_middlewares
            .entry(method)
            .or_default()
            .push(BoxCloneSyncServiceLayer::new(layer));
        self
    }
}

impl<S> CrudApiRouter<S>
where
    S: CrudService + Clone + Send + Sync + 'static,
    ActiveModelOf<S>: ActiveModelTrait + TryIntoModel<ModelOf<S>>,
    ModelOf<S>: for<'de> serde::Deserialize<'de> + serde::Serialize,
    ModelOf<S>: sea_orm::IntoActiveModel<ActiveModelOf<S>>,
    ColumnOf<S>: Iterable,
{
    pub fn router<State>(self) -> Router<State>
    where
        State: Clone + Send + Sync + 'static,
    {
        BaseApiRouter::router_for(&self)
    }
}

#[allow(async_fn_in_trait)]
pub trait BaseApiRouter: BaseRouter
where
    ActiveModelOf<Self::Service>: ActiveModelTrait + TryIntoModel<ModelOf<Self::Service>>,
    ModelOf<Self::Service>: for<'de> serde::Deserialize<'de> + serde::Serialize,
    ModelOf<Self::Service>: sea_orm::IntoActiveModel<ActiveModelOf<Self::Service>>,
    ColumnOf<Self::Service>: Iterable,
{
    type Service: CrudService + Clone + Send + Sync + 'static;

    fn service(&self) -> Self::Service;
    fn base_path(&self) -> &'static str;

    fn allowed_methods(&self) -> &[Method] {
        &DEFAULT_ALLOWED_METHODS
    }

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
            .map_err(|err| {
                AppError::new(
                    StatusCode::BAD_REQUEST,
                    format!("{INVALID_PAYLOAD_MESSAGE}: {err}"),
                )
            })?;
        Ok(active)
    }

    fn build_update(payload: Value) -> Result<ActiveModelOf<Self::Service>, AppError> {
        let mut active = <ActiveModelOf<Self::Service> as ActiveModelTrait>::default();
        active
            .set_from_json(payload)
            .map_err(|err| {
                AppError::new(
                    StatusCode::BAD_REQUEST,
                    format!("{INVALID_PAYLOAD_MESSAGE}: {err}"),
                )
            })?;
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

    fn apply_router_middleware<S>(&self, router: Router<S>) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        router
    }

    fn apply_method_middleware<S>(&self, _method: Method, route: MethodRouter<S>) -> MethodRouter<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        route
    }

    fn register_routes<S>(&self, router: Router<S>) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        router
    }

    fn router_for<S>(&self) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        let base = self.base_path();
        let id_path = format!("{}/{{id}}", base);
        let allowed = self.allowed_methods();
        let mut router = Router::<S>::new();

        if allowed.contains(&Method::Create) {
            let route = post({
                let service = self.service();
                move |Json(payload)| async move {
                    let active = Self::build_create(payload)?;
                    let model: ModelOf<Self::Service> = service.create(active).await?;
                    JsonApiResponse::with_status(
                        StatusCode::CREATED,
                        "created",
                        model,
                    )
                }
            });
            router = router.route(base, self.apply_method_middleware(Method::Create, route));
        }

        if allowed.contains(&Method::List) {
            let route = get({
                let service = self.service();
                move |query: Result<Query<ListQuery>, QueryRejection>| async move {
                    let Query(query) = query
                        .map_err(|err| {
                            AppError::new(
                                StatusCode::BAD_REQUEST,
                                format!("{INVALID_QUERY_MESSAGE}: {err}"),
                            )
                        })?;
                    let page = query.page.unwrap_or(1);
                    let page_size = query.page_size.unwrap_or_else(Self::list_default_page_size);
                    let response = service
                        .find_with_filters(
                            page,
                            page_size,
                            Self::list_order(),
                            query.filters.clone(),
                            |select| Self::list_apply(&query, select),
                        )
                        .await?;
                    JsonApiResponse::ok(response)
                }
            });
            router = router.route(base, self.apply_method_middleware(Method::List, route));
        }

        if allowed.contains(&Method::Get) {
            let route = get({
                let service = self.service();
                move |Path(id): Path<Uuid>| async move {
                    let model: ModelOf<Self::Service> = service.find_by_id(id).await?;
                    JsonApiResponse::ok(model)
                }
            });
            router = router.route(&id_path, self.apply_method_middleware(Method::Get, route));
        }

        if allowed.contains(&Method::Patch) {
            let route = patch({
                let service = self.service();
                move |Path(id): Path<Uuid>, Json(payload)| async move {
                    let patch = Self::build_update(payload)?;
                    let model: ModelOf<Self::Service> = service
                        .update(id, move |active| Self::apply_patch(active, patch))
                        .await?;
                    JsonApiResponse::ok(model)
                }
            });
            router = router.route(&id_path, self.apply_method_middleware(Method::Patch, route));
        }

        if allowed.contains(&Method::Delete) {
            let route = delete({
                let service = self.service();
                move |Path(id): Path<Uuid>| async move {
                    service.delete(id).await?;
                    JsonApiResponse::with_status(
                        StatusCode::NO_CONTENT,
                        "deleted",
                        Value::Null,
                    )
                }
            });
            router = router.route(
                &id_path,
                self.apply_method_middleware(Method::Delete, route),
            );
        }

        let router = self.register_routes(router);
        <Self as BaseApiRouter>::apply_router_middleware(self, router)
    }
}

// Blanket implementation so all CRUD routers automatically satisfy BaseRouter.
impl<T> BaseRouter for T
where
    T: BaseApiRouter,
{
    fn apply_router_middleware<S>(&self, router: Router<S>) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        <T as BaseApiRouter>::apply_router_middleware(self, router)
    }

    fn router_for<S>(&self) -> Router<S>
    where
        S: Clone + Send + Sync + 'static,
    {
        <T as BaseApiRouter>::router_for(self)
    }
}

impl<S> BaseApiRouter for CrudApiRouter<S>
where
    S: CrudService + Clone + Send + Sync + 'static,
    ActiveModelOf<S>: ActiveModelTrait + TryIntoModel<ModelOf<S>>,
    ModelOf<S>: for<'de> serde::Deserialize<'de> + serde::Serialize,
    ModelOf<S>: sea_orm::IntoActiveModel<ActiveModelOf<S>>,
    ColumnOf<S>: Iterable,
{
    type Service = S;

    fn service(&self) -> Self::Service {
        self.service.clone()
    }

    fn base_path(&self) -> &'static str {
        self.base_path
    }

    fn allowed_methods(&self) -> &[Method] {
        self.allowed_methods.as_slice()
    }

    fn apply_method_middleware<State>(
        &self,
        method: Method,
        route: MethodRouter<State>,
    ) -> MethodRouter<State>
    where
        State: Clone + Send + Sync + 'static,
    {
        match self.method_middlewares.get(&method) {
            Some(layers) => layers
                .iter()
                .fold(route, |route, layer| route.route_layer(layer.clone())),
            None => route,
        }
    }
}
