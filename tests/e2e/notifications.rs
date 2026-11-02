use crate::common::{TestApp, User, spawn_app};
use reqwest::StatusCode;
use sqlx::sqlite::SqlitePool;

/// A campaign with a GM (Gina) and two members (Mark, Nina).
async fn setup(app: &TestApp) -> (i64, User, User, User) {
    let gm = app.user("gina").await;
    let id = gm.create_campaign("Camp").await;
    let mark = app.user("mark").await;
    let nina = app.user("nina").await;
    app.add_member(id, &mark).await;
    app.add_member(id, &nina).await;
    (id, gm, mark, nina)
}

/// The user's unseen notifications as "Actor text", newest first (from the notifications page).
async fn unseen(user: &User) -> Vec<String> {
    user.get("/notifications")
        .await
        .ok()
        .texts("#unseen .notification__text")
}

async fn unseen_count(app: &TestApp, user: &User) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM notifications WHERE user_id = ? AND seen_at IS NULL")
        .bind(user.id)
        .fetch_one(&app.db)
        .await
        .unwrap()
}

async fn new_session(gm: &User, campaign: i64) -> i64 {
    gm.post(
        &format!("/campaigns/{campaign}/sessions"),
        &[("title", ""), ("starts_at", "2030-01-10T18:00")],
    )
    .await
    .redirect_id("/sessions/")
}

#[sqlx::test]
async fn others_are_told_and_opening_the_page_clears_it(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, nina) = setup(&app).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "Session 5"), ("options", "2030-01-10T18:00")],
        )
        .await
        .redirect_id("/schedule/");

    // the GM did it: nothing for her; both members get it
    assert_eq!(unseen_count(&app, &gm).await, 0);
    assert_eq!(
        unseen(&mark).await,
        ["Gina opened the scheduling poll “Session 5”"]
    );
    assert_eq!(unseen_count(&app, &nina).await, 1);

    // the dashboard card and the nav count
    let r = mark.get("/").await;
    assert_eq!(
        r.ok().texts("#notifications .notification__text"),
        ["Gina opened the scheduling poll “Session 5”"]
    );
    assert_eq!(
        r.attr("#notifications .notification__link", "href"),
        Some(format!("/schedule/{poll}"))
    );
    assert!(r.text("#notifications").contains("Camp · just now"));
    assert_eq!(r.text(".site-nav__count"), "1");
    assert_eq!(
        r.attr("#nav-notifications", "aria-label").as_deref(),
        Some("Notifications: 1 new")
    );
    // looking at the list doesn't clear it
    assert_eq!(unseen_count(&app, &mark).await, 1);

    // opening the poll does, and its own nav already shows that
    let r = mark.get(&format!("/schedule/{poll}")).await;
    assert!(!r.ok().has(".site-nav__count"));
    assert_eq!(unseen_count(&app, &mark).await, 0);
    assert!(!mark.get("/").await.has("#notifications"));
    let r = mark.get("/notifications").await;
    assert_eq!(r.text("#all-seen"), "You're all caught up.");
    assert_eq!(
        r.texts("#seen .notification__text"),
        ["Gina opened the scheduling poll “Session 5”"]
    );
    assert_eq!(unseen_count(&app, &nina).await, 1, "Nina hasn't looked");

    // a different action is a new notification
    gm.post(&format!("/schedule/{poll}/cancel"), &[])
        .await
        .redirect();
    assert_eq!(
        unseen(&mark).await,
        ["Gina cancelled the scheduling poll “Session 5”"]
    );
    assert_eq!(unseen_count(&app, &nina).await, 2);
}

