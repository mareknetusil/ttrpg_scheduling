use super::{
    db,
    model::{
        Answer, DecideForm, GenerateQuery, NewPollForm, PatternForm, PatternView, Poll, PollStatus,
        PollView, VoteForm,
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
    markdown,
    time::When,
};
use axum::{
    extract::{Form, Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
};
use minijinja::{Value, context, value::Serde};
use std::{collections::HashMap, sync::Arc};

/// The new-poll form. With `?pattern_id=…&weeks=…` the dates are filled from that regular slot.
pub async fn new_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<GenerateQuery>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let weeks = query.weeks.unwrap_or(3).clamp(1, 8);
    let mut options = Vec::new();
    if let Some(pattern_id) = query.pattern_id {
        let row = db::pattern(&state.db, id, pattern_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if let Some(pattern) = row.pattern() {
            options = pattern
                .generate(chrono::Utc::now(), weeks, state.tz)
                .into_iter()
                .map(|t| When::new(t, state.tz).input)
                .collect();
        }
    }
    let number = db::next_session_number(&state.db, id).await?;
    let form = NewPollForm {
        title: format!("Session {number}"),
        description_md: String::new(),
        deadline: String::new(),
        options,
    };
    let patterns = pattern_views(&state, id).await?;
    state.render(
        "schedule/new.html",
        context! {
            current_user => Serde(&user),
            campaign => Serde(&campaign),
            form => Serde(&form),
            patterns => Serde(&patterns),
            selected_pattern => query.pattern_id,
            weeks,
        },
    )
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
    form.trim();
    let poll = match form.validate(state.tz) {
        Ok(poll) => poll,
        Err(error) => {
            let patterns = pattern_views(&state, id).await?;
            return Ok(state
                .render(
                    "schedule/new.html",
                    context! {
                        current_user => Serde(&user),
                        campaign => Serde(&campaign),
                        form => Serde(&form),
                        patterns => Serde(&patterns),
                        weeks => 3,
                        error
                    },
                )?
                .into_response());
        }
    };
    let poll_id = db::create(&state.db, id, user.id, &poll).await?;
    Ok(Redirect::to(&format!("/schedule/{poll_id}")).into_response())
}

pub async fn show(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    let ctx = poll_context(&state, &user, &poll, &campaign, &access).await?;
    state.render("schedule/show.html", ctx)
}

/// `PUT` via htmx from a vote button; answers with the re-rendered grid (block `grid`), since one
/// vote changes the counts and the "most yes" marker too.
pub async fn vote(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((poll_id, option_id)): Path<(i64, i64)>,
    Form(form): Form<VoteForm>,
) -> Result<Html<String>, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    // Only members vote; an admin who isn't a member can look but not vote.
    if access.role.is_none() {
        return Err(AppError::NotFound);
    }
    if !db::set_vote(&state.db, poll_id, option_id, user.id, form.answer).await? {
        return Err(AppError::NotFound);
    }
    let ctx = poll_context(&state, &user, &poll, &campaign, &access).await?;
    state.render_block("schedule/show.html", "grid", ctx)
}

pub async fn decide(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
    Form(form): Form<DecideForm>,
) -> Result<Response, AppError> {
    let (_, _, access) = load_poll(&state, poll_id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    match db::decide(&state.db, poll_id, form.option_id).await? {
        Some(session_id) => Ok(Redirect::to(&format!("/sessions/{session_id}")).into_response()),
        // Already decided or cancelled (e.g. a double click): show the poll as it is now.
        None => Ok(Redirect::to(&format!("/schedule/{poll_id}")).into_response()),
    }
}

pub async fn cancel(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
) -> Result<Response, AppError> {
    let (_, _, access) = load_poll(&state, poll_id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    db::cancel(&state.db, poll_id).await?;
    Ok(Redirect::to(&format!("/schedule/{poll_id}")).into_response())
}

pub async fn patterns_page(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    render_patterns(&state, &user, &campaign, None, None).await
}

pub async fn create_pattern(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    axum_extra::extract::Form(form): axum_extra::extract::Form<PatternForm>,
) -> Result<Response, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let today = chrono::Utc::now().with_timezone(&state.tz).date_naive();
    match form.validate(today) {
        Ok(pattern) => {
            db::create_pattern(&state.db, id, user.id, &pattern).await?;
            Ok(Redirect::to(&format!("/campaigns/{id}/schedule/patterns")).into_response())
        }
        Err(error) => Ok(
            render_patterns(&state, &user, &campaign, Some(&form), Some(error))
                .await?
                .into_response(),
        ),
    }
}

/// `DELETE` via htmx; an empty 200 lets `hx-swap="delete"` remove the row.
pub async fn delete_pattern(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((id, pattern_id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let (_, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    if db::delete_pattern(&state.db, id, pattern_id).await? == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::OK.into_response())
}

async fn pattern_views(state: &AppState, campaign_id: i64) -> Result<Vec<PatternView>, AppError> {
    Ok(db::patterns(&state.db, campaign_id)
        .await?
        .iter()
        .filter_map(PatternView::new)
        .collect())
}

async fn render_patterns(
    state: &AppState,
    user: &CurrentUser,
    campaign: &Campaign,
    form: Option<&PatternForm>,
    error: Option<&str>,
) -> Result<Html<String>, AppError> {
    let patterns = pattern_views(state, campaign.id).await?;
    let today = chrono::Utc::now().with_timezone(&state.tz).date_naive();
    state.render(
        "schedule/patterns.html",
        context! {
            current_user => Serde(user),
            campaign => Serde(campaign),
            patterns => Serde(&patterns),
            form => Serde(&form),
            today => today.format("%Y-%m-%d").to_string(),
            error,
        },
    )
}

/// Loads a poll and checks the user may see its campaign.
async fn load_poll(
    state: &AppState,
    poll_id: i64,
    user: &CurrentUser,
) -> Result<(Poll, Campaign, Access), AppError> {
    let poll = db::get(&state.db, poll_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let (campaign, access) = load_for(&state.db, poll.campaign_id, user).await?;
    Ok((poll, campaign, access))
}

/// One row of the voting grid.
#[derive(serde::Serialize)]
struct OptionView {
    id: i64,
    when: When,
    yes: usize,
    maybe: usize,
    no: usize,
    /// One answer per member, in the order of `members`
    cells: Vec<Option<Answer>>,
    mine: Option<Answer>,
    is_best: bool,
    is_decided: bool,
}

async fn poll_context(
    state: &AppState,
    user: &CurrentUser,
    poll: &Poll,
    campaign: &Campaign,
    access: &Access,
) -> Result<Value, AppError> {
    let members = campaigns::db::members(&state.db, campaign.id).await?;
    let option_rows = db::options(&state.db, poll.id).await?;
    let mut answers: HashMap<(i64, i64), Answer> = HashMap::new();
    for v in db::votes(&state.db, poll.id).await? {
        answers.insert((v.option_id, v.user_id), v.answer);
    }

    let mut options: Vec<OptionView> = option_rows
        .iter()
        .map(|o| {
            let cells: Vec<Option<Answer>> = members
                .iter()
                .map(|m| answers.get(&(o.id, m.user_id)).copied())
                .collect();
            let count = |a: Answer| cells.iter().filter(|c| **c == Some(a)).count();
            OptionView {
                id: o.id,
                when: When::from_db(&o.starts_at, state.tz),
                yes: count(Answer::Yes),
                maybe: count(Answer::Maybe),
                no: count(Answer::No),
                mine: answers.get(&(o.id, user.id)).copied(),
                cells,
                is_best: false,
                is_decided: poll.decided_option_id == Some(o.id),
            }
        })
        .collect();
    // "Most yes" (ties broken by maybe), only once someone said yes. Ties all get the marker.
    let best = options.iter().map(|o| (o.yes, o.maybe)).max();
    if let Some(best) = best.filter(|(yes, _)| *yes > 0) {
        for o in &mut options {
            o.is_best = (o.yes, o.maybe) == best;
        }
    }
    let unanswered = options.iter().filter(|o| o.mine.is_none()).count();

    Ok(context! {
        current_user => Serde(user),
        campaign => Serde(campaign),
        can_manage => access.can_manage(),
        can_vote => access.role.is_some() && poll.status == PollStatus::Open,
        poll => Serde(PollView::new(poll, state.tz)),
        description_html => markdown::render(&poll.description_md),
        members => Serde(&members),
        options => Serde(&options),
        unanswered,
    })
}
