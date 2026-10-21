//! Comments (R7) on a campaign or on one of its game sessions. Posting, editing and deleting
//! go through htmx and answer with HTML fragments.

pub mod db;
mod handlers;
pub mod model;

pub use handlers::views;

use crate::AppState;
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

/// Nested at `/comments`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/{comment_id}",
            get(handlers::show)
                .put(handlers::update)
                .delete(handlers::delete),
        )
        .route("/{comment_id}/edit", get(handlers::edit_form))
}

/// `POST /campaigns/{id}/comments`
pub fn campaign_router() -> Router<Arc<AppState>> {
    Router::new().route("/comments", post(handlers::create_for_campaign))
}

/// `POST /sessions/{id}/comments`
pub fn session_router() -> Router<Arc<AppState>> {
    Router::new().route("/comments", post(handlers::create_for_session))
}
