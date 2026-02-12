use axum::{
    Json, Router,
    extract::rejection::QueryRejection,
    extract::{Path, Query},
    http::StatusCode,
    routing::{MethodRouter, delete, get, patch, post},
};
use sea_orm::{
    ActiveModelTrait, ActiveValue, EntityTrait, IdenStatic, Iterable, Order, PrimaryKeyToColumn,
    Select, TryIntoModel,
};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

use super::base_router::BaseRouter;
use crate::{
    db::dao::DaoBase, error::AppError, routes::JsonApiResponse, services::crud_service::CrudService,
};

pub(crate) type DaoOf<S> = <S as CrudService>::Dao;
pub(crate) type EntityOf<S> = <DaoOf<S> as DaoBase>::Entity;
pub(crate) type ModelOf<S> = <EntityOf<S> as EntityTrait>::Model;
pub(crate) type ActiveModelOf<S> = <EntityOf<S> as EntityTrait>::ActiveModel;
pub(crate) type ColumnOf<S> = <EntityOf<S> as EntityTrait>::Column;

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

pub(crate) const DEFAULT_ALLOWED_METHODS: [Method; 5] = [
    Method::Create,
    Method::List,
    Method::Get,
    Method::Patch,
    Method::Delete,
];

const INVALID_PAYLOAD_MESSAGE: &str = "Invalid payload";
const INVALID_QUERY_MESSAGE: &str = "Invalid query";

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
            .map_err(|err| AppError::bad_request(format!("{INVALID_PAYLOAD_MESSAGE}: {err}")))?;
        Ok(active)
    }

    fn build_update(payload: Value) -> Result<ActiveModelOf<Self::Service>, AppError> {
        let mut active = <ActiveModelOf<Self::Service> as ActiveModelTrait>::default();
        active
            .set_from_json(payload)
            .map_err(|err| AppError::bad_request(format!("{INVALID_PAYLOAD_MESSAGE}: {err}")))?;
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
                    JsonApiResponse::with_status(StatusCode::CREATED, "created", model)
                }
            });
            router = router.route(base, self.apply_method_middleware(Method::Create, route));
        }

        if allowed.contains(&Method::List) {
            let route = get({
                let service = self.service();
                move |query: Result<Query<ListQuery>, QueryRejection>| async move {
                    let Query(query) = query.map_err(|err| {
                        AppError::bad_request(format!("{INVALID_QUERY_MESSAGE}: {err}"))
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
                    JsonApiResponse::with_status(StatusCode::NO_CONTENT, "deleted", Value::Null)
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

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use chrono::{FixedOffset, TimeZone};
    use sea_orm::entity::prelude::*;
    use sea_orm::{
        ActiveValue, DatabaseBackend, DatabaseConnection, MockDatabase, Order, Select, Set,
    };
    use serde_json::json;
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::{BaseApiRouter, Method};
    use crate::{
        db::dao::{
            DaoBase, HasCreatedAtColumn, HasIdActiveModel, PaginatedResponse,
            TimestampedActiveModel,
        },
        error::AppError,
        services::crud_service::CrudService,
    };

    mod test_entity {
        use sea_orm::entity::prelude::*;

        #[derive(
            Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, DeriveEntityModel,
        )]
        #[sea_orm(table_name = "test_base_api_router_items")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: uuid::Uuid,
            pub created_at: DateTimeWithTimeZone,
            pub updated_at: DateTimeWithTimeZone,
            pub title: String,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {}

        impl ActiveModelBehavior for ActiveModel {}
    }

    impl HasCreatedAtColumn for test_entity::Entity {
        fn created_at_column() -> Self::Column {
            test_entity::Column::CreatedAt
        }
    }

    impl HasIdActiveModel for test_entity::ActiveModel {
        fn set_id(&mut self, id: Uuid) {
            self.id = Set(id);
        }
    }

    impl TimestampedActiveModel for test_entity::ActiveModel {
        fn set_created_at(&mut self, ts: DateTimeWithTimeZone) {
            self.created_at = Set(ts);
        }

        fn set_updated_at(&mut self, ts: DateTimeWithTimeZone) {
            self.updated_at = Set(ts);
        }
    }

    #[derive(Clone)]
    struct TestDao {
        db: DatabaseConnection,
    }

    impl DaoBase for TestDao {
        type Entity = test_entity::Entity;

        fn new(db: &DatabaseConnection) -> Self {
            Self { db: db.clone() }
        }

        fn db(&self) -> &DatabaseConnection {
            &self.db
        }
    }

    #[derive(Clone)]
    struct TestCrudService {
        dao: TestDao,
    }

    impl TestCrudService {
        fn new() -> Self {
            let db = MockDatabase::new(DatabaseBackend::Postgres).into_connection();
            Self {
                dao: TestDao::new(&db),
            }
        }
    }

    #[async_trait::async_trait]
    impl CrudService for TestCrudService {
        type Dao = TestDao;

        fn dao(&self) -> &Self::Dao {
            &self.dao
        }

        async fn create<T>(&self, data: T) -> Result<test_entity::Model, AppError>
        where
            T: sea_orm::IntoActiveModel<test_entity::ActiveModel> + Send,
        {
            let active = data.into_active_model();
            let title = match active.title {
                ActiveValue::Set(v) | ActiveValue::Unchanged(v) => v,
                ActiveValue::NotSet => "created".to_string(),
            };
            Ok(model(Uuid::new_v4(), &title))
        }

        async fn find_by_id(&self, id: Uuid) -> Result<test_entity::Model, AppError> {
            Ok(model(id, "found"))
        }

        async fn find_with_filters<F>(
            &self,
            page: u64,
            page_size: u64,
            _order: Option<(test_entity::Column, Order)>,
            _filters: std::collections::HashMap<String, String>,
            _apply: F,
        ) -> Result<PaginatedResponse<test_entity::Model>, AppError>
        where
            F: FnOnce(Select<test_entity::Entity>) -> Select<test_entity::Entity> + Send,
            test_entity::Column: sea_orm::ColumnTrait + Clone,
        {
            Ok(PaginatedResponse {
                data: vec![],
                page,
                page_size,
                has_next: false,
                total: None,
            })
        }

        async fn update<F>(&self, id: Uuid, apply: F) -> Result<test_entity::Model, AppError>
        where
            F: for<'a> FnOnce(&'a mut test_entity::ActiveModel) + Send,
        {
            let mut active = test_entity::ActiveModel {
                id: Set(id),
                title: Set("before".to_string()),
                created_at: Set(ts()),
                updated_at: Set(ts()),
            };
            apply(&mut active);
            let title = match active.title {
                ActiveValue::Set(v) | ActiveValue::Unchanged(v) => v,
                ActiveValue::NotSet => "before".to_string(),
            };
            Ok(model(id, &title))
        }

        async fn delete(&self, _id: Uuid) -> Result<(), AppError> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct TestBaseRouter {
        service: TestCrudService,
        allowed_methods: Vec<Method>,
    }

    impl TestBaseRouter {
        fn new(allowed_methods: &[Method]) -> Self {
            Self {
                service: TestCrudService::new(),
                allowed_methods: allowed_methods.to_vec(),
            }
        }
    }

    impl BaseApiRouter for TestBaseRouter {
        type Service = TestCrudService;

        fn service(&self) -> Self::Service {
            self.service.clone()
        }

        fn base_path(&self) -> &'static str {
            "/items"
        }

        fn allowed_methods(&self) -> &[Method] {
            self.allowed_methods.as_slice()
        }
    }

    fn ts() -> chrono::DateTime<chrono::FixedOffset> {
        FixedOffset::east_opt(0)
            .expect("offset should be valid")
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp should be valid")
    }

    fn model(id: Uuid, title: &str) -> test_entity::Model {
        test_entity::Model {
            id,
            created_at: ts(),
            updated_at: ts(),
            title: title.to_string(),
        }
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        serde_json::from_slice::<serde_json::Value>(&bytes).expect("response should be valid json")
    }

    #[test]
    fn build_create_rejects_invalid_payload() {
        let err = TestBaseRouter::build_create(json!({ "title": 123 }))
            .expect_err("invalid payload should fail");

        assert!(err.message().starts_with("Invalid payload:"));
    }

    #[test]
    fn build_update_rejects_invalid_payload() {
        let err = TestBaseRouter::build_update(json!({ "title": 123 }))
            .expect_err("invalid payload should fail");

        assert!(err.message().starts_with("Invalid payload:"));
    }

    #[test]
    fn apply_patch_updates_non_primary_key_field() {
        let id = Uuid::new_v4();
        let mut active = test_entity::ActiveModel {
            id: Set(id),
            title: Set("before".to_string()),
            created_at: Set(ts()),
            updated_at: Set(ts()),
        };
        let patch = TestBaseRouter::build_update(json!({ "title": "after" }))
            .expect("update payload should parse");

        TestBaseRouter::apply_patch(&mut active, patch);

        assert!(matches!(active.title, ActiveValue::Set(ref title) if title == "after"));
    }

    #[test]
    fn apply_patch_does_not_override_primary_key_field() {
        let original_id = Uuid::new_v4();
        let mut active = test_entity::ActiveModel {
            id: Set(original_id),
            title: Set("before".to_string()),
            created_at: Set(ts()),
            updated_at: Set(ts()),
        };
        let patch = TestBaseRouter::build_update(json!({ "id": Uuid::new_v4() }))
            .expect("update payload should parse");

        TestBaseRouter::apply_patch(&mut active, patch);

        assert!(matches!(active.id, ActiveValue::Set(id) if id == original_id));
    }

    #[tokio::test]
    async fn list_route_defaults_page_to_one() {
        let router = TestBaseRouter::new(&[Method::List]).router_for();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/items")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        let json = response_json(response).await;
        assert_eq!(json["data"]["page"], 1);
    }

    #[tokio::test]
    async fn list_route_defaults_page_size_to_twenty_five() {
        let router = TestBaseRouter::new(&[Method::List]).router_for();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/items")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        let json = response_json(response).await;
        assert_eq!(json["data"]["page_size"], 25);
    }

    #[tokio::test]
    async fn list_route_uses_query_page_value() {
        let router = TestBaseRouter::new(&[Method::List]).router_for();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/items?page=3")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        let json = response_json(response).await;
        assert_eq!(json["data"]["page"], 3);
    }

    #[tokio::test]
    async fn list_route_uses_query_page_size_value() {
        let router = TestBaseRouter::new(&[Method::List]).router_for();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/items?page_size=7")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        let json = response_json(response).await;
        assert_eq!(json["data"]["page_size"], 7);
    }

    #[tokio::test]
    async fn list_route_maps_query_rejection_to_bad_request_status() {
        let router = TestBaseRouter::new(&[Method::List]).router_for();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/items?page=not-a-number")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_route_maps_query_rejection_to_invalid_query_message() {
        let router = TestBaseRouter::new(&[Method::List]).router_for();
        let response = router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/items?page=not-a-number")
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should succeed");

        let json = response_json(response).await;
        let message = json["message"]
            .as_str()
            .expect("message should be a string");
        assert!(message.starts_with("Invalid query:"));
    }
}
