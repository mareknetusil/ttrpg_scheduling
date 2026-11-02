use super::{
    db,
    model::{
        Answer, DecideForm, Layout, NewPollForm, Poll, PollStatus, PollView, VoteForm,
        calendar_weeks, usual_time,
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
    time::{self, When},
};
use axum::{
    extract::{Form, Path, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use chrono::{Datelike, NaiveDate};
use chrono_tz::Tz;
use minijinja::{Value, context, value::Serde};
use std::{collections::HashMap, sync::Arc};

/// The new-poll form, in the calendar layout.
pub async fn new_form(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (campaign, access) = load_for(&state.db, id, &user).await?;
    if !access.can_manage() {
        return Err(AppError::NotFound);
    }
    // Another open poll is probably for the next session already, so count past it
    let number =
        db::next_session_number(&state.db, id).await? + db::open_poll_count(&state.db, id).await?;
    let form = NewPollForm {
        title: format!("Session {number}"),
        description_md: String::new(),
        layout: Layout::Calendar,
        deadline: String::new(),
        options: Vec::new(),
    };
    render_form(&state, &user, &campaign, None, &form, None).await
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
            return Ok(
                render_form(&state, &user, &campaign, None, &form, Some(error))
                    .await?
                    .into_response(),
            );
        }
    };
    let poll_id = db::create(&state.db, id, user.id, &poll).await?;
    Ok(Redirect::to(&format!("/schedule/{poll_id}")).into_response())
}

pub async fn show(
    State(state): State<Arc<AppState>>,
    mut user: CurrentUser,
    Path(poll_id): Path<i64>,
) -> Result<Html<String>, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    crate::notifications::viewed(
        &state.db,
        &mut user,
        crate::notifications::model::Viewing::SchedulePoll(poll_id),
    )
    .await?;
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
        layout: poll.layout,
        deadline: match &poll.deadline_at {
            Some(d) => When::from_db(d, state.tz).input,
            None => String::new(),
        },
        options,
    };
    Ok(
        render_form(&state, &user, &campaign, Some(&poll), &form, None)
            .await?
            .into_response(),
    )
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
                render_form(&state, &user, &campaign, Some(&poll), &form, Some(error))
                    .await?
                    .into_response(),
            );
        }
    };
    // false: decided or cancelled meanwhile; the poll page shows that
    db::update(&state.db, poll_id, &new_poll, state.tz).await?;
    Ok(Redirect::to(&format!("/schedule/{poll_id}")).into_response())
}

/// `schedule/new.html`: a new poll (`poll` is `None`) or an open poll being edited.
async fn render_form(
    state: &AppState,
    user: &CurrentUser,
    campaign: &Campaign,
    poll: Option<&Poll>,
    form: &NewPollForm,
    error: Option<String>,
) -> Result<Html<String>, AppError> {
    // Answers per date, keyed like the form's datetime-local values
    let mut votes: HashMap<String, i64> = HashMap::new();
    if let Some(poll) = poll {
        for (starts_at, count) in db::vote_counts(&state.db, poll.id).await? {
            votes.insert(When::from_db(&starts_at, state.tz).input, count);
        }
    }
    // The calendar's start time: the usual time of the dates in the form, else of the campaign's
    // previous poll, else 18:00
    let form_times: Vec<&str> = form.options.iter().filter_map(|o| o.get(11..16)).collect();
    let previous: Vec<String> = db::latest_poll_starts(&state.db, campaign.id)
        .await?
        .iter()
        .map(|s| When::from_db(s, state.tz).time)
        .collect();
    let previous: Vec<&str> = previous.iter().map(String::as_str).collect();
    let start_time = usual_time(&form_times)
        .or_else(|| usual_time(&previous))
        .unwrap_or_else(|| "18:00".to_string());
    state.render(
        "schedule/new.html",
        context! {
            current_user => Serde(user),
            campaign => Serde(campaign),
            poll => Serde(poll.map(|p| PollView::new(p, state.tz))),
            form => Serde(form),
            votes => Serde(&votes),
            start_time,
            today => today(state.tz).format("%Y-%m-%d").to_string(),
            error,
        },
    )
}

/// Today in the group's time zone.
fn today(tz: Tz) -> NaiveDate {
    chrono::Utc::now().with_timezone(&tz).date_naive()
}

/// The local day of a DB timestamp. A malformed value (which the app never writes) counts as
/// today rather than failing the page.
fn local_day(starts_at: &str, tz: Tz) -> NaiveDate {
    match time::from_db(starts_at) {
        Some(t) => t.with_timezone(&tz).date_naive(),
        None => today(tz),
    }
}

/// `DELETE` via htmx (clicking your current answer again): back to "no answer". Answers like
/// `vote`.
pub async fn clear_vote(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((poll_id, option_id)): Path<(i64, i64)>,
) -> Result<Response, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    if access.role.is_none() {
        return Err(AppError::NotFound);
    }
    if !db::clear_vote(&state.db, poll_id, option_id, user.id).await? {
        let options = db::options(&state.db, poll_id).await?;
        if !options.iter().any(|o| o.id == option_id) {
            return Err(AppError::NotFound);
        }
        return Ok([("HX-Refresh", "true")].into_response());
    }
    let ctx = poll_context(&state, &user, &poll, &campaign, &access).await?;
    Ok(state
        .render_block("schedule/show.html", "grid", ctx)?
        .into_response())
}

