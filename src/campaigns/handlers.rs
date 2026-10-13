use super::{
    db,
    model::{Access, Campaign, CampaignForm, Role},
};
use crate::{AppState, auth::extractor::CurrentUser, error::AppError};
use axum::{
    extract::{Form, Path, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::context;
use sqlx::sqlite::SqlitePool;
use std::sync::Arc;

pub async fn new_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
) -> Result<Html<String>, AppError> {
    let template = state.templates.get_template("campaigns/new.html")?;
    let body = template.render(context! { current_user => minijinja::value::Serde(user)})?;
    Ok(Html(body))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Form(mut form): Form<CampaignForm>,
) -> Result<Response, AppError> {
    form.name = form.name.trim().to_string();
    form.system = form.system.trim().to_string();
    form.tagline = form.tagline.trim().to_string();
    form.description_md = form.description_md.trim().to_string();

    if form.name.is_empty() || form.system.is_empty() {
        let template = state.templates.get_template("campaigns/new.html")?;
        let body = template.render(context! {
            current_user => minijinja::value::Serde(user),
            form => minijinja::value::Serde(form),
            error => "Fill campaign name and rule system!"
        })?;
        return Ok(Html(body).into_response());
    }
    let id = db::create(&state.db, &form, user.id).await?;
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
        current_user => minijinja::value::Serde(&user),
        campaign => minijinja::value::Serde(&campaign),
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
