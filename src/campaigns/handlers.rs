use super::{
    db,
    model::{Access, Campaign, CampaignForm, GmForm, InviteForm, Role},
};
use crate::{AppState, auth::extractor::CurrentUser, error::AppError};
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
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

pub async fn members_page(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    render_members(&state, &user, &campaign, &access, None).await
}

pub async fn create_invite(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(form): Form<InviteForm>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    if !(1..=20).contains(&form.max_uses) {
        return Ok(render_members(
            &state,
            &user,
            &campaign,
            &access,
            Some("Pick between 1 and 20 players."),
        )
        .await?
        .into_response());
    }
    db::create_invite(&state.db, id, user.id, form.max_uses).await?;
    Ok(Redirect::to(&format!("/campaigns/{id}/members")).into_response())
}

pub async fn revoke_invite(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((id, invite_id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let (_, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let deleted = db::delete_invite(&state.db, id, invite_id).await?;
    if deleted == 0 {
        Err(AppError::NotFound)
    } else {
        Ok(StatusCode::OK.into_response())
    }
}

pub async fn remove_member(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((id, uid)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() && uid != user.id {
        return Err(AppError::NotFound);
    }
    if uid == campaign.gm_user_id {
        return Err(AppError::NotFound);
    }
    let deleted = db::remove_member(&state.db, id, uid).await?;
    if deleted == 0 {
        Err(AppError::NotFound)
    } else if uid == user.id {
        Ok([("HX-Redirect", "/")].into_response())
    } else {
        Ok(StatusCode::OK.into_response())
    }
}

pub async fn change_gm(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(form): Form<GmForm>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    if !db::is_member(&state.db, id, form.user_id).await? {
        return Ok(render_members(
            &state,
            &user,
            &campaign,
            &access,
            Some("The new GM must be a member of the campaign."),
        )
        .await?
        .into_response());
    }
    db::set_gm(&state.db, id, form.user_id).await?;
    Ok(Redirect::to(&format!("/campaigns/{id}/members")).into_response())
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

async fn render_members(
    state: &AppState,
    user: &CurrentUser,
    campaign: &Campaign,
    access: &Access,
    error: Option<&str>,
) -> Result<Html<String>, AppError> {
    let invites = if access.can_manage() {
        db::invites(&state.db, campaign.id).await?
    } else {
        Vec::new()
    };
    let members = db::members(&state.db, campaign.id).await?;
    let template = state.templates.get_template("campaigns/members.html")?;
    let body = template.render(context! {
        current_user => Serde(user),
        campaign => Serde(campaign),
        can_manage => access.can_manage(),
        members => Serde(&members),
        invites => Serde(&invites),
        error
    })?;
    Ok(Html(body))
}
