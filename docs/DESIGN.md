# TTRPG Scheduling: Design Document

Status: draft v0.7 (2026-10-21). Progress: M0–M4 done.

## 1. Goals and non-goals

**Goals**

- Schedule game sessions for a few campaigns with a small, fixed group of friends (roughly 5–30 users).
- Keep campaign knowledge in one place: description, wiki-like pages, session history, recaps.
- Light discussion: comments and polls.
- A Rust learning project: the code should stay readable and simple over clever.

**Non-goals (for now)**

- Public internet exposure, multi-tenancy, scale. The app runs behind a VPN for one group.
- Real-time chat, VTT features, character sheets, dice.
- Email or push notifications (see "Later ideas").
- A JS-heavy SPA.

## 2. Requirements

| #  | Requirement | Notes |
|----|-------------|-------|
| R1 | A user creates a campaign (name, rule system, short description, ...). The creator is the default GM; the GM can be reassigned. Users can be invited to a campaign. | Invitation by link (see §5.2). |
| R2 | The GM creates a scheduling poll for a game session. Candidate dates are either picked manually or generated from a regular pattern (e.g. "every Friday 18:00"). Members vote. | Votes: yes / maybe / no per date. |
| R3 | The GM picks the final date. It does not have to be the most-voted one. | Picking creates a `game_session`. |
| R4 | Every game session shows in the campaign history with date and attendees. The GM can edit attendance. | Attendance defaults to the "yes" voters. |
| R5 | A game session can hold several content entries (recap, notes, loot, ...). | |
| R6 | A campaign has a rich description and extra GM-written pages. | Wiki-lite. |
| R7 | Members can comment on campaigns and game sessions. | |
| R8 | The GM can create polls for a campaign or a specific game session. | Single or multiple choice. |
| R9 | GM and player content is Markdown, rendered safely. | Sanitized HTML. |

## 3. Architecture

```
   VPN (WireGuard / Tailscale)
            │
   ┌────────▼────────┐
   │  Rust binary    │  axum router ─ handlers ─ sqlx ─ SQLite file
   │  (single proc)  │       │
   │                 │   minijinja templates  ─►  HTML (full page or htmx fragment)
   └─────────────────┘
            │ static/ (CSS, htmx.js, alpine.js, vendored)
```

- **Runs on a Raspberry Pi 3B+** (1 GB RAM), cross-compiled from the desktop (§8).
- **One binary, one SQLite file.** Backups mean copying the DB file (or `sqlite3 .backup`).
- **Server-rendered HTML.** htmx swaps fragments for interactive bits (voting, comments, polls);
  Alpine.js handles purely client-side state (toggles, modals, Markdown preview tabs).
- **htmx fragments:** a handler checks the `HX-Request` header. If present, it renders only a named
  block of the template, otherwise the full page.

### 3.1 Crate choices

| Concern | Crate | Why |
|---|---|---|
| HTTP | `axum` | Most common Rust web framework; extractor model resembles FastAPI dependencies. |
| Runtime | `tokio` | Required by axum. |
| DB | `sqlx` (SQLite) | Plain SQL (no ORM to learn), compile-time-checked queries, built-in migrations. SQLite is compiled in (bundled), so it's the only C code to cross-compile. |
| Templates | `minijinja` | Jinja2 syntax you already know. Templates live outside Rust, so frontend agents never touch `.rs` files. Supports rendering single blocks (`eval_to_state(..)?.render_block(..)`, for htmx). Alternative: `askama` (compile-time checked, but each template needs a Rust struct, which couples frontend and backend work). |
| Markdown | `pulldown-cmark` + `ammonia` | Render CommonMark (+ tables, strikethrough, task lists), then sanitize the HTML. |
| Passwords | `argon2` | Current standard password hash. |
| Time | `chrono` + `chrono-tz` | Well supported by sqlx. |
| Errors | `thiserror` + `anyhow` | Typed `AppError` (thiserror) that implements `IntoResponse` for handlers; `anyhow` + `.context()` for startup code. |
| Config | env vars (`dotenvy` loads an optional `.env` in dev) | `DATABASE_URL` (required), `BIND_ADDR` (default `0.0.0.0:8080`); later `DISPLAY_TZ`, `SECRET`... |
| Logging | `tracing` + `tracing-subscriber`, `tower-http` `TraceLayer` | Request logs. |
| Static files | `tower-http` `ServeDir` | |

