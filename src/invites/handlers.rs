use super::{
    db,
    model::{JoinOutcome, RegisterForm},
};
use crate::{
    AppState,
    auth::{extractor::CurrentUser, password, session},
    campaigns,
    error::AppError,
};
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use minijinja::{context, value::Serde};
use std::sync::Arc;

pub async fn show(
    State(state): State<Arc<AppState>>,
    user: Option<CurrentUser>,
    Path(token): Path<String>,
) -> Result<Response, AppError> {
    let invite = db::find(&state.db, &token).await?;
    let already_member = if let (Some(user), Some(invite)) = (&user, &invite) {
        campaigns::db::is_member(&state.db, invite.campaign_id, user.id).await?
    } else {
        false
    };
    let template = state.templates.get_template("invite.html")?;
    let body = template.render(context! {
        current_user => Serde(&user),
        invite => Serde(&invite),
        already_member,
        token
    })?;
    if invite.is_none() {
        return Ok((StatusCode::NOT_FOUND, Html(body)).into_response());
    }
    Ok(Html(body).into_response())
}

pub async fn join(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(token): Path<String>,
) -> Result<Response, AppError> {
    match db::join(&state.db, &token, user.id).await? {
        JoinOutcome::Joined(id) | JoinOutcome::AlreadyMember(id) => {
            Ok(Redirect::to(&format!("/campaigns/{id}")).into_response())
        }
        JoinOutcome::InvalidInvite => Ok(Redirect::to(&format!("/invite/{token}")).into_response()),
    }
}

pub async fn register(
    State(state): State<Arc<AppState>>,
    user: Option<CurrentUser>,
    jar: CookieJar,
    Path(token): Path<String>,
    Form(mut form): Form<RegisterForm>,
) -> Result<Response, AppError> {
    if user.is_some() {
        return Ok(Redirect::to(&format!("/invite/{token}")).into_response());
    }
    form.trim();
    if let Some(error) = form.error() {
        return register_failed(&state, &token, &form, error).await;
    }
    let password_hash = password::hash(form.password.clone()).await?;
    let registered = db::register(
        &state.db,
        &token,
        &form.username,
        &form.display_name,
        &password_hash,
    )
    .await;
    let (user_id, campaign_id) = match registered {
        Ok(Some(ids)) => ids,
        Ok(None) => return Ok(Redirect::to(&format!("/invite/{token}")).into_response()),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return register_failed(&state, &token, &form, "That username is taken.").await;
        }
        Err(e) => return Err(e.into()),
    };
    let session_token = session::create(&state.db, user_id).await?;
    Ok((
        jar.add(session::cookie(session_token)),
        Redirect::to(&format!("/campaigns/{campaign_id}")),
    )
        .into_response())
}

/// Re-renders the register form with an error. An invite that became invalid meanwhile gets the
/// "not valid" page instead.
async fn register_failed(
    state: &AppState,
    token: &str,
    form: &RegisterForm,
    error: &str,
) -> Result<Response, AppError> {
    let invite = db::find(&state.db, token).await?;
    let template = state.templates.get_template("invite.html")?;
    let body = template.render(context! {
        invite => Serde(&invite),
        token,
        form => Serde(form),
        error
    })?;
    if invite.is_none() {
        return Ok((StatusCode::NOT_FOUND, Html(body)).into_response());
    }
    Ok(Html(body).into_response())
}
