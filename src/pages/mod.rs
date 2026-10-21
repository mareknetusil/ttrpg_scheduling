//! Campaign pages (R6, wiki-lite): GM-written Markdown pages under `/campaigns/{id}/pages`.

pub mod db;
mod handlers;
pub mod model;

use crate::AppState;
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;

/// Nested at `/campaigns/{id}/pages`. `/new` is a static segment, so it wins over `/{slug}`;
/// `db::create` never hands out the slug "new".
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", post(handlers::create))
        .route("/new", get(handlers::new_form))
        .route(
            "/{slug}",
            get(handlers::show)
                .post(handlers::update)
                .delete(handlers::delete),
        )
        .route("/{slug}/edit", get(handlers::edit_form))
}
