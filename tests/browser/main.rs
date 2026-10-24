//! Browser tests for the parts that need JavaScript: htmx swaps and Alpine state. They drive a
//! headless Firefox through geckodriver (WebDriver) with `thirtyfour`.
//!
//! They need `geckodriver` (`cargo install geckodriver`) and Firefox, so they're `#[ignore]`d:
//!
//! ```sh
//! cargo test --test browser -- --ignored
//! ```
//!
//! Firefox is found on the PATH, or set `FIREFOX_BINARY`; the Flatpak build is found too.

#[path = "../e2e/common.rs"]
#[allow(dead_code)]
mod common;

use common::{PASSWORD, TestApp, spawn_app};
use sqlx::sqlite::SqlitePool;
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};
use thirtyfour::prelude::*;

/// A headless Firefox, driven through its own geckodriver process.
struct Browser {
    driver: WebDriver,
    geckodriver: Child,
    base: String,
}

impl Browser {
    async fn start(app: &TestApp) -> Browser {
        // A free port for geckodriver
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let geckodriver =
            Command::new(std::env::var("GECKODRIVER").unwrap_or("geckodriver".into()))
                .args(["--port", &port.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("geckodriver not found (cargo install geckodriver)");
        for _ in 0..50 {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let mut caps = DesiredCapabilities::firefox();
        caps.add_arg("-headless").unwrap();
        if let Some(binary) = firefox_binary() {
            caps.set_firefox_binary(&binary).unwrap();
        }
        let driver = WebDriver::new(format!("http://127.0.0.1:{port}"), caps)
            .await
            .expect("can't start Firefox");
        Browser {
            driver,
            geckodriver,
            base: app.base.clone(),
        }
    }

    async fn goto(&self, path: &str) {
        self.driver
            .goto(format!("{}{path}", self.base))
            .await
            .unwrap();
    }

    /// Logs in through the login form, like a person would.
    async fn login(&self, username: &str) {
        self.goto("/login").await;
        self.el("#username")
            .await
            .send_keys(username)
            .await
            .unwrap();
        self.el("#password")
            .await
            .send_keys(PASSWORD)
            .await
            .unwrap();
        self.el("form[action='/login'] button")
            .await
            .click()
            .await
            .unwrap();
        self.driver
            .query(By::Css(".site-nav__user"))
            .first()
            .await
            .expect("not logged in");
    }

    /// Waits (up to the default 20 s) for an element.
    async fn el(&self, css: &str) -> WebElement {
        self.driver.query(By::Css(css)).first().await.unwrap()
    }

    async fn js<T: serde::de::DeserializeOwned>(&self, script: &str) -> T {
        self.driver
            .execute(script, Vec::new())
            .await
            .unwrap()
            .convert()
            .unwrap()
    }

    /// Marks the current page; `still_same_page` is true until the browser loads another page.
    /// That's how the tests prove htmx swapped a fragment instead of reloading.
    async fn mark_page(&self) {
        self.js::<serde_json::Value>("window.__marked = true; return null;")
            .await;
    }

    async fn still_same_page(&self) -> bool {
        self.js("return window.__marked === true;").await
    }

    async fn quit(self) {
        let Browser {
            driver,
            mut geckodriver,
            ..
        } = self;
        driver.quit().await.unwrap();
        geckodriver.kill().ok();
    }
}

fn firefox_binary() -> Option<String> {
    if let Ok(binary) = std::env::var("FIREFOX_BINARY") {
        return Some(binary);
    }
    let flatpak =
        "/var/lib/flatpak/app/org.mozilla.firefox/current/active/files/lib/firefox/firefox";
    Path::new(flatpak).exists().then(|| flatpak.to_string())
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn voting_grid_updates_in_place(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-11T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    let opt = app.scalar("SELECT MIN(id) FROM schedule_options").await;

    let b = Browser::start(&app).await;
    b.login("mark").await;
    b.goto(&format!("/schedule/{poll}")).await;
    b.mark_page().await;

    b.el(&format!("#vote-{opt}-yes"))
        .await
        .click()
        .await
        .unwrap();
    // the grid is swapped: the new button is pressed, the tally and "Most yes" update
    b.el(&format!("#vote-{opt}-yes[aria-pressed='true']")).await;
    assert_eq!(
        b.el("tr.is-best .badge").await.text().await.unwrap(),
        "Most yes"
    );
    b.el(&format!("#vote-{opt}-no"))
        .await
        .click()
        .await
        .unwrap();
    b.el(&format!("#vote-{opt}-no[aria-pressed='true']")).await;
    assert_eq!(
        app.scalar("SELECT COUNT(*) FROM schedule_votes WHERE answer = 'no'")
            .await,
        1
    );
    // clicking the pressed answer again clears it
    b.el(&format!("#vote-{opt}-no"))
        .await
        .click()
        .await
        .unwrap();
    b.el(&format!("#vote-{opt}-no[aria-pressed='false']")).await;
    // "Answer all: Maybe" fills both dates
    b.el("#answer-rest-maybe").await.click().await.unwrap();
    b.el(&format!("#vote-{opt}-maybe[aria-pressed='true']"))
        .await;
    assert!(b.still_same_page().await, "htmx swaps, not page loads");
    assert_eq!(
        app.scalar("SELECT COUNT(*) FROM schedule_votes WHERE answer = 'maybe'")
            .await,
        2
    );
    b.quit().await;
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn new_poll_date_list_and_markdown_preview(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;

    let b = Browser::start(&app).await;
    b.login("gina").await;
    b.goto(&format!("/campaigns/{id}/schedule/new")).await;

    // Markdown preview tab (Alpine tab + htmx POST /markdown/preview)
    b.el("#description_md")
        .await
        .send_keys("Bring **dice**")
        .await
        .unwrap();
    b.el(".md-editor .tabs__tab:nth-child(2)")
        .await
        .click()
        .await
        .unwrap();
    assert_eq!(
        b.el("#description_md-preview strong")
            .await
            .text()
            .await
            .unwrap(),
        "dice"
    );
    assert!(!b.el("#description_md").await.is_displayed().await.unwrap());
    b.el(".md-editor .tabs__tab:nth-child(1)")
        .await
        .click()
        .await
        .unwrap();

    // Date list (Alpine): set the first date, add the same day next week
    b.js::<serde_json::Value>(
        "const i = document.querySelector('input[name=options]');
         i.value = '2030-01-10T18:00'; i.dispatchEvent(new Event('input')); return null;",
    )
    .await;
    let buttons = b
        .driver
        .find_all(By::Css(".date-list .row button"))
        .await
        .unwrap();
    buttons[1].click().await.unwrap(); // "+ Same day next week"
    b.el("input[name=options]:nth-of-type(1)").await;
    let values: Vec<String> = b
        .js("return [...document.querySelectorAll('input[name=options]')].map(i => i.value);")
        .await;
    assert_eq!(values, ["2030-01-10T18:00", "2030-01-17T18:00"]);

    b.el("form[action$='/schedule'] button[type=submit]")
        .await
        .click()
        .await
        .unwrap();
    b.el(".vote-grid").await;
    assert_eq!(app.scalar("SELECT COUNT(*) FROM schedule_options").await, 2);
    b.quit().await;
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn comments_post_and_delete_in_place(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;

    let b = Browser::start(&app).await;
    b.login("gina").await;
    b.goto(&format!("/campaigns/{id}")).await;
    b.mark_page().await;

    b.el("#comment-body")
        .await
        .send_keys("First!")
        .await
        .unwrap();
    b.el(".comment-form button").await.click().await.unwrap();
    assert_eq!(
        b.el(".comment .prose").await.text().await.unwrap(),
        "First!"
    );
    // the form was reset and the "No comments yet" note removed
    assert_eq!(
        b.el("#comment-body")
            .await
            .value()
            .await
            .unwrap()
            .as_deref(),
        Some("")
    );
    assert!(
        b.driver
            .find_all(By::Css("#no-comments"))
            .await
            .unwrap()
            .is_empty()
    );

    // delete asks for confirmation (hx-confirm), then removes the comment
    b.el(".comment__actions .link-button--danger")
        .await
        .click()
        .await
        .unwrap();
    b.driver.accept_alert().await.unwrap();
    b.driver
        .query(By::Css(".comment"))
        .wait(Duration::from_secs(5), Duration::from_millis(100))
        .not_exists()
        .await
        .unwrap();
    assert!(b.still_same_page().await);
    assert_eq!(app.scalar("SELECT COUNT(*) FROM comments").await, 0);
    b.quit().await;
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn invite_page_switches_to_registration(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    gm.post(&format!("/campaigns/{id}/invites"), &[("max_uses", "1")])
        .await
        .redirect();
    let token: String = sqlx::query_scalar("SELECT token FROM campaign_invites")
        .fetch_one(&app.db)
        .await
        .unwrap();

    let b = Browser::start(&app).await;
    b.goto(&format!("/invite/{token}")).await;
    // login form first, the register form hidden until asked for
    assert!(b.el("#login-username").await.is_displayed().await.unwrap());
    assert!(!b.el("#reg-username").await.is_displayed().await.unwrap());
    b.el(".switch .link-button").await.click().await.unwrap();
    b.el("#reg-username")
        .await
        .wait_until()
        .displayed()
        .await
        .unwrap();

    b.el("#reg-username")
        .await
        .send_keys("newbie")
        .await
        .unwrap();
    b.el("#display_name")
        .await
        .send_keys("Newbie")
        .await
        .unwrap();
    b.el("#password")
        .await
        .send_keys("longenough")
        .await
        .unwrap();
    b.el("#password_confirm")
        .await
        .send_keys("longenough")
        .await
        .unwrap();
    b.el("form[action$='/register'] button")
        .await
        .click()
        .await
        .unwrap();
    assert_eq!(
        b.el(".site-nav__user").await.text().await.unwrap(),
        "Newbie"
    );
    assert_eq!(b.el("h1").await.text().await.unwrap(), "Camp");
    b.quit().await;
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn deleting_a_campaign_needs_the_typed_name(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Doomed \"one\"").await;
    app.admin("ada").await;

    let b = Browser::start(&app).await;
    b.login("ada").await;
    b.goto(&format!("/admin/campaigns/{id}/delete")).await;
    let button = b.el(".danger-zone button[type=submit]").await;
    assert!(
        !button.is_enabled().await.unwrap(),
        "disabled before typing"
    );
    b.el("#confirm_name")
        .await
        .send_keys("Doomed")
        .await
        .unwrap();
    assert!(
        !button.is_enabled().await.unwrap(),
        "disabled for a partial name"
    );
    b.el("#confirm_name")
        .await
        .send_keys(" \"one\"")
        .await
        .unwrap();
    button.wait_until().enabled().await.unwrap();
    button.click().await.unwrap();
    // HX-Redirect to the campaign list
    b.el(".admin-tabs a[aria-current='page'][href='/admin/campaigns']")
        .await;
    assert_eq!(app.scalar("SELECT COUNT(*) FROM campaigns").await, 0);
    b.quit().await;
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn failed_votes_are_reported(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "S1"), ("options", "2030-01-10T18:00")],
        )
        .await
        .redirect_id("/schedule/");
    let opt = app.scalar("SELECT MIN(id) FROM schedule_options").await;

    let b = Browser::start(&app).await;
    b.login("mark").await;
    b.goto(&format!("/schedule/{poll}")).await;

    // The GM cancels while the page is open: the vote reloads the page (HX-Refresh), which
    // says why the buttons are gone.
    gm.post(&format!("/schedule/{poll}/cancel"), &[])
        .await
        .redirect();
    b.mark_page().await;
    b.el(&format!("#vote-{opt}-yes"))
        .await
        .click()
        .await
        .unwrap();
    b.driver
        .query(By::XPath("//p[contains(., 'This poll was cancelled.')]"))
        .first()
        .await
        .unwrap();
    assert!(!b.still_same_page().await, "the page was reloaded");

    // A request that fails (404: no longer a member) shows the error notice instead of nothing
    let other = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "S2"), ("options", "2030-01-11T18:00")],
        )
        .await
        .redirect_id("/schedule/");
    let opt = app.scalar("SELECT MAX(id) FROM schedule_options").await;
    b.goto(&format!("/schedule/{other}")).await;
    b.el(&format!("#vote-{opt}-yes")).await;
    sqlx::query("DELETE FROM campaign_members WHERE user_id = ?")
        .bind(mark.id)
        .execute(&app.db)
        .await
        .unwrap();
    b.el(&format!("#vote-{opt}-yes"))
        .await
        .click()
        .await
        .unwrap();
    b.el("#htmx-error:not([hidden])").await;
    assert!(
        b.el("#htmx-error__text")
            .await
            .text()
            .await
            .unwrap()
            .contains("out of date")
    );
    b.quit().await;
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn edit_poll_shows_answers_per_date(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-11T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    let opt = app.scalar("SELECT MIN(id) FROM schedule_options").await;
    gm.put(
        &format!("/schedule/{poll}/options/{opt}/vote"),
        &[("answer", "yes")],
    )
    .await
    .ok();

    let b = Browser::start(&app).await;
    b.login("gina").await;
    b.goto(&format!("/schedule/{poll}/edit")).await;
    b.el(".date-list__row").await;
    let hints: Vec<String> = b
        .js(
            r#"return [...document.querySelectorAll(".date-list__row span.muted")]
                 .map(s => s.offsetParent ? s.textContent : "");"#,
        )
        .await;
    assert_eq!(hints, ["1 answer", ""]);
    b.quit().await;
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn slot_dates_are_added_in_place(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    gm.post(
        &format!("/campaigns/{id}/schedule/patterns"),
        &[
            ("weekdays", "5"),
            ("start_time", "18:00"),
            ("every_n_weeks", "1"),
        ],
    )
    .await
    .redirect();

    let b = Browser::start(&app).await;
    b.login("gina").await;
    b.goto(&format!("/campaigns/{id}/schedule/new")).await;
    b.mark_page().await;
    b.el("#title").await.clear().await.unwrap();
    b.el("#title").await.send_keys("Kept title").await.unwrap();
    b.js::<serde_json::Value>(
        "const i = document.querySelector('input[name=options]');
         i.value = '2099-01-01T18:00'; i.dispatchEvent(new Event('input')); return null;",
    )
    .await;
    b.js::<serde_json::Value>("document.getElementById('weeks').value = '2'; return null;")
        .await;
    b.el(".fill-form button").await.click().await.unwrap();
    b.driver
        .query(By::XPath(
            "//*[contains(@class, 'fill-form__message')][starts-with(., 'Added ')]",
        ))
        .first()
        .await
        .unwrap();
    let values: Vec<String> = b
        .js("return [...document.querySelectorAll('input[name=options]')].map(i => i.value);")
        .await;
    // the next Saturdays (1 or 2, depending on today), then the typed date (sorted); the title
    // is untouched
    assert!((2..=3).contains(&values.len()), "{values:?}");
    assert_eq!(values.last().unwrap(), "2099-01-01T18:00");
    assert_eq!(
        b.el("#title").await.prop("value").await.unwrap().as_deref(),
        Some("Kept title")
    );
    assert!(b.still_same_page().await);

    // adding again changes nothing
    b.el(".fill-form button").await.click().await.unwrap();
    b.driver
        .query(By::XPath(
            "//*[contains(@class, 'fill-form__message')][contains(., 'already in the list')]",
        ))
        .first()
        .await
        .unwrap();
    b.quit().await;
}

#[sqlx::test]
#[ignore = "needs Firefox and geckodriver"]
async fn voting_grid_fits_a_phone(db: SqlitePool) {
    let app = spawn_app(db).await;
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    app.add_member(id, &mark).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[
                ("title", "S1"),
                ("options", "2030-01-10T18:00"),
                ("options", "2030-01-11T18:00"),
            ],
        )
        .await
        .redirect_id("/schedule/");
    // Both say yes: the date gets the "No one said no" and "Most yes" badges
    let opt = app.scalar("SELECT MIN(id) FROM schedule_options").await;
    for u in [&gm, &mark] {
        u.put(
            &format!("/schedule/{poll}/options/{opt}/vote"),
            &[("answer", "yes")],
        )
        .await;
    }

    let b = Browser::start(&app).await;
    b.login("gina").await;
    b.goto(&format!("/schedule/{poll}")).await;
    b.el("tr.is-best .badge").await;
    // Headless Firefox won't make the window narrower than 500 px, so narrow the page instead
    let (date, wrap): (f64, f64) = b
        .js(r#"
            document.body.style.maxWidth = "360px";
            return [document.querySelector("tr.is-best .vote-grid__date").offsetWidth,
                    document.querySelector(".vote-grid-wrap").clientWidth];
        "#)
        .await;
    // The sticky date column must leave room for the answers, or it covers the whole row
    assert!(date < wrap / 2.0, "date column {date}px of {wrap}px");
    b.quit().await;
}