#[sqlx::test]
async fn edits_fold_into_one_until_seen(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, nina) = setup(&app).await;
    let session = new_session(&gm, id).await;
    assert_eq!(
        unseen(&mark).await,
        ["Gina added session 1, Thu 10 Jan 2030, 18:00"]
    );
    // moving it before Mark looked: still the one "added" notification, with the new date
    gm.post(
        &format!("/sessions/{session}"),
        &[("title", ""), ("starts_at", "2030-01-11T18:00")],
    )
    .await
    .redirect();
    assert_eq!(
        unseen(&mark).await,
        ["Gina added session 1, Fri 11 Jan 2030, 18:00"]
    );
    mark.get(&format!("/sessions/{session}")).await.ok();

    // Mark adds a recap and edits it twice: one notification for the others, at the entry
    let entries = format!("/sessions/{session}/entries");
    let entry = {
        let r = mark
            .post(&entries, &[("kind", "recap"), ("body_md", "We met.")])
            .await;
        r.redirect()
            .rsplit("#entry-")
            .next()
            .unwrap()
            .parse::<i64>()
            .unwrap()
    };
    for body in ["We met a dragon.", "We met two dragons."] {
        mark.post(
            &format!("/entries/{entry}"),
            &[("kind", "recap"), ("body_md", body)],
        )
        .await
        .redirect();
    }
    assert_eq!(
        unseen(&nina).await,
        [
            "Mark added a recap to session 1",
            "Gina added session 1, Fri 11 Jan 2030, 18:00"
        ]
    );
    let r = nina.get("/notifications").await;
    assert_eq!(
        r.attr("#unseen .notification__link", "href"),
        Some(format!("/sessions/{session}#entry-{entry}"))
    );
    assert_eq!(
        unseen_count(&app, &mark).await,
        0,
        "not about his own recap"
    );

    // once seen, the next edit is new; the GM moving and cancelling the session is too
    gm.get(&format!("/sessions/{session}")).await.ok();
    mark.post(
        &format!("/entries/{entry}"),
        &[("kind", "recap"), ("body_md", "Three dragons.")],
    )
    .await
    .redirect();
    assert_eq!(unseen(&gm).await, ["Mark edited the recap of session 1"]);
    gm.post(
        &format!("/sessions/{session}"),
        &[
            ("title", ""),
            ("starts_at", "2030-01-11T18:00"),
            ("status", "cancelled"),
        ],
    )
    .await
    .redirect();
    assert_eq!(unseen(&mark).await, ["Gina cancelled session 1"]);
    // saving without a change of date or status tells nobody
    gm.post(
        &format!("/sessions/{session}"),
        &[("title", "Renamed"), ("starts_at", "2030-01-11T18:00")],
    )
    .await
    .redirect();
    assert_eq!(unseen_count(&app, &mark).await, 1);
}

#[sqlx::test]
async fn each_page_clears_what_it_shows(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    let session = new_session(&gm, id).await;
    let poll = gm
        .post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", "S2"), ("options", "2030-02-10T18:00")],
        )
        .await
        .redirect_id("/schedule/");
    // comments on the campaign, the session and the scheduling poll
    gm.post(
        &format!("/campaigns/{id}/comments"),
        &[("body_md", "Hi **all**")],
    )
    .await
    .ok();
    gm.post(
        &format!("/sessions/{session}/comments"),
        &[("body_md", "Bring snacks")],
    )
    .await
    .ok();
    gm.post(
        &format!("/schedule/{poll}/comments"),
        &[("body_md", "Vote please")],
    )
    .await
    .ok();
    // a poll on the session, a page, campaign details, a GM change
    gm.post(
        &format!("/campaigns/{id}/polls"),
        &[
            ("question", "Pizza?"),
            ("options", "Yes"),
            ("options", "No"),
            ("game_session_id", &session.to_string()),
        ],
    )
    .await
    .redirect();
    gm.post(
        &format!("/campaigns/{id}/pages"),
        &[("title", "Lore"), ("body_md", "Old")],
    )
    .await
    .redirect();
    gm.post(
        &format!("/campaigns/{id}"),
        &[
            ("name", "Camp"),
            ("system", "PF2e"),
            ("tagline", "Now with a tagline"),
            ("description_md", ""),
        ],
    )
    .await
    .redirect();
    gm.post(
        &format!("/campaigns/{id}/gm"),
        &[("user_id", &mark.id.to_string())],
    )
    .await
    .redirect();
    let all = unseen(&mark).await;
    assert_eq!(
        all,
        [
            // Mark is now the GM himself, but Gina did it
            "Gina made Mark the GM",
            "Gina edited the campaign details",
            "Gina added the page “Lore”",
            "Gina asked “Pizza?”",
            "Gina commented on “S2”",
            "Gina commented on session 1",
            "Gina commented on the campaign",
            "Gina opened the scheduling poll “S2”",
            "Gina added session 1, Thu 10 Jan 2030, 18:00",
        ]
    );
    let r = mark.get("/notifications").await;
    assert_eq!(
        r.texts("#unseen .notification__excerpt"),
        ["“Vote please”", "“Bring snacks”", "“Hi **all**”"]
    );

    // the campaign overview: details, campaign comments (not the GM change)
    mark.get(&format!("/campaigns/{id}")).await.ok();
    assert_eq!(unseen(&mark).await.len(), all.len() - 2);
    // the game session: the session, its comment and poll
    mark.get(&format!("/sessions/{session}")).await.ok();
    assert_eq!(unseen(&mark).await.len(), all.len() - 5);
    // the scheduling poll: the poll and its comment
    mark.get(&format!("/schedule/{poll}")).await.ok();
    // the page, then the members page
    mark.get(&format!("/campaigns/{id}/pages/lore")).await.ok();
    assert_eq!(unseen(&mark).await, ["Gina made Mark the GM"]);
    mark.get(&format!("/campaigns/{id}/members")).await.ok();
    assert_eq!(unseen_count(&app, &mark).await, 0);

    // the new GM edits the page: Gina is told, and the link opens the page
    mark.post(
        &format!("/campaigns/{id}/pages/lore"),
        &[("title", "Lore"), ("body_md", "New")],
    )
    .await
    .redirect();
    let r = gm.get("/notifications").await;
    assert_eq!(
        r.ok().texts("#unseen .notification__text"),
        ["Mark edited the page “Lore”"]
    );
    assert_eq!(
        r.attr("#unseen .notification__link", "href"),
        Some(format!("/campaigns/{id}/pages/lore"))
    );
}

