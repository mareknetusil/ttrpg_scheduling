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
    assert!(r.body.contains(r#"dateList(["2026-11-01T25:00"])"#));
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

    // decided: no more votes or decisions, the poll page says so
    assert_eq!(
        vote(&mark, poll, opts[1], "yes").await.status,
        StatusCode::NOT_FOUND
    );
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
async fn outsiders_and_non_member_admins(db: SqlitePool) {
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

    // an admin can look and cancel, but only members vote
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
    ada.post(&format!("/schedule/{poll}/cancel"), &[])
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
    let alpine = r.ok().attr("form[x-data]", "x-data").unwrap();
    let dates = alpine.matches("T18:00").count();
    assert!((3..=4).contains(&dates), "{alpine}");

    let other = gm.create_campaign("Other").await;
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