/// `PUT` via htmx, field `answer`: answers every date the user hasn't answered yet.
pub async fn vote_unanswered(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
    Form(form): Form<VoteForm>,
) -> Result<Response, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    if access.role.is_none() {
        return Err(AppError::NotFound);
    }
    if !db::set_unanswered(&state.db, poll_id, user.id, form.answer).await? {
        return Ok([("HX-Refresh", "true")].into_response());
    }
    let ctx = poll_context(&state, &user, &poll, &campaign, &access).await?;
    Ok(state
        .render_block("schedule/show.html", "grid", ctx)?
        .into_response())
}

/// `PUT` via htmx from a day of a calendar poll, field `answer` (the brush): sets that answer, or
/// clears it if the day had it already. Answers like `vote`.
pub async fn paint(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((poll_id, option_id)): Path<(i64, i64)>,
    Form(form): Form<VoteForm>,
) -> Result<Response, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    if access.role.is_none() {
        return Err(AppError::NotFound);
    }
    let options = db::options(&state.db, poll_id).await?;
    if !options.iter().any(|o| o.id == option_id) {
        return Err(AppError::NotFound);
    }
    if !db::paint(&state.db, poll_id, user.id, &[option_id], form.answer).await? {
        return Ok([("HX-Refresh", "true")].into_response());
    }
    let ctx = poll_context(&state, &user, &poll, &campaign, &access).await?;
    Ok(state
        .render_block("schedule/show.html", "grid", ctx)?
        .into_response())
}

/// `PUT` via htmx from a weekday header of a calendar poll (`weekday` 0 = Monday … 6 = Sunday),
/// field `answer`: the same as `paint` for every upcoming date on that weekday at once.
pub async fn paint_weekday(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path((poll_id, weekday)): Path<(i64, u32)>,
    Form(form): Form<VoteForm>,
) -> Result<Response, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    if access.role.is_none() || weekday > 6 {
        return Err(AppError::NotFound);
    }
    let option_ids: Vec<i64> = db::options(&state.db, poll_id)
        .await?
        .iter()
        .filter(|o| {
            local_day(&o.starts_at, state.tz)
                .weekday()
                .num_days_from_monday()
                == weekday
        })
        .map(|o| o.id)
        .collect();
    if !db::paint(&state.db, poll_id, user.id, &option_ids, form.answer).await? {
        return Ok([("HX-Refresh", "true")].into_response());
    }
    let ctx = poll_context(&state, &user, &poll, &campaign, &access).await?;
    Ok(state
        .render_block("schedule/show.html", "grid", ctx)?
        .into_response())
}

/// `GET` via htmx every minute while voting is open: the grid with other members' latest answers.
/// Once voting has closed (decided, cancelled, deadline) the whole page reloads instead, so its
/// banner updates too; the reloaded grid no longer polls.
pub async fn grid(
    State(state): State<Arc<AppState>>,
    user: CurrentUser,
    Path(poll_id): Path<i64>,
) -> Result<Response, AppError> {
    let (poll, campaign, access) = load_poll(&state, poll_id, &user).await?;
    if !poll.voting_open() {
        return Ok([("HX-Refresh", "true")].into_response());
    }
    let ctx = poll_context(&state, &user, &poll, &campaign, &access).await?;
    Ok(state
        .render_block("schedule/show.html", "grid", ctx)?
        .into_response())
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
    match db::decide(&state.db, poll_id, form.option_id, user.id).await? {
        Some(session_id) => Ok(Redirect::to(&format!("/sessions/{session_id}")).into_response()),
        // Already decided or cancelled (e.g. a double click), or the date has passed: show the
        // poll as it is now.
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
    db::cancel(&state.db, poll_id, user.id).await?;
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
    db::reopen(&state.db, poll_id, user.id).await?;
    Ok(Redirect::to(&format!("/schedule/{poll_id}")).into_response())
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

    // The calendar layout: week rows, the time most dates start at, and which weekday headers
    // have upcoming dates to answer
    let days: Vec<NaiveDate> = option_rows
        .iter()
        .map(|o| local_day(&o.starts_at, state.tz))
        .collect();
    let weeks = match poll.layout {
        Layout::Calendar => calendar_weeks(&days, today(state.tz)),
        Layout::List => Vec::new(),
    };
    let times: Vec<&str> = options.iter().map(|o| o.when.time.as_str()).collect();
    let mut weekday_open = [false; 7];
    for (o, day) in options.iter().zip(&days) {
        if !o.when.is_past {
            weekday_open[day.weekday().num_days_from_monday() as usize] = true;
        }
    }

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
        weeks => Serde(&weeks),
        usual_time => usual_time(&times),
        weekday_open => Serde(&weekday_open),
        waiting_on => Serde(&waiting_on),
        session => Serde(&session),
        can_reopen,
    })
}