### 3.2 Project layout

```
Cargo.toml
build.rs               # rebuild when migrations/ changes
migrations/            # sqlx SQL migrations (embedded into the binary)
src/
  main.rs              # parse CLI, startup, then create-admin or serve (AppState + index handler)
  cli.rs               # clap Cli/Command, create_admin()
  config.rs            # Config::from_env()
  db.rs                # pool + migrations
  state.rs             # (planned) AppState { db: SqlitePool, templates: Environment, ... }
  error.rs             # AppError + IntoResponse
  markdown.rs          # md -> sanitized html
  auth/
    mod.rs
    password.rs        # argon2 hash()/verify() via spawn_blocking
    session.rs         # create()/remove() rows in auth_sessions
    extractor.rs       # CurrentUser (FromRequestParts)
    handlers.rs        # GET/POST /login, POST /logout
  campaigns/           # one module per feature: handlers.rs + db.rs (queries) + model.rs
  scheduling/
  game_sessions/
  comments/
  polls/
tests/                 # (M11) end-to-end tests against the real app; see §10.1
templates/
  base.html
  campaigns/...
static/
  css/style.css
  js/  (htmx-<ver>.min.js, alpine-<ver>.min.js: vendored, version in file name, no CDN)
docs/
```

A single crate is enough. Split into a workspace only if compile times start to hurt.

## 4. Data model (SQLite)

All `*_at` columns hold UTC timestamps. `*_md` columns hold Markdown source; HTML is rendered on
read (cheap at this scale, and no cache invalidation to get wrong).

```
users
  id, username (unique), display_name, password_hash, is_admin, created_at

auth_sessions
  id (random token, primary key), user_id → users, created_at, expires_at

campaigns
  id, name, system (rule system), tagline, description_md, gm_user_id → users,
  created_by → users, created_at, archived_at NULL

campaign_members
  campaign_id → campaigns, user_id → users, joined_at
  PK (campaign_id, user_id)            -- the GM is also a member

campaign_invites
  id, campaign_id, token (unique; random, generated by the app like session tokens), created_by,
  created_at, expires_at (default now + 14 days), max_uses (> 0, set by the GM), use_count
  CHECK (use_count <= max_uses)        -- revoking an invite deletes its row

campaign_pages                         -- R6, wiki-lite
  id, campaign_id, title, slug, body_md, sort_order, created_by, updated_at
  UNIQUE (campaign_id, slug)

schedule_patterns                      -- R2, "regular" scheduling
  id, campaign_id, weekdays (bitmask or "MO,FR"), start_time (local), duration_minutes,
  every_n_weeks, active

schedule_polls                         -- R2
  id, campaign_id, title, description_md, status ('open' | 'decided' | 'cancelled'),
  deadline_at NULL, created_by, created_at,
  decided_option_id NULL → schedule_options, game_session_id NULL → game_sessions

schedule_options
  id, poll_id, starts_at, ends_at NULL

schedule_votes
  option_id → schedule_options, user_id → users, answer ('yes' | 'maybe' | 'no'),
  note NULL, updated_at
  PK (option_id, user_id)

game_sessions                          -- R3, R4
  id, campaign_id, number (per campaign), title NULL, starts_at,
  status ('planned' | 'played' | 'cancelled'), created_at

game_session_attendance                -- R4
  game_session_id, user_id, PK (game_session_id, user_id)

game_session_entries                   -- R5
  id, game_session_id, kind ('recap' | 'notes' | 'loot' | 'other'), title, body_md,
  author_id, created_at, updated_at

comments                               -- R7
  id, campaign_id, game_session_id NULL, author_id, body_md,
  created_at, edited_at NULL, deleted_at NULL

polls                                  -- R8
  id, campaign_id, game_session_id NULL, question, description_md,
  multiple_choice, closes_at NULL, closed_at NULL, created_by, created_at

poll_options
  id, poll_id, label, sort_order

poll_votes
  poll_option_id, user_id, created_at, PK (poll_option_id, user_id)
```

**Design notes**

- Comments and polls attach to a campaign and *optionally* to a game session
  (`game_session_id NULL` means "campaign-level"). This avoids polymorphic `target_type/target_id`
  columns and keeps real foreign keys.
