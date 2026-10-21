use crate::common::spawn_app;
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

#[sqlx::test]
async fn gm_writes_pages_members_read_them(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let base = format!("/campaigns/{id}/pages");

    gm.get(&format!("{base}/new")).await.ok();
    let r = gm.post(&base, &[("title", "  "), ("body_md", "x")]).await;
    assert_eq!(r.error(), "Fill in a title.");

    // slugs: from the title, transliterated, unique, never "new"
    let r = gm
        .post(
            &base,
            &[
                ("title", "Čarodějnice & NPCs"),
                ("body_md", "# Witches\n\n- [x] met"),
            ],
        )
        .await;
    assert_eq!(r.redirect(), format!("{base}/carodejnice-npcs"));
    let r = gm
        .post(&base, &[("title", "Čarodějnice & NPCs"), ("body_md", "")])
        .await;
    assert_eq!(r.redirect(), format!("{base}/carodejnice-npcs-2"));
    let r = gm.post(&base, &[("title", "New"), ("body_md", "")]).await;
    assert_eq!(r.redirect(), format!("{base}/new-2"));

    // a member reads; the campaign page lists the pages
    let r = mark.get(&format!("{base}/carodejnice-npcs")).await;
    assert_eq!(r.ok().text("h1"), "Čarodějnice & NPCs");
    assert_eq!(r.text(".prose h1"), "Witches");
    assert!(r.has(".prose input[type=checkbox][checked][disabled]"));
    assert!(!r.has(&format!("a[href='{base}/carodejnice-npcs/edit']")));
    assert_eq!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .count(".link-list li"),
        3
    );

    // members can't write
    assert_eq!(
        mark.post(&base, &[("title", "x"), ("body_md", "")])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mark.get(&format!("{base}/new-2/edit")).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mark.delete(&format!("{base}/new-2")).await.status,
        StatusCode::NOT_FOUND
    );

    // edit keeps the slug
    gm.get(&format!("{base}/new-2/edit")).await.ok();
    let r = gm
        .post(
            &format!("{base}/new-2"),
            &[("title", "Renamed"), ("body_md", "text")],
        )
        .await;
    assert_eq!(r.redirect(), format!("{base}/new-2"));
    assert_eq!(
        mark.get(&format!("{base}/new-2")).await.text("h1"),
        "Renamed"
    );

    // delete: htmx redirect to the campaign
    let r = gm.delete(&format!("{base}/new-2")).await;
    assert_eq!(r.ok().hx_redirect, Some(format!("/campaigns/{id}")));
    assert_eq!(
        mark.get(&format!("{base}/new-2")).await.status,
        StatusCode::NOT_FOUND
    );
}
