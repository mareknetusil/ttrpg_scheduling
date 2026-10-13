mod auth;
mod campaigns;
mod cli;
mod config;
mod db;
mod error;

use crate::auth::extractor::CurrentUser;
use anyhow::Context;
use axum::{
    Router,
    extract::State,
    response::Html,
    routing::{get, post},
};
use clap::Parser;
use minijinja::{Environment, context};
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

struct AppState {
    templates: Environment<'static>,
    db: SqlitePool,
}

async fn fallback_error_handler() -> error::AppError {
    error::AppError::NotFound
}

#[axum::debug_handler]
async fn index(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
) -> Result<Html<String>, error::AppError> {
    let user_count: i64 = sqlx::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let template = state.templates.get_template("index.html")?;
    let body =
        template.render(context! { current_user => minijinja::value::Serde(user), user_count })?;
    Ok(Html(body))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = cli::Cli::parse();

    let config = config::Config::from_env()?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=debug")),
        )
        .init();
    let db = db::connect(&config.database_url).await?;

    match cli.command {
        Some(cli::Command::CreateAdmin {
            username,
            display_name,
        }) => cli::create_admin(&db, username, display_name).await,
        None => serve(config, db).await,
    }
}

async fn serve(config: config::Config, db: SqlitePool) -> anyhow::Result<()> {
    let mut env = Environment::new();
    env.set_loader(minijinja::path_loader("templates"));
    let state = AppState { templates: env, db };
    let app = Router::new()
        .route("/", get(index))
        .route(
            "/login",
            get(auth::handlers::login_page).post(auth::handlers::login),
        )
        .route("/logout", post(auth::handlers::logout))
        .nest("/campaigns", campaigns::router())
        .nest_service("/static", ServeDir::new("static"))
        .with_state(Arc::new(state))
        .fallback(fallback_error_handler)
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_addr))?;
    tracing::info!(addr = %listener.local_addr()?, "listening");
    axum::serve(listener, app).await?;

    Ok(())
}
