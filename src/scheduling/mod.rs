//! Scheduling polls (R2, R3): the GM proposes dates, members vote yes / maybe / no, the GM picks
//! one, which creates a game session.

pub mod db;
mod handlers;
pub mod model;
pub mod pattern;

use crate::AppState;
use axum::{
    Router,
    routing::{delete, get, post, put},
};
use std::sync::Arc;

/// Nested at `/campaigns/{id}/schedule`.
pub fn campaign_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", post(handlers::create))
        .route("/new", get(handlers::new_form))
        .route("/slot-dates", get(handlers::slot_dates_json))
        .route(
            "/patterns",
            get(handlers::patterns_page).post(handlers::create_pattern),
        )
        .route("/patterns/{pattern_id}", delete(handlers::delete_pattern))
}

/// Nested at `/schedule`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/{poll_id}", get(handlers::show).post(handlers::update))
        .route("/{poll_id}/edit", get(handlers::edit_form))
        .route("/{poll_id}/options/{option_id}/vote", put(handlers::vote))
        .route("/{poll_id}/decide", post(handlers::decide))
        .route("/{poll_id}/cancel", post(handlers::cancel))
}
