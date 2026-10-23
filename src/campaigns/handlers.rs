use super::{
    db, load_for,
    model::{Access, Campaign, CampaignForm, GmForm, InviteForm, MAX_BANNER_BYTES, sniff_image},
};
use crate::{AppState, auth::extractor::CurrentUser, error::AppError};
use axum::{
    extract::{Form, Multipart, Path, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::{context, value::Serde};
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
    let pages = crate::pages::db::list(&state.db, id).await?;
    let polls = crate::scheduling::db::for_campaign(&state.db, id, user.id).await?;
    let comments = crate::comments::views(&state, id, None, None, &user, &access).await?;
    let questions = crate::polls::views(&state, id, None, &user, &access).await?;
    let sessions: Vec<_> = crate::game_sessions::db::upcoming_for_campaign(&state.db, id)
        .await?
        .iter()
        .map(|s| crate::game_sessions::model::SessionView::new(s, state.tz))
        .collect();
    state.render(
        "campaigns/show.html",
        context! {
            current_user => Serde(&user),
            campaign => Serde(&campaign),
            description_html => crate::markdown::render(&campaign.description_md),
            can_manage => access.can_manage(),
            is_member => access.role.is_some(),
            pages => Serde(&pages),
            polls => Serde(&polls),
            sessions => Serde(&sessions),
            comments => Serde(&comments),
            can_comment => access.role.is_some(),
            questions => Serde(&questions),
        },
    )
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

/// `GET /campaigns/{id}/banner`: the card banner image, for anyone who can see the campaign.
/// Pages link it with `?v=<updated_at>`, so it can be cached for long.
pub async fn banner(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    load_for(&state.db, id, &user).await?;
    let banner = db::banner(&state.db, id).await?.ok_or(AppError::NotFound)?;
    Ok((
        [
            (header::CONTENT_TYPE, banner.content_type),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable".to_string(),
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
        ],
        banner.data,
    )
        .into_response())
}

/// `POST /campaigns/{id}/banner` (multipart, field `banner`): the GM uploads a banner.
pub async fn upload_banner(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    mut multipart: Multipart,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let mut data = Vec::new();
    // A broken or oversized upload (axum's 2 MB body limit) is a client error, not ours.
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest)?
    {
        if field.name() == Some("banner") {
            data = field
                .bytes()
                .await
                .map_err(|_| AppError::BadRequest)?
                .to_vec();
        }
    }
    let checked = match sniff_image(&data) {
        _ if data.is_empty() => Err("Choose an image file first."),
        _ if data.len() > MAX_BANNER_BYTES => Err("The image is too large; the limit is 1 MB."),
        Some(content_type) => Ok(content_type),
        None => Err("Use a PNG, JPEG or WebP image."),
    };
    let content_type = match checked {
        Ok(content_type) => content_type,
        Err(error) => {
            return Ok(render_edit(&state, &user, &campaign, Some(error))?.into_response());
        }
    };
    db::set_banner(&state.db, id, content_type, &data).await?;
    Ok(Redirect::to(&format!("/campaigns/{id}/edit#banner")).into_response())
}

/// htmx `DELETE`: remove the banner, then reload the edit page.
pub async fn delete_banner(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let (_, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    db::delete_banner(&state.db, id).await?;
    Ok([("HX-Redirect", format!("/campaigns/{id}/edit#banner"))].into_response())
}

fn render_edit(
    state: &AppState,
    user: &CurrentUser,
    campaign: &Campaign,
    banner_error: Option<&str>,
) -> Result<Html<String>, AppError> {
    state.render(
        "campaigns/edit.html",
        context! {
            current_user => Serde(user),
            campaign => Serde(campaign),
            form => Serde(campaign),
            banner_error,
        },
    )
}

/// A site admin takes over the GM role (e.g. when the GM has left the group).
pub async fn take_gm(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Response, AppError> {
    let (campaign, _) = load_for(&state.db, id, &user).await?;
    if !user.is_admin {
        return Err(AppError::NotFound);
    }
    if campaign.gm_user_id != user.id {
        db::take_gm(&state.db, id, user.id).await?;
    }
    Ok(Redirect::to(&format!("/campaigns/{id}/members")).into_response())
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
        can_take_gm => user.is_admin && campaign.gm_user_id != user.id,
        is_member => access.role.is_some(),
        members => Serde(&members),
        invites => Serde(&invites),
        error
    })?;
    Ok(Html(body))
}