#[sqlx::test]
async fn quiet_changes_tell_nobody(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    // the accent only, and the GM "changed" to herself
    gm.post(
        &format!("/campaigns/{id}"),
        &[
            ("name", "Camp"),
            ("system", "PF2e"),
            ("tagline", ""),
            ("description_md", ""),
            ("accent", "teal"),
        ],
    )
    .await
    .redirect();
    gm.post(
        &format!("/campaigns/{id}/gm"),
        &[("user_id", &gm.id.to_string())],
    )
    .await
    .redirect();
    // votes and answers
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
    assert_eq!(unseen_count(&app, &gm).await, 0);
    assert_eq!(unseen_count(&app, &mark).await, 1, "only the poll itself");
}

#[sqlx::test]
async fn deleting_and_leaving_remove_notifications(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, nina) = setup(&app).await;
    let session = new_session(&gm, id).await;
    gm.post(
        &format!("/sessions/{session}/comments"),
        &[("body_md", "Oops")],
    )
    .await
    .ok();
    let comment = app.scalar("SELECT id FROM comments").await;
    assert_eq!(unseen_count(&app, &mark).await, 2);
    gm.delete(&format!("/comments/{comment}")).await.ok();
    assert_eq!(
        unseen(&mark).await,
        ["Gina added session 1, Thu 10 Jan 2030, 18:00"]
    );

    // a deleted page or entry takes its notifications along too
    gm.post(
        &format!("/campaigns/{id}/pages"),
        &[("title", "Lore"), ("body_md", "x")],
    )
    .await
    .redirect();
    gm.delete(&format!("/campaigns/{id}/pages/lore")).await;
    mark.post(
        &format!("/sessions/{session}/entries"),
        &[("kind", "loot"), ("body_md", "A sword")],
    )
    .await
    .redirect();
    assert_eq!(unseen(&nina).await[0], "Mark added loot to session 1");
    let entry = app.scalar("SELECT id FROM game_session_entries").await;
    mark.delete(&format!("/entries/{entry}")).await.ok();
    assert_eq!(unseen_count(&app, &nina).await, 1);

    // leaving: Nina's notifications from the campaign go
    nina.delete(&format!("/campaigns/{id}/members/{}", nina.id))
        .await;
    assert_eq!(unseen_count(&app, &nina).await, 0);

    // a new member gets nothing from before, only what comes after
    let olga = app.user("olga").await;
    app.add_member(id, &olga).await;
    assert!(unseen(&olga).await.is_empty());
    gm.post(
        &format!("/campaigns/{id}/comments"),
        &[("body_md", "Welcome")],
    )
    .await
    .ok();
    assert_eq!(unseen(&olga).await, ["Gina commented on the campaign"]);

    // a site admin who isn't a member gets none; taking the GM role over tells the members
    let ada = app.admin("ada").await;
    assert_eq!(unseen_count(&app, &ada).await, 0);
    ada.post(&format!("/campaigns/{id}/gm/take"), &[])
        .await
        .redirect();
    assert_eq!(unseen(&olga).await[0], "Ada took over as GM");
}

#[sqlx::test]
async fn mark_all_as_seen(db: SqlitePool) {
    let app = spawn_app(db).await;
    let (id, gm, mark, _) = setup(&app).await;
    for title in ["A", "B", "C", "D", "E", "F"] {
        gm.post(
            &format!("/campaigns/{id}/schedule"),
            &[("title", title), ("options", "2030-01-10T18:00")],
        )
        .await
        .redirect();
    }
    // the dashboard shows the newest five and links to the rest
    let r = mark.get("/").await;
    assert_eq!(r.ok().count("#notifications .notification"), 5);
    assert_eq!(
        r.text("#notifications a[href='/notifications']"),
        "All notifications (6 new)"
    );
    assert_eq!(r.text(".site-nav__count"), "6");
    assert_eq!(
        r.attr("#mark-all-seen", "hx-put").as_deref(),
        Some("/notifications/seen")
    );

    let r = mark.put("/notifications/seen", &[]).await;
    assert!(r.ok().hx_refresh);
    assert_eq!(unseen_count(&app, &mark).await, 0);
    assert_eq!(unseen_count(&app, &gm).await, 0);
    let r = mark.get("/notifications").await;
    assert_eq!(r.count("#seen .notification"), 6);
    assert!(!r.has("#mark-all-seen"));

    // only for people who are logged in
    let anon = app.anon();
    assert_eq!(
        anon.get("/notifications").await.status,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        anon.put("/notifications/seen", &[]).await.status,
        StatusCode::SEE_OTHER
    );
}
