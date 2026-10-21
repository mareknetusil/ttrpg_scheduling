use super::{
    db,
    model::{DeleteQuery, FlagForm, NewUserForm, PasswordForm, UserRow},
};
use crate::{AppState, auth::extractor::CurrentUser, auth::password, campaigns, error::AppError};
use axum::{
    extract::{Form, Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::{context, value::Serde};
use std::sync::Arc;

/// Admin pages are invisible to everyone else (404, like a campaign you're not in).
fn require_admin(user: &CurrentUser) -> Result<(), AppError> {
    if user.is_admin {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

pub async fn index(user: CurrentUser) -> Result<Redirect, AppError> {
    require_admin(&user)?;
    Ok(Redirect::to("/admin/users"))
}

pub async fn users(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
) -> Result<Html<String>, AppError> {
    require_admin(&user)?;
    render_users(&state, &user, None, None, None).await
}

pub async fn create_user(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Form(mut form): Form<NewUserForm>,
) -> Result<Response, AppError> {
    require_admin(&user)?;
    if let Some(error) = form.error() {
        return Ok(render_users(&state, &user, Some(&form), Some(error), None)
            .await?
            .into_response());
    }
    let hash = password::hash(form.password.clone()).await?;
    match db::create_user(&state.db, &form.username, &form.display_name, &hash).await {
        Ok(()) => {}
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => {
            return Ok(render_users(
                &state,
                &user,
                Some(&form),
                Some("That username is taken."),
                None,
            )
            .await?
            .into_response());
        }
        Err(e) => return Err(e.into()),
    }
    let notice = format!(
        "Created {}. Send them the username and password.",
        form.username
    );
    Ok(render_users(&state, &user, None, None, Some(&notice))
        .await?
        .into_response())
}

/// htmx: make or remove an admin; answers with the user's row. Not for yourself, so the site
/// can't end up without an admin by accident.
pub async fn set_admin(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(form): Form<FlagForm>,
) -> Result<Html<String>, AppError> {
    require_admin(&user)?;
    let target = load_other(&state, &user, id).await?;
    db::set_admin(&state.db, target.id, form.value == 1).await?;
    render_row(&state, &user, id, None, None).await
}

/// htmx: deactivate or reactivate an account (never yourself); answers with the user's row.
pub async fn set_active(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(form): Form<FlagForm>,
) -> Result<Html<String>, AppError> {
    require_admin(&user)?;
    let target = load_other(&state, &user, id).await?;
    db::set_active(&state.db, target.id, form.value == 1).await?;
    render_row(&state, &user, id, None, None).await
}

/// htmx: set a new password (also for yourself); answers with the user's row and a note.
pub async fn set_password(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(form): Form<PasswordForm>,
) -> Result<Html<String>, AppError> {
    require_admin(&user)?;
    db::user(&state.db, id).await?.ok_or(AppError::NotFound)?;
    if let Some(error) = crate::auth::validate::password(&form.password) {
        return render_row(&state, &user, id, None, Some(error)).await;
    }
    let hash = password::hash(form.password).await?;
    db::set_password(&state.db, id, &hash).await?;
    let notice = if id == user.id {
        "Password changed. Log in again with the new one."
    } else {
        "Password changed; they've been logged out everywhere."
    };
    render_row(&state, &user, id, Some(notice), None).await
}

pub async fn campaigns(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
) -> Result<Html<String>, AppError> {
    require_admin(&user)?;
    let campaigns = db::campaigns(&state.db).await?;
    state.render(
        "admin/campaigns.html",
        context! { current_user => Serde(&user), campaigns => Serde(&campaigns) },
    )
}

pub async fn delete_campaign_page(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    require_admin(&user)?;
    let campaign = campaigns::db::get(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let contents = db::contents(&state.db, id).await?;
    state.render(
        "admin/delete_campaign.html",
        context! {
            current_user => Serde(&user),
            campaign => Serde(&campaign),
            contents => Serde(&contents),
        },
    )
}

/// htmx `DELETE`. The safety net: `confirm_name` must be the campaign's exact name. On a mismatch
/// the answer is an error message (swapped into the form); on success `HX-Redirect` to the list.
pub async fn delete_campaign(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<DeleteQuery>,
) -> Result<Response, AppError> {
    require_admin(&user)?;
    let campaign = campaigns::db::get(&state.db, id)
        .await?
        .ok_or(AppError::NotFound)?;
    if query.confirm_name.trim() != campaign.name {
        return Ok(Html(
            r#"<p class="alert alert--error" role="alert">The name doesn't match. Nothing was deleted.</p>"#,
        )
        .into_response());
    }
    campaigns::db::delete(&state.db, id).await?;
    tracing::warn!(campaign_id = id, name = %campaign.name, admin = %user.username, "campaign deleted");
    Ok([("HX-Redirect", "/admin/campaigns")].into_response())
}

/// Another user (the toggles refuse yourself).
async fn load_other(state: &AppState, user: &CurrentUser, id: i64) -> Result<UserRow, AppError> {
    if id == user.id {
        return Err(AppError::NotFound);
    }
    db::user(&state.db, id).await?.ok_or(AppError::NotFound)
}

async fn render_users(
    state: &AppState,
    user: &CurrentUser,
    form: Option<&NewUserForm>,
    error: Option<&str>,
    notice: Option<&str>,
) -> Result<Html<String>, AppError> {
    let users = db::users(&state.db).await?;
    state.render(
        "admin/users.html",
        context! {
            current_user => Serde(user),
            users => Serde(&users),
            form => Serde(form),
            error,
            notice,
        },
    )
}

async fn render_row(
    state: &AppState,
    user: &CurrentUser,
    id: i64,
    notice: Option<&str>,
    error: Option<&str>,
) -> Result<Html<String>, AppError> {
    let u = db::user(&state.db, id).await?.ok_or(AppError::NotFound)?;
    state.render(
        "admin/_user_row.html",
        context! { u => Serde(&u), me => user.id, notice, error },
    )
}
