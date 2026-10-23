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
    game_sessions::{self, model::SessionView},
    markdown,
    time::When,
};
use axum::{
    extract::{Form, Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json, Redirect, Response},
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
    let options = match query.pattern_id {
        Some(pattern_id) => slot_dates(&state, id, pattern_id, weeks).await?,
        None => Vec::new(),
    };
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

/// `GET /campaigns/{id}/schedule/slot-dates?pattern_id=…&weeks=…`: the dates a regular slot
/// generates, as a JSON list of datetime-local strings. The poll form's "Fill" adds them to its
/// date list without reloading the page.
pub async fn slot_dates_json(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
    Query(query): Query<GenerateQuery>,
) -> Result<Json<Vec<String>>, AppError> {
    let (_, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    let pattern_id = query.pattern_id.ok_or(AppError::BadRequest)?;
    let weeks = query.weeks.unwrap_or(3).clamp(1, 8);
    Ok(Json(slot_dates(&state, id, pattern_id, weeks).await?))
}

/// The next `weeks` weeks of a campaign's regular slot, as datetime-local strings.
async fn slot_dates(
    state: &AppState,
    campaign_id: i64,
    pattern_id: i64,
    weeks: u32,
) -> Result<Vec<String>, AppError> {
    let row = db::pattern(&state.db, campaign_id, pattern_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let Some(pattern) = row.pattern() else {
        return Ok(Vec::new());
    };
    Ok(pattern
        .generate(chrono::Utc::now(), weeks, state.tz)
        .into_iter()
        .map(|t| When::new(t, state.tz).input)
        .collect())
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
    let comments =
        crate::comments::views(&state, campaign.id, None, Some(poll.id), &user, &access).await?;
    state.render(
        "schedule/show.html",
        context! {
            comments => Serde(&comments),
            can_comment => access.role.is_some(),
            ..ctx
        },
    )
}

/// `PUT` via htmx from a vote button; answers with the re-rendered grid (block `grid`), since one
/// vote changes the counts and the "most yes" marker too.
pub async fn vote(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((poll_id, option_id)): Path<(i64, i64)>,
    Form(form): Form<VoteForm>,
) -> Result<Response, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    // Only members vote; an admin who isn't a member can look but not vote.
    if access.role.is_none() {
        return Err(AppError::NotFound);
    }
    if !db::set_vote(&state.db, poll_id, option_id, user.id, form.answer).await? {
        let options = db::options(&state.db, poll_id).await?;
        if !options.iter().any(|o| o.id == option_id) {
            return Err(AppError::NotFound);
        }
        // A real option that can't be voted on any more: since the page was loaded the poll was
        // decided or cancelled, or the deadline or the date passed. htmx reloads the page, which
        // shows why.
        return Ok([("HX-Refresh", "true")].into_response());
    }
    let ctx = poll_context(&state, &user, &poll, &campaign, &access).await?;
    Ok(state
        .render_block("schedule/show.html", "grid", ctx)?
        .into_response())
}

/// The new-poll form, filled with an open poll's current values. Only the GM edits, and only while
/// the poll is open.
pub async fn edit_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
) -> Result<Response, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    if poll.status != PollStatus::Open {
        return Ok(Redirect::to(&format!("/schedule/{poll_id}")).into_response());
    }
    let options = db::options(&state.db, poll_id)
        .await?
        .iter()
        .map(|o| When::from_db(&o.starts_at, state.tz).input)
        .collect();
    let form = NewPollForm {
        title: poll.title.clone(),
        description_md: poll.description_md.clone(),
        deadline: match &poll.deadline_at {
            Some(d) => When::from_db(d, state.tz).input,
            None => String::new(),
        },
        options,
    };
    Ok(render_edit(&state, &user, &poll, &campaign, &form, None)
        .await?
        .into_response())
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
    axum_extra::extract::Form(mut form): axum_extra::extract::Form<NewPollForm>,
) -> Result<Response, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    form.trim();
    let new_poll = match form.validate(state.tz) {
        Ok(new_poll) => new_poll,
        Err(error) => {
            return Ok(
                render_edit(&state, &user, &poll, &campaign, &form, Some(error))
                    .await?
                    .into_response(),
            );
        }
    };
    // false: decided or cancelled meanwhile; the poll page shows that
    db::update(&state.db, poll_id, &new_poll).await?;
    Ok(Redirect::to(&format!("/schedule/{poll_id}")).into_response())
}

