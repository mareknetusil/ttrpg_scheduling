//! Test helpers ("fixtures"): start the real app on a random port against the test's own
//! database, create users and log them in, and look at the HTML that comes back.

use reqwest::{Client, StatusCode, redirect::Policy};
use scraper::{Html, Selector};
use sqlx::sqlite::SqlitePool;
use ttrpg_scheduling::{AppState, auth::password, router};

/// Every test user has this password.
pub const PASSWORD: &str = "correct horse battery";

pub struct TestApp {
    pub base: String,
    pub db: SqlitePool,
}

/// Starts the app on `127.0.0.1:<random port>`. The server task lives as long as the test's
/// tokio runtime, i.e. until the test ends.
pub async fn spawn_app(db: SqlitePool) -> TestApp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(AppState::new(db.clone(), chrono_tz::Europe::Prague));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    TestApp {
        base: format!("http://{addr}"),
        db,
    }
}

/// A browser-like client: keeps cookies, doesn't follow redirects (so tests can check them).
pub fn client() -> Client {
    Client::builder()
        .cookie_store(true)
        .redirect(Policy::none())
        .build()
        .unwrap()
}

impl TestApp {
    /// A client that isn't logged in.
    pub fn anon(&self) -> User {
        User {
            id: 0,
            client: client(),
            base: self.base.clone(),
        }
    }

    /// Creates a user and logs them in.
    pub async fn user(&self, username: &str) -> User {
        self.create_user(username, false).await
    }

    pub async fn admin(&self, username: &str) -> User {
        self.create_user(username, true).await
    }

    async fn create_user(&self, username: &str, is_admin: bool) -> User {
        let hash = password::hash(PASSWORD.to_string()).await.unwrap();
        let display_name = capitalize(username);
        let id = sqlx::query(
            "INSERT INTO users (username, display_name, password_hash, is_admin) VALUES (?, ?, ?, ?)",
        )
        .bind(username)
        .bind(&display_name)
        .bind(hash)
        .bind(is_admin)
        .execute(&self.db)
        .await
        .unwrap()
        .last_insert_rowid();
        let user = User {
            id,
            client: client(),
            base: self.base.clone(),
        };
        let r = user
            .post("/login", &[("username", username), ("password", PASSWORD)])
            .await;
        assert_eq!(
            r.status,
            StatusCode::SEE_OTHER,
            "login of {username} failed"
        );
        user
    }

    /// Adds a user to a campaign directly in the DB (the invite flow has its own tests).
    pub async fn add_member(&self, campaign_id: i64, user: &User) {
        sqlx::query("INSERT INTO campaign_members (campaign_id, user_id) VALUES (?, ?)")
            .bind(campaign_id)
            .bind(user.id)
            .execute(&self.db)
            .await
            .unwrap();
    }

    /// One number from the test DB. sqlx 0.9 wants SQL as a `&'static str` (so it can't be built
    /// from user input by accident).
    pub async fn scalar(&self, sql: &'static str) -> i64 {
        sqlx::query_scalar(sql).fetch_one(&self.db).await.unwrap()
    }
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A logged-in (or anonymous) browser session.
pub struct User {
    pub id: i64,
    pub client: Client,
    base: String,
}

impl User {
    pub async fn get(&self, path: &str) -> Resp {
        Resp::from(self.client.get(self.url(path)).send().await.unwrap()).await
    }

    /// A plain form POST.
    pub async fn post(&self, path: &str, form: &[(&str, &str)]) -> Resp {
        let r = self.client.post(self.url(path)).form(form).send().await;
        Resp::from(r.unwrap()).await
    }

    /// An htmx request (`HX-Request: true`) with the given method and form body.
    pub async fn htmx(&self, method: reqwest::Method, path: &str, form: &[(&str, &str)]) -> Resp {
        let r = self
            .client
            .request(method, self.url(path))
            .header("HX-Request", "true")
            .form(form)
            .send()
            .await;
        Resp::from(r.unwrap()).await
    }

    pub async fn delete(&self, path: &str) -> Resp {
        self.htmx(reqwest::Method::DELETE, path, &[]).await
    }

    pub async fn put(&self, path: &str, form: &[(&str, &str)]) -> Resp {
        self.htmx(reqwest::Method::PUT, path, form).await
    }

    /// Creates a campaign (this user becomes its GM) and returns its id.
    pub async fn create_campaign(&self, name: &str) -> i64 {
        let r = self
            .post(
                "/campaigns",
                &[
                    ("name", name),
                    ("system", "PF2e"),
                    ("tagline", ""),
                    ("description_md", ""),
                ],
            )
            .await;
        r.redirect_id("/campaigns/")
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

/// A response with its body read, plus helpers to query the HTML.
pub struct Resp {
    pub status: StatusCode,
    pub location: Option<String>,
    pub hx_redirect: Option<String>,
    pub body: String,
}

impl Resp {
    async fn from(r: reqwest::Response) -> Resp {
        let header = |name: &str| {
            r.headers()
                .get(name)
                .map(|v| v.to_str().unwrap().to_string())
        };
        let location = header("location");
        let hx_redirect = header("hx-redirect");
        let status = r.status();
        Resp {
            status,
            location,
            hx_redirect,
            body: r.text().await.unwrap(),
        }
    }

    /// Asserts a 303 redirect and returns its target.
    #[track_caller]
    pub fn redirect(&self) -> &str {
        assert_eq!(self.status, StatusCode::SEE_OTHER, "body: {}", self.body);
        self.location.as_deref().unwrap()
    }

    /// For a redirect to `<prefix><id>…`: the id.
    #[track_caller]
    pub fn redirect_id(&self, prefix: &str) -> i64 {
        let target = self.redirect();
        let rest = target
            .strip_prefix(prefix)
            .unwrap_or_else(|| panic!("redirect {target} doesn't start with {prefix}"));
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        digits.parse().unwrap()
    }

    #[track_caller]
    pub fn ok(&self) -> &Self {
        assert_eq!(self.status, StatusCode::OK, "body: {}", self.body);
        self
    }

    /// The text of every element matching a CSS selector.
    pub fn texts(&self, css: &str) -> Vec<String> {
        let doc = Html::parse_document(&self.body);
        let sel = Selector::parse(css).unwrap();
        doc.select(&sel)
            .map(|e| {
                e.text()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    }

    /// The text of the first match.
    #[track_caller]
    pub fn text(&self, css: &str) -> String {
        self.texts(css)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no element matches {css}"))
    }

    pub fn count(&self, css: &str) -> usize {
        self.texts(css).len()
    }

    pub fn has(&self, css: &str) -> bool {
        self.count(css) > 0
    }

    /// The value of an attribute on the first match.
    pub fn attr(&self, css: &str, name: &str) -> Option<String> {
        let doc = Html::parse_document(&self.body);
        let sel = Selector::parse(css).unwrap();
        doc.select(&sel)
            .next()
            .and_then(|e| e.value().attr(name).map(str::to_string))
    }

    /// The error alert of a re-rendered form.
    #[track_caller]
    pub fn error(&self) -> String {
        self.ok();
        self.text(".alert--error")
    }
}