- `schedule_patterns` don't create anything by themselves. They're a **generator for candidate
  dates**: "new scheduling poll from pattern, next 3 weeks" produces e.g. 3 Fridays + 3 Saturdays
  as `schedule_options`, which the GM can then edit. This keeps the "regular" and "manual" flows
  the same after creation.
- Deciding a poll (R3) is one DB transaction: set `decided_option_id`, create a `game_session`,
  pre-fill attendance with the "yes" voters, set `status = 'decided'`.
- Enums are stored as `TEXT` with a `CHECK` constraint and mapped to Rust enums via `sqlx::Type`.

## 5. Auth and permissions

### 5.1 Authentication

The VPN is the outer wall, but we still need to know *who* is voting.

- Username + password, Argon2 hash, random session token in an `HttpOnly`, `SameSite=Lax` cookie,
  stored in `auth_sessions`.
  - The token is 32 random bytes, hex-encoded.
  - The cookie is named `session`, with `Path=/`.
  - The cookie is not `Secure`, because it's served over plain HTTP inside the VPN.
  - The cookie is long-lived; the real limit is `expires_at`, 30 days after login.
  - Expired rows are rejected on lookup and deleted on the next login.
  - Logout deletes the row and expires the cookie.
- A `CurrentUser` axum extractor loads the user from the cookie and redirects to `/login` if
  missing. Handlers that take `CurrentUser` as an argument are authenticated automatically.
- First admin is created by a CLI subcommand:
  `ttrpg-scheduling create-admin <username> <display_name>`. It reads the password from stdin.
- CSRF: `SameSite=Lax` plus checking the `Origin` header on non-GET requests is enough here.
  The `Origin` check is not implemented yet.

### 5.2 Invitations and registration

- The GM generates an invite link for a campaign: `/invite/<token>`, choosing how many people
  may use it (`max_uses`). It expires after 14 days. Anyone holding the link can use it until then,
  so revoking it deletes it.
- Logged-in user opens it: they join the campaign.
- Logged-out visitor opens it: an invite is an invite to a *campaign*, and most visitors already
  have an account, so the page shows a login form first (it returns to the invite via
  `/login?next=`). Registering a new account, which also joins the campaign, is the second option.
- So there's no open registration; every account comes from an invite or from an admin.

### 5.3 Permission matrix

| Action | Admin | GM of campaign | Member | Non-member |
|---|:-:|:-:|:-:|:-:|
| Create campaign | ✓ | ✓ (any user) | ✓ (any user) | ✓ (any user) |
| View campaign, history, pages | ✓ | ✓ | ✓ | – |
| Edit campaign, pages, reassign GM | ✓ | ✓ | – | – |
| Create invites, remove members | ✓ | ✓ | – | – |
| Create / decide scheduling poll | ✓ | ✓ | – | – |
| Vote in scheduling poll / poll | | ✓ | ✓ | – |
| Edit attendance | ✓ | ✓ | – | – |
| Add game session entries (recap...) | | ✓ | ✓ | – |
| Edit/delete own session entry | | ✓ | ✓ | – |
| Edit/delete any session entry | ✓ | ✓ | – | – |
| Comment | | ✓ | ✓ | – |
| Edit/delete own comment | | ✓ | ✓ | – |
| Delete any comment | ✓ | ✓ | – | – |
| Create polls | ✓ | ✓ | – | – |

Non-members get a 404 (not 403) on campaign URLs.

## 6. Routes (backend ↔ frontend contract)

Full pages unless marked *(fragment)*. Fragments are returned when the request has `HX-Request`.

