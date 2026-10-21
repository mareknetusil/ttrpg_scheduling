use super::{db, model::SessionView};
use crate::{AppState, auth::extractor::CurrentUser, campaigns::load_for, error::AppError};
use axum::{
    extract::{Path, State},
    response::Html,
};
use minijinja::{context, value::Serde};
use std::sync::Arc;

pub async fn show(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let session = db::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    let (campaign, access) = load_for(&state.db, session.campaign_id, &user).await?;
    let attendees = db::attendees(&state.db, id).await?;
    state.render(
        "sessions/show.html",
        context! {
            current_user => Serde(&user),
            campaign => Serde(&campaign),
            can_manage => access.can_manage(),
            session => Serde(SessionView::new(&session, state.tz)),
            attendees => Serde(&attendees),
        },
    )
}
