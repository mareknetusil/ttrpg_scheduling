//! Game sessions (R3, R4, R5): created when a scheduling poll is decided.

pub mod db;
mod handlers;
pub mod model;

use crate::AppState;
use axum::{Router, routing::get};
use std::sync::Arc;

/// Nested at `/sessions`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/{id}", get(handlers::show))
}
