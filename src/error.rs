use axum::{http, response};
use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub enum AppError {
    NotFound,
    Template(minijinja::Error),
    Database(sqlx::Error),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::Template(_) => write!(f, "failed to render template"),
            Self::Database(_) => write!(f, "database error"),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NotFound => None,
            Self::Template(e) => Some(e),
            Self::Database(e) => Some(e),
        }
    }
}

impl From<minijinja::Error> for AppError {
    fn from(error: minijinja::Error) -> Self {
        Self::Template(error)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

impl response::IntoResponse for AppError {
    fn into_response(self) -> response::Response {
        match self {
            Self::NotFound => (http::StatusCode::NOT_FOUND, "Not Found").into_response(),
            Self::Template(e) => {
                tracing::error!(error = ?e, "template error");
                (http::StatusCode::INTERNAL_SERVER_ERROR, "Internal Error").into_response()
            }
            Self::Database(e) => {
                tracing::error!(error = ?e, "database error");
                (http::StatusCode::INTERNAL_SERVER_ERROR, "Internal Error").into_response()
            }
        }
    }
}
