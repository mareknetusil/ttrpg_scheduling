use axum::{Router, extract::State, http::StatusCode, response::Html, routing::get};
use minijinja::{Environment, context};
use std::sync::Arc;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

fn internal_error(e: minijinja::Error) -> StatusCode {
    tracing::error!(error = ?e, "template error");
    StatusCode::INTERNAL_SERVER_ERROR
}

struct AppState {
    templates: Environment<'static>,
}

#[axum::debug_handler]
async fn index(State(state): State<Arc<AppState>>) -> Result<Html<String>, StatusCode> {
    let template = state
        .templates
        .get_template("index.html")
        .map_err(internal_error)?;
    let body = template
        .render(context! { name => "x" })
        .map_err(internal_error)?;
    Ok(Html(body))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug")),
        )
        .init();
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));
    let state = AppState { templates: env };

    let app = Router::new()
        .route("/", get(index))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(Arc::new(state))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();
    tracing::info!(addr = %listener.local_addr().unwrap(), "listening");
    axum::serve(listener, app).await.unwrap();
}
