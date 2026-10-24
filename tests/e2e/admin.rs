use crate::common::{PASSWORD, spawn_app};
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

#[sqlx::test]
async fn admin_pages_are_admin_only(db: SqlitePool) {
    let app = spawn_app(db).await;
    let mark = app.user("mark").await;
    for path in [
        "/admin",
        "/admin/users",
        "/admin/campaigns",
        "/admin/campaigns/1/delete",
    ] {
        assert_eq!(mark.get(path).await.status, StatusCode::NOT_FOUND, "{path}");
    }
    assert!(!mark.get("/").await.has("a[href='/admin']"));
    let ada = app.admin("ada").await;
    assert!(ada.get("/").await.has("a[href='/admin']"));
    assert_eq!(ada.get("/admin").await.redirect(), "/admin/users");
    let r = ada.get("/admin/users").await;
    assert_eq!(r.ok().count(".user-row"), 2);
    assert_eq!(
        r.attr(".site-nav a[href='/admin']", "aria-current")
            .as_deref(),
        Some("page")
    );
}

#[sqlx::test]
async fn admin_sees_any_campaign_but_must_take_over_to_run_it(db: SqlitePool) {
    let app = spawn_app(db).await;
    let keldon = app.user("keldon").await;
    let id = keldon.create_campaign("Sexy hexy").await;
    let ada = app.admin("ada").await;

    // listed for the admin, viewable, members visible
    assert!(
        ada.get("/admin/campaigns")
            .await
            .texts(".user-row strong")
            .contains(&"Sexy hexy".to_string())
    );
    let r = ada.get(&format!("/campaigns/{id}")).await;
    assert!(r.ok().text(".page-head .muted").contains("GM: Keldon"));
    assert!(r.has(".admin-note"));
    assert!(!r.has(&format!("a[href='/campaigns/{id}/edit']")));
    let r = ada.get(&format!("/campaigns/{id}/members")).await;
    assert_eq!(r.ok().texts(".member strong"), ["Keldon"]);
    assert!(!r.has(".invite-list") && r.has(&format!("form[action='/campaigns/{id}/gm/take']")));
    // not the GM: no editing, invites or GM changes
    assert_eq!(
        ada.get(&format!("/campaigns/{id}/edit")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ada.post(&format!("/campaigns/{id}/invites"), &[("max_uses", "1")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ada.post(
            &format!("/campaigns/{id}/gm"),
            &[("user_id", &ada.id.to_string())]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // members can't take over; the admin can
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    assert_eq!(
        mark.post(&format!("/campaigns/{id}/gm/take"), &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ada.post(&format!("/campaigns/{id}/gm/take"), &[])
            .await
            .redirect(),
        format!("/campaigns/{id}/members")
    );
    let r = ada.get(&format!("/campaigns/{id}/members")).await;
    assert_eq!(r.texts(".member strong"), ["Ada", "Keldon", "Mark"]);
    assert!(!r.has(".admin-note"));
    ada.get(&format!("/campaigns/{id}/edit")).await.ok();
    assert!(
        keldon
            .get(&format!("/campaigns/{id}"))
            .await
            .text(".page-head .muted")
            .contains("GM: Ada")
    );
    // and it's on the admin's dashboard now, as GM
    assert!(ada.get("/").await.text(".campaign-card").contains("GM"));
}

#[sqlx::test]
async fn admin_manages_users(db: SqlitePool) {
    let app = spawn_app(db).await;
    let ada = app.admin("ada").await;
    let mark = app.user("mark").await;

    // create
    let r = ada
        .post(
            "/admin/users",
            &[
                ("username", "x"),
                ("display_name", "X"),
                ("password", "longenough"),
            ],
        )
        .await;
    assert_eq!(r.error(), "The username must be 3 to 32 characters long.");
    let r = ada
        .post(
            "/admin/users",
            &[
                ("username", "MARK"),
                ("display_name", "M"),
                ("password", "longenough"),
            ],
        )
        .await;
    assert_eq!(r.error(), "That username is taken.");
    let r = ada
        .post(
            "/admin/users",
            &[
                ("username", "newbie"),
                ("display_name", "Newbie"),
                ("password", "longenough"),
            ],
        )
        .await;
    assert!(r.ok().text(".alert--success").contains("Created newbie"));
    let r = app
        .anon()
        .post(
            "/login",
            &[("username", "newbie"), ("password", "longenough")],
        )
        .await;
    assert_eq!(r.redirect(), "/");

    // make admin (row fragment), not for yourself
    let r = ada
        .put(
            &format!("/admin/users/{}/admin", mark.id),
            &[("value", "1")],
        )
        .await;
    assert!(r.ok().body.trim_start().starts_with("<li class=\"user-row"));
    assert!(r.has(".badge"));
    mark.get("/admin/users").await.ok();
    ada.put(
        &format!("/admin/users/{}/admin", mark.id),
        &[("value", "0")],
    )
    .await
    .ok();
    assert_eq!(mark.get("/admin/users").await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        ada.put(&format!("/admin/users/{}/admin", ada.id), &[("value", "0")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        ada.put(
            &format!("/admin/users/{}/active", ada.id),
            &[("value", "0")]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );

    // deactivate: logged out at once, can't log in; reactivate
    let r = ada
        .put(
            &format!("/admin/users/{}/active", mark.id),
            &[("value", "0")],
        )
        .await;
    assert!(r.ok().text(".status").contains("Deactivated"));
    assert_eq!(mark.get("/").await.redirect(), "/login");
    let login = [("username", "mark"), ("password", PASSWORD)];
    assert_eq!(
        app.anon().post("/login", &login).await.error(),
        "Invalid username or password"
    );
    ada.put(
        &format!("/admin/users/{}/active", mark.id),
        &[("value", "1")],
    )
    .await
    .ok();
    assert_eq!(app.anon().post("/login", &login).await.redirect(), "/");

    // set a password: validation, then the old one stops working and sessions end
    let mark = {
        let m = app.anon();
        m.post("/login", &login).await.redirect();
        m
    };
    let path = format!(
        "/admin/users/{}/password",
        app.scalar("SELECT id FROM users WHERE username = 'mark'")
            .await
    );
    let r = ada.post(&path, &[("password", "short")]).await;
    assert!(r.ok().text(".alert--error").contains("at least 8"));
    let r = ada.post(&path, &[("password", "brand new pw")]).await;
    assert!(r.ok().text(".alert--success").contains("Password changed"));
    assert_eq!(mark.get("/").await.redirect(), "/login");
    assert_eq!(
        app.anon().post("/login", &login).await.status,
        StatusCode::OK
    );
    let r = app
        .anon()
        .post(
            "/login",
            &[("username", "mark"), ("password", "brand new pw")],
        )
        .await;
    assert_eq!(r.redirect(), "/");
}

#[sqlx::test]
async fn deleting_a_campaign_needs_its_exact_name(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Doomed").await;
    let keep = gm.create_campaign("Keeper").await;
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let ada = app.admin("ada").await;

    // fill the campaign with one of everything
    gm.post(
        &format!("/campaigns/{id}/pages"),
        &[("title", "Lore"), ("body_md", "x")],
    )
    .await
    .redirect();
    gm.post(&format!("/campaigns/{id}/invites"), &[("max_uses", "2")])
        .await
        .redirect();
    gm.post(
        &format!("/campaigns/{id}/schedule/patterns"),
        &[
            ("weekdays", "4"),
            ("start_time", "18:00"),
            ("every_n_weeks", "1"),
        ],
    )
    .await
    .redirect();
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "S1"), ("options", "2030-01-10T18:00")],
        )
        .await
        .redirect_id("/schedule/");
    let opt = app
        .scalar("SELECT id FROM schedule_options")
        .await
        .to_string();
    mark.put(
        &format!("/schedule/{poll}/options/{opt}/vote"),
        &[("answer", "yes")],
    )
    .await
    .ok();
    let session = gm
        .post(&format!("/schedule/{poll}/decide"), &[("option_id", &opt)])
        .await
        .redirect_id("/sessions/");
    mark.post(
        &format!("/sessions/{session}/entries"),
        &[("kind", "recap"), ("title", ""), ("body_md", "x")],
    )
    .await
    .redirect();
    mark.htmx(
        reqwest::Method::POST,
        &format!("/sessions/{session}/comments"),
        &[("body_md", "x")],
    )
    .await
    .ok();
    mark.htmx(
        reqwest::Method::POST,
        &format!("/campaigns/{id}/comments"),
        &[("body_md", "x")],
    )
    .await
    .ok();
    let qpoll = gm
        .post(
            &format!("/campaigns/{id}/polls"),
            &[("question", "Q"), ("options", "A"), ("options", "B")],
        )
        .await
        .redirect_id(&format!("/campaigns/{id}#poll-"));
    let qopt = app
        .scalar("SELECT MIN(id) FROM poll_options")
        .await
        .to_string();
    mark.post(&format!("/polls/{qpoll}/vote"), &[("option_ids", &qopt)])
        .await
        .ok();
    gm.post(
        &format!("/campaigns/{keep}/pages"),
        &[("title", "Kept"), ("body_md", "x")],
    )
    .await
    .redirect();

    // only admins, and the GM isn't enough
    let delete = format!("/admin/campaigns/{id}");
    assert_eq!(
        gm.delete(&format!("{delete}?confirm_name=Doomed"))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let r = ada.get(&format!("{delete}/delete")).await;
    assert!(r.ok().text(".danger-zone p").contains("1 game session"));
    assert!(
        r.has("button[disabled]"),
        "disabled until the name is typed"
    );

    // the wrong name (even only in case) deletes nothing
    for wrong in ["", "doomed", "Doom"] {
        let r = ada.delete(&format!("{delete}?confirm_name={wrong}")).await;
        assert!(r.ok().text(".alert--error").contains("doesn't match"));
        assert!(r.hx_redirect.is_none());
    }
    assert_eq!(app.scalar("SELECT COUNT(*) FROM campaigns").await, 2);

    let r = ada.delete(&format!("{delete}?confirm_name=Doomed")).await;
    assert_eq!(r.ok().hx_redirect.as_deref(), Some("/admin/campaigns"));
    assert_eq!(
        mark.get(&format!("/campaigns/{id}")).await.status,
        StatusCode::NOT_FOUND
    );
    for (table, expected) in [
        ("SELECT COUNT(*) FROM campaigns", 1),
        ("SELECT COUNT(*) FROM campaign_members", 1),
        ("SELECT COUNT(*) FROM campaign_pages", 1),
        ("SELECT COUNT(*) FROM campaign_invites", 0),
        ("SELECT COUNT(*) FROM schedule_patterns", 0),
        ("SELECT COUNT(*) FROM schedule_polls", 0),
        ("SELECT COUNT(*) FROM schedule_options", 0),
        ("SELECT COUNT(*) FROM schedule_votes", 0),
        ("SELECT COUNT(*) FROM game_sessions", 0),
        ("SELECT COUNT(*) FROM game_session_attendance", 0),
        ("SELECT COUNT(*) FROM game_session_entries", 0),
        ("SELECT COUNT(*) FROM comments", 0),
        ("SELECT COUNT(*) FROM polls", 0),
        ("SELECT COUNT(*) FROM poll_options", 0),
        ("SELECT COUNT(*) FROM poll_votes", 0),
        ("SELECT COUNT(*) FROM users", 3),
    ] {
        assert_eq!(app.scalar(table).await, expected, "{table}");
    }
    gm.get(&format!("/campaigns/{keep}")).await.ok();
}
