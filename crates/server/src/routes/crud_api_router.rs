use axum::{
    Router,
    extract::Request,
    response::Response,
    routing::{MethodRouter, Route},
};
use sea_orm::{ActiveModelTrait, Iterable, TryIntoModel};
use std::{collections::HashMap, convert::Infallible};
use tower::{Layer, Service, util::BoxCloneSyncServiceLayer};

use super::base_api_router::{
    ActiveModelOf, BaseApiRouter, ColumnOf, DEFAULT_ALLOWED_METHODS, ModelOf,
};
use crate::services::crud_service::CrudService;

pub use super::base_api_router::Method;

type MethodLayer = BoxCloneSyncServiceLayer<Route<Infallible>, Request, Response, Infallible>;

pub struct CrudApiRouter<S> {
    service: S,
    base_path: &'static str,
    allowed_methods: Vec<Method>,
    method_middlewares: HashMap<Method, Vec<MethodLayer>>,
}

impl<S> CrudApiRouter<S> {
    pub fn new(service: S, base_path: &'static str) -> Self {
        let a = 30

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

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::Body,
        extract::Request,
        http::{StatusCode, header},
        middleware::{Next, from_fn},
        response::{IntoResponse, Response},
    };
    use chrono::{FixedOffset, TimeZone};
    use sea_orm::entity::prelude::*;
    use sea_orm::{DatabaseBackend, DatabaseConnection, MockDatabase, Set};
    use tower::ServiceExt;
    use uuid::Uuid;

    use super::{CrudApiRouter, Method};
    use crate::db::dao::{DaoBase, HasCreatedAtColumn, HasIdActiveModel, TimestampedActiveModel};
    use crate::error::AppError;
    use crate::services::crud_service::{CrudService, FilterMode, FilterParseStrategy};

    mod test_entity {
        use sea_orm::entity::prelude::*;

