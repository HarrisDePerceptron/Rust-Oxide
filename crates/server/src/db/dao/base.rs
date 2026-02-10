use chrono::Utc;
use sea_orm::sea_query::{Expr, ExprTrait, LikeExpr};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, FromQueryResult,
    IntoActiveModel, Order, PrimaryKeyTrait, QueryFilter, QueryOrder, QuerySelect, Select,
};
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
    Like {
        pattern: String,
        escape: char,
    },
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

    fn new(db: &DatabaseConnection) -> Self;

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
        let filtered = filters
            .iter()
            .fold(filtered, |select, filter| match &filter.op {
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
                    Expr::col(filter.column.clone()).like(LikeExpr::new(pattern).escape(*escape)),
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

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};
    use sea_orm::entity::prelude::*;
    use sea_orm::sea_query::Value;
    use sea_orm::{
        DatabaseBackend, DatabaseConnection, DbErr, MockDatabase, MockExecResult, Order,
        QueryFilter, Set,
    };
    use uuid::Uuid;

    use super::{
        ColumnFilter, CompareOp, DaoBase, DaoLayerError, FilterOp, HasCreatedAtColumn,
        HasIdActiveModel, TimestampedActiveModel,
    };

    mod test_entity {
        use sea_orm::entity::prelude::*;

        #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
        #[sea_orm(table_name = "test_records")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: uuid::Uuid,
            pub created_at: DateTimeWithTimeZone,
            pub updated_at: DateTimeWithTimeZone,
            pub name: String,
            pub score: i32,
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

    struct DaoFixture {
        dao: TestDao,
        db: DatabaseConnection,
    }

    struct DaoFixtureBuilder {
        mock: MockDatabase,
    }

    impl DaoFixtureBuilder {
        fn new() -> Self {
            Self {
                mock: MockDatabase::new(DatabaseBackend::Postgres),
            }
        }

        fn with_query_results(mut self, sets: Vec<Vec<test_entity::Model>>) -> Self {
            self.mock = self.mock.append_query_results(sets);
            self
        }

        fn with_query_error(mut self, error: DbErr) -> Self {
            self.mock = self.mock.append_query_errors([error]);
            self
        }

        fn with_exec_result(mut self, rows_affected: u64) -> Self {
            self.mock = self.mock.append_exec_results([MockExecResult {
                last_insert_id: 0,
                rows_affected,
            }]);
            self
        }

        fn with_exec_error(mut self, error: DbErr) -> Self {
            self.mock = self.mock.append_exec_errors([error]);
            self
        }

        fn build(self) -> DaoFixture {
            let db = self.mock.into_connection();
            let dao = TestDao::new(&db);
            DaoFixture { dao, db }
        }
    }

    fn ts() -> chrono::DateTime<chrono::FixedOffset> {
        FixedOffset::east_opt(0)
            .expect("offset should be valid")
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp should be valid")
    }

    fn model(id: Uuid, name: &str, score: i32) -> test_entity::Model {
        let now = ts();
        test_entity::Model {
            id,
            created_at: now,
            updated_at: now,
            name: name.to_string(),
            score,
        }
    }

    fn active(name: &str, score: i32) -> test_entity::ActiveModel {
        test_entity::ActiveModel {
            name: Set(name.to_string()),
            score: Set(score),
            ..Default::default()
        }
    }

    fn sql_log(db: &DatabaseConnection) -> Vec<String> {
        db.clone()
            .into_transaction_log()
            .into_iter()
            .flat_map(|txn| {
                txn.statements()
                    .iter()
                    .map(|stmt| format!("{stmt}").to_lowercase())
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn first_sql(db: &DatabaseConnection) -> String {
        sql_log(db)
            .into_iter()
            .next()
            .expect("expected at least one statement")
    }

    fn second_sql(db: &DatabaseConnection) -> String {
        sql_log(db)
            .into_iter()
            .nth(1)
            .expect("expected at least two statements")
    }

    #[tokio::test]
    async fn create_returns_inserted_model_on_success() {
        let expected_id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(expected_id, "created", 10)]])
            .build();

        let created = fixture
            .dao
            .create(active("created", 10))
            .await
            .expect("create should succeed");

        assert_eq!(created.id, expected_id);
    }

    #[tokio::test]
    async fn create_maps_insert_error_to_db_error() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_error(DbErr::Custom("insert failed".to_string()))
            .build();

        let err = fixture
            .dao
            .create(active("created", 10))
            .await
            .expect_err("create should fail");

        assert!(matches!(err, DaoLayerError::Db(_)));
    }

    #[tokio::test]
    async fn create_sets_id_and_timestamps_in_insert_statement() {
        let expected_id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(expected_id, "created", 10)]])
            .build();

        fixture
            .dao
            .create(active("created", 10))
            .await
            .expect("create should succeed");

        let sql = first_sql(&fixture.db);

        assert!(
            sql.contains("\"id\"")
                && sql.contains("\"created_at\"")
                && sql.contains("\"updated_at\"")
        );
    }

    #[tokio::test]
    async fn find_by_id_returns_model_when_present() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(id, "first", 1)]])
            .build();

        let found = fixture
            .dao
            .find_by_id(id)
            .await
            .expect("find_by_id should succeed");

        assert_eq!(found.id, id);
    }

    #[tokio::test]
    async fn find_by_id_returns_not_found_when_record_missing() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        let err = fixture
            .dao
            .find_by_id(id)
            .await
            .expect_err("find_by_id should fail");

        assert!(matches!(err, DaoLayerError::NotFound { id: missing, .. } if missing == id));
    }

    #[tokio::test]
    async fn find_by_id_maps_query_error_to_db_error() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_error(DbErr::Custom("select failed".to_string()))
            .build();

        let err = fixture
            .dao
            .find_by_id(id)
            .await
            .expect_err("find_by_id should fail");

        assert!(matches!(err, DaoLayerError::Db(_)));
    }

    #[tokio::test]
    async fn find_returns_requested_page_value() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(Uuid::new_v4(), "first", 1)]])
            .build();

        let page = fixture
            .dao
            .find(2, 1, None, |query| query)
            .await
            .expect("find should succeed")
            .page;

        assert_eq!(page, 2);
    }

    #[tokio::test]
    async fn find_rejects_page_zero() {
        let fixture = DaoFixtureBuilder::new().build();

        let err = fixture
            .dao
            .find(0, 1, None, |query| query)
            .await
            .expect_err("find should fail");

        assert!(matches!(
            err,
            DaoLayerError::InvalidPagination {
                page: 0,
                page_size: 1
            }
        ));
    }

    #[tokio::test]
    async fn find_rejects_page_size_zero() {
        let fixture = DaoFixtureBuilder::new().build();

        let err = fixture
            .dao
            .find(1, 0, None, |query| query)
            .await
            .expect_err("find should fail");

        assert!(matches!(
            err,
            DaoLayerError::InvalidPagination {
                page: 1,
                page_size: 0
            }
        ));
    }

    #[tokio::test]
    async fn find_rejects_page_size_above_max() {
        let fixture = DaoFixtureBuilder::new().build();

        let err = fixture
            .dao
            .find(1, TestDao::MAX_PAGE_SIZE + 1, None, |query| query)
            .await
            .expect_err("find should fail");

        assert!(matches!(
            err,
            DaoLayerError::InvalidPagination {
                page: 1,
                page_size: v
            } if v == TestDao::MAX_PAGE_SIZE + 1
        ));
    }

    #[tokio::test]
    async fn find_sets_has_next_true_when_fetch_exceeds_page_size() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![
                model(Uuid::new_v4(), "first", 1),
                model(Uuid::new_v4(), "second", 2),
            ]])
            .build();

        let has_next = fixture
            .dao
            .find(1, 1, None, |query| query)
            .await
            .expect("find should succeed")
            .has_next;

        assert!(has_next);
    }

    #[tokio::test]
    async fn find_truncates_data_to_page_size_when_has_next() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![
                model(Uuid::new_v4(), "first", 1),
                model(Uuid::new_v4(), "second", 2),
            ]])
            .build();

        let len = fixture
            .dao
            .find(1, 1, None, |query| query)
            .await
            .expect("find should succeed")
            .data
            .len();

        assert_eq!(len, 1);
    }

    #[tokio::test]
    async fn find_sets_has_next_false_when_fetch_fits_page_size() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(Uuid::new_v4(), "first", 1)]])
            .build();

        let has_next = fixture
            .dao
            .find(1, 1, None, |query| query)
            .await
            .expect("find should succeed")
            .has_next;

        assert!(!has_next);
    }

    #[tokio::test]
    async fn find_uses_default_created_at_desc_order_when_order_is_none() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        fixture
            .dao
            .find(1, 1, None, |query| query)
            .await
            .expect("find should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("order by") && sql.contains("created_at") && sql.contains("desc"));
    }

    #[tokio::test]
    async fn find_uses_explicit_order_when_provided() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        fixture
            .dao
            .find(
                1,
                1,
                Some((test_entity::Column::Name, Order::Asc)),
                |query| query,
            )
            .await
            .expect("find should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("order by") && sql.contains("name") && sql.contains("asc"));
    }

    #[tokio::test]
    async fn find_applies_query_transformer_closure() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        fixture
            .dao
            .find(1, 1, None, |query| {
                query.filter(test_entity::Column::Score.eq(7))
            })
            .await
            .expect("find should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("where") && sql.contains("score") && sql.contains("= 7"));
    }

    #[tokio::test]
    async fn find_maps_query_error_to_db_error() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_error(DbErr::Custom("find failed".to_string()))
            .build();

        let err = fixture
            .dao
            .find(1, 1, None, |query| query)
            .await
            .expect_err("find should fail");

        assert!(matches!(err, DaoLayerError::Db(_)));
    }

    #[tokio::test]
    async fn find_with_filters_returns_requested_page_size_value() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        let page_size = fixture
            .dao
            .find_with_filters(1, 3, None, &[], |query| query)
            .await
            .expect("find_with_filters should succeed")
            .page_size;

        assert_eq!(page_size, 3);
    }

    #[tokio::test]
    async fn find_with_filters_rejects_page_zero() {
        let fixture = DaoFixtureBuilder::new().build();

        let err = fixture
            .dao
            .find_with_filters(0, 1, None, &[], |query| query)
            .await
            .expect_err("find_with_filters should fail");

        assert!(matches!(
            err,
            DaoLayerError::InvalidPagination {
                page: 0,
                page_size: 1
            }
        ));
    }

    #[tokio::test]
    async fn find_with_filters_rejects_page_size_above_max() {
        let fixture = DaoFixtureBuilder::new().build();

        let err = fixture
            .dao
            .find_with_filters(1, TestDao::MAX_PAGE_SIZE + 1, None, &[], |query| query)
            .await
            .expect_err("find_with_filters should fail");

        assert!(matches!(
            err,
            DaoLayerError::InvalidPagination {
                page: 1,
                page_size: v
            } if v == TestDao::MAX_PAGE_SIZE + 1
        ));
    }

    #[tokio::test]
    async fn find_with_filters_applies_eq_filter() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();
        let filters = vec![ColumnFilter {
            column: test_entity::Column::Name,
            op: FilterOp::Eq(Value::from("alice".to_string())),
        }];

        fixture
            .dao
            .find_with_filters(1, 1, None, &filters, |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("where") && sql.contains("name") && sql.contains("= 'alice'"));
    }

    #[tokio::test]
    async fn find_with_filters_applies_lt_filter() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();
        let filters = vec![ColumnFilter {
            column: test_entity::Column::Score,
            op: FilterOp::Compare {
                op: CompareOp::Lt,
                value: Value::from(5),
            },
        }];

        fixture
            .dao
            .find_with_filters(1, 1, None, &filters, |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("where") && sql.contains("score") && sql.contains("< 5"));
    }

    #[tokio::test]
    async fn find_with_filters_applies_lte_filter() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();
        let filters = vec![ColumnFilter {
            column: test_entity::Column::Score,
            op: FilterOp::Compare {
                op: CompareOp::Lte,
                value: Value::from(5),
            },
        }];

        fixture
            .dao
            .find_with_filters(1, 1, None, &filters, |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("where") && sql.contains("score") && sql.contains("<= 5"));
    }

    #[tokio::test]
    async fn find_with_filters_applies_gt_filter() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();
        let filters = vec![ColumnFilter {
            column: test_entity::Column::Score,
            op: FilterOp::Compare {
                op: CompareOp::Gt,
                value: Value::from(5),
            },
        }];

        fixture
            .dao
            .find_with_filters(1, 1, None, &filters, |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("where") && sql.contains("score") && sql.contains("> 5"));
    }

    #[tokio::test]
    async fn find_with_filters_applies_gte_filter() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();
        let filters = vec![ColumnFilter {
            column: test_entity::Column::Score,
            op: FilterOp::Compare {
                op: CompareOp::Gte,
                value: Value::from(5),
            },
        }];

        fixture
            .dao
            .find_with_filters(1, 1, None, &filters, |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("where") && sql.contains("score") && sql.contains(">= 5"));
    }

    #[tokio::test]
    async fn find_with_filters_applies_like_filter_with_escape() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();
        let filters = vec![ColumnFilter {
            column: test_entity::Column::Name,
            op: FilterOp::Like {
                pattern: "%a!_%".to_string(),
                escape: '!',
            },
        }];

        fixture
            .dao
            .find_with_filters(1, 1, None, &filters, |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains(" like ") && sql.contains("escape '!'") && sql.contains("%a!_%"));
    }

    #[tokio::test]
    async fn find_with_filters_applies_between_filter() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();
        let filters = vec![ColumnFilter {
            column: test_entity::Column::Score,
            op: FilterOp::Between {
                min: Value::from(1),
                max: Value::from(9),
            },
        }];

        fixture
            .dao
            .find_with_filters(1, 1, None, &filters, |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains(" between ") && sql.contains("1") && sql.contains("9"));
    }

    #[tokio::test]
    async fn find_with_filters_applies_all_filters() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();
        let filters = vec![
            ColumnFilter {
                column: test_entity::Column::Name,
                op: FilterOp::Eq(Value::from("alice".to_string())),
            },
            ColumnFilter {
                column: test_entity::Column::Score,
                op: FilterOp::Compare {
                    op: CompareOp::Gte,
                    value: Value::from(5),
                },
            },
        ];

        fixture
            .dao
            .find_with_filters(1, 1, None, &filters, |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.matches(" and ").count() >= 1);
    }

    #[tokio::test]
    async fn find_with_filters_uses_default_created_at_desc_order_when_order_is_none() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        fixture
            .dao
            .find_with_filters(1, 1, None, &[], |query| query)
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("order by") && sql.contains("created_at") && sql.contains("desc"));
    }

    #[tokio::test]
    async fn find_with_filters_uses_explicit_order_when_provided() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        fixture
            .dao
            .find_with_filters(
                1,
                1,
                Some((test_entity::Column::Score, Order::Asc)),
                &[],
                |query| query,
            )
            .await
            .expect("find_with_filters should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("order by") && sql.contains("score") && sql.contains("asc"));
    }

    #[tokio::test]
    async fn find_with_filters_maps_query_error_to_db_error() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_error(DbErr::Custom("find_with_filters failed".to_string()))
            .build();

        let err = fixture
            .dao
            .find_with_filters(1, 1, None, &[], |query| query)
            .await
            .expect_err("find_with_filters should fail");

        assert!(matches!(err, DaoLayerError::Db(_)));
    }

    #[tokio::test]
    async fn find_iter_defaults_page_size_to_max() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        let mut pager = fixture.dao.find_iter(None, None, |query| query);
        pager.next_page().await.expect("next_page should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains(&format!("limit {}", TestDao::MAX_PAGE_SIZE + 1)));
    }

    #[tokio::test]
    async fn find_iter_uses_explicit_page_size_when_provided() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        let mut pager = fixture.dao.find_iter(Some(5), None, |query| query);
        pager.next_page().await.expect("next_page should succeed");

        let sql = first_sql(&fixture.db);

        assert!(sql.contains("limit 6"));
    }

    #[tokio::test]
    async fn next_page_returns_some_before_done() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(Uuid::new_v4(), "only", 1)]])
            .build();

        let mut pager = fixture.dao.find_iter(Some(1), None, |query| query);
        let page = pager.next_page().await.expect("next_page should succeed");

        assert!(page.is_some());
    }

    #[tokio::test]
    async fn next_page_returns_none_after_last_page() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(Uuid::new_v4(), "only", 1)]])
            .build();

        let mut pager = fixture.dao.find_iter(Some(1), None, |query| query);
        let _ = pager.next_page().await.expect("first call should succeed");
        let second = pager.next_page().await.expect("second call should succeed");

        assert!(second.is_none());
    }

    #[tokio::test]
    async fn next_page_increments_offset_between_calls() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![
                vec![
                    model(Uuid::new_v4(), "first", 1),
                    model(Uuid::new_v4(), "second", 2),
                ],
                vec![model(Uuid::new_v4(), "third", 3)],
            ])
            .build();

        let mut pager = fixture.dao.find_iter(Some(1), None, |query| query);
        let _ = pager.next_page().await.expect("first call should succeed");
        let _ = pager.next_page().await.expect("second call should succeed");

        let sql = second_sql(&fixture.db);

        assert!(sql.contains("offset 1"));
    }

    #[tokio::test]
    async fn next_page_does_not_query_again_once_done() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(Uuid::new_v4(), "only", 1)]])
            .build();

        let mut pager = fixture.dao.find_iter(Some(1), None, |query| query);
        let _ = pager.next_page().await.expect("first call should succeed");
        let _ = pager.next_page().await.expect("second call should succeed");

        let query_count = sql_log(&fixture.db).len();

        assert_eq!(query_count, 1);
    }

    #[tokio::test]
    async fn next_page_propagates_find_error() {
        let fixture = DaoFixtureBuilder::new()
            .with_query_error(DbErr::Custom("page failed".to_string()))
            .build();

        let mut pager = fixture.dao.find_iter(Some(1), None, |query| query);
        let err = pager.next_page().await.expect_err("next_page should fail");

        assert!(matches!(err, DaoLayerError::Db(_)));
    }

    #[tokio::test]
    async fn update_returns_updated_model_when_found() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![
                vec![model(id, "before", 1)],
                vec![model(id, "after", 1)],
            ])
            .build();

        let updated = fixture
            .dao
            .update(id, |active| {
                active.name = Set("after".to_string());
            })
            .await
            .expect("update should succeed");

        assert_eq!(updated.name, "after");
    }

    #[tokio::test]
    async fn update_returns_not_found_when_record_missing() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![Vec::<test_entity::Model>::new()])
            .build();

        let err = fixture
            .dao
            .update(id, |_active| {})
            .await
            .expect_err("update should fail");

        assert!(matches!(err, DaoLayerError::NotFound { id: missing, .. } if missing == id));
    }

    #[tokio::test]
    async fn update_maps_lookup_query_error_to_db_error() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_error(DbErr::Custom("lookup failed".to_string()))
            .build();

        let err = fixture
            .dao
            .update(id, |_active| {})
            .await
            .expect_err("update should fail");

        assert!(matches!(err, DaoLayerError::Db(_)));
    }

    #[tokio::test]
    async fn update_maps_update_query_error_to_db_error() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![vec![model(id, "before", 1)]])
            .with_query_error(DbErr::Custom("update failed".to_string()))
            .build();

        let err = fixture
            .dao
            .update(id, |active| {
                active.name = Set("after".to_string());
            })
            .await
            .expect_err("update should fail");

        assert!(matches!(err, DaoLayerError::Db(_)));
    }

    #[tokio::test]
    async fn update_applies_mutation_closure_to_update_statement() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![
                vec![model(id, "before", 1)],
                vec![model(id, "before", 77)],
            ])
            .build();

        fixture
            .dao
            .update(id, |active| {
                active.score = Set(77);
            })
            .await
            .expect("update should succeed");

        let sql = second_sql(&fixture.db);

        assert!(sql.contains("set") && sql.contains("\"score\" ="));
    }

    #[tokio::test]
    async fn update_sets_updated_at_in_update_statement() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_query_results(vec![
                vec![model(id, "before", 1)],
                vec![model(id, "before", 1)],
            ])
            .build();

        fixture
            .dao
            .update(id, |_active| {})
            .await
            .expect("update should succeed");

        let sql = second_sql(&fixture.db);

        assert!(sql.contains("\"updated_at\""));
    }

    #[tokio::test]
    async fn delete_returns_id_when_rows_affected_is_one() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new().with_exec_result(1).build();

        let deleted = fixture.dao.delete(id).await.expect("delete should succeed");

        assert_eq!(deleted, id);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_no_rows_affected() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new().with_exec_result(0).build();

        let err = fixture
            .dao
            .delete(id)
            .await
            .expect_err("delete should fail");

        assert!(matches!(err, DaoLayerError::NotFound { id: missing, .. } if missing == id));
    }

    #[tokio::test]
    async fn delete_maps_exec_error_to_db_error() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new()
            .with_exec_error(DbErr::Custom("delete failed".to_string()))
            .build();

        let err = fixture
            .dao
            .delete(id)
            .await
            .expect_err("delete should fail");

        assert!(matches!(err, DaoLayerError::Db(_)));
    }

    #[tokio::test]
    async fn delete_returns_id_when_rows_affected_is_more_than_one() {
        let id = Uuid::new_v4();
        let fixture = DaoFixtureBuilder::new().with_exec_result(2).build();

        let deleted = fixture.dao.delete(id).await.expect("delete should succeed");

        assert_eq!(deleted, id);
    }
}
