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

    let r = gm.post(&path, &[("title", "S1"), ("options", "")]).await;
    assert_eq!(r.error(), "Add at least one date.");
    let r = gm
        .post(&path, &[("title", "S1"), ("options", "2026-11-01T25:00")])
        .await;
    assert!(r.error().contains("isn't a valid date and time"));
    // the entered dates come back to Alpine as JSON
    assert!(r.body.contains(r#"dateList(["2026-11-01T25:00"], {})"#));
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
            .starts_with("<div id=\"vote-grid\">")
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
    assert!(r.text(".alert--success").contains("Thu 10 Jan 2030, 18:00"));
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

    // dashboard nudges members who haven't voted
    assert_eq!(mark.get("/").await.text(".item-list .badge"), "Vote");

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
    assert!(!mark.get("/").await.has(".item-list .badge"));
}

#[sqlx::test]
async fn regular_slots_fill_a_new_poll(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let path = format!("/campaigns/{id}/schedule/patterns");
    gm.get(&path).await.ok();
    assert_eq!(mark.get(&path).await.status, StatusCode::NOT_FOUND);

    let r = gm
        .post(&path, &[("start_time", "18:00"), ("every_n_weeks", "1")])
        .await;
    assert_eq!(r.error(), "Pick at least one day of the week.");
    let r = gm
        .post(
            &path,
            &[
                ("weekdays", "4"),
                ("weekdays", "5"),
                ("start_time", "18:00"),
                ("every_n_weeks", "1"),
            ],
        )
        .await;
    assert_eq!(r.redirect(), path);
    assert_eq!(
        gm.get(&path).await.text(".member__name"),
        "Fri, Sat at 18:00, every week"
    );

    let pattern = app.scalar("SELECT id FROM schedule_patterns").await;
    let r = gm
        .get(&format!(
            "/campaigns/{id}/schedule/new?pattern_id={pattern}&weeks=2"
        ))
        .await;
    // two weeks of Fridays and Saturdays: 3 or 4 dates depending on today's weekday and time
    let alpine = r.ok().attr("[x-data^=dateList]", "x-data").unwrap();
    let dates_in_form = alpine.matches("T18:00").count();
    assert!((3..=4).contains(&dates_in_form), "{alpine}");

    // the same dates as JSON, for the form's "Add" button
    let json = format!("/campaigns/{id}/schedule/slot-dates?pattern_id={pattern}&weeks=2");
    let r = gm.get(&json).await;
    let dates: Vec<String> = serde_json::from_str(&r.ok().body).unwrap();
    assert_eq!(dates.len(), dates_in_form);
    assert!(dates.iter().all(|d| d.ends_with("T18:00")));
    assert_eq!(mark.get(&json).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        gm.get(&format!("/campaigns/{id}/schedule/slot-dates"))
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    let other = gm.create_campaign("Other").await;
    assert_eq!(
        gm.get(&format!(
            "/campaigns/{other}/schedule/slot-dates?pattern_id={pattern}"
        ))
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let r = gm
        .get(&format!(
            "/campaigns/{other}/schedule/new?pattern_id={pattern}"
        ))
        .await;
    assert_eq!(
        r.status,
        StatusCode::NOT_FOUND,
        "a slot of another campaign"
    );

    assert_eq!(gm.delete(&format!("{path}/{pattern}")).await.ok().body, "");
    assert_eq!(
        gm.delete(&format!("{path}/{pattern}")).await.status,
        StatusCode::NOT_FOUND
    );
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
    assert_eq!(r.count(".vote-btn"), 3, "only the future date");
    assert_eq!(
        r.text("#vote-grid > p"),
        "You haven't answered 1 of 1 date yet."
    );
    assert!(vote(&mark, poll, opts[0], "yes").await.ok().hx_refresh);
    // the dashboard badge only counts future dates
    vote(&mark, poll, opts[1], "yes").await.ok();
    assert!(!mark.get("/").await.has(".item-list .badge"));

    let r = gm.get(&format!("/schedule/{poll}")).await;
    assert_eq!(
        r.count("form[action$='/decide']"),
        1,
        "no Pick on the past date"
    );
    // "Most yes" ignores past dates
    assert!(!r.has("tr.is-past .badge"));

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
    let alpine = r.attr("[x-data^=dateList]", "x-data").unwrap();
    assert_eq!(
        alpine,
        r#"dateList(["2030-01-10T18:00", "2030-01-11T18:00"], {"2030-01-10T18:00": 2, "2030-01-11T18:00": 1})"#
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
