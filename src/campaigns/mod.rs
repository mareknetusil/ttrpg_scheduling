pub mod db;
mod handlers;
mod model;

use crate::AppState;
use axum::{
    Router,
    routing::{delete, get, post},
};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", post(handlers::create))
        .route("/new", get(handlers::new_form))
        .route("/{id}", get(handlers::show).post(handlers::update))
        .route("/{id}/edit", get(handlers::edit_form))
        .route("/{id}/members", get(handlers::members_page))
        .route("/{id}/invites", post(handlers::create_invite))
        .route("/{id}/invites/{invite_id}", delete(handlers::revoke_invite))
        .route("/{id}/members/{uid}", delete(handlers::remove_member))
        .route("/{id}/gm", post(handlers::change_gm))
}
