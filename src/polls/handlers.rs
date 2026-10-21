use super::{
    db,
    model::{NewPollForm, NewPollQuery, OptionView, Poll, PollView, VoteForm},
};
use crate::{
    AppState,
    auth::extractor::CurrentUser,
    campaigns::{
        load_for,
        model::{Access, Campaign},
    },
    error::AppError,
    game_sessions::{self, model::SessionView},
    markdown,
    time::When,
};
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::{context, value::Serde};
use std::sync::Arc;

/// The polls of a campaign or game session, prepared for the page that shows them.
pub async fn views(
    state: &AppState,
    campaign_id: i64,
    session_id: Option<i64>,
    user: &CurrentUser,
    access: &Access,
) -> Result<Vec<PollView>, AppError> {
    let mut views = Vec::new();
    for poll in db::list(&state.db, campaign_id, session_id).await? {
        views.push(view(state, &poll, user, access).await?);
    }
    Ok(views)
}

async fn view(
    state: &AppState,
    poll: &Poll,
    user: &CurrentUser,
    access: &Access,
) -> Result<PollView, AppError> {
    let votes = db::votes(&state.db, poll.id).await?;
    let mut voter_ids: Vec<i64> = votes.iter().map(|v| v.user_id).collect();
    voter_ids.sort_unstable();
    voter_ids.dedup();
    let voter_count = voter_ids.len();
    let options: Vec<OptionView> = db::options(&state.db, poll.id)
        .await?
        .into_iter()
        .map(|o| {
            let voters: Vec<String> = votes
                .iter()
                .filter(|v| v.poll_option_id == o.id)
                .map(|v| v.display_name.clone())
                .collect();
            let mine = votes
                .iter()
                .any(|v| v.poll_option_id == o.id && v.user_id == user.id);
            OptionView {
                id: o.id,
                label: o.label,
                count: voters.len(),
                percent: (voters.len() * 100).checked_div(voter_count).unwrap_or(0),
                voters,
                mine,
            }
        })
        .collect();
    let is_closed = poll.is_closed();
    Ok(PollView {
        id: poll.id,
        question: poll.question.clone(),
        description_html: markdown::render(&poll.description_md),
        multiple_choice: poll.multiple_choice,
        is_closed,
        closes: poll
            .closes_at
            .as_deref()
            .map(|t| When::from_db(t, state.tz)),
        has_voted: voter_ids.contains(&user.id),
        options,
        voter_count,
        can_vote: access.role.is_some() && !is_closed,
        can_manage: access.can_manage(),
    })
}

/// `GET /campaigns/{id}/polls/new[?session_id=…]`
pub async fn new_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<NewPollQuery>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let session = session_in_campaign(&state, &campaign, query.session_id).await?;
    render_new(&state, &user, &campaign, session, None, None)
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    axum_extra::extract::Form(mut form): axum_extra::extract::Form<NewPollForm>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let session = session_in_campaign(&state, &campaign, form.game_session_id).await?;
    let poll = match form.validate(state.tz) {
        Ok(poll) => poll,
        Err(error) => {
            return Ok(
                render_new(&state, &user, &campaign, session, Some(&form), Some(error))?
                    .into_response(),
            );
        }
    };
    let session_id = session.as_ref().map(|s| s.id);
    let poll_id = db::create(&state.db, id, session_id, user.id, &poll).await?;
    let target = match session_id {
        Some(sid) => format!("/sessions/{sid}#poll-{poll_id}"),
        None => format!("/campaigns/{id}#poll-{poll_id}"),
    };
    Ok(Redirect::to(&target).into_response())
}

/// htmx: answers with the re-rendered poll.
pub async fn vote(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
    axum_extra::extract::Form(form): axum_extra::extract::Form<VoteForm>,
) -> Result<Html<String>, AppError> {
    let (poll, access) = load(&state, poll_id, &user).await?;
    if access.role.is_none() {
        return Err(AppError::NotFound);
    }
    if !poll.is_closed() {
        let mut option_ids = form.option_ids;
        if !poll.multiple_choice {
            option_ids.truncate(1);
        }
        db::set_votes(&state.db, poll_id, user.id, &option_ids).await?;
    }
    render_poll(&state, &poll, &user, &access).await
}

/// htmx, GM/admin: answers with the re-rendered (closed) poll.
pub async fn close(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (_, access) = load(&state, poll_id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    db::close(&state.db, poll_id).await?;
    let (poll, access) = load(&state, poll_id, &user).await?;
    render_poll(&state, &poll, &user, &access).await
}

async fn load(
    state: &AppState,
    poll_id: i64,
    user: &CurrentUser,
) -> Result<(Poll, Access), AppError> {
    let poll = db::get(&state.db, poll_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (_, access) = load_for(&state.db, poll.campaign_id, user).await?;
    Ok((poll, access))
}

/// The game session a new poll is for, which must belong to this campaign.
async fn session_in_campaign(
    state: &AppState,
    campaign: &Campaign,
    session_id: Option<i64>,
) -> Result<Option<SessionView>, AppError> {
    let Some(session_id) = session_id else {
        return Ok(None);
    };
    let session = game_sessions::db::get(&state.db, session_id)
        .await?
        .filter(|s| s.campaign_id == campaign.id)
        .ok_or(AppError::NotFound)?;
    Ok(Some(SessionView::new(&session, state.tz)))
}

async fn render_poll(
    state: &AppState,
    poll: &Poll,
    user: &CurrentUser,
    access: &Access,
) -> Result<Html<String>, AppError> {
    let p = view(state, poll, user, access).await?;
    state.render("polls/_poll.html", context! { p => Serde(&p) })
}

fn render_new(
    state: &AppState,
    user: &CurrentUser,
    campaign: &Campaign,
    session: Option<SessionView>,
    form: Option<&NewPollForm>,
    error: Option<&str>,
) -> Result<Html<String>, AppError> {
    state.render(
        "polls/new.html",
        context! {
            current_user => Serde(user),
            campaign => Serde(campaign),
            session => Serde(&session),
            form => Serde(form),
            error,
        },
    )
}
