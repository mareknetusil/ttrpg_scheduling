use crate::common::spawn_app;
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

#[sqlx::test]
async fn home_screen_app_and_error_pages(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gina = app.user("gina").await;

    // "Add to Home Screen": every page links the manifest and the icons, which are served
    let r = gina.get("/").await;
    let manifest = r.ok().attr("link[rel=manifest]", "href").unwrap();
    for href in [
        manifest.clone(),
        r.attr("link[rel=icon]", "href").unwrap(),
        r.attr("link[rel=apple-touch-icon]", "href").unwrap(),
    ] {
        assert_eq!(gina.get_raw(&href).await.status(), StatusCode::OK, "{href}");
    }
    let r = gina.get_raw(&manifest).await;
    assert_eq!(r.headers()["content-type"], "application/manifest+json");
    let manifest: serde_json::Value = serde_json::from_str(&r.text().await.unwrap()).unwrap();
    assert_eq!(manifest["display"], "standalone");
    for icon in manifest["icons"].as_array().unwrap() {
        let src = icon["src"].as_str().unwrap();
        assert_eq!(gina.get_raw(src).await.status(), StatusCode::OK, "{src}");
    }

    // An error is a page with a way home (a home-screen app has no back button)
    for path in ["/no-such-page", "/campaigns/999"] {
        let r = gina.get(path).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(r.text("h1"), "Page not found");
        assert!(r.has("a[href='/']"), "{path}");
    }
}
