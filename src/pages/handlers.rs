use super::{db, model::PageForm};
use crate::{
    AppState, auth::extractor::CurrentUser, campaigns::load_for, error::AppError, markdown,
};
use axum::{
    extract::{Form, Path, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::{context, value::Serde};
use std::sync::Arc;

pub async fn show(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((id, slug)): Path<(i64, String)>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    let page = db::get(&state.db, id, &slug)
        .await?
        .ok_or(AppError::NotFound)?;
    let pages = db::list(&state.db, id).await?;
    state.render(
        "pages/show.html",
        context! {
            current_user => Serde(&user),
            campaign => Serde(&campaign),
            can_manage => access.can_manage(),
            page => Serde(&page),
            body_html => markdown::render(&page.body_md),
            pages => Serde(&pages),
        },
    )
}

pub async fn new_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    state.render(
        "pages/edit.html",
        context! { current_user => Serde(&user), campaign => Serde(&campaign) },
    )
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(mut form): Form<PageForm>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    form.trim();
    if let Some(error) = form.error() {
        return Ok(state
            .render(
                "pages/edit.html",
                context! {
                    current_user => Serde(&user),
                    campaign => Serde(&campaign),
                    form => Serde(&form),
                    error
                },
            )?
            .into_response());
    }
    let slug = db::create(&state.db, id, user.id, &form).await?;
    Ok(Redirect::to(&format!("/campaigns/{id}/pages/{slug}")).into_response())
}

pub async fn edit_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((id, slug)): Path<(i64, String)>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let page = db::get(&state.db, id, &slug)
        .await?
        .ok_or(AppError::NotFound)?;
    state.render(
        "pages/edit.html",
        context! {
            current_user => Serde(&user),
            campaign => Serde(&campaign),
            page => Serde(&page),
            form => Serde(&page),
        },
    )
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((id, slug)): Path<(i64, String)>,
    Form(mut form): Form<PageForm>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let page = db::get(&state.db, id, &slug)
        .await?
        .ok_or(AppError::NotFound)?;
    form.trim();
    if let Some(error) = form.error() {
        return Ok(state
            .render(
                "pages/edit.html",
                context! {
                    current_user => Serde(&user),
                    campaign => Serde(&campaign),
                    page => Serde(&page),
                    form => Serde(&form),
                    error
                },
            )?
            .into_response());
    }
    db::update(&state.db, page.id, &form).await?;
    Ok(Redirect::to(&format!("/campaigns/{id}/pages/{slug}")).into_response())
}

/// `DELETE` via htmx; the page is gone, so send the browser to the campaign.
pub async fn delete(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((id, slug)): Path<(i64, String)>,
) -> Result<Response, AppError> {
    let (_, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let page = db::get(&state.db, id, &slug)
        .await?
        .ok_or(AppError::NotFound)?;
    db::delete(&state.db, page.id).await?;
    Ok([("HX-Redirect", format!("/campaigns/{id}"))].into_response())
}
