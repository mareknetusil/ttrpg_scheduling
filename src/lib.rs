//! The app as a library: `main.rs` is a thin binary around it, and the end-to-end tests in
//! `tests/` build the same router (`router()`) against a test database.

mod admin;
pub mod auth;
mod campaigns;
pub mod cli;
mod comments;
pub mod config;
pub mod db;
mod error;
mod game_sessions;
mod invites;
mod markdown;
mod pages;
mod polls;
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
use minijinja::{Environment, context};
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;
use tower_http::{services::ServeDir, trace::TraceLayer};

pub struct AppState {
    templates: Environment<'static>,
    db: SqlitePool,
    /// The display time zone (`DISPLAY_TZ`)
    tz: chrono_tz::Tz,
}

impl AppState {
    /// Templates are loaded from `templates/`, relative to the working directory.
    pub fn new(db: SqlitePool, tz: chrono_tz::Tz) -> AppState {
        let mut templates = Environment::new();
        templates.set_loader(minijinja::path_loader("templates"));
        templates.add_global("app_version", env!("APP_VERSION"));
        templates.add_global(
            "accents",
            minijinja::Value::from(campaigns::model::ACCENTS.to_vec()),
        );
        // `{{ (m.joined_at|when).date_long }}`: a DB timestamp kept as a string in a row struct,
        // as the same `When` that handlers pass for other timestamps
        templates.add_filter("when", move |s: String| {
            minijinja::Value::from(minijinja::value::Serde(time::When::from_db(&s, tz)))
        });
        AppState { templates, db, tz }
    }

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
    // Polls still waiting for some of the user's answers get a card of their own, at the top
    let (to_vote, polls): (Vec<_>, Vec<_>) = scheduling::db::open_for_user(&state.db, user.id)
        .await?
        .into_iter()
        .partition(|p| p.voting_open && p.my_vote_count < p.option_count);
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
            to_vote => minijinja::value::Serde(&to_vote),
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

/// `GET /healthz`: 200 when the database answers. For the deploy script and monitoring.
async fn healthz(State(state): State<Arc<AppState>>) -> Result<String, error::AppError> {
    sqlx::query("SELECT 1").execute(&state.db).await?;
    Ok(format!("ok {}\n", env!("APP_VERSION")))
}

/// Resolves on Ctrl+C, or on SIGTERM (what `systemctl stop` sends). In-flight requests then
/// finish before the server exits, and the SQLite pool closes cleanly.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    // SIGTERM only exists on Unix; `cfg` compiles the other branch on other platforms.
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(e) => {
                tracing::warn!(error = %e, "can't listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

/// The whole app: every route, static files, request logging. Tests call this too.
pub fn router(state: AppState) -> Router {
    Router::new()
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
        .nest("/campaigns/{id}", comments::campaign_router())
        .nest("/sessions/{id}", comments::session_router())
        .nest("/schedule/{id}", comments::poll_router())
        .nest("/comments", comments::router())
        .nest("/campaigns/{id}/polls", polls::campaign_router())
        .nest("/polls", polls::router())
        .nest("/invite", invites::router())
        .nest("/admin", admin::router())
        .route("/markdown/preview", post(markdown_preview))
        .route("/healthz", get(healthz))
        .nest_service("/static", ServeDir::new("static"))
        .with_state(Arc::new(state))
        .fallback(fallback_error_handler)
        .layer(TraceLayer::new_for_http())
}

pub async fn serve(config: config::Config, db: SqlitePool) -> anyhow::Result<()> {
    let app = router(AppState::new(db, config.display_tz));
    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .with_context(|| format!("failed to bind {}", config.bind_addr))?;
    tracing::info!(addr = %listener.local_addr()?, version = env!("APP_VERSION"), "listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    tracing::info!("shut down");
    Ok(())
}
