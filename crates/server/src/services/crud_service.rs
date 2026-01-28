use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, NaiveTime};
use sea_orm::{
    ColumnTrait, EntityTrait, IdenStatic, IntoActiveModel, Iterable, Order, Select,
};
use sea_orm::sea_query::{ColumnType, Value as QueryValue};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::db::dao::{ColumnFilter, CompareOp, DaoBase, DaoLayerError, FilterOp, PaginatedResponse};
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
        let detail = err.to_string();
        let message = match err {
            DaoLayerError::Db(_) => {
                let context = match op {
                    CrudOp::Create => errors.create_failed,
                    CrudOp::Find | CrudOp::List => errors.find_failed,
                    CrudOp::Update => errors.update_failed,
                    CrudOp::Delete => errors.delete_failed,
                };
                format!("{context}: {detail}")
            }
            DaoLayerError::NotFound { .. } => detail,
            DaoLayerError::InvalidPagination { .. } => detail,
        };
        AppError::bad_request(message)
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
        CrudColumn<Self::Dao>: ColumnTrait + Clone,
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
        CrudColumn<Self::Dao>: ColumnTrait + Clone,
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
                    let spec = spec_map
                        .get(key.as_str())
                        .ok_or_else(invalid_filter)?;
                    let parsed_op = (spec.parse)(&value)?;
                    parsed.push(ColumnFilter {
                        column: spec.column.clone(),
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
                    let column = column_map
                        .get(key.as_str())
                        .ok_or_else(invalid_filter)?;
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
                        column: column.clone(),
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
    raw.trim()
        .parse::<T>()
        .map_err(|err| invalid_filter_value_with(err))
}

fn parse_float(raw: &str) -> Result<f64, AppError> {
    raw.trim()
        .parse::<f64>()
        .map_err(|err| invalid_filter_value_with(err))
}

fn parse_date(raw: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d")
        .map_err(|err| invalid_filter_value_with(err))
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
    DateTime::parse_from_rfc3339(raw.trim()).map_err(|err| invalid_filter_value_with(err))
}

fn parse_json(raw: &str) -> Result<JsonValue, AppError> {
    serde_json::from_str(raw.trim()).map_err(|err| invalid_filter_value_with(err))
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
            let uuid =
                Uuid::parse_str(raw.trim()).map_err(|err| invalid_filter_value_with(err))?;
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