```
GET  /login, POST /login, POST /logout
GET  /invite/{token}, POST /invite/{token}             # show the invite; join (logged in)
POST /invite/{token}/register                          # register + join + log in (logged out)

GET  /                                                 # dashboard: my campaigns, open polls, upcoming sessions
GET  /campaigns/new, POST /campaigns
GET  /campaigns/{id}                                   # overview: description, next session, open polls
GET  /campaigns/{id}/edit, POST /campaigns/{id}
GET  /campaigns/{id}/members                           # member list (members); invites, remove, GM (GM/admin)
POST /campaigns/{id}/invites, DELETE /campaigns/{id}/invites/{invite_id}   # DELETE via htmx (revoke)
DELETE /campaigns/{id}/members/{uid}                   # htmx: GM/admin removes a member, or a member leaves
POST /campaigns/{id}/gm                                # reassign the GM (field user_id)
GET  /campaigns/{id}/pages/{slug}, (new|edit|save) …
GET  /campaigns/{id}/history                           # list of game sessions

GET  /campaigns/{id}/schedule/new                      # manual or from pattern
POST /campaigns/{id}/schedule
GET  /schedule/{poll_id}                               # voting grid
PUT  /schedule/{poll_id}/options/{opt_id}/vote         # (fragment) one cell of the grid
POST /schedule/{poll_id}/decide

GET  /sessions/{id}                                    # game session page: entries, attendance, comments, polls
POST /sessions/{id}/attendance
POST /sessions/{id}/entries, POST /entries/{id}, POST /entries/{id}/delete

POST /campaigns/{id}/comments, POST /sessions/{id}/comments   # (fragment) new comment
POST /comments/{id}, POST /comments/{id}/delete               # (fragment)

POST /campaigns/{id}/polls, POST /sessions/{id}/polls
POST /polls/{id}/vote                                  # (fragment) updated results
POST /polls/{id}/close

POST /markdown/preview                                 # (fragment) for the editor preview tab
```

Deletes and in-place updates use the real HTTP method through htmx (`hx-delete`, `hx-put`); the
handler answers with a fragment, an empty 200 (with `hx-swap="delete"`), or an `HX-Redirect`
header. Plain forms (POST, then a 303 redirect) are for creating things and for edits that reload
the page.

Template context conventions: every page gets `current_user` (no CSRF tokens needed, see §5.1). Markdown
fields are passed as `<name>_html` (already sanitized). Datetimes are passed already converted to
the display time zone, together with an ISO string for `<time datetime="…">`.

## 7. Markdown

- `pulldown-cmark` with tables, strikethrough, task lists, footnotes.
- Output is passed through `ammonia` with the default allow-list (+ `class` on `code` for syntax
  highlighting later). Raw HTML in Markdown is therefore stripped of scripts, event handlers, etc.
- Exposed in templates only as pre-rendered `*_html` fields; there's no Markdown filter in templates,
  so raw user input can't accidentally be marked safe.
- Editor: a `<textarea>` with Write/Preview tabs (Alpine + `hx-post="/markdown/preview"`).

## 8. Deployment (Raspberry Pi 3B+)

**Target hardware:** Raspberry Pi 3B+ running Raspberry Pi OS. It has a quad-core Cortex-A53
(ARMv8) and **1 GB RAM**, and stores data on an SD card. That's plenty for serving a few dozen
users. It is far too little for *compiling* a Rust web app with sqlx macros on the device,
so we **cross-compile on the desktop** and copy the binary over.

### 8.1 Rust target

The Pi runs the **64-bit** Raspberry Pi OS (`uname -m` → `aarch64`), so the target is
**`aarch64-unknown-linux-musl`**.

**Why musl:** it produces a fully static binary. There's no dependency on the Pi's glibc version
and no "GLIBC_2.xx not found" errors after copying. musl's allocator is slower than glibc's, which
doesn't matter at this scale. (If it ever does, switch to the `*-gnu` target and pin the glibc
version with zigbuild, e.g. `aarch64-unknown-linux-gnu.2.36`.)

### 8.2 Toolchain: `cargo-zigbuild`

The only C dependency is SQLite. sqlx compiles it from source via `libsqlite3-sys` (bundled), so
we need a C cross-compiler and linker for ARM. Zig provides both in one package:

