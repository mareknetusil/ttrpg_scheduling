pub mod db;
mod handlers;
pub mod model;

use crate::{AppState, auth::extractor::CurrentUser, error::AppError};
use axum::{
    Router,
    routing::{delete, get, post},
};
use model::{Access, Campaign, Role};
use sqlx::sqlite::SqlitePool;
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

/// Loads a campaign together with the user's access to it. Non-members (who aren't admins) get
/// `NotFound`, like a campaign that doesn't exist (DESIGN §5.3).
pub async fn load_for(
    db: &SqlitePool,
    campaign_id: i64,
    user: &CurrentUser,
) -> Result<(Campaign, Access), AppError> {
    let campaign = db::get(db, campaign_id).await?.ok_or(AppError::NotFound)?;
    let role = if campaign.gm_user_id == user.id {
        Some(Role::Gm)
    } else if db::is_member(db, campaign_id, user.id).await? {
        Some(Role::Member)
    } else {
        None
    };
    let access = Access {
        role,
        is_admin: user.is_admin,
    };
    if !access.can_view() {
        return Err(AppError::NotFound);
    }
    Ok((campaign, access))
}
