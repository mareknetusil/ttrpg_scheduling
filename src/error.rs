use axum::{http, response};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not found")]
    NotFound,

    #[error("failed to render template")]
    Template(#[from] minijinja::Error),

    #[error("database error")]
    Database(#[from] sqlx::Error),

    #[error("internal error")]
    Internal(#[from] anyhow::Error),
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
            Self::Internal(e) => {
                tracing::error!(error = ?e, "internal error");
                (http::StatusCode::INTERNAL_SERVER_ERROR, "Internal Error").into_response()
            }
        }
    }
}
