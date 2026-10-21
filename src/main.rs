mod auth;
mod campaigns;
mod cli;
mod config;
mod db;
mod error;
mod game_sessions;
mod invites;
mod markdown;
mod pages;
mod scheduling;
mod time;

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
    /// The display time zone (`DISPLAY_TZ`)
    tz: chrono_tz::Tz,
}

impl AppState {
    /// Renders a full-page template.
    fn render(&self, name: &str, ctx: minijinja::Value) -> Result<Html<String>, error::AppError> {
        Ok(Html(self.templates.get_template(name)?.render(ctx)?))
    }

    /// Renders one `{% block %}` of a template, for htmx fragments.
    fn render_block(
        &self,
        name: &str,
        block: &str,
        ctx: minijinja::Value,
    ) -> Result<Html<String>, error::AppError> {
        // minijinja 3: render once while keeping the state, then render just the block from it.
        let mut captured = self.templates.get_template(name)?.render_captured(ctx)?;
        Ok(Html(
            captured.with_state_mut(|state| state.render_block(block))?,
        ))
    }
}

async fn fallback_error_handler() -> error::AppError {
    error::AppError::NotFound
}

async fn index(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
) -> Result<Html<String>, error::AppError> {
    let campaigns = campaigns::db::for_user(&state.db, user.id).await?;
    let polls = scheduling::db::open_for_user(&state.db, user.id).await?;
    let sessions: Vec<_> = game_sessions::db::upcoming_for_user(&state.db, user.id)
        .await?
        .into_iter()
        .map(|row| game_sessions::model::UpcomingView::new(row, state.tz))
        .collect();
    state.render(
        "index.html",
        context! {
            current_user => minijinja::value::Serde(&user),
            campaigns => minijinja::value::Serde(&campaigns),
            polls => minijinja::value::Serde(&polls),
            sessions => minijinja::value::Serde(&sessions),
        },
    )
}

/// `POST /markdown/preview`: the editor's Preview tab. The form has one field (the textarea,
/// whatever its name), sent by `hx-include`.
async fn markdown_preview(
    _user: CurrentUser,
    axum::extract::Form(fields): axum::extract::Form<std::collections::HashMap<String, String>>,
) -> Html<String> {
    let source = fields.into_values().next().unwrap_or_default();
    if source.trim().is_empty() {
        return Html("<p class=\"muted\">Nothing to preview.</p>".to_string());
    }
    Html(markdown::render(&source))
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
    let state = AppState {
        templates: env,
        db,
        tz: config.display_tz,
    };
    let app = Router::new()
        .route("/", get(index))
        .route(
            "/login",
            get(auth::handlers::login_page).post(auth::handlers::login),
        )
        .route("/logout", post(auth::handlers::logout))
        .nest("/campaigns", campaigns::router())
        .nest("/campaigns/{id}/pages", pages::router())
        .nest("/campaigns/{id}/schedule", scheduling::campaign_router())
        .nest("/schedule", scheduling::router())
        .nest("/sessions", game_sessions::router())
        .nest("/entries", game_sessions::entries_router())
        .nest("/campaigns/{id}", game_sessions::campaign_router())
        .nest("/invite", invites::router())
        .route("/markdown/preview", post(markdown_preview))
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
