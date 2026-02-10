use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use sea_orm::sea_query::{ColumnType, Value as QueryValue};
use sea_orm::{ColumnTrait, EntityTrait, IdenStatic, IntoActiveModel, Iterable, Order, Select};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::db::dao::{
    ColumnFilter, CompareOp, DaoBase, DaoLayerError, FilterOp, PaginatedResponse,
};
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

const INVALID_FILTER_MESSAGE: &str = "Invalid filter";
const INVALID_FILTER_VALUE_MESSAGE: &str = "Invalid filter value";

pub struct FilterSpec<C> {
    pub key: &'static str,
    pub column: C,
    pub parse: fn(&str) -> Result<FilterOp, AppError>,
}

#[derive(Clone, Copy)]
pub enum FilterParseStrategy {
    ByColumnType,
    StringsOnly,
    BestEffortString,
}

pub enum FilterMode<C: 'static> {
    Allowlist(&'static [FilterSpec<C>]),
    AllColumns {
        deny: &'static [&'static str],
        parse: FilterParseStrategy,
    },
}

#[async_trait::async_trait]
pub trait CrudService {
    type Dao: DaoBase;

    fn dao(&self) -> &Self::Dao;
    fn list_filter_mode(&self) -> FilterMode<CrudColumn<Self::Dao>> {
        FilterMode::AllColumns {
            deny: &[],
            parse: FilterParseStrategy::ByColumnType,
        }
    }

    fn errors(&self) -> CrudErrors {
        CrudErrors::default()
    }

    fn map_error(&self, op: CrudOp, err: DaoLayerError) -> AppError {
        let errors = self.errors();
        match err {
            DaoLayerError::Db(db_err) => {
                let context = match op {
                    CrudOp::Create => errors.create_failed,
                    CrudOp::Find | CrudOp::List => errors.find_failed,
                    CrudOp::Update => errors.update_failed,
                    CrudOp::Delete => errors.delete_failed,
                };
                let message = format!("{context}. Please check the logs for more details");
                AppError::internal_with_source(message, db_err)
            }
            DaoLayerError::NotFound { .. } => AppError::not_found(errors.not_found),
            DaoLayerError::InvalidPagination { .. } => AppError::bad_request(err.to_string()),
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

    async fn find_with_filters<F>(
        &self,
        page: u64,
        page_size: u64,
        order: Option<(CrudColumn<Self::Dao>, Order)>,
        filters: HashMap<String, String>,
        apply: F,
    ) -> Result<PaginatedResponse<CrudModel<Self::Dao>>, AppError>
    where
        F: FnOnce(Select<CrudEntity<Self::Dao>>) -> Select<CrudEntity<Self::Dao>> + Send,
        CrudColumn<Self::Dao>: ColumnTrait + Copy,
    {
        let column_filters = self.build_column_filters(filters)?;
        self.dao()
            .find_with_filters(page, page_size, order, &column_filters, apply)
            .await
            .map_err(|err| self.map_error(CrudOp::List, err))
    }

    async fn update<F>(&self, id: Uuid, apply: F) -> Result<CrudModel<Self::Dao>, AppError>
    where
        F: for<'a> FnOnce(&'a mut CrudActiveModel<Self::Dao>) + Send,
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

    fn build_column_filters(
        &self,
        filters: HashMap<String, String>,
    ) -> Result<Vec<ColumnFilter<CrudColumn<Self::Dao>>>, AppError>
    where
        CrudColumn<Self::Dao>: ColumnTrait + Copy,
    {
        if filters.is_empty() {
            return Ok(Vec::new());
        }

        match self.list_filter_mode() {
            FilterMode::Allowlist(specs) => {
                let spec_map: HashMap<&'static str, &FilterSpec<CrudColumn<Self::Dao>>> =
                    specs.iter().map(|spec| (spec.key, spec)).collect();
                let mut parsed = Vec::with_capacity(filters.len());
                for (key, value) in filters {
                    let spec = spec_map.get(key.as_str()).ok_or_else(invalid_filter)?;
                    let parsed_op = (spec.parse)(&value)?;
                    parsed.push(ColumnFilter {
                        column: spec.column,
                        op: parsed_op,
                    });
                }
                Ok(parsed)
            }
            FilterMode::AllColumns { deny, parse } => {
                let deny_set: HashSet<&'static str> = deny.iter().copied().collect();
                let column_map: HashMap<&'static str, CrudColumn<Self::Dao>> =
                    CrudColumn::<Self::Dao>::iter()
                        .map(|column| (column.as_str(), column))
                        .collect();

                let mut parsed = Vec::with_capacity(filters.len());
                for (key, value) in filters {
                    if deny_set.contains(key.as_str()) {
                        return Err(invalid_filter());
                    }
                    let column = column_map.get(key.as_str()).ok_or_else(invalid_filter)?;
                    let column_def = column.def();
                    let column_type = column_def.get_column_type();
                    let parsed_op = match parse {
                        FilterParseStrategy::BestEffortString => {
                            if is_string_column_type(column_type) {
                                parse_string_filter(&value)?
                            } else {
                                parse_non_string_filter(&value, column_type)?
                            }
                        }
                        FilterParseStrategy::StringsOnly => {
                            if !is_string_column_type(column_type) {
                                return Err(invalid_filter());
                            }
                            parse_string_filter(&value)?
                        }
                        FilterParseStrategy::ByColumnType => {
                            if is_string_column_type(column_type) {
                                parse_string_filter(&value)?
                            } else {
                                parse_non_string_filter(&value, column_type)?
                            }
                        }
                    };
                    parsed.push(ColumnFilter {
                        column: *column,
                        op: parsed_op,
                    });
                }
                Ok(parsed)
            }
        }
    }
}

fn invalid_filter() -> AppError {
    AppError::bad_request(INVALID_FILTER_MESSAGE)
}

fn invalid_filter_value() -> AppError {
    AppError::bad_request(INVALID_FILTER_VALUE_MESSAGE)
}

fn invalid_filter_value_with(detail: impl std::fmt::Display) -> AppError {
    AppError::bad_request(format!("{INVALID_FILTER_VALUE_MESSAGE}: {detail}"))
}

fn parse_bool(raw: &str) -> Result<bool, AppError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "t" | "1" | "yes" | "y" => Ok(true),
        "false" | "f" | "0" | "no" | "n" => Ok(false),
        _ => Err(invalid_filter_value()),
    }
}

