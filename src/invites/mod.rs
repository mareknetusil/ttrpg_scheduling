mod db;
mod handlers;
mod model;

use crate::AppState;
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/{token}", get(handlers::show).post(handlers::join))
        .route("/{token}/register", post(handlers::register))
}