/// `schedule/new.html` in edit mode (`poll` is set).
async fn render_edit(
    state: &AppState,
    user: &CurrentUser,
    poll: &Poll,
    campaign: &Campaign,
    form: &NewPollForm,
    error: Option<String>,
) -> Result<Html<String>, AppError> {
    // Answers per date, keyed like the form's datetime-local values
    let votes: HashMap<String, i64> = db::vote_counts(&state.db, poll.id)
        .await?
        .into_iter()
        .map(|(starts_at, count)| (When::from_db(&starts_at, state.tz).input, count))
        .collect();
    let patterns = pattern_views(state, campaign.id).await?;
    state.render(
        "schedule/new.html",
        context! {
            current_user => Serde(user),
            campaign => Serde(campaign),
            poll => Serde(PollView::new(poll, state.tz)),
            form => Serde(form),
            votes => Serde(&votes),
            patterns => Serde(&patterns),
            weeks => 3,
            error,
        },
    )
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

/// Undo a decision (GM). If the session can't be removed any more, the poll page simply shows it
/// again (the Reopen button is only offered while it can).
pub async fn reopen(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
) -> Result<Response, AppError> {
    let (_, _, access) = load_poll(&state, poll_id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    db::reopen(&state.db, poll_id).await?;
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
    /// Every member answered, and nobody said no
    no_conflicts: bool,
    is_decided: bool,
}

/// A member who hasn't answered every upcoming date yet.
#[derive(serde::Serialize)]
struct WaitingOn {
    display_name: String,
    /// How many upcoming dates they haven't answered
    unanswered: usize,
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
                no_conflicts: !members.is_empty()
                    && cells
                        .iter()
                        .all(|c| matches!(c, Some(Answer::Yes | Answer::Maybe))),
                cells,
                is_best: false,
                is_decided: poll.decided_option_id == Some(o.id),
            }
        })
        .collect();
    // "Most yes" (ties broken by maybe) among the dates still ahead, only once someone said yes.
    // Ties all get the marker.
    let best = options
        .iter()
        .filter(|o| !o.when.is_past)
        .map(|o| (o.yes, o.maybe))
        .max();
    if let Some(best) = best.filter(|(yes, _)| *yes > 0) {
        for o in &mut options {
            o.is_best = !o.when.is_past && (o.yes, o.maybe) == best;
        }
    }
    for o in &mut options {
        o.no_conflicts = o.no_conflicts && !o.when.is_past;
    }
    let upcoming = options.iter().filter(|o| !o.when.is_past).count();
    let waiting_on: Vec<WaitingOn> = members
        .iter()
        .enumerate()
        .map(|(i, m)| WaitingOn {
            display_name: m.display_name.clone(),
            unanswered: options
                .iter()
                .filter(|o| !o.when.is_past && o.cells[i].is_none())
                .count(),
        })
        .filter(|w| w.unanswered > 0)
        .collect();
    // A decided poll shows the game session as it is now (the GM may have moved or cancelled it).
    let mut session = None;
    let mut can_reopen = false;
    if let Some(session_id) = poll.game_session_id
        && let Some(s) = game_sessions::db::get(&state.db, session_id).await?
    {
        can_reopen = access.can_manage()
            && db::session_untouched(&mut *state.db.acquire().await?, session_id).await?;
        session = Some(SessionView::new(&s, state.tz));
    }
    let unanswered = options
        .iter()
        .filter(|o| !o.when.is_past && o.mine.is_none())
        .count();

    Ok(context! {
        current_user => Serde(user),
        campaign => Serde(campaign),
        can_manage => access.can_manage(),
        can_vote => access.role.is_some() && poll.voting_open(),
        poll => Serde(PollView::new(poll, state.tz)),
        description_html => markdown::render(&poll.description_md),
        members => Serde(&members),
        options => Serde(&options),
        upcoming,
        unanswered,
        waiting_on => Serde(&waiting_on),
        session => Serde(&session),
        can_reopen,
    })
}
