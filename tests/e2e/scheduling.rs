use crate::common::{TestApp, User, spawn_app};
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

/// A campaign with a GM and two members.
async fn setup(app: &TestApp) -> (i64, User, User, User) {
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    let nina = app.user("nina").await;
    app.add_member(id, &mark).await;
    app.add_member(id, &nina).await;
    (id, gm, mark, nina)
}

async fn vote(user: &User, poll: i64, option: i64, answer: &str) -> crate::common::Resp {
    let path = format!("/schedule/{poll}/options/{option}/vote");
    user.put(&path, &[("answer", answer)]).await
}

async fn option_ids(app: &TestApp, poll_id: i64) -> Vec<i64> {
    sqlx::query_scalar("SELECT id FROM schedule_options WHERE poll_id = ? ORDER BY starts_at")
        .bind(poll_id)
        .fetch_all(&app.db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn new_poll_validation(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, _, _) = setup(&app).await;
    let path = format!("/campaigns/{id}/schedule");
    let r = gm.get(&format!("{path}/new")).await;
    assert_eq!(r.ok().attr("#title", "value").as_deref(), Some("Session 1"));
    // a new poll starts as a calendar, at 18:00 (no previous poll)
    let alpine = r.attr("form.form", "x-data").unwrap();
    assert!(
        alpine.starts_with(r#"pollForm([], {}, "calendar", ""#),
        "{alpine}"
    );
    assert!(alpine.ends_with(r#", "18:00")"#), "{alpine}");

    let r = gm.post(&path, &[("title", "S1"), ("options", "")]).await;
    assert_eq!(r.error(), "Add at least one date.");
    let r = gm
        .post(&path, &[("title", "S1"), ("options", "2026-11-01T25:00")])
        .await;
    assert!(r.error().contains("isn't a valid date and time"));
    // the entered dates come back to Alpine as JSON; a form without a layout is a list
    assert!(
        r.attr("form.form", "x-data")
            .unwrap()
            .starts_with(r#"pollForm(["2026-11-01T25:00"], {}, "list", "#)
    );
}

#[sqlx::test]
async fn vote_and_decide(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, nina) = setup(&app).await;
    let r = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "Session 1"),
                ("description_md", "Pick **one**"),
                ("deadline", "2030-01-01T20:00"),
                ("options", "2030-01-11T18:00"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-10T18:00"),
            ],
        )
        .await;
    let poll = r.redirect_id("/schedule/");
    let opts = option_ids(&app, poll).await;
    assert_eq!(opts.len(), 2, "duplicates are dropped");

    let page = mark.get(&format!("/schedule/{poll}")).await;
    assert_eq!(page.ok().count(".vote-grid tbody tr"), 2);
    // Prague is UTC+1 in January; shown in local time
    assert_eq!(
        page.text(".vote-grid tbody tr:first-child .vote-grid__date time"),
        "Thu 10 Jan"
    );
    assert!(
        page.text(".page-head .muted")
            .contains("Tue 1 Jan 2030, 20:00")
    );
    assert!(page.body.contains("<strong>one</strong>"));

    // votes answer with the grid block only
    let r = vote(&mark, poll, opts[0], "yes").await;
    assert!(
        r.ok()
            .body
            .trim_start()
            .starts_with("<div id=\"vote-grid\"")
    );
    assert!(!r.body.contains("<html"));
    vote(&nina, poll, opts[0], "maybe").await.ok();
    vote(&nina, poll, opts[0], "yes").await.ok(); // changing a vote
    vote(&gm, poll, opts[1], "no").await.ok();
    let r = vote(&mark, poll, opts[1], "maybe").await;
    assert_eq!(
        r.attr(&format!("#vote-{}-yes", opts[0]), "aria-pressed")
            .as_deref(),
        Some("true")
    );
    assert_eq!(r.text("tbody tr.is-best .badge"), "Most yes");
    assert_eq!(app.scalar("SELECT COUNT(*) FROM schedule_votes").await, 4);

    // bad input
    assert_eq!(
        vote(&mark, poll, opts[0], "sure").await.status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        vote(&mark, poll, 9999, "yes").await.status,
        StatusCode::NOT_FOUND
    );
    // another poll's date, through this poll's URL
    let other = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "Other"), ("options", "2030-02-01T18:00")],
        )
        .await
        .redirect_id("/schedule/");
    let other_opt = option_ids(&app, other).await[0];
    assert_eq!(
        vote(&mark, poll, other_opt, "yes").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mark.delete(&format!("/schedule/{poll}/options/{other_opt}/vote"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.scalar("SELECT COUNT(*) FROM schedule_votes WHERE option_id = (SELECT MAX(id) FROM schedule_options)")
            .await,
        0
    );

    // only the GM decides; it creates session 1 with the yes voters attending
    let decide = format!("/schedule/{poll}/decide");
    let first = opts[0].to_string();
    assert_eq!(
        mark.post(&decide, &[("option_id", &first)]).await.status,
        StatusCode::NOT_FOUND
    );
    let session = gm
        .post(&decide, &[("option_id", &first)])
        .await
        .redirect_id("/sessions/");
    let r = mark.get(&format!("/sessions/{session}")).await;
    assert_eq!(r.ok().text("h1"), "Session 1");
    assert_eq!(r.texts("#attendance .chip"), ["Mark", "Nina"]);

    // decided: no more votes or decisions, the poll page says so. A vote from a page loaded
    // before the decision makes htmx reload it.
    assert!(vote(&mark, poll, opts[1], "yes").await.ok().hx_refresh);
    let second = opts[1].to_string();
    assert_eq!(
        gm.post(&decide, &[("option_id", &second)]).await.redirect(),
        format!("/schedule/{poll}")
    );
    let r = mark.get(&format!("/schedule/{poll}")).await;
    assert!(r.text(".decided").contains("Thu 10 Jan 2030, 18:00"));
    assert!(!r.has(".vote-btn"));
    assert_eq!(app.scalar("SELECT COUNT(*) FROM game_sessions").await, 1);

    // dashboard and campaign page show the upcoming session
    assert!(
        mark.get("/")
            .await
            .text(".item-list")
            .contains("Camp · session 1")
    );
}

#[sqlx::test]
async fn outsiders_and_site_admins(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "S"), ("options", "2030-01-10T18:00")],
        )
        .await
        .redirect_id("/schedule/");
    let opt = option_ids(&app, poll).await[0];
    let vote = format!("/schedule/{poll}/options/{opt}/vote");

    let olga = app.user("olga").await;
    assert_eq!(
        olga.get(&format!("/schedule/{poll}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        olga.put(&vote, &[("answer", "yes")]).await.status,
        StatusCode::NOT_FOUND
    );

    // a site admin can look, but neither vote nor cancel
    let ada = app.admin("ada").await;
    assert!(
        !ada.get(&format!("/schedule/{poll}"))
            .await
            .ok()
            .has(".vote-btn")
    );
    assert_eq!(
        ada.put(&vote, &[("answer", "yes")]).await.status,
        StatusCode::NOT_FOUND
    );

    // dashboard and campaign page nudge members who haven't voted
    let r = mark.get("/").await;
    assert_eq!(r.texts("#to-vote .action-link strong"), ["S"]);
    assert!(r.text("#to-vote").contains("Camp · 1 of 1 date to answer"));
    assert_eq!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .text(".to-answer"),
        "1 to answer"
    );

    // cancel
    assert_eq!(
        mark.post(&format!("/schedule/{poll}/cancel"), &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ada.post(&format!("/schedule/{poll}/cancel"), &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    gm.post(&format!("/schedule/{poll}/cancel"), &[])
        .await
        .redirect();
    assert_eq!(
        mark.get(&format!("/schedule/{poll}")).await.text(".alert"),
        "This poll was cancelled."
    );
    assert!(!mark.get("/").await.has("#to-vote"));
}

/// A calendar poll for 2030: Thu 10 and Fri 11 Jan at 18:00, Sun 13 Jan at 14:00 (one week).
async fn calendar_poll(gm: &User, campaign: i64) -> i64 {
    gm.post(
        &format!("/campaigns/{campaign}/schedule"),
        &[
            ("title", "S1"),
            ("layout", "calendar"),
            ("options", "2030-01-10T18:00"),
            ("options", "2030-01-11T18:00"),
            ("options", "2030-01-13T14:00"),
        ],
    )
    .await
    .redirect_id("/schedule/")
}

async fn paint(user: &User, poll: i64, option: i64, answer: &str) -> crate::common::Resp {
    let path = format!("/schedule/{poll}/options/{option}/paint");
    user.put(&path, &[("answer", answer)]).await
}

async fn answers(app: &TestApp, user: &User) -> Vec<(i64, String)> {
    sqlx::query_as(
        "SELECT option_id, answer FROM schedule_votes WHERE user_id = ? ORDER BY option_id",
    )
    .bind(user.id)
    .fetch_all(&app.db)
    .await
    .unwrap()
}

#[sqlx::test]
async fn calendar_poll_shows_weeks(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let poll = calendar_poll(&gm, id).await;
    assert_eq!(
        app.scalar("SELECT COUNT(*) FROM schedule_polls WHERE layout = 'calendar'")
            .await,
        1
    );

    let r = mark.get(&format!("/schedule/{poll}")).await;
    // one week, Mon 7 to Sun 13 Jan; the usual time once, the other time on its day
    assert_eq!(r.ok().count(".cal--vote tbody tr"), 1);
    assert_eq!(
        r.texts(".cal--vote tbody .cal__label"),
        ["7 Jan", "8", "9", "10", "11", "12", "13"]
    );
    assert_eq!(r.text(".cal-note strong"), "18:00");
    assert_eq!(r.texts(".cal--vote .cal__time"), ["14:00"]);
    assert_eq!(r.count(".cal__day--empty"), 4);
    // the brush starts on yes; only weekdays with dates can be tapped
    assert_eq!(r.attr("#brush-yes", "checked").as_deref(), Some(""));
    assert!(r.has("#fill-rest"));
    for (weekday, tappable) in [(0, false), (3, true), (4, true), (6, true)] {
        assert_eq!(r.has(&format!("#weekday-{weekday}")), tappable, "{weekday}");
    }
    // under it, everyone's answers, read-only (your own column too)
    assert!(r.has("details.everyone .vote-grid"));
    assert!(!r.has(".vote-grid .vote-btn"));
    assert_eq!(r.texts(".vote-grid__member"), ["Gina", "You", "Nina"]);
    assert!(!r.has(".cal__pick"), "only the GM picks");

    // the GM picks in the calendar (behind "Pick the date" while voting is open)
    let r = gm.get(&format!("/schedule/{poll}")).await;
    assert_eq!(r.ok().count(".cal__pick"), 3);
    assert_eq!(r.count(".vote-grid .pick-form"), 0);
    assert_eq!(r.attr(".cal__pick", "x-show").as_deref(), Some("picking"));

    // a site admin who isn't a member sees the calendar without buttons
    let ada = app.admin("ada").await;
    let r = ada.get(&format!("/schedule/{poll}")).await;
    assert!(r.ok().has(".cal--vote"));
    assert!(!r.has("#brush"));
    assert!(!r.has("button.cal__btn"));

    // a calendar has one time per day
    let r = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S2"),
                ("layout", "calendar"),
                ("options", "2030-01-10T14:00"),
                ("options", "2030-01-10T19:00"),
            ],
        )
        .await;
    assert!(r.error().starts_with("Thu 10 Jan has two times."));
    // the form comes back as a calendar
    assert!(
        r.attr("form.form", "x-data")
            .unwrap()
            .contains(r#"}, "calendar", "#)
    );
}

#[sqlx::test]
async fn the_brush_paints_days_and_weekdays(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let poll = calendar_poll(&gm, id).await;
    let opts = option_ids(&app, poll).await;

    // a day: set, change, and the same answer again clears it
    let r = paint(&mark, poll, opts[0], "yes").await;
    assert!(
        r.ok()
            .body
            .trim_start()
            .starts_with("<div id=\"vote-grid\"")
    );
    assert_eq!(
        r.attr(&format!("#day-{}", opts[0]), "data-mine").as_deref(),
        Some("yes")
    );
    assert_eq!(answers(&app, &mark).await, [(opts[0], "yes".into())]);
    paint(&mark, poll, opts[0], "maybe").await.ok();
    assert_eq!(answers(&app, &mark).await, [(opts[0], "maybe".into())]);
    let r = paint(&mark, poll, opts[0], "maybe").await;
    assert_eq!(
        r.ok()
            .attr(&format!("#day-{}", opts[0]), "data-mine")
            .as_deref(),
        Some("")
    );
    assert!(answers(&app, &mark).await.is_empty());

    // a weekday (4 = Friday): all its dates, and again clears them
    let friday = format!("/schedule/{poll}/weekdays/4");
    mark.put(&friday, &[("answer", "no")]).await.ok();
    assert_eq!(answers(&app, &mark).await, [(opts[1], "no".into())]);
    mark.put(&friday, &[("answer", "no")]).await.ok();
    assert!(answers(&app, &mark).await.is_empty());
    // a weekday without dates is harmless: nothing to answer, so the page reloads
    assert!(
        mark.put(
            &format!("/schedule/{poll}/weekdays/0"),
            &[("answer", "yes")]
        )
        .await
        .ok()
        .hx_refresh
    );
    assert_eq!(
        mark.put(
            &format!("/schedule/{poll}/weekdays/7"),
            &[("answer", "yes")]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // fill the rest with the brush: only the unanswered days
    paint(&mark, poll, opts[0], "yes").await.ok();
    let r = mark
        .put(
            &format!("/schedule/{poll}/vote-unanswered"),
            &[("answer", "maybe")],
        )
        .await;
    assert!(
        r.ok()
            .text(".your-progress")
            .starts_with("You've answered every date.")
    );
    assert_eq!(
        answers(&app, &mark).await,
        [
            (opts[0], "yes".into()),
            (opts[1], "maybe".into()),
            (opts[2], "maybe".into())
        ]
    );

    // the calendar shows the counts and markers: one yes on the 10th, the most
    let r = gm.get(&format!("/schedule/{poll}")).await;
    assert!(r.ok().has(".cal__day.is-best"));
    assert_eq!(r.text(".cal__day.is-best .cal__mark"), "★");

    // outsiders, admins who aren't members, unknown or other polls' dates
    let olga = app.user("olga").await;
    assert_eq!(
        paint(&olga, poll, opts[0], "yes").await.status,
        StatusCode::NOT_FOUND
    );
    let ada = app.admin("ada").await;
    assert_eq!(
        paint(&ada, poll, opts[0], "yes").await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ada.put(&friday, &[("answer", "yes")]).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        paint(&mark, poll, 9999, "yes").await.status,
        StatusCode::NOT_FOUND
    );
    let other = calendar_poll(&gm, id).await;
    let other_opt = option_ids(&app, other).await[0];
    assert_eq!(
        paint(&mark, poll, other_opt, "yes").await.status,
        StatusCode::NOT_FOUND
    );

    // once voting closes, a tap reloads the page
    gm.post(&format!("/schedule/{poll}/cancel"), &[])
        .await
        .redirect();
    assert!(paint(&mark, poll, opts[0], "no").await.ok().hx_refresh);
    assert!(mark.put(&friday, &[("answer", "no")]).await.ok().hx_refresh);
    assert_eq!(answers(&app, &mark).await[0], (opts[0], "yes".into()));
}

#[sqlx::test]
async fn editing_a_calendar_keeps_answers_by_day(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let poll = calendar_poll(&gm, id).await;
    let opts = option_ids(&app, poll).await;
    for opt in &opts {
        paint(&mark, poll, *opt, "yes").await.ok();
    }

    let edit = format!("/schedule/{poll}/edit");
    let alpine = gm
        .get(&edit)
        .await
        .ok()
        .attr("form.form", "x-data")
        .unwrap();
    assert!(alpine.contains(r#"}, "calendar", "#), "{alpine}");
    assert!(
        alpine.ends_with(r#", "18:00")"#),
        "the usual time: {alpine}"
    );

    // the 10th moves to 19:00, the 11th goes, the 12th is new, the 13th stays
    let save = format!("/schedule/{poll}");
    gm.post(
        &save,
        &[
            ("title", "S1"),
            ("layout", "calendar"),
            ("options", "2030-01-10T19:00"),
            ("options", "2030-01-12T18:00"),
            ("options", "2030-01-13T14:00"),
        ],
    )
    .await
    .redirect();
    let new_opts = option_ids(&app, poll).await;
    assert_eq!(new_opts.len(), 3);
    assert_eq!(new_opts[0], opts[0], "the moved day keeps its date");
    assert_eq!(new_opts[2], opts[2]);
    assert_eq!(
        answers(&app, &mark).await,
        [(opts[0], "yes".into()), (opts[2], "yes".into())]
    );
    // three different times: the tie goes to the first date's 19:00, the others show theirs
    let r = mark.get(&save).await;
    assert_eq!(r.ok().text(".cal-note strong"), "19:00");
    assert_eq!(r.texts(".cal--vote .cal__time"), ["18:00", "14:00"]);

    // switched to a list: matched by the exact time, so moving the 10th again loses its answer
    gm.post(
        &save,
        &[
            ("title", "S1"),
            ("layout", "list"),
            ("options", "2030-01-10T20:00"),
            ("options", "2030-01-10T21:00"),
            ("options", "2030-01-13T14:00"),
        ],
    )
    .await
    .redirect();
    assert_eq!(answers(&app, &mark).await, [(opts[2], "yes".into())]);
    assert!(mark.get(&save).await.ok().has(".vote-grid .vote-btn"));
    // back to a calendar is refused while a day has two times
    let r = gm
        .post(
            &save,
            &[
                ("title", "S1"),
                ("layout", "calendar"),
                ("options", "2030-01-10T20:00"),
                ("options", "2030-01-10T21:00"),
            ],
        )
        .await;
    assert!(r.error().contains("has two times"));
    assert_eq!(r.attr("form.form", "action"), Some(save.clone()));

    // a new poll starts at the previous poll's usual time (20:00, 21:00, 14:00: a tie, so the
    // first date's)
    let r = gm.get(&format!("/campaigns/{id}/schedule/new")).await;
    let alpine = r.ok().attr("form.form", "x-data").unwrap();
    assert!(alpine.ends_with(r#", "20:00")"#), "{alpine}");
}

#[sqlx::test]
async fn deadline_closes_voting(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("deadline", "2020-01-01T20:00"),
                ("options", "2030-01-10T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    let opt = option_ids(&app, poll).await[0];

    let r = mark.get(&format!("/schedule/{poll}")).await;
    assert!(
        r.ok()
            .text(".page-head .muted")
            .contains("Voting closed on")
    );
    assert!(r.text(".alert").contains("Waiting for the GM"));
    assert!(!r.has(".vote-btn"));
    let r = vote(&mark, poll, opt, "yes").await;
    assert!(r.ok().hx_refresh);
    assert_eq!(app.scalar("SELECT COUNT(*) FROM schedule_votes").await, 0);
    assert_eq!(
        mark.get("/").await.text(".item-list .muted.small"),
        "voting closed"
    );
    assert!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .body
            .contains("voting closed")
    );

    // the GM can still pick a date
    let r = gm.get(&format!("/schedule/{poll}")).await;
    assert!(r.ok().text(".alert").contains("Pick a date below"));
    assert_eq!(r.count("form[action$='/decide']"), 1);
    // with voting closed the Pick buttons are always shown, without the toggle
    assert!(!r.has("#pick-toggle"));
    assert_eq!(r.attr(".pick-form", "x-show"), None);
}

#[sqlx::test]
async fn past_dates_cant_be_voted_or_picked(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("options", "2020-01-10T18:00"),
                ("options", "2030-01-10T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    let opts = option_ids(&app, poll).await;

    let r = mark.get(&format!("/schedule/{poll}")).await;
    assert_eq!(r.ok().text("tr.is-past td.muted"), "Past");
    assert_eq!(r.count(".vote-grid .vote-btn"), 3, "only the future date");
    assert!(
        r.text(".your-progress")
            .starts_with("You haven't answered 1 of 1 date yet.")
    );
    assert!(vote(&mark, poll, opts[0], "yes").await.ok().hx_refresh);
    // the dashboard's "Your vote is needed" only counts future dates
    vote(&mark, poll, opts[1], "yes").await.ok();
    let r = mark.get("/").await;
    assert!(!r.has("#to-vote"));
    assert_eq!(r.text(".item-list .muted.small"), "answered");

    let r = gm.get(&format!("/schedule/{poll}")).await;
    assert_eq!(
        r.count("form[action$='/decide']"),
        1,
        "no Pick on the past date"
    );
    // "Most yes" ignores past dates
    assert!(!r.has("tr.is-past .badge"));
    // and the backend refuses a crafted Pick of the past date
    let decide = format!("/schedule/{poll}/decide");
    assert_eq!(
        gm.post(&decide, &[("option_id", &opts[0].to_string())])
            .await
            .redirect(),
        format!("/schedule/{poll}")
    );
    assert_eq!(app.scalar("SELECT COUNT(*) FROM game_sessions").await, 0);

    // all dates past
    let old = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "S0"), ("options", "2020-01-10T18:00")],
        )
        .await
        .redirect_id("/schedule/");
    let r = gm.get(&format!("/schedule/{old}")).await;
    assert!(
        r.ok()
            .text(".alert")
            .contains("All the proposed dates have passed.")
    );
}

#[sqlx::test]
async fn waiting_on_and_no_conflicts(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, nina) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-11T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    let opts = option_ids(&app, poll).await;

    let r = mark.get(&format!("/schedule/{poll}")).await;
    assert_eq!(r.ok().text(".waiting-on"), "Waiting on: Gina, Mark, Nina");

    vote(&gm, poll, opts[0], "yes").await.ok();
    vote(&gm, poll, opts[1], "yes").await.ok();
    vote(&mark, poll, opts[0], "maybe").await.ok();
    let r = vote(&nina, poll, opts[0], "maybe").await;
    assert_eq!(
        r.ok().text(".waiting-on"),
        "Waiting on: Mark (1 left), Nina (1 left)"
    );
    // everyone answered the first date, nobody said no
    assert_eq!(
        r.texts("tbody tr:first-child .badge"),
        ["No one said no", "Most yes"]
    );

    vote(&mark, poll, opts[1], "yes").await.ok();
    let r = vote(&nina, poll, opts[1], "no").await;
    assert_eq!(r.ok().text(".waiting-on"), "Everyone has answered.");
    // the second date has more yes but also a "no": the markers point at different dates
    assert_eq!(r.texts("tbody tr:first-child .badge"), ["No one said no"]);
    assert_eq!(r.texts("tbody tr:nth-child(2) .badge"), ["Most yes"]);
}

#[sqlx::test]
async fn edit_an_open_poll(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, nina) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("deadline", "2020-01-01T20:00"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-11T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    let opts = option_ids(&app, poll).await;
    // votes from before the deadline
    for (user, opt) in [(&mark, opts[0]), (&nina, opts[0]), (&mark, opts[1])] {
        sqlx::query("INSERT INTO schedule_votes (option_id, user_id, answer) VALUES (?, ?, 'yes')")
            .bind(opt)
            .bind(user.id)
            .execute(&app.db)
            .await
            .unwrap();
    }

    let edit = format!("/schedule/{poll}/edit");
    assert_eq!(mark.get(&edit).await.status, StatusCode::NOT_FOUND);
    let r = gm.get(&format!("/schedule/{poll}")).await;
    assert_eq!(r.ok().attr("a[href$='/edit']", "href"), Some(edit.clone()));
    let r = gm.get(&edit).await;
    assert_eq!(r.ok().text("h1"), "Edit poll");
    assert_eq!(r.attr("#title", "value").as_deref(), Some("S1"));
    assert_eq!(
        r.attr("#deadline", "value").as_deref(),
        Some("2020-01-01T20:00")
    );
    let alpine = r.attr("form.form", "x-data").unwrap();
    assert!(
        alpine.starts_with(
            r#"pollForm(["2030-01-10T18:00", "2030-01-11T18:00"], {"2030-01-10T18:00": 2, "2030-01-11T18:00": 1}, "list", "#
        ),
        "{alpine}"
    );

    // errors re-render the edit form
    let save = format!("/schedule/{poll}");
    let r = gm
        .post(&save, &[("title", ""), ("options", "2030-01-10T18:00")])
        .await;
    assert!(r.error().contains("Fill in a title"));
    assert_eq!(r.attr("form.form", "action"), Some(save.clone()));
    assert_eq!(
        mark.post(&save, &[("title", "X"), ("options", "2030-01-10T18:00")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // keep the 10th (2 votes), drop the 11th (1 vote), add the 12th, reopen voting
    let r = gm
        .post(
            &save,
            &[
                ("title", "Session one"),
                ("deadline", "2030-01-01T20:00"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-12T18:00"),
            ],
        )
        .await;
    assert_eq!(r.redirect(), save);
    let new_opts = option_ids(&app, poll).await;
    assert_eq!(new_opts.len(), 2);
    assert_eq!(new_opts[0], opts[0], "the kept date keeps its id");
    assert_eq!(app.scalar("SELECT COUNT(*) FROM schedule_votes").await, 2);
    let r = mark.get(&save).await;
    assert_eq!(r.ok().text("h1"), "Session one");
    assert!(r.has(".vote-btn"), "the later deadline reopened voting");

    // a decided poll can't be edited
    gm.post(
        &format!("/schedule/{poll}/decide"),
        &[("option_id", &new_opts[0].to_string())],
    )
    .await
    .redirect_id("/sessions/");
    assert_eq!(gm.get(&edit).await.redirect(), save);
    gm.post(
        &save,
        &[("title", "Changed"), ("options", "2030-02-01T18:00")],
    )
    .await
    .redirect();
    assert_eq!(gm.get(&save).await.text("h1"), "Session one");
    assert!(!gm.get(&save).await.has("a[href$='/edit']"));
}

#[sqlx::test]
async fn comments_on_a_poll(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "S1"), ("options", "2030-01-10T18:00")],
        )
        .await
        .redirect_id("/schedule/");
    let post = format!("/schedule/{poll}/comments");
    let r = mark
        .post(&post, &[("body_md", "Only if we start at **19:00**")])
        .await;
    assert!(r.ok().body.contains("<strong>19:00</strong>"));

    let page = gm.get(&format!("/schedule/{poll}")).await;
    assert_eq!(page.ok().count("#comment-list .comment"), 1);
    assert_eq!(page.attr("#comments form", "hx-post"), Some(post.clone()));
    // not on the campaign page
    let r = gm.get(&format!("/campaigns/{id}")).await;
    assert_eq!(r.ok().count("#comment-list .comment"), 0);
    // a vote answers with the grid only, without the comments
    let opt = option_ids(&app, poll).await[0];
    assert!(!vote(&mark, poll, opt, "yes").await.ok().has("#comments"));

    let olga = app.user("olga").await;
    assert_eq!(
        olga.post(&post, &[("body_md", "hi")]).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        gm.post("/schedule/9999/comments", &[("body_md", "hi")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
}

#[sqlx::test]
async fn reopen_a_decided_poll(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-11T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    let opts = option_ids(&app, poll).await;
    vote(&mark, poll, opts[0], "yes").await.ok();
    let decide = format!("/schedule/{poll}/decide");
    let reopen = format!("/schedule/{poll}/reopen");
    let page = format!("/schedule/{poll}");
    gm.post(&decide, &[("option_id", &opts[0].to_string())])
        .await
        .redirect_id("/sessions/");
    // a later game session, added by hand, is number 2
    let later = gm
        .post(
            &format!("/campaigns/{id}/sessions"),
            &[("starts_at", "2030-02-01T18:00"), ("title", "")],
        )
        .await
        .redirect_id("/sessions/");

    // only the GM gets the button
    assert!(gm.get(&page).await.ok().has("form[action$='/reopen']"));
    assert!(!mark.get(&page).await.has("form[action$='/reopen']"));
    assert_eq!(mark.post(&reopen, &[]).await.status, StatusCode::NOT_FOUND);

    // reopening deletes the session (and its attendance), keeps the votes and renumbers the rest
    assert_eq!(gm.post(&reopen, &[]).await.redirect(), page);
    assert_eq!(app.scalar("SELECT COUNT(*) FROM game_sessions").await, 1);
    assert_eq!(
        app.scalar("SELECT number FROM game_sessions").await,
        1,
        "the later game session moved up"
    );
    assert_eq!(
        app.scalar("SELECT COUNT(*) FROM game_session_attendance")
            .await,
        0,
        "the decided session's attendance went with it (the hand-added one has none)"
    );
    assert_eq!(app.scalar("SELECT COUNT(*) FROM schedule_votes").await, 1);
    let r = mark.get(&page).await;
    assert!(r.ok().has(".vote-btn"));
    assert!(!r.has(".decided"));

    // decide the other date; once the session has a comment it can't be undone any more
    let session = gm
        .post(&decide, &[("option_id", &opts[1].to_string())])
        .await
        .redirect_id("/sessions/");
    let number = |id: i64| {
        sqlx::query_scalar::<_, i64>("SELECT number FROM game_sessions WHERE id = ?")
            .bind(id)
            .fetch_one(&app.db)
    };
    assert_eq!(number(session).await.unwrap(), 1);
    assert_eq!(number(later).await.unwrap(), 2);
    mark.post(
        &format!("/sessions/{session}/comments"),
        &[("body_md", "See you")],
    )
    .await
    .ok();
    let r = gm.get(&page).await;
    assert!(!r.has("form[action$='/reopen']"));
    assert!(r.text(".decided").contains("edit the game session"));
    gm.post(&reopen, &[]).await.redirect();
    assert_eq!(app.scalar("SELECT COUNT(*) FROM game_sessions").await, 2);
    assert!(gm.get(&page).await.has(".decided"), "still decided");

    // the banner follows the session, not the option that was picked
    sqlx::query("UPDATE game_sessions SET status = 'cancelled' WHERE id = ?")
        .bind(session)
        .execute(&app.db)
        .await
        .unwrap();
    assert!(
        mark.get(&page)
            .await
            .text(".decided")
            .contains("The game session for this poll was cancelled.")
    );
}

#[sqlx::test]
async fn grid_conveniences(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, nina) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-11T18:00"),
                ("options", "2030-01-12T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    let opts = option_ids(&app, poll).await;
    let my_answers = async |user: &crate::common::User| -> Vec<(i64, String)> {
        sqlx::query_as(
            "SELECT option_id, answer FROM schedule_votes WHERE user_id = ? ORDER BY option_id",
        )
        .bind(user.id)
        .fetch_all(&app.db)
        .await
        .unwrap()
    };

    // a voter doesn't get a column of their own (the "Your answer" column shows it)
    let r = mark.get(&format!("/schedule/{poll}")).await;
    assert_eq!(r.ok().texts(".vote-grid__member"), ["Gina", "Nina"]);
    assert_eq!(
        r.attr("#vote-grid", "hx-trigger").as_deref(),
        Some("every 60s")
    );
    assert_eq!(
        r.text(".answer-rest").split_whitespace().next(),
        Some("Answer")
    );
    assert!(r.text(".answer-rest").starts_with("Answer all:"));
    // the GM's Pick buttons sit in the date cells; while voting is open, "Pick the date" shows
    // them (Alpine: x-show="picking")
    let r = gm.get(&format!("/schedule/{poll}")).await;
    assert_eq!(r.count(".vote-grid__date .pick-form"), 3);
    assert!(r.has("#pick-toggle"));
    assert_eq!(r.attr(".pick-form", "x-show").as_deref(), Some("picking"));

    // clicking the current answer again clears it
    let r = vote(&mark, poll, opts[0], "yes").await;
    let pressed = format!("#vote-{}-yes", opts[0]);
    assert_eq!(
        r.ok().attr(&pressed, "hx-delete"),
        Some(format!("/schedule/{poll}/options/{}/vote", opts[0]))
    );
    let r = mark
        .delete(&format!("/schedule/{poll}/options/{}/vote", opts[0]))
        .await;
    assert_eq!(
        r.ok().attr(&pressed, "aria-pressed").as_deref(),
        Some("false")
    );
    assert!(my_answers(&mark).await.is_empty());
    assert_eq!(
        nina.delete(&format!("/schedule/{poll}/options/9999/vote"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // answer the rest: only the unanswered dates change
    vote(&mark, poll, opts[1], "yes").await.ok();
    let rest = format!("/schedule/{poll}/vote-unanswered");
    let r = mark.put(&rest, &[("answer", "no")]).await;
    assert!(
        r.ok()
            .text(".your-progress")
            .starts_with("You've answered every date.")
    );
    assert_eq!(
        my_answers(&mark).await,
        [
            (opts[0], "no".into()),
            (opts[1], "yes".into()),
            (opts[2], "no".into())
        ]
    );
    let olga = app.user("olga").await;
    assert_eq!(
        olga.put(&rest, &[("answer", "no")]).await.status,
        StatusCode::NOT_FOUND
    );

    // the refreshed grid, until voting closes; then the page reloads
    let grid = format!("/schedule/{poll}/grid");
    assert!(nina.get(&grid).await.ok().body.contains("vote-grid"));
    assert_eq!(olga.get(&grid).await.status, StatusCode::NOT_FOUND);
    // a second open poll is probably for the session after
    let r = gm.get(&format!("/campaigns/{id}/schedule/new")).await;
    assert_eq!(r.attr("#title", "value").as_deref(), Some("Session 2"));

    gm.post(&format!("/schedule/{poll}/cancel"), &[])
        .await
        .redirect();
    assert!(nina.get(&grid).await.ok().hx_refresh);
    assert!(mark.put(&rest, &[("answer", "yes")]).await.ok().hx_refresh);
    assert!(
        mark.delete(&format!("/schedule/{poll}/options/{}/vote", opts[1]))
            .await
            .ok()
            .hx_refresh
    );
    let r = nina.get(&format!("/schedule/{poll}")).await;
    assert!(
        !r.has("#vote-grid[hx-trigger]"),
        "a closed poll doesn't refresh"
    );
    assert_eq!(r.texts(".vote-grid__member"), ["Gina", "Mark", "You"]);
}

#[sqlx::test]
async fn dashboard_puts_polls_to_vote_first(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let new_poll = |title: &'static str, date: &'static str| {
        let gm = &gm;
        async move {
            gm.post(
                &format!("/campaigns/{id}/schedule"),
                &[("title", title), ("options", date)],
            )
            .await
            .redirect_id("/schedule/")
        }
    };
    let todo = new_poll("To do", "2030-01-10T18:00").await;
    let done = new_poll("Done", "2030-01-11T18:00").await;
    new_poll("Passed", "2020-01-10T18:00").await;
    let opt = option_ids(&app, done).await[0];
    vote(&mark, done, opt, "yes").await.ok();

    let r = mark.get("/").await;
    assert_eq!(r.ok().texts("#to-vote .action-link strong"), ["To do"]);
    assert!(r.has(&format!("#to-vote a[href='/schedule/{todo}']")));
    assert_eq!(r.texts(".dashboard-grid .card h2"), ["Other open polls"]);
    assert_eq!(
        r.texts(".dashboard-grid .item-list .muted.small"),
        ["answered", "all dates passed"]
    );
}