fn parse_int<T>(raw: &str) -> Result<T, AppError>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    raw.trim().parse::<T>().map_err(invalid_filter_value_with)
}

fn parse_float(raw: &str) -> Result<f64, AppError> {
    raw.trim().parse::<f64>().map_err(invalid_filter_value_with)
}

fn parse_date(raw: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d").map_err(invalid_filter_value_with)
}

fn parse_time(raw: &str) -> Result<NaiveTime, AppError> {
    let raw = raw.trim();
    for format in ["%H:%M:%S%.f", "%H:%M:%S"] {
        if let Ok(time) = NaiveTime::parse_from_str(raw, format) {
            return Ok(time);
        }
    }
    Err(invalid_filter_value_with(format!(
        "Unrecognized time format: {raw}"
    )))
}

fn parse_naive_datetime(raw: &str) -> Result<NaiveDateTime, AppError> {
    let raw = raw.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
        return Ok(dt.naive_utc());
    }
    for format in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%d %H:%M:%S",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(raw, format) {
            return Ok(dt);
        }
    }
    Err(invalid_filter_value_with(format!(
        "Unrecognized datetime format: {raw}"
    )))
}

fn parse_datetime_with_tz(raw: &str) -> Result<DateTime<FixedOffset>, AppError> {
    DateTime::parse_from_rfc3339(raw.trim()).map_err(invalid_filter_value_with)
}

fn parse_json(raw: &str) -> Result<JsonValue, AppError> {
    serde_json::from_str(raw.trim()).map_err(invalid_filter_value_with)
}

fn ensure_no_wildcard(raw: &str) -> Result<(), AppError> {
    if raw.contains('*') {
        return Err(invalid_filter_value());
    }
    Ok(())
}

