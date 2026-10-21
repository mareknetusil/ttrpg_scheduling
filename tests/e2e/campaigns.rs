use crate::common::spawn_app;
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

#[sqlx::test]
async fn create_campaign_and_see_it(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    gm.get("/campaigns/new").await.ok();
    let id = gm.create_campaign("Curse of Strahd").await;

    let r = gm.get("/").await;
    assert_eq!(
        r.ok().text(".campaign-card__title strong"),
        "Curse of Strahd"
    );
    assert!(r.has(".campaign-card .badge"), "GM badge");

    let r = gm.get(&format!("/campaigns/{id}")).await;
    assert_eq!(r.ok().text("h1"), "Curse of Strahd");
    assert!(r.text(".page-head .muted").contains("GM: you"));
    assert!(r.has(&format!("a[href='/campaigns/{id}/edit']")));
}

#[sqlx::test]
async fn create_validation_rerenders_form_escaped(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let r = gm
        .post(
            "/campaigns",
            &[
                ("name", "<script>x</script>"),
                ("system", "  "),
                ("tagline", ""),
                ("description_md", ""),
            ],
        )
        .await;
    assert_eq!(r.error(), "Fill campaign name and rule system!");
    assert!(!r.body.contains("<script>x"));
    assert_eq!(
        r.attr("#name", "value").as_deref(),
        Some("<script>x</script>")
    );
    assert_eq!(app.scalar("SELECT COUNT(*) FROM campaigns").await, 0);
}

#[sqlx::test]
async fn edit_campaign_markdown_is_sanitized(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Old").await;
    gm.get(&format!("/campaigns/{id}/edit")).await.ok();
    let r = gm
        .post(
            &format!("/campaigns/{id}"),
            &[
                ("name", "New"),
                ("system", "D&D"),
                ("tagline", "Dark"),
                (
                    "description_md",
                    "# Lore\n\n<img src=x onerror=alert(1)> **bold**",
                ),
            ],
        )
        .await;
    assert_eq!(r.redirect(), format!("/campaigns/{id}"));
    let r = gm.get(&format!("/campaigns/{id}")).await;
    assert_eq!(r.text("h1"), "New");
    assert_eq!(r.text(".prose h1"), "Lore");
    assert!(r.body.contains("<strong>bold</strong>"));
    assert!(!r.body.contains("onerror"));
}

#[sqlx::test]
async fn non_members_get_404_everywhere(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Secret").await;
    let outsider = app.user("olga").await;
    for path in [
        format!("/campaigns/{id}"),
        format!("/campaigns/{id}/edit"),
        format!("/campaigns/{id}/members"),
        format!("/campaigns/{id}/history"),
        format!("/campaigns/{id}/pages/new"),
        format!("/campaigns/{id}/schedule/new"),
        format!("/campaigns/{id}/schedule/patterns"),
        format!("/campaigns/{id}/polls/new"),
    ] {
        assert_eq!(
            outsider.get(&path).await.status,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    let r = outsider
        .post(
            &format!("/campaigns/{id}"),
            &[
                ("name", "Hacked"),
                ("system", "x"),
                ("tagline", ""),
                ("description_md", ""),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(
        outsider.get("/campaigns/999").await.status,
        StatusCode::NOT_FOUND
    );
    // and it's not on their dashboard
    assert_eq!(outsider.get("/").await.count(".campaign-card"), 0);
}

#[sqlx::test]
async fn members_view_but_only_gm_and_admin_manage(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Shared").await;
    let member = app.user("mark").await;
    app.add_member(id, &member).await;
    let admin = app.admin("ada").await;

    let r = member.get(&format!("/campaigns/{id}")).await;
    assert!(!r.ok().has(&format!("a[href='/campaigns/{id}/edit']")));
    assert_eq!(
        member.get(&format!("/campaigns/{id}/edit")).await.status,
        StatusCode::NOT_FOUND
    );
    for path in [
        "pages/new",
        "schedule/new",
        "schedule/patterns",
        "polls/new",
    ] {
        let r = member.get(&format!("/campaigns/{id}/{path}")).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
    }

    // An admin who isn't a member can still see and manage
    admin.get(&format!("/campaigns/{id}")).await.ok();
    admin.get(&format!("/campaigns/{id}/edit")).await.ok();
    admin
        .get(&format!("/campaigns/{id}/schedule/new"))
        .await
        .ok();
}
