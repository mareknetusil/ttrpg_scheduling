use crate::common::spawn_app;
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

#[sqlx::test]
async fn comments(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    let nina = app.user("nina").await;
    app.add_member(id, &mark).await;
    app.add_member(id, &nina).await;
    let post = format!("/campaigns/{id}/comments");

    // posting answers with the new comment fragment
    let r = mark
        .htmx(
            reqwest::Method::POST,
            &post,
            &[("body_md", "Great **game** <script>x</script>")],
        )
        .await;
    assert!(
        r.ok()
            .body
            .trim_start()
            .starts_with("<article class=\"comment\"")
    );
    assert!(r.body.contains("<strong>game</strong>") && !r.body.contains("<script"));
    assert_eq!(
        mark.htmx(reqwest::Method::POST, &post, &[("body_md", "  ")])
            .await
            .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let outsider = app.user("olga").await;
    assert_eq!(
        outsider
            .htmx(reqwest::Method::POST, &post, &[("body_md", "hi")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let c = app.scalar("SELECT id FROM comments").await;

    // session comments are separate from campaign comments
    let s = gm
        .post(
            &format!("/campaigns/{id}/sessions"),
            &[("starts_at", "2025-05-01T19:00"), ("title", "")],
        )
        .await
        .redirect_id("/sessions/");
    nina.htmx(
        reqwest::Method::POST,
        &format!("/sessions/{s}/comments"),
        &[("body_md", "Session talk")],
    )
    .await
    .ok();
    assert_eq!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .count(".comment"),
        1
    );
    assert_eq!(
        mark.get(&format!("/sessions/{s}"))
            .await
            .texts(".comment .prose"),
        ["Session talk"]
    );

    // edit: author only, via fragments
    assert_eq!(
        nina.get(&format!("/comments/{c}/edit")).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(
        mark.get(&format!("/comments/{c}/edit"))
            .await
            .ok()
            .has(&format!("form[hx-put='/comments/{c}']"))
    );
    let r = mark
        .put(&format!("/comments/{c}"), &[("body_md", "Edited")])
        .await;
    assert_eq!(r.ok().text(".prose"), "Edited");
    assert!(r.text(".comment__head").contains("edited"));
    assert_eq!(
        nina.put(&format!("/comments/{c}"), &[("body_md", "x")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    mark.get(&format!("/comments/{c}")).await.ok();

    // delete: not other members, but the GM
    assert_eq!(
        nina.delete(&format!("/comments/{c}")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(gm.delete(&format!("/comments/{c}")).await.ok().body, "");
    assert_eq!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .count(".comment"),
        0
    );
}

#[sqlx::test]
async fn polls(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    let nina = app.user("nina").await;
    app.add_member(id, &mark).await;
    app.add_member(id, &nina).await;
    let new = format!("/campaigns/{id}/polls");

    gm.get(&format!("{new}/new")).await.ok();
    let r = gm
        .post(
            &new,
            &[
                ("question", "Snacks?"),
                ("options", "Pizza"),
                ("options", " "),
            ],
        )
        .await;
    assert_eq!(r.error(), "Add at least two options.");
    let r = gm
        .post(
            &new,
            &[
                ("question", "Snacks?"),
                ("options", "Pizza"),
                ("options", "Sushi"),
            ],
        )
        .await;
    let poll = r.redirect_id(&format!("/campaigns/{id}#poll-"));
    let opts: Vec<i64> = sqlx::query_scalar("SELECT id FROM poll_options ORDER BY sort_order")
        .fetch_all(&app.db)
        .await
        .unwrap();

    // results are hidden until you vote
    let page = mark.get(&format!("/campaigns/{id}")).await;
    assert_eq!(page.ok().text(".poll h3"), "Snacks?");
    assert!(!page.has(".poll .poll-bar"));

    // single choice: only the first of two ids counts
    let vote = format!("/polls/{poll}/vote");
    let (a, b) = (opts[0].to_string(), opts[1].to_string());
    let r = mark
        .post(&vote, &[("option_ids", &a), ("option_ids", &b)])
        .await;
    assert!(r.ok().has(".poll-bar"));
    assert!(r.text(".poll__head p").contains("1 voted"));
    nina.post(&vote, &[("option_ids", &b)]).await.ok();
    assert_eq!(app.scalar("SELECT COUNT(*) FROM poll_votes").await, 2);
    let outsider = app.user("olga").await;
    assert_eq!(
        outsider.post(&vote, &[("option_ids", &a)]).await.status,
        StatusCode::NOT_FOUND
    );

    // close: GM only; afterwards votes don't count
    assert_eq!(
        mark.post(&format!("/polls/{poll}/close"), &[]).await.status,
        StatusCode::NOT_FOUND
    );
    let r = gm.post(&format!("/polls/{poll}/close"), &[]).await;
    assert!(r.ok().text(".poll__head p").starts_with("Closed"));
    mark.post(&vote, &[("option_ids", &b)]).await.ok();
    assert_eq!(
        app.scalar("SELECT COUNT(*) FROM poll_votes WHERE user_id = 2")
            .await,
        1
    );
    assert!(
        gm.get(&format!("/campaigns/{id}"))
            .await
            .text(".poll-results")
            .contains("Mark")
    );

    // a multiple-choice poll on a session, which must be in this campaign
    let s = gm
        .post(
            &format!("/campaigns/{id}/sessions"),
            &[("starts_at", "2025-05-01T19:00"), ("title", "")],
        )
        .await
        .redirect_id("/sessions/");
    let other = gm.create_campaign("Other").await;
    let r = gm
        .get(&format!("/campaigns/{other}/polls/new?session_id={s}"))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let s_str = s.to_string();
    let r = gm
        .post(
            &new,
            &[
                ("question", "Maps?"),
                ("multiple_choice", "on"),
                ("options", "A"),
                ("options", "B"),
                ("game_session_id", &s_str),
            ],
        )
        .await;
    let poll2 = r.redirect_id(&format!("/sessions/{s}#poll-"));
    let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM poll_options WHERE poll_id = ?")
        .bind(poll2)
        .fetch_all(&app.db)
        .await
        .unwrap();
    let r = mark
        .post(
            &format!("/polls/{poll2}/vote"),
            &[
                ("option_ids", &ids[0].to_string()),
                ("option_ids", &ids[1].to_string()),
            ],
        )
        .await;
    assert_eq!(r.ok().count("input[type=checkbox][checked]"), 2);
    assert_eq!(
        mark.get(&format!("/sessions/{s}")).await.texts(".poll h3"),
        ["Maps?"]
    );
    assert_eq!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .texts(".poll h3"),
        ["Snacks?"]
    );
}
