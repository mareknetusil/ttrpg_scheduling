use super::{db, model::CampaignForm};
use crate::{AppState, auth::extractor::CurrentUser, error::AppError};
use axum::{
    extract::{Form, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::context;
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
