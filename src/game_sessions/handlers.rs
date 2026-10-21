use super::{
    db,
    model::{
        AttendanceForm, Entry, EntryForm, EntryView, GameSession, HistoryView, SessionForm,
        SessionStatus, SessionView,
    },
};
use crate::{
    AppState,
    auth::extractor::CurrentUser,
    campaigns::{
        self, load_for,
        model::{Access, Campaign},
    },
    error::AppError,
};
use axum::{
    extract::{Form, Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::{context, value::Serde};
use std::sync::Arc;

pub async fn show(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (session, campaign, access) = load_session(&state, id, &user).await?;
    render_session(
        &state,
        &user,
        &session,
        &campaign,
        &access,
        Errors::default(),
    )
    .await
}

/// Edit title, date and status (GM/admin).
pub async fn update(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(mut form): Form<SessionForm>,
) -> Result<Response, AppError> {
    let (session, campaign, access) = load_session(&state, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let starts_at = match form.validate(state.tz) {
        Ok(starts_at) => starts_at,
        Err(error) => {
            let errors = Errors {
                session: Some(error),
                session_form: Some(&form),
                ..Errors::default()
            };
            return Ok(
                render_session(&state, &user, &session, &campaign, &access, errors)
                    .await?
                    .into_response(),
            );
        }
    };
    let status = form.status.unwrap_or(session.status);
    db::update(&state.db, id, &form.title, &starts_at, status).await?;
    Ok(Redirect::to(&format!("/sessions/{id}")).into_response())
}

/// Replace the attendance (GM/admin); the checkboxes send `user_ids`.
pub async fn attendance(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    axum_extra::extract::Form(form): axum_extra::extract::Form<AttendanceForm>,
) -> Result<Response, AppError> {
    let (session, _, access) = load_session(&state, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    db::set_attendance(&state.db, id, session.campaign_id, &form.user_ids).await?;
    Ok(Redirect::to(&format!("/sessions/{id}#attendance")).into_response())
}

/// Any member can add an entry (recap, notes, loot, …).
pub async fn create_entry(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(mut form): Form<EntryForm>,
) -> Result<Response, AppError> {
    let (session, campaign, access) = load_session(&state, id, &user).await?;
    if access.role.is_none() {
        return Err(AppError::NotFound);
    }
    form.trim();
    if let Some(error) = form.error() {
        let errors = Errors {
            entry: Some(error),
            entry_form: Some(&form),
            ..Errors::default()
        };
        return Ok(
            render_session(&state, &user, &session, &campaign, &access, errors)
                .await?
                .into_response(),
        );
    }
    let entry_id = db::create_entry(&state.db, id, user.id, &form).await?;
    Ok(Redirect::to(&format!("/sessions/{id}#entry-{entry_id}")).into_response())
}

pub async fn edit_entry_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(entry_id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (entry, session, campaign) = load_entry_for_edit(&state, entry_id, &user).await?;
    let form = EntryForm {
        kind: entry.kind,
        title: entry.title.clone(),
        body_md: entry.body_md.clone(),
    };
    render_entry_form(&state, &user, &entry, &session, &campaign, &form, None)
}

pub async fn update_entry(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(entry_id): Path<i64>,
    Form(mut form): Form<EntryForm>,
) -> Result<Response, AppError> {
    let (entry, session, campaign) = load_entry_for_edit(&state, entry_id, &user).await?;
    form.trim();
    if let Some(error) = form.error() {
        return Ok(render_entry_form(
            &state,
            &user,
            &entry,
            &session,
            &campaign,
            &form,
            Some(error),
        )?
        .into_response());
    }
    db::update_entry(&state.db, entry_id, &form).await?;
    Ok(Redirect::to(&format!("/sessions/{}#entry-{entry_id}", session.id)).into_response())
}

/// `DELETE` via htmx; an empty 200 lets `hx-swap="delete"` remove the entry.
pub async fn delete_entry(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(entry_id): Path<i64>,
) -> Result<Response, AppError> {
    load_entry_for_edit(&state, entry_id, &user).await?;
    db::delete_entry(&state.db, entry_id).await?;
    Ok(StatusCode::OK.into_response())
}

/// `GET /campaigns/{id}/history`: every game session of the campaign, newest first.
pub async fn history(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    render_history(&state, &user, &campaign, &access, None, None).await
}

/// `POST /campaigns/{id}/sessions`: add a session by hand, e.g. one played before the app existed.
pub async fn create(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Form(mut form): Form<SessionForm>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let starts_at = match form.validate(state.tz) {
        Ok(starts_at) => starts_at,
        Err(error) => {
            return Ok(
                render_history(&state, &user, &campaign, &access, Some(&form), Some(error))
                    .await?
                    .into_response(),
            );
        }
    };
    let session_id = db::create(&state.db, id, &form.title, &starts_at).await?;
    Ok(Redirect::to(&format!("/sessions/{session_id}")).into_response())
}

async fn load_session(
    state: &AppState,
    id: i64,
    user: &CurrentUser,
) -> Result<(GameSession, Campaign, Access), AppError> {
    let session = db::get(&state.db, id).await?.ok_or(AppError::NotFound)?;
    let (campaign, access) = load_for(&state.db, session.campaign_id, user).await?;
    Ok((session, campaign, access))
}

/// The author (while still a member) or the GM/admin may edit or delete an entry.
fn can_edit_entry(entry: &Entry, user: &CurrentUser, access: &Access) -> bool {
    access.can_manage() || (access.role.is_some() && entry.author_id == user.id)
}

async fn load_entry_for_edit(
    state: &AppState,
    entry_id: i64,
    user: &CurrentUser,
) -> Result<(Entry, GameSession, Campaign), AppError> {
    let entry = db::entry(&state.db, entry_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (session, campaign, access) = load_session(state, entry.game_session_id, user).await?;
    if !can_edit_entry(&entry, user, &access) {
        return Err(AppError::NotFound);
    }
    Ok((entry, session, campaign))
}

/// Form errors to show on the session page, with the submitted values.
#[derive(Default)]
struct Errors<'a> {
    session: Option<&'a str>,
    session_form: Option<&'a SessionForm>,
    entry: Option<&'a str>,
    entry_form: Option<&'a EntryForm>,
}

async fn render_session(
    state: &AppState,
    user: &CurrentUser,
    session: &GameSession,
    campaign: &Campaign,
    access: &Access,
    errors: Errors<'_>,
) -> Result<Html<String>, AppError> {
    let attendees = db::attendees(&state.db, session.id).await?;
    let members = if access.can_manage() {
        campaigns::db::members(&state.db, campaign.id).await?
    } else {
        Vec::new()
    };
    let attendee_ids: Vec<i64> = attendees.iter().map(|a| a.user_id).collect();
    let entries: Vec<EntryView> = db::entries(&state.db, session.id)
        .await?
        .iter()
        .map(|e| EntryView::new(e, can_edit_entry(e, user, access), state.tz))
        .collect();
    let comments =
        crate::comments::views(state, campaign.id, Some(session.id), user, access).await?;
    let questions = crate::polls::views(state, campaign.id, Some(session.id), user, access).await?;
    state.render(
        "sessions/show.html",
        context! {
            current_user => Serde(user),
            campaign => Serde(campaign),
            can_manage => access.can_manage(),
            can_write => access.role.is_some(),
            session => Serde(SessionView::new(session, state.tz)),
            statuses => Serde([SessionStatus::Planned, SessionStatus::Played, SessionStatus::Cancelled]),
            attendees => Serde(&attendees),
            attendee_ids => Serde(&attendee_ids),
            members => Serde(&members),
            entries => Serde(&entries),
            session_error => errors.session,
            session_form => Serde(errors.session_form),
            entry_error => errors.entry,
            entry_form => Serde(errors.entry_form),
            comments => Serde(&comments),
            questions => Serde(&questions),
        },
    )
}

fn render_entry_form(
    state: &AppState,
    user: &CurrentUser,
    entry: &Entry,
    session: &GameSession,
    campaign: &Campaign,
    form: &EntryForm,
    error: Option<&str>,
) -> Result<Html<String>, AppError> {
    state.render(
        "sessions/entry_edit.html",
        context! {
            current_user => Serde(user),
            campaign => Serde(campaign),
            session => Serde(SessionView::new(session, state.tz)),
            entry_id => entry.id,
            form => Serde(form),
            error,
        },
    )
}

async fn render_history(
    state: &AppState,
    user: &CurrentUser,
    campaign: &Campaign,
    access: &Access,
    form: Option<&SessionForm>,
    error: Option<&str>,
) -> Result<Html<String>, AppError> {
    let sessions: Vec<HistoryView> = db::history(&state.db, campaign.id)
        .await?
        .into_iter()
        .map(|r| HistoryView::new(r, state.tz))
        .collect();
    state.render(
        "sessions/history.html",
        context! {
            current_user => Serde(user),
            campaign => Serde(campaign),
            can_manage => access.can_manage(),
            sessions => Serde(&sessions),
            form => Serde(form),
            error,
        },
    )
}
