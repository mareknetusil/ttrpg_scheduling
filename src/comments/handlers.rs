use super::{
    db,
    model::{Comment, CommentForm, CommentView, MAX_LEN},
};
use crate::{
    AppState,
    auth::extractor::CurrentUser,
    campaigns::{load_for, model::Access},
    error::AppError,
    game_sessions, scheduling,
};
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use minijinja::{context, value::Serde};
use std::sync::Arc;

/// The comments of a campaign, game session or scheduling poll, prepared for the page that shows
/// them.
pub async fn views(
    state: &AppState,
    campaign_id: i64,
    session_id: Option<i64>,
    poll_id: Option<i64>,
    user: &CurrentUser,
    access: &Access,
) -> Result<Vec<CommentView>, AppError> {
    Ok(db::list(&state.db, campaign_id, session_id, poll_id)
        .await?
        .iter()
        .map(|c| view(c, user, access, state))
        .collect())
}

fn can_edit(c: &Comment, user: &CurrentUser, access: &Access) -> bool {
    access.role.is_some() && c.author_id == user.id
}

fn view(c: &Comment, user: &CurrentUser, access: &Access, state: &AppState) -> CommentView {
    let can_edit = can_edit(c, user, access);
    CommentView::new(c, can_edit, can_edit || access.can_manage(), state.tz)
}

/// Validated comment text, or `None` (the textarea is `required`, so only crafted requests or
/// whitespace end up here; they get a 422 and htmx swaps nothing).
fn clean(form: &CommentForm) -> Option<&str> {
    let body = form.body_md.trim();
    (!body.is_empty() && body.len() <= MAX_LEN).then_some(body)
}

pub async fn create_for_campaign(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(form): Form<CommentForm>,
) -> Result<Response, AppError> {
    let (_, access) = load_for(&state.db, id, &user).await?;
    create(&state, &user, &access, id, None, None, &form).await
}

pub async fn create_for_session(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(session_id): Path<i64>,
    Form(form): Form<CommentForm>,
) -> Result<Response, AppError> {
    let session = game_sessions::db::get(&state.db, session_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (_, access) = load_for(&state.db, session.campaign_id, &user).await?;
    create(
        &state,
        &user,
        &access,
        session.campaign_id,
        Some(session_id),
        None,
        &form,
    )
    .await
}

pub async fn create_for_poll(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
    Form(form): Form<CommentForm>,
) -> Result<Response, AppError> {
    let poll = scheduling::db::get(&state.db, poll_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (_, access) = load_for(&state.db, poll.campaign_id, &user).await?;
    create(
        &state,
        &user,
        &access,
        poll.campaign_id,
        None,
        Some(poll_id),
        &form,
    )
    .await
}

/// Answers with the new comment (fragment), which htmx appends to the list.
async fn create(
    state: &AppState,
    user: &CurrentUser,
    access: &Access,
    campaign_id: i64,
    session_id: Option<i64>,
    poll_id: Option<i64>,
    form: &CommentForm,
) -> Result<Response, AppError> {
    if access.role.is_none() {
        return Err(AppError::NotFound);
    }
    let Some(body) = clean(form) else {
        return Ok(StatusCode::UNPROCESSABLE_ENTITY.into_response());
    };
    let id = db::create(&state.db, campaign_id, session_id, poll_id, user.id, body).await?;
    let comment = db::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    Ok(render_comment(state, &view(&comment, user, access, state))?.into_response())
}

/// `GET /comments/{id}`: the comment itself (the edit form's Cancel).
pub async fn show(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(comment_id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (comment, access) = load(&state, comment_id, &user).await?;
    render_comment(&state, &view(&comment, &user, &access, &state))
}

/// `GET /comments/{id}/edit`: the edit form, swapped over the comment.
pub async fn edit_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(comment_id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (comment, access) = load(&state, comment_id, &user).await?;
    if !can_edit(&comment, &user, &access) {
        return Err(AppError::NotFound);
    }
    let c = view(&comment, &user, &access, &state);
    state.render("comments/_edit.html", context! { c => Serde(&c) })
}

/// `PUT /comments/{id}`: save, answer with the updated comment.
pub async fn update(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(comment_id): Path<i64>,
    Form(form): Form<CommentForm>,
) -> Result<Response, AppError> {
    let (comment, access) = load(&state, comment_id, &user).await?;
    if !can_edit(&comment, &user, &access) {
        return Err(AppError::NotFound);
    }
    let Some(body) = clean(&form) else {
        return Ok(StatusCode::UNPROCESSABLE_ENTITY.into_response());
    };
    db::update(&state.db, comment_id, body).await?;
    let comment = db::get(&state.db, comment_id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok(render_comment(&state, &view(&comment, &user, &access, &state))?.into_response())
}

/// `DELETE /comments/{id}`: the author, or the GM/admin. An empty 200 removes it on the page.
pub async fn delete(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(comment_id): Path<i64>,
) -> Result<Response, AppError> {
    let (comment, access) = load(&state, comment_id, &user).await?;
    if !(can_edit(&comment, &user, &access) || access.can_manage()) {
        return Err(AppError::NotFound);
    }
    db::delete(&state.db, comment_id).await?;
    Ok(StatusCode::OK.into_response())
}

async fn load(
    state: &AppState,
    comment_id: i64,
    user: &CurrentUser,
) -> Result<(Comment, Access), AppError> {
    let comment = db::get(&state.db, comment_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (_, access) = load_for(&state.db, comment.campaign_id, user).await?;
    Ok((comment, access))
}

fn render_comment(state: &AppState, c: &CommentView) -> Result<Html<String>, AppError> {
    state.render("comments/_comment.html", context! { c => Serde(c) })
}