```sh
rustup target add aarch64-unknown-linux-musl
brew install zig                                 # Bazzite: brew works on the immutable host
cargo install --locked cargo-zigbuild
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

Fallback: `cross` (runs the build in a container; podman is available on the desktop). It's
slower and less convenient for incremental builds, but needs no local cross toolchain.

Development and tests run natively on the desktop (x86_64). Only release builds target ARM.
sqlx compile-time query checks also run on the desktop against the dev DB, so the Pi never needs
`DATABASE_URL` at build time.

### 8.3 Running on the Pi

- Deploy = `rsync` a directory to the Pi: binary, `templates/`, `static/`. Migrations are
  embedded in the binary with `sqlx::migrate!()` and run on startup.
- The Pi has no `.env`: set `DATABASE_URL` (e.g. `sqlite:data/app.db`) in the environment
  (systemd `Environment=`/`EnvironmentFile=`). SQLite creates the DB file but **not** its
  directory, so `data/` must exist; otherwise startup fails with
  "failed to open database …: unable to open database file".
  (Embedding templates/static too, e.g. via `minijinja-embed` / `rust-embed`, is an optional later
  step for a true single-file deploy.)
- A **systemd service** (no Docker; it isn't worth the RAM and disk overhead on a Pi 3), running
  as its own user, binding to the VPN interface address only (e.g. `BIND_ADDR=100.x.y.z:8080`
  for Tailscale).
- A small deploy script (`scripts/deploy.sh`): zigbuild → rsync → `systemctl restart`.

### 8.4 Pi-specific considerations

- **SD card wear and corruption:** SQLite in WAL mode with `synchronous=NORMAL`. Write volume is
  tiny anyway. Backups must leave the card: nightly `sqlite3 data.db ".backup …"` via a systemd
  timer, then copy it off the Pi (e.g. pull from the desktop/NAS with rsync).
- **Argon2 cost:** the default parameters (~19 MiB memory) take a noticeable fraction of a second
  on a Cortex-A53. That's fine for logins and only happens at login time. Don't lower them
  unless it becomes a real problem.
- **RAM:** expected footprint is a few tens of MB. Keep the SQLite pool small (e.g. 4 connections).
- **Clock:** the Pi has no RTC and relies on NTP after boot. Timestamps are only as good as
  network time, so make sure `systemd-timesyncd` is enabled.

## 9. Decisions (resolved 2026-10-09)

1. **Time zones:** all players share one time zone, so there's one `DISPLAY_TZ` setting and no
   per-user time zone.
2. **Session entries:** any campaign member can write them, not only the GM.
3. **Scheduling votes are visible** to all members (who said yes / maybe / no).
4. **One GM per campaign.** Co-GMs may come later; a bigger rewrite then is acceptable.
5. **No secret GM content.** The app is player-facing first; everything in a campaign is visible
   to all its members.
6. **Passwords, not Tailscale identity.** The app must not depend on a specific VPN.

## 10. Milestones (learning path)

Each milestone should end in something that runs. The Rust concepts it introduces are listed
next to it. A milestone counts as done when it runs locally. Deploying to the Pi isn't repeated
for every milestone: M0 proved the cross-compile toolchain, and the Pi gets updated when a
version is worth using.

| # | Milestone | Rust concepts | Status |
|---|---|---|---|
| M0 | `cargo new`, axum "hello world"; **cross-compile it and run it on the Pi** | Cargo, crates, `async`/`.await`, `#[tokio::main]`, targets & release profile | done |
| M1 | `AppState`, minijinja base template, static files, `tracing` request logs | `Arc`, the `State` extractor, `Result` + `map_err` + `?`, closures, `use`/paths/prelude, `Display` vs `Debug` | done |
| M2 | SQLite pool, first migration (`users`), `AppError`, config from env, `main` returning `Result` | enums with data + exhaustive `match`, implementing traits (`Display`, `Error` + `source()`, `From`, `IntoResponse`), trait objects, `?` + `From`, `thiserror` vs `anyhow`, modules, builder pattern, `FromStr`/`parse`, `&str` params, compile-time-checked SQL | done |
| M3 | Users, login/logout, `CurrentUser` extractor, `create-admin` CLI | implementing a library trait (`FromRequestParts`, associated types), ownership (moves, partial moves, borrows, `String` vs `&str`), `spawn_blocking` + `move` closures, `Option` (`ok_or`, `then_some`, `if let`, `let ... else`), one return type per fn (`Response`), `query_as!`, cookies, argon2, clap derive, unit tests + `#[sqlx::test]` | done |
| M4 | Campaign CRUD, members, invites, permission checks | `sqlx::query_as!`, `sqlx::Type` enums, `match`, forms with `Form<T>` + `Deserialize` | done |
| M5 | Markdown rendering + campaign pages | `&str` vs `String`, ownership through function boundaries | |
| M6 | Scheduling polls: manual options, voting grid (htmx), decide → game session | transactions, `chrono`, `HashMap`/grouping, iterators | |
| M7 | Regular patterns → generated candidate dates | iterators, `chrono` date arithmetic, unit tests | |
| M8 | Game session history, attendance, entries | putting it together; refactoring shared code | |
| M9 | Comments and polls | generic helpers if (and only if) duplication appears | |
| M10 | systemd hardening, backups, deploy script polish | release profile, `cfg` attributes, build scripts | |
| M11 | Automated end-to-end tests: HTTP-level tests of the real app, then a few browser tests for the htmx/Alpine flows (see §10.1) | library + binary crate (`src/lib.rs`), `tests/` integration tests, test fixtures and helpers, `reqwest` + cookie store, `scraper` (CSS selectors on HTML), WebDriver from Rust (`thirtyfour`/`fantoccini`) | |

