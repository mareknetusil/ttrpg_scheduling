use super::{
    db,
    model::{Access, Campaign, CampaignForm, Role},
};
use crate::{AppState, auth::extractor::CurrentUser, error::AppError};
use axum::{
    extract::{Form, Path, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::{context, value::Serde};
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

pub async fn new_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let template = state.templates.get_template("campaigns/new.html")?;
    let body = template.render(context! { current_user => Serde(&user)})?;
    Ok(Html(body))
}

pub async fn edit_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let template = state.templates.get_template("campaigns/edit.html")?;
    let body = template.render(context! {
        current_user => Serde(&user),
        campaign => Serde(&campaign),
        form => Serde(&campaign)
    })?;
    Ok(Html(body))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Form(mut form): Form<CampaignForm>,
) -> Result<Response, AppError> {
    form.trim();

    if let Some(error) = form.error() {
        let template = state.templates.get_template("campaigns/new.html")?;
        let body = template.render(context! {
            current_user => Serde(&user),
            form => Serde(&form),
            error
        })?;
        return Ok(Html(body).into_response());
    }
    let id = db::create(&state.db, &form, user.id).await?;
    Ok(Redirect::to(&format!("/campaigns/{id}")).into_response())
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(mut form): Form<CampaignForm>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    form.trim();
    if let Some(error) = form.error() {
        let template = state.templates.get_template("campaigns/edit.html")?;
        let body = template.render(context! {
            current_user => Serde(&user),
            form => Serde(&form),
            campaign => Serde(&campaign),
            error
        })?;
        return Ok(Html(body).into_response());
    }
    db::update(&state.db, id, &form).await?;
    Ok(Redirect::to(&format!("/campaigns/{id}")).into_response())
}

pub async fn show(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    let template = state.templates.get_template("campaigns/show.html")?;
    let body = template.render(context! {
        current_user => Serde(&user),
        campaign => Serde(&campaign),
        can_manage => access.can_manage()
    })?;
    Ok(Html(body))
}

async fn load_for(
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