fn escape_like(raw: &str) -> String {
    let mut escaped = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '%' => escaped.push_str("\\%"),
            '_' => escaped.push_str("\\_"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn parse_string_filter(raw: &str) -> Result<FilterOp, AppError> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "*" {
        return Err(invalid_filter_value());
    }

    let leading = raw.starts_with('*');
    let trailing = raw.ends_with('*');
    let inner = raw.trim_matches('*');
    if inner.is_empty() {
        return Err(invalid_filter_value());
    }
    if inner.contains('*') {
        return Err(invalid_filter_value());
    }

    if !leading && !trailing {
        return Ok(FilterOp::Eq(QueryValue::String(Some(inner.to_string()))));
    }

    let escaped = escape_like(inner);
    let pattern = match (leading, trailing) {
        (true, true) => format!("%{escaped}%"),
        (true, false) => format!("%{escaped}"),
        (false, true) => format!("{escaped}%"),
        (false, false) => escaped,
    };
    Ok(FilterOp::Like {
        pattern,
        escape: '\\',
    })
}

fn parse_comparison(raw: &str) -> Option<(CompareOp, &str)> {
    let raw = raw.trim_start();
    if let Some(rest) = raw.strip_prefix(">=") {
        return Some((CompareOp::Gte, rest.trim_start()));
    }
    if let Some(rest) = raw.strip_prefix("<=") {
        return Some((CompareOp::Lte, rest.trim_start()));
    }
    if let Some(rest) = raw.strip_prefix('>') {
        return Some((CompareOp::Gt, rest.trim_start()));
    }
    if let Some(rest) = raw.strip_prefix('<') {
        return Some((CompareOp::Lt, rest.trim_start()));
    }
    None
}

fn parse_range(raw: &str) -> Result<Option<(&str, &str)>, AppError> {
    let raw = raw.trim();
    let Some(idx) = raw.find("..") else {
        return Ok(None);
    };
    let (start, rest) = raw.split_at(idx);
    let end = &rest[2..];
    if end.contains("..") {
        return Err(invalid_filter_value());
    }
    let start = start.trim();
    let end = end.trim();
    if start.is_empty() || end.is_empty() {
        return Err(invalid_filter_value());
    }
    Ok(Some((start, end)))
}

fn is_orderable_column_type(column_type: &ColumnType) -> bool {
    matches!(
        column_type,
        ColumnType::TinyInteger
            | ColumnType::SmallInteger
            | ColumnType::Integer
            | ColumnType::BigInteger
            | ColumnType::TinyUnsigned
            | ColumnType::SmallUnsigned
            | ColumnType::Unsigned
            | ColumnType::BigUnsigned
            | ColumnType::Float
            | ColumnType::Double
            | ColumnType::Decimal(_)
            | ColumnType::Money(_)
            | ColumnType::DateTime
            | ColumnType::Timestamp
            | ColumnType::TimestampWithTimeZone
            | ColumnType::Time
            | ColumnType::Date
            | ColumnType::Year
    )
}

fn parse_non_string_filter(raw: &str, column_type: &ColumnType) -> Result<FilterOp, AppError> {
    ensure_no_wildcard(raw)?;
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(invalid_filter_value());
    }

    if let Some((op, rest)) = parse_comparison(raw) {
        if rest.is_empty() || !is_orderable_column_type(column_type) {
            return Err(invalid_filter());
        }
        let value = parse_value_by_column_type(rest, column_type)?;
        return Ok(FilterOp::Compare { op, value });
    }

    if let Some((start, end)) = parse_range(raw)? {
        if !is_orderable_column_type(column_type) {
            return Err(invalid_filter());
        }
        let min = parse_value_by_column_type(start, column_type)?;
        let max = parse_value_by_column_type(end, column_type)?;
        return Ok(FilterOp::Between { min, max });
    }

    Ok(FilterOp::Eq(parse_value_by_column_type(raw, column_type)?))
}

fn is_string_column_type(column_type: &ColumnType) -> bool {
    matches!(
        column_type,
        ColumnType::Char(_) | ColumnType::String(_) | ColumnType::Text | ColumnType::Enum { .. }
    )
}