### 10.1 M11: end-to-end tests (plan)

**Why:** most checks during M3–M4 were manual (`curl` with a cookie, `grep` on the HTML,
permission 404s, form re-render on error). A typo in a template name (`"campaigns/html"`) compiled
fine and only failed when that branch ran. Tests that drive the real app over HTTP catch this
class of bug, which the compiler can't.

**What Rust backend developers usually do** (from most to least common):

1. **In-process tests of the router.** Build the `Router` in the test and call it directly with
   `tower::ServiceExt::oneshot(request)`, with no network involved. This is what the axum examples
   do. `axum-test` (`TestServer`) wraps it with a friendlier API: cookies, form posts, assertions.
2. **Spawn the real server on a random port** (`TcpListener::bind("127.0.0.1:0")`) inside the
   test and drive it with `reqwest`. This is the "black-box" style popularized by *Zero To
   Production in Rust* (a `spawn_app()` helper). It's the closest to how a user hits the app, and
   still runs as plain `cargo test`.
3. **Browser tests**, needed only where JavaScript matters (htmx swaps, Alpine state).
   - **Rust options:** WebDriver clients `thirtyfour` and `fantoccini`, which need geckodriver or
     chromedriver running, or `chromiumoxide`/`headless_chrome`, which drive Chrome over the
     DevTools protocol.
   - **Playwright:** there's no official Playwright for Rust. Many Rust projects simply use
     Playwright from TypeScript or Python for this layer; for example, the Leptos templates ship an
     `end2end/` Playwright folder.

**Plan for this project:**

- **Refactor so tests can build the app.**
  - **Library + binary:** integration tests in `tests/` can only `use` a **library** crate, never
    `main.rs`. Move everything except `main()` into `src/lib.rs`, so the package becomes a library
    plus a thin binary.
  - **A router builder:** split `serve()` into a function that builds the `Router` from an
    `AppState`, and the part that binds and serves.
- **HTTP-level E2E in Rust (the bulk of the tests).**
  - **The app:** use style 2, the real server on a random port. Each test gets a fresh migrated
    DB from `#[sqlx::test]`, which is passed into `AppState`.
  - **Requests:** a `reqwest::Client` with `cookie_store(true)` keeps the login session.
  - **Assertions:** `scraper` parses the returned HTML and checks it with CSS selectors.
  - **Fixtures:** small helpers that create a user, log in and create a campaign, like pytest
    fixtures.
  - **What it covers:** status codes, redirects, the permission matrix (§5.3), form errors and
    escaping. Every template branch should be rendered at least once.
- **Browser E2E (a handful).**
  - **What it covers:** the htmx voting grid (M6) and the other JS-dependent flows.
  - **The tool:** `thirtyfour` against a headless Firefox or Chrome, to learn the Rust way.
    Comparing it with Playwright from Python is part of the milestone.
  - **Running them:** these tests need an external driver process, so mark them `#[ignore]` and run
    them with `cargo test -- --ignored`.
- **Optional:** `cargo-nextest` as a faster test runner, and `insta` snapshot tests for rendered
  templates.

## 11. Later ideas

- iCal feed per user (`/calendar/<token>.ics`) so decided sessions appear in phone calendars.
- Discord webhook notification when a poll opens or a date is decided.
- Reminders before the vote deadline.
- File/image uploads for campaign pages (maps, handouts).
- Embedding templates/static into the binary for a single-file deploy.
