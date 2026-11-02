//! Notifications (see GLOSSARY.md): what changed in your campaigns since you last looked.
//! Feature modules call `db::notify` in the transaction that makes a change, and the pages that
//! show a thing call `viewed`, which marks its notifications as seen.

pub mod db;
mod handlers;
pub mod model;

pub use handlers::views;

use crate::{AppState, auth::extractor::CurrentUser};
use axum::{
    Router,
    routing::{get, put},
};
use model::Viewing;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

/// Nested at `/notifications`.
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(handlers::index))
        .route("/seen", put(handlers::mark_all_seen))
}

/// The user opened a page: its notifications are seen. Also takes them off `user.unseen`, the
/// nav count the page is about to render (the extractor counted them before).
pub async fn viewed(
    db: &SqlitePool,
    user: &mut CurrentUser,
    viewing: Viewing,
) -> Result<(), sqlx::Error> {
    let cleared = db::seen(db, user.id, &viewing).await?;
    user.unseen = (user.unseen - cleared as i64).max(0);
    Ok(())
}
