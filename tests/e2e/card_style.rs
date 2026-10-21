use crate::common::spawn_app;
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

/// The start of a PNG file: enough for the type check, which only looks at the first bytes.
fn png() -> Vec<u8> {
    let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
    data.extend_from_slice(&[0u8; 100]);
    data
}

fn campaign_form<'a>(name: &'a str, accent: &'a str) -> [(&'static str, &'a str); 5] {
    [
        ("name", name),
        ("system", "PF2e"),
        ("tagline", ""),
        ("description_md", ""),
        ("accent", accent),
    ]
}

#[sqlx::test]
async fn gm_picks_a_card_colour(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm
        .post("/campaigns", &campaign_form("Teal one", "teal"))
        .await
        .redirect_id("/campaigns/");
    assert!(gm.get("/").await.has(".campaign-card.accent--teal"));

    let r = gm.get(&format!("/campaigns/{id}/edit")).await;
    assert_eq!(
        r.ok()
            .attr("input[name=accent][checked]", "value")
            .as_deref(),
        Some("teal")
    );
    assert_eq!(r.count("input[name=accent]"), 11, "default + 10 colours");

    let r = gm
        .post(
            &format!("/campaigns/{id}"),
            &campaign_form("Teal one", "chartreuse"),
        )
        .await;
    assert_eq!(r.error(), "Pick one of the colours.");
    gm.post(&format!("/campaigns/{id}"), &campaign_form("Teal one", ""))
        .await
        .redirect();
    let r = gm.get("/").await;
    assert!(r.has(".campaign-card") && !r.has(".campaign-card[class*='accent--']"));
}

#[sqlx::test]
async fn gm_uploads_a_banner(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let path = format!("/campaigns/{id}/banner");

    assert_eq!(
        gm.get_raw(&path).await.status(),
        StatusCode::NOT_FOUND,
        "no banner yet"
    );
    assert!(!gm.get("/").await.has(".campaign-card__banner"));

    // only the GM uploads; only images are accepted
    assert_eq!(
        mark.upload(&path, "banner", "b.png", png()).await.status,
        StatusCode::NOT_FOUND
    );
    let svg =
        br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#.to_vec();
    assert_eq!(
        gm.upload(&path, "banner", "b.png", svg).await.error(),
        "Use a PNG, JPEG or WebP image."
    );
    let big = [png(), vec![0u8; 1024 * 1024]].concat();
    assert!(
        gm.upload(&path, "banner", "big.png", big)
            .await
            .error()
            .contains("too large")
    );
    assert_eq!(
        gm.upload(&path, "banner", "e.png", Vec::new())
            .await
            .error(),
        "Choose an image file first."
    );

    let r = gm.upload(&path, "banner", "b.png", png()).await;
    assert_eq!(r.redirect(), format!("/campaigns/{id}/edit#banner"));

    // members see it on the dashboard and the campaign page, served with the sniffed type
    let r = mark.get("/").await;
    let src = r.attr(".campaign-card__banner", "src").unwrap();
    assert!(src.starts_with(&format!("{path}?v=")));
    assert!(
        mark.get(&format!("/campaigns/{id}"))
            .await
            .has(".campaign-banner")
    );
    let img = mark.get_raw(&src).await;
    assert_eq!(img.status(), StatusCode::OK);
    assert_eq!(img.headers()["content-type"], "image/png");
    assert_eq!(img.headers()["x-content-type-options"], "nosniff");
    assert_eq!(img.bytes().await.unwrap().to_vec(), png());
    let outsider = app.user("olga").await;
    assert_eq!(
        outsider.get_raw(&path).await.status(),
        StatusCode::NOT_FOUND
    );

    // replace (JPEG), remove
    gm.upload(&path, "banner", "b.jpg", vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2])
        .await
        .redirect();
    assert_eq!(
        mark.get_raw(&path).await.headers()["content-type"],
        "image/jpeg"
    );
    assert_eq!(app.scalar("SELECT COUNT(*) FROM campaign_banners").await, 1);
    assert_eq!(mark.delete(&path).await.status, StatusCode::NOT_FOUND);
    let r = gm.delete(&path).await;
    assert_eq!(
        r.ok().hx_redirect,
        Some(format!("/campaigns/{id}/edit#banner"))
    );
    assert_eq!(app.scalar("SELECT COUNT(*) FROM campaign_banners").await, 0);

    // deleting the campaign deletes its banner
    gm.upload(&path, "banner", "b.png", png()).await.redirect();
    let ada = app.admin("ada").await;
    ada.delete(&format!("/admin/campaigns/{id}?confirm_name=Camp"))
        .await
        .ok();
    assert_eq!(app.scalar("SELECT COUNT(*) FROM campaign_banners").await, 0);
}
