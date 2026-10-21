//! Site admin tools: all users (create, reset password, admin flag, deactivate) and all
//! campaigns (list, delete). Every handler checks `is_admin`; others get a 404.

mod db;
mod handlers;
mod model;

use crate::AppState;
use axum::{
    Router,
    routing::{get, post, put},
};
use std::sync::Arc;

/// Nested at `/admin`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(handlers::index))
        .route("/users", get(handlers::users).post(handlers::create_user))
        .route("/users/{id}/admin", put(handlers::set_admin))
        .route("/users/{id}/active", put(handlers::set_active))
        .route("/users/{id}/password", post(handlers::set_password))
        .route("/campaigns", get(handlers::campaigns))
        .route(
            "/campaigns/{id}",
            axum::routing::delete(handlers::delete_campaign),
        )
        .route(
            "/campaigns/{id}/delete",
            get(handlers::delete_campaign_page),
        )
}
