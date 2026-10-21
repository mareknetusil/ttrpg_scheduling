//! Polls (R8): single or multiple choice questions on a campaign or one of its game sessions.
//! Voting and closing go through htmx and answer with the re-rendered poll.

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

/// Nested at `/polls`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/{poll_id}/vote", post(handlers::vote))
        .route("/{poll_id}/close", post(handlers::close))
}

/// Nested at `/campaigns/{id}/polls`.
pub fn campaign_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", post(handlers::create))
        .route("/new", get(handlers::new_form))
}
