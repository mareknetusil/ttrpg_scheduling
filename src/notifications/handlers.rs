use super::{db, model::NotificationView};
use crate::{AppState, auth::extractor::CurrentUser, error::AppError};
use axum::{
    extract::State,
    response::{Html, IntoResponse, Response},
};
use minijinja::{context, value::Serde};
use std::sync::Arc;

/// The user's unseen (`seen` false) or seen notifications, newest first, at most `limit`
/// (-1: all). Also for the dashboard's card.
pub async fn views(
    state: &AppState,
    user_id: i64,
    seen: bool,
    limit: i64,
) -> Result<Vec<NotificationView>, AppError> {
    Ok(db::list(&state.db, user_id, seen, limit)
        .await?
        .into_iter()
        .map(|row| NotificationView::new(row, state.tz))
        .collect())
}

/// `GET /notifications`: every unseen notification, then the last 20 seen ones. Looking at the
/// list doesn't mark anything as seen; opening what a notification points to does.
pub async fn index(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let unseen = views(&state, user.id, false, -1).await?;
    let seen = views(&state, user.id, true, 20).await?;
    state.render(
        "notifications/index.html",
        context! {
            current_user => Serde(&user),
            unseen => Serde(&unseen),
            seen => Serde(&seen),
        },
    )
}

/// `PUT /notifications/seen` via htmx ("Mark all as seen"): the page reloads, so the nav count
/// updates too.
pub async fn mark_all_seen(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
) -> Result<Response, AppError> {
    db::seen_all(&state.db, user.id).await?;
    Ok([("HX-Refresh", "true")].into_response())
}
