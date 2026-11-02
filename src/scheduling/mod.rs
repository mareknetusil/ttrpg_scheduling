//! Scheduling polls (R2, R3): the GM proposes dates, members vote yes / maybe / no, the GM picks
//! one, which creates a game session.

pub mod db;
mod handlers;
pub mod model;

use crate::AppState;
use axum::{
    Router,
    routing::{get, post, put},
};
use std::sync::Arc;

/// Nested at `/campaigns/{id}/schedule`.
pub fn campaign_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", post(handlers::create))
        .route("/new", get(handlers::new_form))
}

/// Nested at `/schedule`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/{poll_id}", get(handlers::show).post(handlers::update))
        .route("/{poll_id}/edit", get(handlers::edit_form))
        .route("/{poll_id}/grid", get(handlers::grid))
        .route(
            "/{poll_id}/options/{option_id}/vote",
            put(handlers::vote).delete(handlers::clear_vote),
        )
        .route("/{poll_id}/options/{option_id}/paint", put(handlers::paint))
        .route(
            "/{poll_id}/weekdays/{weekday}",
            put(handlers::paint_weekday),
        )
        .route("/{poll_id}/vote-unanswered", put(handlers::vote_unanswered))
        .route("/{poll_id}/decide", post(handlers::decide))
        .route("/{poll_id}/cancel", post(handlers::cancel))
        .route("/{poll_id}/reopen", post(handlers::reopen))
}
