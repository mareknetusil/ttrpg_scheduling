use crate::common::spawn_app;
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

#[sqlx::test]
async fn history_attendance_and_session_edits(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let olga = app.user("olga").await;

    let r = gm.get(&format!("/campaigns/{id}/history")).await;
    assert!(r.ok().text(".empty").contains("No game sessions yet"));

    // add a session by hand
    let add = format!("/campaigns/{id}/sessions");
    assert_eq!(
        mark.post(&add, &[("starts_at", "2025-05-01T19:00")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        gm.post(&add, &[("starts_at", ""), ("title", "")])
            .await
            .error(),
        "Fill in a valid date and time."
    );
    let s = gm
        .post(
            &add,
            &[("starts_at", "2025-05-01T19:00"), ("title", "Prologue")],
        )
        .await
        .redirect_id("/sessions/");
    let r = mark.get(&format!("/campaigns/{id}/history")).await;
    assert_eq!(r.text(".history__main strong"), "Prologue");
    assert!(
        r.text(".history__main .muted")
            .contains("Thu 1 May 2025, 19:00")
    );
    assert_eq!(
        olga.get(&format!("/sessions/{s}")).await.status,
        StatusCode::NOT_FOUND
    );

    // edit: GM only; status change
    let path = format!("/sessions/{s}");
    let form = [
        ("title", "Prologue"),
        ("starts_at", "2025-05-01T20:00"),
        ("status", "played"),
    ];
    assert_eq!(mark.post(&path, &form).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        gm.post(
            &path,
            &[("title", ""), ("starts_at", "x"), ("status", "played")]
        )
        .await
        .error(),
        "Fill in a valid date and time."
    );
    gm.post(&path, &form).await.redirect();
    let r = mark.get(&path).await;
    assert_eq!(r.text(".status"), "Played");
    assert!(r.text(".page-head time").contains("20:00"));

    // attendance: GM only, members only
    let att = format!("/sessions/{s}/attendance");
    let ids = |u: i64| u.to_string();
    assert_eq!(
        mark.post(&att, &[("user_ids", &ids(mark.id))]).await.status,
        StatusCode::NOT_FOUND
    );
    gm.post(
        &att,
        &[("user_ids", &ids(mark.id)), ("user_ids", &ids(olga.id))],
    )
    .await
    .redirect();
    assert_eq!(mark.get(&path).await.texts("#attendance .chip"), ["Mark"]);
    gm.post(&att, &[]).await.redirect();
    assert_eq!(mark.get(&path).await.count("#attendance .chip"), 0);
}

#[sqlx::test]
async fn entries_permissions(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    let nina = app.user("nina").await;
    app.add_member(id, &mark).await;
    app.add_member(id, &nina).await;
    let s = gm
        .post(
            &format!("/campaigns/{id}/sessions"),
            &[("starts_at", "2025-05-01T19:00"), ("title", "")],
        )
        .await
        .redirect_id("/sessions/");
    let entries = format!("/sessions/{s}/entries");

    let r = mark
        .post(
            &entries,
            &[("kind", "recap"), ("title", ""), ("body_md", " ")],
        )
        .await;
    assert_eq!(r.error(), "Write something first.");
    let r = mark
        .post(
            &entries,
            &[
                ("kind", "recap"),
                ("title", "What happened"),
                ("body_md", "We **won**"),
            ],
        )
        .await;
    let e = r.redirect_id(&format!("/sessions/{s}#entry-"));
    nina.post(
        &entries,
        &[("kind", "loot"), ("title", ""), ("body_md", "A sword")],
    )
    .await
    .redirect();
    assert_eq!(
        mark.post(
            &entries,
            &[("kind", "poem"), ("title", ""), ("body_md", "x")]
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let r = mark.get(&format!("/sessions/{s}")).await;
    assert_eq!(r.texts(".entry .badge"), ["Recap", "Loot"]);
    assert!(r.body.contains("<strong>won</strong>"));
    // edit links only on your own entries
    assert_eq!(r.count(".entry__actions"), 1);
    assert_eq!(
        gm.get(&format!("/sessions/{s}"))
            .await
            .count(".entry__actions"),
        2
    );

    // nina can't edit or delete mark's entry; mark and the GM can
    assert_eq!(
        nina.get(&format!("/entries/{e}/edit")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        nina.delete(&format!("/entries/{e}")).await.status,
        StatusCode::NOT_FOUND
    );
    mark.get(&format!("/entries/{e}/edit")).await.ok();
    let r = mark
        .post(
            &format!("/entries/{e}"),
            &[("kind", "notes"), ("title", "T"), ("body_md", "")],
        )
        .await;
    assert_eq!(r.error(), "Write something first.");
    mark.post(
        &format!("/entries/{e}"),
        &[("kind", "notes"), ("title", "T"), ("body_md", "Edited")],
    )
    .await
    .redirect();
    assert_eq!(gm.delete(&format!("/entries/{e}")).await.ok().body, "");
    assert_eq!(
        app.scalar("SELECT COUNT(*) FROM game_session_entries")
            .await,
        1
    );
}

#[sqlx::test]
async fn new_session_without_a_poll(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let new = format!("/campaigns/{id}/sessions/new");
    let add = format!("/campaigns/{id}/sessions");

    // GM only, linked from the campaign page and the history
    assert_eq!(mark.get(&new).await.status, StatusCode::NOT_FOUND);
    let link = format!("a[href='{new}']");
    assert!(gm.get(&format!("/campaigns/{id}")).await.has(&link));
    assert!(!mark.get(&format!("/campaigns/{id}")).await.has(&link));
    assert!(gm.get(&format!("/campaigns/{id}/history")).await.has(&link));
    assert!(
        !mark
            .get(&format!("/campaigns/{id}/history"))
            .await
            .has(&link)
    );

    // every member is ticked by default; after an error the submitted ticks are kept
    let r = gm.get(&new).await;
    assert_eq!(r.ok().count("input[name=user_ids]"), 2);
    assert_eq!(r.count("input[name=user_ids][checked]"), 2);
    let mark_id = mark.id.to_string();
    let r = gm
        .post(&add, &[("starts_at", ""), ("user_ids", &mark_id)])
        .await;
    assert_eq!(r.error(), "Fill in a valid date and time.");
    assert_eq!(
        r.attr("input[name=user_ids][checked]", "value"),
        Some(mark_id.clone())
    );

    // a past date is recorded as played, with the ticked attendance
    let later = gm
        .post(
            &add,
            &[
                ("starts_at", "2025-06-01T19:00"),
                ("title", "The heist"),
                ("user_ids", &mark_id),
            ],
        )
        .await
        .redirect_id("/sessions/");
    let r = mark.get(&format!("/sessions/{later}")).await;
    assert_eq!(r.text(".status"), "Played");
    assert_eq!(r.texts("#attendance .chip"), ["Mark"]);

    // a future date is planned and shows as upcoming
    let future = gm
        .post(&add, &[("starts_at", "2030-01-10T18:00")])
        .await
        .redirect_id("/sessions/");
    assert_eq!(
        mark.get(&format!("/sessions/{future}"))
            .await
            .text(".status"),
        "Planned"
    );
    assert!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .has(&format!("a[href='/sessions/{future}']"))
    );

    // sessions are numbered by date: an earlier one added afterwards becomes #1
    let earlier = gm
        .post(&add, &[("starts_at", "2025-05-01T19:00")])
        .await
        .redirect_id("/sessions/");
    let history = format!("/campaigns/{id}/history");
    assert_eq!(
        mark.get(&history).await.texts(".history__number"),
        ["#3", "#2", "#1"]
    );
    assert_eq!(
        mark.get(&format!("/sessions/{earlier}")).await.text("h1"),
        "Session 1"
    );

    // moving a session's date renumbers too
    gm.post(
        &format!("/sessions/{earlier}"),
        &[
            ("title", ""),
            ("starts_at", "2025-07-01T19:00"),
            ("status", "played"),
        ],
    )
    .await
    .redirect();
    assert_eq!(
        mark.get(&format!("/sessions/{later}")).await.text("h1"),
        "Session 1: The heist"
    );
    assert_eq!(
        mark.get(&format!("/sessions/{earlier}")).await.text("h1"),
        "Session 2"
    );
}