        #[derive(
            Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, DeriveEntityModel,
        )]
        #[sea_orm(table_name = "test_router_items")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: uuid::Uuid,
            pub created_at: DateTimeWithTimeZone,
            pub updated_at: DateTimeWithTimeZone,
            pub title: String,
            pub score: i32,
            pub done: bool,
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
        deny: &'static [&'static str],
    }

    impl TestCrudService {
        fn new() -> Self {
            Self::with_deny(&[])
        }

        fn with_deny(deny: &'static [&'static str]) -> Self {
            let db = MockDatabase::new(DatabaseBackend::Postgres)
                .append_query_results([vec![model(Uuid::new_v4())]])
                .into_connection();
            Self {
                dao: TestDao::new(&db),
                deny,
            }
        }
    }

    #[async_trait::async_trait]
    impl CrudService for TestCrudService {
        type Dao = TestDao;

        fn dao(&self) -> &Self::Dao {
            &self.dao
        }

        fn list_filter_mode(&self) -> FilterMode<test_entity::Column> {
            FilterMode::AllColumns {
                deny: self.deny,
                parse: FilterParseStrategy::ByColumnType,
            }
        }

        async fn create<T>(&self, _data: T) -> Result<test_entity::Model, AppError>
        where
            T: sea_orm::IntoActiveModel<test_entity::ActiveModel> + Send,
        {
            Ok(model(Uuid::new_v4()))
        }

        async fn find_by_id(&self, id: Uuid) -> Result<test_entity::Model, AppError> {
            Ok(model(id))
        }

        async fn update<F>(&self, id: Uuid, _apply: F) -> Result<test_entity::Model, AppError>
        where
            F: for<'a> FnOnce(&'a mut test_entity::ActiveModel) + Send,
        {
            Ok(model(id))
        }

        async fn delete(&self, _id: Uuid) -> Result<(), AppError> {
            Ok(())
        }
    }

    fn model(id: Uuid) -> test_entity::Model {
        let now = FixedOffset::east_opt(0)
            .expect("offset should be valid")
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp should be valid");
        test_entity::Model {
            id,
            created_at: now,
            updated_at: now,
            title: "item".to_string(),
            score: 7,
            done: true,
        }
    }

    fn app() -> Router {
        CrudApiRouter::new(TestCrudService::new(), "/items").router()
    }

    fn app_with_denied_title_filter() -> Router {
        CrudApiRouter::new(TestCrudService::with_deny(&["title"]), "/items").router()
    }

    fn json_request(method: &str, uri: &str, body: &str) -> Request {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("request should build")
    }

    fn empty_request(method: &str, uri: &str) -> Request {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request should build")
    }

    async fn request_status(router: Router, request: Request) -> StatusCode {
        router
            .oneshot(request)
            .await
            .expect("request should succeed")
            .status()
    }

    async fn block_create(_req: Request, _next: Next) -> Response {
        StatusCode::UNAUTHORIZED.into_response()
    }

    #[tokio::test]
    async fn create_returns_created_for_valid_payload() {
        let app = app();

        let status = request_status(app, json_request("POST", "/items", "{}")).await;

        assert_eq!(status, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn create_rejects_invalid_payload_type() {
        let app = app();

        let status = request_status(app, json_request("POST", "/items", r#"{"title":123}"#)).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_returns_ok_without_query() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_string_equality_filter() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=item")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_integer_equality_filter() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?score=7")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_boolean_equality_filter() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?done=true")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_uuid_equality_filter() {
        let app = app();
        let id = Uuid::new_v4();

        let status = request_status(app, empty_request("GET", &format!("/items?id={id}"))).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_datetime_equality_filter() {
        let app = app();

        let status = request_status(
            app,
            empty_request("GET", "/items?created_at=2026-01-01T00:00:00%2B00:00"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_prefix_wildcard_filter() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=item*")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_suffix_wildcard_filter() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=*item")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_contains_wildcard_filter() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=*item*")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_rejects_middle_wildcard_pattern() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=a*b")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_accepts_greater_than_filter_for_orderable_column() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?score=%3E7")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_greater_than_or_equal_filter_for_orderable_column() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?score=%3E%3D7")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_less_than_filter_for_orderable_column() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?score=%3C7")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_less_than_or_equal_filter_for_orderable_column() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?score=%3C%3D7")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_between_filter_for_orderable_column() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?score=1..9")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_rejects_malformed_between_filter() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?score=1..2..3")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_rejects_comparison_on_boolean_column() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?done=%3Etrue")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_accepts_multiple_filters_in_single_request() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=item&score=7")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_pagination_with_filters() {
        let app = app();

        let status = request_status(
            app,
            empty_request("GET", "/items?page=2&page_size=10&title=item"),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_rejects_unknown_filter_key() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?unknown=1")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_rejects_denied_filter_key() {
        let app = app_with_denied_title_filter();

        let status = request_status(app, empty_request("GET", "/items?title=item")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_rejects_invalid_integer_filter_value() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?score=not-a-number")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_rejects_empty_filter_value() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_accepts_url_encoded_filter_value() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=%2Aitem%2A")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_accepts_duplicate_filter_key_query_shape() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?title=one&title=two")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn list_rejects_non_numeric_page() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?page=abc")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_rejects_non_numeric_page_size() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?page_size=abc")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_rejects_zero_page() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?page=0")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_rejects_zero_page_size() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?page_size=0")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn list_rejects_page_size_over_max() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items?page_size=101")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_returns_ok_for_valid_uuid_path() {
        let app = app();
        let id = Uuid::new_v4();

        let status = request_status(app, empty_request("GET", &format!("/items/{id}"))).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn get_rejects_invalid_uuid_path() {
        let app = app();

        let status = request_status(app, empty_request("GET", "/items/not-a-uuid")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn patch_returns_ok_for_valid_uuid_and_payload() {
        let app = app();
        let id = Uuid::new_v4();

        let status =
            request_status(app, json_request("PATCH", &format!("/items/{id}"), "{}")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn patch_rejects_invalid_uuid_path() {
        let app = app();

        let status = request_status(app, json_request("PATCH", "/items/not-a-uuid", "{}")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn patch_rejects_invalid_payload_type() {
        let app = app();
        let id = Uuid::new_v4();

        let status = request_status(
            app,
            json_request("PATCH", &format!("/items/{id}"), r#"{"title":123}"#),
        )
        .await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn delete_returns_no_content_for_valid_uuid_path() {
        let app = app();
        let id = Uuid::new_v4();

        let status = request_status(app, empty_request("DELETE", &format!("/items/{id}"))).await;

        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn delete_rejects_invalid_uuid_path() {
        let app = app();

        let status = request_status(app, empty_request("DELETE", "/items/not-a-uuid")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn disallowed_id_method_returns_method_not_allowed() {
        let app = CrudApiRouter::new(TestCrudService::new(), "/items")
            .set_allowed_methods(&[Method::Get])
            .router();
        let id = Uuid::new_v4();

        let status =
            request_status(app, json_request("PATCH", &format!("/items/{id}"), "{}")).await;

        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn create_method_middleware_can_override_create_response() {
        let app = CrudApiRouter::new(TestCrudService::new(), "/items")
            .set_method_middleware(Method::Create, from_fn(block_create))
            .router();

        let status = request_status(app, json_request("POST", "/items", "{}")).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn create_method_middleware_does_not_affect_list_route() {
        let app = CrudApiRouter::new(TestCrudService::new(), "/items")
            .set_method_middleware(Method::Create, from_fn(block_create))
            .router();

        let status = request_status(app, empty_request("GET", "/items")).await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn disallowed_list_method_returns_method_not_allowed_on_base_path() {
        let app = CrudApiRouter::new(TestCrudService::new(), "/items")
            .set_allowed_methods(&[Method::Create])
            .router();

        let status = request_status(app, empty_request("GET", "/items")).await;

        assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    }
}
