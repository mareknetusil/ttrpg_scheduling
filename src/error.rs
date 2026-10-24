use axum::{
    http,
    response::{self, IntoResponse},
};

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

    #[error("unauthorized")]
    Unauthorized,

    /// A malformed request that the UI never sends (e.g. a broken upload).
    #[error("bad request")]
    BadRequest,
}

impl response::IntoResponse for AppError {
    fn into_response(self) -> response::Response {
        match self {
            Self::NotFound => error_page(http::StatusCode::NOT_FOUND, "Page not found"),
            Self::Template(e) => {
                tracing::error!(error = ?e, "template error");
                error_page(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong",
                )
            }
            Self::Database(e) => {
                tracing::error!(error = ?e, "database error");
                error_page(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong",
                )
            }
            Self::Internal(e) => {
                tracing::error!(error = ?e, "internal error");
                error_page(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    "Something went wrong",
                )
            }
            Self::Unauthorized => response::Redirect::to("/login").into_response(),
            Self::BadRequest => error_page(http::StatusCode::BAD_REQUEST, "Bad request"),
        }
    }
}

/// A small error page with a link home: the app added to a phone's home screen has no address
/// bar or back button, so a bare "Not Found" would be a dead end. Plain HTML rather than a
/// template, so it still works when rendering a template is what failed. `title` is always one
/// of the fixed strings above, never user input.
fn error_page(status: http::StatusCode, title: &str) -> response::Response {
    let body = format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title} · TTRPG Scheduling</title>
  <link rel="stylesheet" href="/static/css/style.css">
</head>
<body>
  <main class="container">
    <section class="auth card">
      <h1>{title}</h1>
      <p class="muted">Error {code}.</p>
      <a class="button" href="/">Back to Home</a>
    </section>
  </main>
</body>
</html>
"#,
        code = status.as_u16()
    );
    (status, response::Html(body)).into_response()
}
