use crate::common::spawn_app;
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

/// Creates an invite for `max_uses` people and returns its token.
async fn invite(
    app: &crate::common::TestApp,
    gm: &crate::common::User,
    id: i64,
    max_uses: &str,
) -> String {
    let r = gm
        .post(
            &format!("/campaigns/{id}/invites"),
            &[("max_uses", max_uses)],
        )
        .await;
    assert_eq!(r.redirect(), format!("/campaigns/{id}/members"));
    sqlx::query_scalar("SELECT token FROM campaign_invites ORDER BY id DESC LIMIT 1")
        .fetch_one(&app.db)
        .await
        .unwrap()
}

#[sqlx::test]
async fn members_page_and_invite_management(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let r = gm
        .post(&format!("/campaigns/{id}/invites"), &[("max_uses", "0")])
        .await;
    assert_eq!(r.error(), "Pick between 1 and 20 players.");
    let token = invite(&app, &gm, id, "2").await;

    let r = gm.get(&format!("/campaigns/{id}/members")).await;
    assert_eq!(r.ok().count(".invite"), 1);
    assert!(r.text(".invite").contains("0/2 used"));
    assert!(r.body.contains(&token));

    // a member sees members but no invites
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let r = mark.get(&format!("/campaigns/{id}/members")).await;
    assert_eq!(r.ok().count(".member"), 2);
    assert!(!r.body.contains(&token));

    // revoke: members can't, the GM can (htmx DELETE, empty 200)
    let invite_id = app.scalar("SELECT id FROM campaign_invites").await;
    let path = format!("/campaigns/{id}/invites/{invite_id}");
    assert_eq!(mark.delete(&path).await.status, StatusCode::NOT_FOUND);
    assert_eq!(gm.delete(&path).await.ok().body, "");
    assert_eq!(gm.delete(&path).await.status, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn logged_in_user_joins_once(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let token = invite(&app, &gm, id, "2").await;
    let path = format!("/invite/{token}");
    let pat = app.user("pat").await;

    let r = pat.get(&path).await;
    assert!(r.ok().text("p").contains("Camp"));
    assert!(r.has(&format!("form[action='{path}'] button")));

    assert_eq!(
        pat.post(&path, &[]).await.redirect(),
        format!("/campaigns/{id}")
    );
    assert_eq!(
        app.scalar("SELECT use_count FROM campaign_invites").await,
        1
    );
    pat.get(&format!("/campaigns/{id}")).await.ok();

    // joining again doesn't use the invite up
    assert_eq!(
        pat.post(&path, &[]).await.redirect(),
        format!("/campaigns/{id}")
    );
    assert_eq!(
        app.scalar("SELECT use_count FROM campaign_invites").await,
        1
    );
    assert!(
        pat.get(&path)
            .await
            .text(".card")
            .contains("already a member")
    );
}

#[sqlx::test]
async fn logged_out_visitor_sees_login_first_then_registers(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let token = invite(&app, &gm, id, "2").await;
    let path = format!("/invite/{token}");
    let anon = app.anon();

    let r = anon.get(&path).await;
    assert_eq!(
        r.ok()
            .attr("form[action='/login'] input[name=next]", "value"),
        Some(path.clone())
    );
    assert!(r.has(&format!("form[action='{path}/register']")));

    // validation errors re-render the register form, refilled, and use nothing up
    let r = anon
        .post(
            &format!("{path}/register"),
            &[
                ("username", "bo"),
                ("display_name", "Bob"),
                ("password", "longenough"),
                ("password_confirm", "longenough"),
            ],
        )
        .await;
    assert_eq!(r.error(), "The username must be 3 to 32 characters long.");
    assert_eq!(r.attr("#reg-username", "value").as_deref(), Some("bo"));
    let r = anon
        .post(
            &format!("{path}/register"),
            &[
                ("username", "bob"),
                ("display_name", "Bob"),
                ("password", "longenough"),
                ("password_confirm", "different"),
            ],
        )
        .await;
    assert_eq!(r.error(), "The passwords don't match.");
    let r = anon
        .post(
            &format!("{path}/register"),
            &[
                ("username", "GINA"),
                ("display_name", "Fake"),
                ("password", "longenough"),
                ("password_confirm", "longenough"),
            ],
        )
        .await;
    assert_eq!(r.error(), "That username is taken.");
    assert_eq!(
        app.scalar("SELECT use_count FROM campaign_invites").await,
        0
    );

    // success: logged in and a member
    let r = anon
        .post(
            &format!("{path}/register"),
            &[
                ("username", "bob"),
                ("display_name", "Bob"),
                ("password", "longenough"),
                ("password_confirm", "longenough"),
            ],
        )
        .await;
    assert_eq!(r.redirect(), format!("/campaigns/{id}"));
    assert_eq!(
        anon.get(&format!("/campaigns/{id}"))
            .await
            .ok()
            .text(".site-nav__user"),
        "Bob"
    );
    assert_eq!(
        app.scalar("SELECT use_count FROM campaign_invites").await,
        1
    );

    // the new account can log in normally
    let r = app
        .anon()
        .post("/login", &[("username", "bob"), ("password", "longenough")])
        .await;
    assert_eq!(r.redirect(), "/");
}

#[sqlx::test]
async fn used_up_and_unknown_invites(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let token = invite(&app, &gm, id, "1").await;
    let path = format!("/invite/{token}");
    app.user("pat").await.post(&path, &[]).await.redirect();

    let late = app.user("lou").await;
    let r = late.get(&path).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.text("h1"), "Invitation not valid");
    assert_eq!(late.post(&path, &[]).await.redirect(), path);
    assert_eq!(
        app.anon().get("/invite/bogus").await.status,
        StatusCode::NOT_FOUND
    );
    // and the used-up invite disappears from the GM's list
    assert_eq!(
        gm.get(&format!("/campaigns/{id}/members"))
            .await
            .count(".invite"),
        0
    );
}

#[sqlx::test]
async fn leaving_removing_and_changing_gm(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    let nina = app.user("nina").await;
    app.add_member(id, &mark).await;
    app.add_member(id, &nina).await;

    // members can't remove others, or change the GM
    let r = mark
        .delete(&format!("/campaigns/{id}/members/{}", nina.id))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = mark
        .post(
            &format!("/campaigns/{id}/gm"),
            &[("user_id", &mark.id.to_string())],
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // a member leaves: htmx redirect to the dashboard
    let r = nina
        .delete(&format!("/campaigns/{id}/members/{}", nina.id))
        .await;
    assert_eq!(r.ok().hx_redirect.as_deref(), Some("/"));
    assert_eq!(
        nina.get(&format!("/campaigns/{id}")).await.status,
        StatusCode::NOT_FOUND
    );

    // the GM can't leave
    let r = gm
        .delete(&format!("/campaigns/{id}/members/{}", gm.id))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // the new GM must be a member
    let r = gm
        .post(
            &format!("/campaigns/{id}/gm"),
            &[("user_id", &nina.id.to_string())],
        )
        .await;
    assert_eq!(r.error(), "The new GM must be a member of the campaign.");
    let r = gm
        .post(
            &format!("/campaigns/{id}/gm"),
            &[("user_id", &mark.id.to_string())],
        )
        .await;
    assert_eq!(r.redirect(), format!("/campaigns/{id}/members"));
    assert!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .text(".page-head .muted")
            .contains("GM: you")
    );
    // the old GM is now a plain member and can be removed by the new GM
    let r = mark
        .delete(&format!("/campaigns/{id}/members/{}", gm.id))
        .await;
    assert_eq!(r.ok().body, "");
}
