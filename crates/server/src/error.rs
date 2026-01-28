use std::error::Error;

#[derive(Debug)]
pub(crate) struct InternalError {
    message: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Internal(InternalError),
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(InternalError {
            message: message.into(),
            source: None,
        })
    }

    pub fn internal_with_source(
        message: impl Into<String>,
        source: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self::Internal(InternalError {
            message: message.into(),
            source: Some(Box::new(source)),
        })
    }

    pub fn message(&self) -> &str {
        match self {
            Self::BadRequest(message)
            | Self::Unauthorized(message)
            | Self::Forbidden(message)
            | Self::NotFound(message)
            | Self::Conflict(message) => message.as_str(),
            Self::Internal(internal) => internal.message.as_str(),
        }
    }

    pub fn source(&self) -> Option<&(dyn Error + Send + Sync)> {
        match self {
            Self::Internal(internal) => internal.source.as_deref(),
            _ => None,
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl From<crate::db::dao::DaoLayerError> for AppError {
    fn from(err: crate::db::dao::DaoLayerError) -> Self {
        match err {
            //crate::db::dao::DaoLayerError::NotFound { .. } => AppError::not_found(err.to_string()),
            //crate::db::dao::DaoLayerError::InvalidPagination { .. } => {
            //    AppError::bad_request(err.to_string())
            //}
            crate::db::dao::DaoLayerError::Db(db_err) => AppError::internal_with_source(
                "database operation failed. Please check the logs for more details",
                db_err,
            ),
            _ => AppError::bad_request(format!("Database error: {}", err)),
        }
    }
}