fn parse_value_by_column_type(raw: &str, column_type: &ColumnType) -> Result<QueryValue, AppError> {
    if raw.trim().eq_ignore_ascii_case("null") {
        return Err(invalid_filter_value());
    }

    match column_type {
        ColumnType::Char(_) => {
            let mut chars = raw.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Ok(QueryValue::Char(Some(ch))),
                _ => Err(invalid_filter_value()),
            }
        }
        ColumnType::String(_) | ColumnType::Text => Ok(QueryValue::String(Some(raw.to_string()))),
        ColumnType::TinyInteger => Ok(QueryValue::TinyInt(Some(parse_int::<i8>(raw)?))),
        ColumnType::SmallInteger => Ok(QueryValue::SmallInt(Some(parse_int::<i16>(raw)?))),
        ColumnType::Integer => Ok(QueryValue::Int(Some(parse_int::<i32>(raw)?))),
        ColumnType::BigInteger => Ok(QueryValue::BigInt(Some(parse_int::<i64>(raw)?))),
        ColumnType::TinyUnsigned => Ok(QueryValue::TinyUnsigned(Some(parse_int::<u8>(raw)?))),
        ColumnType::SmallUnsigned => Ok(QueryValue::SmallUnsigned(Some(parse_int::<u16>(raw)?))),
        ColumnType::Unsigned => Ok(QueryValue::Unsigned(Some(parse_int::<u32>(raw)?))),
        ColumnType::BigUnsigned => Ok(QueryValue::BigUnsigned(Some(parse_int::<u64>(raw)?))),
        ColumnType::Float => Ok(QueryValue::Float(Some(parse_float(raw)? as f32))),
        ColumnType::Double => Ok(QueryValue::Double(Some(parse_float(raw)?))),
        ColumnType::Decimal(_) | ColumnType::Money(_) => {
            Ok(QueryValue::Double(Some(parse_float(raw)?)))
        }
        ColumnType::DateTime | ColumnType::Timestamp => {
            Ok(QueryValue::ChronoDateTime(Some(parse_naive_datetime(raw)?)))
        }
        ColumnType::TimestampWithTimeZone => Ok(QueryValue::ChronoDateTimeWithTimeZone(Some(
            parse_datetime_with_tz(raw)?,
        ))),
        ColumnType::Time => Ok(QueryValue::ChronoTime(Some(parse_time(raw)?))),
        ColumnType::Date => Ok(QueryValue::ChronoDate(Some(parse_date(raw)?))),
        ColumnType::Boolean => Ok(QueryValue::Bool(Some(parse_bool(raw)?))),
        ColumnType::Json | ColumnType::JsonBinary => {
            Ok(QueryValue::Json(Some(Box::new(parse_json(raw)?))))
        }
        ColumnType::Uuid => {
            let uuid = Uuid::parse_str(raw.trim()).map_err(invalid_filter_value_with)?;
            Ok(QueryValue::Uuid(Some(uuid)))
        }
        ColumnType::Enum { variants, .. } => {
            let raw = raw.trim();
            if variants.iter().any(|variant| variant.to_string() == raw) {
                Ok(QueryValue::String(Some(raw.to_string())))
            } else {
                Err(invalid_filter_value())
            }
        }
        ColumnType::Year => Ok(QueryValue::Int(Some(parse_int::<i32>(raw)?))),
        ColumnType::Binary(_)
        | ColumnType::VarBinary(_)
        | ColumnType::Blob
        | ColumnType::Bit(_)
        | ColumnType::VarBit(_)
        | ColumnType::Interval(_, _)
        | ColumnType::Array(_)
        | ColumnType::Vector(_)
        | ColumnType::Cidr
        | ColumnType::Inet
        | ColumnType::MacAddr
        | ColumnType::LTree
        | ColumnType::Custom(_) => Err(invalid_filter()),
        _ => Err(invalid_filter()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::{FixedOffset, TimeZone};
    use sea_orm::entity::prelude::*;
    use sea_orm::{
        DatabaseBackend, DatabaseConnection, DbErr, IntoMockRow, MockDatabase, MockExecResult, Set,
    };
    use uuid::Uuid;

    use crate::db::dao::{
        DaoBase, DaoLayerError, HasCreatedAtColumn, HasIdActiveModel, TimestampedActiveModel,
    };
    use crate::error::AppError;

    use super::{
        CompareOp, CrudErrors, CrudOp, CrudService, FilterMode, FilterOp, FilterParseStrategy,
        FilterSpec, QueryValue,
    };

    mod test_entity {
        use sea_orm::entity::prelude::*;

        #[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
        #[sea_orm(table_name = "test_crud_records")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub id: uuid::Uuid,
            pub created_at: DateTimeWithTimeZone,
            pub updated_at: DateTimeWithTimeZone,
            pub title: String,
            pub score: i32,
            pub done: bool,
            pub external_id: uuid::Uuid,
            pub scheduled_at: DateTimeWithTimeZone,
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

    #[derive(Clone, Copy)]
    enum FilterModeKind {
        AllColumns,
        Allowlist,
    }

    #[derive(Clone)]
    struct TestCrudService {
        dao: TestDao,
        parse: FilterParseStrategy,
        deny: &'static [&'static str],
        errors: CrudErrors,
        filter_mode: FilterModeKind,
    }

    fn parse_allowlist_title(raw: &str) -> Result<FilterOp, AppError> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("bad") {
            return Err(AppError::bad_request("Invalid filter value"));
        }
        if raw.is_empty() {
            return Err(AppError::bad_request("Invalid filter value"));
        }
        Ok(FilterOp::Eq(QueryValue::String(Some(raw.to_string()))))
    }

    static TITLE_ALLOWLIST: &[FilterSpec<test_entity::Column>] = &[FilterSpec {
        key: "title",
        column: test_entity::Column::Title,
        parse: parse_allowlist_title,
    }];

    #[async_trait::async_trait]
    impl CrudService for TestCrudService {
        type Dao = TestDao;

        fn dao(&self) -> &Self::Dao {
            &self.dao
        }

        fn list_filter_mode(&self) -> FilterMode<test_entity::Column> {
            match self.filter_mode {
                FilterModeKind::AllColumns => FilterMode::AllColumns {
                    deny: self.deny,
                    parse: self.parse,
                },
                FilterModeKind::Allowlist => FilterMode::Allowlist(TITLE_ALLOWLIST),
            }
        }

        fn errors(&self) -> CrudErrors {
            self.errors
        }
    }

    struct CrudFixtureBuilder {
        mock: MockDatabase,
        parse: FilterParseStrategy,
        deny: &'static [&'static str],
        errors: CrudErrors,
        filter_mode: FilterModeKind,
    }

    impl CrudFixtureBuilder {
        fn new() -> Self {
            Self {
                mock: MockDatabase::new(DatabaseBackend::Postgres),
                parse: FilterParseStrategy::ByColumnType,
                deny: &[],
                errors: CrudErrors::default(),
                filter_mode: FilterModeKind::AllColumns,
            }
        }

        fn with_parse(mut self, parse: FilterParseStrategy) -> Self {
            self.parse = parse;
            self
        }

        fn with_deny(mut self, deny: &'static [&'static str]) -> Self {
            self.deny = deny;
            self
        }

        fn with_errors(mut self, errors: CrudErrors) -> Self {
            self.errors = errors;
            self
        }

        fn with_allowlist_mode(mut self) -> Self {
            self.filter_mode = FilterModeKind::Allowlist;
            self
        }

        fn with_query_results<T, I, II>(mut self, sets: II) -> Self
        where
            T: IntoMockRow,
            I: IntoIterator<Item = T>,
            II: IntoIterator<Item = I>,
        {
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

        fn build(self) -> TestCrudService {
            let db = self.mock.into_connection();
            let dao = TestDao::new(&db);
            TestCrudService {
                dao,
                parse: self.parse,
                deny: self.deny,
                errors: self.errors,
                filter_mode: self.filter_mode,
            }
        }
    }

    fn ts() -> chrono::DateTime<chrono::FixedOffset> {
        FixedOffset::east_opt(0)
            .expect("offset should be valid")
            .with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
            .single()
            .expect("timestamp should be valid")
    }

    fn model(id: Uuid, title: &str, score: i32, done: bool) -> test_entity::Model {
        let now = ts();
        test_entity::Model {
            id,
            created_at: now,
            updated_at: now,
            title: title.to_string(),
            score,
            done,
            external_id: Uuid::new_v4(),
            scheduled_at: now,
        }
    }

    fn active(title: &str, score: i32, done: bool) -> test_entity::ActiveModel {
        test_entity::ActiveModel {
            title: Set(title.to_string()),
            score: Set(score),
            done: Set(done),
            external_id: Set(Uuid::new_v4()),
            scheduled_at: Set(ts()),
            ..Default::default()
        }
    }

    fn filters(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[tokio::test]
    async fn create_returns_model_on_success() {
        let id = Uuid::new_v4();
        let service = CrudFixtureBuilder::new()
            .with_query_results([vec![model(id, "first", 1, false)]])
            .build();

        let created = service
            .create(active("first", 1, false))
            .await
            .expect("create should succeed");

        assert_eq!(created.id, id);
    }

    #[tokio::test]
    async fn create_maps_db_error_to_internal_with_create_message() {
        let service = CrudFixtureBuilder::new()
            .with_query_error(DbErr::Custom("insert failed".to_string()))
            .build();

        let err = service
            .create(active("first", 1, false))
            .await
            .expect_err("create should fail");

        assert_eq!(
            err.message(),
            "Create failed. Please check the logs for more details"
        );
    }

    #[tokio::test]
    async fn find_by_id_returns_model_on_success() {
        let id = Uuid::new_v4();
        let service = CrudFixtureBuilder::new()
            .with_query_results([vec![model(id, "first", 1, false)]])
            .build();

        let found = service
            .find_by_id(id)
            .await
            .expect("find_by_id should succeed");

        assert_eq!(found.id, id);
    }

    #[tokio::test]
    async fn find_by_id_maps_not_found_to_service_not_found_message() {
        let id = Uuid::new_v4();
        let service = CrudFixtureBuilder::new()
            .with_query_results([Vec::<test_entity::Model>::new()])
            .build();

        let err = service
            .find_by_id(id)
            .await
            .expect_err("find_by_id should fail");

        assert_eq!(err.message(), "Resource not found");
    }

    #[tokio::test]
    async fn find_by_id_maps_db_error_to_internal_with_find_message() {
        let service = CrudFixtureBuilder::new()
            .with_query_error(DbErr::Custom("select failed".to_string()))
            .build();

        let err = service
            .find_by_id(Uuid::new_v4())
            .await
            .expect_err("find_by_id should fail");

        assert_eq!(
            err.message(),
            "Find failed. Please check the logs for more details"
        );
    }

    #[tokio::test]
    async fn find_returns_paginated_response_on_success() {
        let service = CrudFixtureBuilder::new()
            .with_query_results([vec![model(Uuid::new_v4(), "first", 1, false)]])
            .build();

        let response = service
            .find(1, 1, None, |query| query)
            .await
            .expect("find should succeed");

        assert_eq!(response.data.len(), 1);
    }

    #[tokio::test]
    async fn find_maps_invalid_pagination_to_bad_request() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .find(0, 1, None, |query| query)
            .await
            .expect_err("find should fail");

        assert_eq!(err.message(), "Invalid pagination: page=0 page_size=1");
    }

    #[tokio::test]
    async fn find_maps_db_error_to_internal_with_find_message() {
        let service = CrudFixtureBuilder::new()
            .with_query_error(DbErr::Custom("find failed".to_string()))
            .build();

        let err = service
            .find(1, 1, None, |query| query)
            .await
            .expect_err("find should fail");

        assert_eq!(
            err.message(),
            "Find failed. Please check the logs for more details"
        );
    }

    #[tokio::test]
    async fn find_with_filters_returns_paginated_response_on_success() {
        let service = CrudFixtureBuilder::new()
            .with_query_results([vec![model(Uuid::new_v4(), "alpha", 7, true)]])
            .build();

        let response = service
            .find_with_filters(1, 1, None, filters(&[("title", "alpha")]), |query| query)
            .await
            .expect("find_with_filters should succeed");

        assert_eq!(response.data.len(), 1);
    }

    #[tokio::test]
    async fn find_with_filters_rejects_invalid_filter_key_before_dao_call() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .find_with_filters(1, 1, None, filters(&[("unknown", "1")]), |query| query)
            .await
            .expect_err("find_with_filters should fail");

        assert_eq!(err.message(), "Invalid filter");
    }

    #[tokio::test]
    async fn find_with_filters_rejects_invalid_filter_value_before_dao_call() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .find_with_filters(1, 1, None, filters(&[("score", "abc")]), |query| query)
            .await
            .expect_err("find_with_filters should fail");

        assert!(err.message().starts_with("Invalid filter value"));
    }

    #[tokio::test]
    async fn find_with_filters_maps_invalid_pagination_to_bad_request() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .find_with_filters(0, 1, None, filters(&[("title", "alpha")]), |query| query)
            .await
            .expect_err("find_with_filters should fail");

        assert_eq!(err.message(), "Invalid pagination: page=0 page_size=1");
    }

    #[tokio::test]
    async fn find_with_filters_maps_db_error_to_internal_with_find_message() {
        let service = CrudFixtureBuilder::new()
            .with_query_error(DbErr::Custom("find failed".to_string()))
            .build();

        let err = service
            .find_with_filters(1, 1, None, filters(&[("title", "alpha")]), |query| query)
            .await
            .expect_err("find_with_filters should fail");

        assert_eq!(
            err.message(),
            "Find failed. Please check the logs for more details"
        );
    }

    #[tokio::test]
    async fn update_returns_model_on_success() {
        let id = Uuid::new_v4();
        let service = CrudFixtureBuilder::new()
            .with_query_results([
                vec![model(id, "before", 1, false)],
                vec![model(id, "after", 1, false)],
            ])
            .build();

        let updated = service
            .update(id, |active| {
                active.title = Set("after".to_string());
            })
            .await
            .expect("update should succeed");

        assert_eq!(updated.title, "after");
    }

    #[tokio::test]
    async fn update_maps_not_found_to_service_not_found_message() {
        let service = CrudFixtureBuilder::new()
            .with_query_results([Vec::<test_entity::Model>::new()])
            .build();

        let err = service
            .update(Uuid::new_v4(), |_active| {})
            .await
            .expect_err("update should fail");

        assert_eq!(err.message(), "Resource not found");
    }

    #[tokio::test]
    async fn update_maps_db_error_to_internal_with_update_message() {
        let service = CrudFixtureBuilder::new()
            .with_query_error(DbErr::Custom("lookup failed".to_string()))
            .build();

        let err = service
            .update(Uuid::new_v4(), |_active| {})
            .await
            .expect_err("update should fail");

        assert_eq!(
            err.message(),
            "Update failed. Please check the logs for more details"
        );
    }

    #[tokio::test]
    async fn delete_returns_unit_on_success() {
        let service = CrudFixtureBuilder::new().with_exec_result(1).build();

        let result = service.delete(Uuid::new_v4()).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn delete_maps_not_found_to_service_not_found_message() {
        let service = CrudFixtureBuilder::new().with_exec_result(0).build();

        let err = service
            .delete(Uuid::new_v4())
            .await
            .expect_err("delete should fail");

        assert_eq!(err.message(), "Resource not found");
    }

    #[tokio::test]
    async fn delete_maps_db_error_to_internal_with_delete_message() {
        let service = CrudFixtureBuilder::new()
            .with_exec_error(DbErr::Custom("delete failed".to_string()))
            .build();

        let err = service
            .delete(Uuid::new_v4())
            .await
            .expect_err("delete should fail");

        assert_eq!(
            err.message(),
            "Delete failed. Please check the logs for more details"
        );
    }

    #[test]
    fn build_column_filters_returns_empty_for_empty_input() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(HashMap::new())
            .expect("empty filters should parse");

        assert!(parsed.is_empty());
    }

    #[test]
    fn allowlist_accepts_configured_key() {
        let service = CrudFixtureBuilder::new().with_allowlist_mode().build();

        let parsed = service
            .build_column_filters(filters(&[("title", "hello")]))
            .expect("allowlist key should parse");

        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn allowlist_rejects_unknown_key() {
        let service = CrudFixtureBuilder::new().with_allowlist_mode().build();

        let err = service
            .build_column_filters(filters(&[("score", "1")]))
            .expect_err("unknown allowlist key should fail");

        assert_eq!(err.message(), "Invalid filter");
    }

    #[test]
    fn allowlist_propagates_custom_parse_error() {
        let service = CrudFixtureBuilder::new().with_allowlist_mode().build();

        let err = service
            .build_column_filters(filters(&[("title", "bad")]))
            .expect_err("allowlist parse error should fail");

        assert_eq!(err.message(), "Invalid filter value");
    }

    #[test]
    fn all_columns_rejects_denied_column() {
        let service = CrudFixtureBuilder::new().with_deny(&["title"]).build();

        let err = service
            .build_column_filters(filters(&[("title", "hello")]))
            .expect_err("denied column should fail");

        assert_eq!(err.message(), "Invalid filter");
    }

    #[test]
    fn all_columns_rejects_unknown_column() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .build_column_filters(filters(&[("unknown", "1")]))
            .expect_err("unknown column should fail");

        assert_eq!(err.message(), "Invalid filter");
    }

    #[test]
    fn strings_only_rejects_non_string_column() {
        let service = CrudFixtureBuilder::new()
            .with_parse(FilterParseStrategy::StringsOnly)
            .build();

        let err = service
            .build_column_filters(filters(&[("score", "1")]))
            .expect_err("non-string column should fail");

        assert_eq!(err.message(), "Invalid filter");
    }

    #[test]
    fn by_column_type_parses_string_exact_as_eq() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("title", "hello")]))
            .expect("string eq should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Eq(QueryValue::String(Some(v))) if v == "hello"
        ));
    }

    #[test]
    fn by_column_type_parses_string_contains_wildcard_as_like() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("title", "*hello*")]))
            .expect("contains wildcard should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Like { pattern, .. } if pattern == "%hello%"
        ));
    }

    #[test]
    fn by_column_type_parses_string_prefix_wildcard_as_like() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("title", "hello*")]))
            .expect("prefix wildcard should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Like { pattern, .. } if pattern == "hello%"
        ));
    }

    #[test]
    fn by_column_type_parses_string_suffix_wildcard_as_like() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("title", "*hello")]))
            .expect("suffix wildcard should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Like { pattern, .. } if pattern == "%hello"
        ));
    }

    #[test]
    fn by_column_type_rejects_interior_wildcard() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .build_column_filters(filters(&[("title", "a*b")]))
            .expect_err("interior wildcard should fail");

        assert_eq!(err.message(), "Invalid filter value");
    }

    #[test]
    fn by_column_type_escapes_like_metacharacters() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("title", "*a%b_c*")]))
            .expect("escaped wildcard should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Like { pattern, .. } if pattern == "%a\\%b\\_c%"
        ));
    }

    #[test]
    fn by_column_type_parses_numeric_comparison() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("score", ">=7")]))
            .expect("numeric comparison should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Compare {
                op: CompareOp::Gte,
                value: QueryValue::Int(Some(7))
            }
        ));
    }

    #[test]
    fn by_column_type_parses_numeric_range() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("score", "1..9")]))
            .expect("numeric range should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Between {
                min: QueryValue::Int(Some(1)),
                max: QueryValue::Int(Some(9))
            }
        ));
    }

    #[test]
    fn by_column_type_rejects_malformed_range() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .build_column_filters(filters(&[("score", "1..2..3")]))
            .expect_err("malformed range should fail");

        assert_eq!(err.message(), "Invalid filter value");
    }

    #[test]
    fn by_column_type_rejects_comparison_on_non_orderable_type() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .build_column_filters(filters(&[("done", ">true")]))
            .expect_err("comparison on bool should fail");

        assert_eq!(err.message(), "Invalid filter");
    }

    #[test]
    fn by_column_type_parses_boolean_aliases() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("done", "yes")]))
            .expect("boolean alias should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Eq(QueryValue::Bool(Some(true)))
        ));
    }

    #[test]
    fn by_column_type_rejects_invalid_boolean() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .build_column_filters(filters(&[("done", "truthy")]))
            .expect_err("invalid boolean should fail");

        assert_eq!(err.message(), "Invalid filter value");
    }

    #[test]
    fn by_column_type_parses_uuid() {
        let service = CrudFixtureBuilder::new().build();
        let uuid = Uuid::new_v4().to_string();

        let parsed = service
            .build_column_filters(filters(&[("external_id", uuid.as_str())]))
            .expect("uuid should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Eq(QueryValue::Uuid(Some(v)))
                if *v == uuid.parse::<Uuid>().expect("uuid parse should work")
        ));
    }

    #[test]
    fn by_column_type_rejects_invalid_uuid() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .build_column_filters(filters(&[("external_id", "not-a-uuid")]))
            .expect_err("invalid uuid should fail");

        assert!(err.message().starts_with("Invalid filter value:"));
    }

    #[test]
    fn by_column_type_rejects_null_literal() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .build_column_filters(filters(&[("score", "null")]))
            .expect_err("null literal should fail");

        assert_eq!(err.message(), "Invalid filter value");
    }

    #[test]
    fn by_column_type_parses_rfc3339_datetime() {
        let service = CrudFixtureBuilder::new().build();

        let parsed = service
            .build_column_filters(filters(&[("scheduled_at", "2026-01-01T00:00:00+00:00")]))
            .expect("datetime should parse");

        assert!(matches!(
            &parsed[0].op,
            FilterOp::Eq(QueryValue::ChronoDateTimeWithTimeZone(Some(_)))
        ));
    }

    #[test]
    fn by_column_type_rejects_invalid_datetime() {
        let service = CrudFixtureBuilder::new().build();

        let err = service
            .build_column_filters(filters(&[("scheduled_at", "not-a-datetime")]))
            .expect_err("invalid datetime should fail");

        assert!(err.message().starts_with("Invalid filter value:"));
    }

    #[test]
    fn map_error_uses_custom_create_failed_message() {
        let service = CrudFixtureBuilder::new()
            .with_errors(CrudErrors {
                create_failed: "Create boom",
                ..CrudErrors::default()
            })
            .build();

        let err = service.map_error(CrudOp::Create, DaoLayerError::Db(DbErr::Custom("x".into())));

        assert_eq!(
            err.message(),
            "Create boom. Please check the logs for more details"
        );
    }

    #[test]
    fn map_error_uses_custom_find_failed_message_for_list() {
        let service = CrudFixtureBuilder::new()
            .with_errors(CrudErrors {
                find_failed: "Find boom",
                ..CrudErrors::default()
            })
            .build();

        let err = service.map_error(CrudOp::List, DaoLayerError::Db(DbErr::Custom("x".into())));

        assert_eq!(
            err.message(),
            "Find boom. Please check the logs for more details"
        );
    }

    #[test]
    fn map_error_uses_custom_update_failed_message() {
        let service = CrudFixtureBuilder::new()
            .with_errors(CrudErrors {
                update_failed: "Update boom",
                ..CrudErrors::default()
            })
            .build();

        let err = service.map_error(CrudOp::Update, DaoLayerError::Db(DbErr::Custom("x".into())));

        assert_eq!(
            err.message(),
            "Update boom. Please check the logs for more details"
        );
    }

    #[test]
    fn map_error_uses_custom_delete_failed_message() {
        let service = CrudFixtureBuilder::new()
            .with_errors(CrudErrors {
                delete_failed: "Delete boom",
                ..CrudErrors::default()
            })
            .build();

        let err = service.map_error(CrudOp::Delete, DaoLayerError::Db(DbErr::Custom("x".into())));

        assert_eq!(
            err.message(),
            "Delete boom. Please check the logs for more details"
        );
    }

    #[test]
    fn map_error_uses_custom_not_found_message() {
        let service = CrudFixtureBuilder::new()
            .with_errors(CrudErrors {
                not_found: "Gone",
                ..CrudErrors::default()
            })
            .build();

        let err = service.map_error(
            CrudOp::Find,
            DaoLayerError::NotFound {
                entity: "test",
                id: Uuid::new_v4(),
            },
        );

        assert_eq!(err.message(), "Gone");
    }
}
