//! Game sessions (R3, R4, R5): created when a scheduling poll is decided, or by hand. History,
//! attendance and entries (recaps, notes, loot).

pub mod db;
mod handlers;
pub mod model;

use crate::AppState;
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

/// Nested at `/sessions`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/{id}", get(handlers::show).post(handlers::update))
        .route("/{id}/attendance", post(handlers::attendance))
        .route("/{id}/entries", post(handlers::create_entry))
}

/// Nested at `/entries`.
pub fn entries_router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/{entry_id}",
            post(handlers::update_entry).delete(handlers::delete_entry),
        )
        .route("/{entry_id}/edit", get(handlers::edit_entry_form))
}

/// Nested at `/campaigns/{id}`.
pub fn campaign_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/history", get(handlers::history))
        .route("/sessions", post(handlers::create))
        .route("/sessions/new", get(handlers::new_form))
}
