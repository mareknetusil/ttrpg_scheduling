use crate::{
    AppState,
    auth::{password, session},
    error::AppError,
};
use axum::{
    extract::{Form, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use minijinja::context;
use std::sync::Arc;

#[derive(serde::Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
    next: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct NextQuery {
    next: Option<String>,
}

pub async fn login_page(
    State(state): State<Arc<AppState>>,
    Query(query): Query<NextQuery>,
) -> Result<Html<String>, AppError> {
    let template = state.templates.get_template("login.html")?;
    let body = template.render(context! { next => query.next })?;
    Ok(Html(body))
}

/// Only a local path is a valid redirect target; anything else (`https://…`, `//host`,
/// `/\host`) would be an open redirect.
fn safe_next(next: Option<String>) -> String {
    next.filter(|n| n.starts_with('/') && !n.starts_with("//") && !n.starts_with("/\\"))
        .unwrap_or_else(|| "/".to_string())
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Form(form): Form<LoginForm>,
) -> Result<Response, AppError> {
    let user = sqlx::query!(
        "SELECT id, password_hash FROM users WHERE username = ?",
        form.username
    )
    .fetch_optional(&state.db)
    .await?;

    let valid_user_id = match user {
        Some(user) => password::verify(form.password, user.password_hash)
            .await?
            .then_some(user.id),
        None => None,
    };
    let Some(user_id) = valid_user_id else {
        return login_failed(&form.username, form.next.as_deref(), &state);
    };

    let token = session::create(&state.db, user_id).await?;
    let target = safe_next(form.next);
    Ok((jar.add(session::cookie(token)), Redirect::to(&target)).into_response())
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<(CookieJar, Redirect), AppError> {
    if let Some(cookie) = jar.get("session") {
        session::remove(&state.db, cookie.value()).await?;
    }
    Ok((
        jar.remove(Cookie::build("session").path("/")),
        Redirect::to("/login"),
    ))
}

fn login_failed(
    username: &str,
    next: Option<&str>,
    state: &AppState,
) -> Result<Response, AppError> {
    let body = state
        .templates
        .get_template("login.html")?
        .render(context! {
            error => "Invalid username or password",
            username,
            next
        })?;
    Ok(Html(body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_must_be_a_local_path() {
        let next = |s: &str| safe_next(Some(s.to_string()));
        assert_eq!(next("/invite/abc"), "/invite/abc");
        assert_eq!(next("//evil.example"), "/");
        assert_eq!(next("/\\evil.example"), "/");
        assert_eq!(next("https://evil.example"), "/");
        assert_eq!(safe_next(None), "/");
    }
}
