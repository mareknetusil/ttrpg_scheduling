use crate::{
    AppState,
    auth::{password, session},
    error::AppError,
};
use axum::{
    extract::{Form, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use minijinja::context;
use std::sync::Arc;

#[derive(serde::Deserialize)]
pub struct LoginForm {
    username: String,
    password: String,
}

pub async fn login_page(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    let template = state.templates.get_template("login.html")?;
    let body = template.render(context! {})?;
    Ok(Html(body))
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
        return login_failed(&form.username, &state);
    };

    let token = session::create(&state.db, user_id).await?;
    let cookie = Cookie::build(("session", token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .permanent()
        .build();
    Ok((jar.add(cookie), Redirect::to("/")).into_response())
}

pub async fn logout(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
) -> Result<Response, AppError> {
    if let Some(cookie) = jar.get("session") {
        session::remove(&state.db, cookie.value()).await?;
    }
    Ok((
        jar.remove(Cookie::build("session").path("/")),
        Redirect::to("/login"),
    )
        .into_response())
}

fn login_failed(username: &str, state: &AppState) -> Result<Response, AppError> {
    let body = state
        .templates
        .get_template("login.html")?
        .render(context! {
            error => "Invalid username or password",
            username
        })?;
    Ok(Html(body).into_response())
}
