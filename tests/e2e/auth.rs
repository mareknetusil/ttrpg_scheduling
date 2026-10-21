use crate::common::{PASSWORD, spawn_app};
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

#[sqlx::test]
async fn pages_need_login(db: SqlitePool) {
    let app = spawn_app(db).await;
    let anon = app.anon();
    for path in [
        "/",
        "/campaigns/new",
        "/campaigns/1",
        "/schedule/1",
        "/sessions/1",
    ] {
        assert_eq!(anon.get(path).await.redirect(), "/login", "{path}");
    }
    anon.get("/login").await.ok();
    assert!(anon.get("/login").await.has("form[action='/login']"));
}

#[sqlx::test]
async fn wrong_password_shows_error_and_keeps_username(db: SqlitePool) {
    let app = spawn_app(db).await;
    app.user("alice").await;
    let anon = app.anon();
    let r = anon
        .post("/login", &[("username", "alice"), ("password", "nope")])
        .await;
    assert_eq!(r.error(), "Invalid username or password");
    assert_eq!(r.attr("#username", "value").as_deref(), Some("alice"));
    // unknown user: same message
    let r = anon
        .post("/login", &[("username", "nobody"), ("password", "nope")])
        .await;
    assert_eq!(r.error(), "Invalid username or password");
}

#[sqlx::test]
async fn login_logout(db: SqlitePool) {
    let app = spawn_app(db).await;
    let alice = app.user("alice").await;
    let r = alice.get("/").await;
    assert_eq!(r.ok().text("h1"), "Hello, Alice!");
    assert_eq!(r.text(".site-nav__user"), "Alice");

    assert_eq!(alice.post("/logout", &[]).await.redirect(), "/login");
    assert_eq!(alice.get("/").await.redirect(), "/login");
    assert_eq!(app.scalar("SELECT COUNT(*) FROM auth_sessions").await, 0);
}

#[sqlx::test]
async fn usernames_are_case_insensitive(db: SqlitePool) {
    let app = spawn_app(db).await;
    app.user("alice").await;
    let r = app
        .anon()
        .post("/login", &[("username", "ALICE"), ("password", PASSWORD)])
        .await;
    assert_eq!(r.redirect(), "/");
}

#[sqlx::test]
async fn login_next_only_redirects_to_local_paths(db: SqlitePool) {
    let app = spawn_app(db).await;
    app.user("alice").await;
    let r = app.anon().get("/login?next=/invite/abc").await;
    assert_eq!(
        r.attr("input[name=next]", "value").as_deref(),
        Some("/invite/abc")
    );

    for (next, expected) in [
        ("/invite/abc", "/invite/abc"),
        ("//evil.example", "/"),
        ("/\\evil.example", "/"),
        ("https://evil.example", "/"),
    ] {
        let r = app
            .anon()
            .post(
                "/login",
                &[
                    ("username", "alice"),
                    ("password", PASSWORD),
                    ("next", next),
                ],
            )
            .await;
        assert_eq!(r.redirect(), expected, "next={next}");
    }

    // A wrong password keeps `next` for the second try
    let r = app
        .anon()
        .post(
            "/login",
            &[
                ("username", "alice"),
                ("password", "x"),
                ("next", "/invite/abc"),
            ],
        )
        .await;
    assert_eq!(
        r.attr("input[name=next]", "value").as_deref(),
        Some("/invite/abc")
    );
}

#[sqlx::test]
async fn healthz_and_unknown_routes(db: SqlitePool) {
    let app = spawn_app(db).await;
    let anon = app.anon();
    let r = anon.get("/healthz").await;
    assert!(r.ok().body.starts_with("ok "));
    assert_eq!(anon.get("/nope").await.status, StatusCode::NOT_FOUND);
    anon.get("/static/css/style.css").await.ok();
}

#[sqlx::test]
async fn markdown_preview_needs_login_and_sanitizes(db: SqlitePool) {
    let app = spawn_app(db).await;
    assert_eq!(
        app.anon()
            .post("/markdown/preview", &[("body_md", "x")])
            .await
            .redirect(),
        "/login"
    );
    let alice = app.user("alice").await;
    let r = alice
        .post(
            "/markdown/preview",
            &[("body_md", "**hi** <script>alert(1)</script>")],
        )
        .await;
    assert!(r.ok().body.contains("<strong>hi</strong>"));
    assert!(!r.body.contains("<script"));
    let r = alice.post("/markdown/preview", &[("body_md", "  ")]).await;
    assert!(r.body.contains("Nothing to preview"));
}
