# TTRPG Scheduling

A small self-hosted web app for scheduling tabletop RPG sessions within a group of friends.
It is reachable only over a VPN. Inspired by RollRota; Doodle and Hyperfictive were tried and rejected.

**Production target: Raspberry Pi 3B+ (64-bit Raspberry Pi OS, aarch64, 1 GB RAM).** Never build on the Pi.
Cross-compile on the x86_64 desktop with `cargo-zigbuild` to a static musl target (see
`docs/DESIGN.md` §8). Keep that in mind when choosing dependencies: avoid crates that need
system C libraries (prefer pure Rust or `bundled` features, e.g. rustls instead of OpenSSL).

See `docs/DESIGN.md` for requirements, architecture, data model and the milestone plan.

## Stack

- **Backend:** Rust (edition 2024): `axum` 0.8 (HTTP), `tokio` (async runtime),
  `tower-http` 0.7 (static files, request tracing), `tracing` (logging),
  `sqlx` 0.9 + SQLite (features `runtime-tokio`, `sqlite`; SQLite is bundled C code),
  `thiserror` (typed `AppError`) + `anyhow` (startup/glue errors), `dotenvy` (`.env` in dev),
  `minijinja` 3 (server-side templates; v3 differs from the many v2 examples online:
  `path_loader` needs no feature flag, serde support is the opt-in `serde` feature, which is
  enabled. Pass a struct to a template as `minijinja::value::Serde(&x)`; `context!` doesn't
  convert structs on its own).
  Auth: `argon2` (password hashing), `rand` (session tokens), `axum-extra` (feature `cookie`,
  `CookieJar`), `serde` (`Form` structs, template values), `clap` (the `create-admin` subcommand).
  Markdown: `pulldown-cmark` + `ammonia` (sanitizing), `deunicode` (page slugs).
  Time: `chrono` + `chrono-tz` (`DISPLAY_TZ`). `axum_extra::extract::Form` (feature `form`) for
  forms with repeated fields (`Vec<String>`); axum's own `Form` can't do that.
  minijinja features: `serde`, `json` (the `tojson` filter, used to hand data to Alpine).
- **Frontend:** server-rendered HTML + htmx + Alpine.js + plain CSS. No JS build step, no SPA.

## How we work together (important)

The owner is an experienced Python + htmx + Alpine.js developer. The project started as a guided
Rust learning project (the user wrote the backend, the agent taught and reviewed; see the M3/M4
docs). **Since 2026-10-21 the agent implements the backend too**, because the owner wants the app
finished. Keep the code simple and explicit so the owner can still read and maintain it:

### Backend (`src/`, `Cargo.toml`, `migrations/`)

- Follow the existing style: feature modules (`mod.rs` router, `model.rs`, `db.rs`,
  `handlers.rs`), `query!`/`query_as!` macros, `Result<_, AppError>` + `?` in handlers.
- Prefer simple, explicit Rust over clever abstractions. No macros, generics or traits added just
  for elegance.
- When explaining a change to the owner, map Rust concepts to Python where it helps.
- If the owner says they want to write a piece themselves again, switch back to teaching mode
  for it: explain, propose the shape, hint, review.

### Frontend (`templates/`, `static/`): vibe-code mode

- Write templates, CSS and small Alpine/htmx snippets freely.
- The contract between backend and frontend is the template context (the variables each handler
  passes to a template, documented in a comment at the top of every template) and the routes
  listed in `docs/DESIGN.md`. Keep both up to date when either side changes.
- `templates/base.html` is the page shell; pages `{% extends "base.html" %}` and fill the
  `title`, `head` and `content` blocks. The nav renders user links only when `current_user` is
  in the context.
- Styling lives in `static/css/style.css`; colours and spacing are CSS variables on `:root`
  (with a `prefers-color-scheme: dark` override). Reuse them instead of hard-coding values.
- htmx and Alpine.js are vendored in `static/js/` with the version in the file name
  (htmx 2.0.x, Alpine 3.x). No CDNs, no web fonts: the Pi serves only over the VPN and may have
  no internet access. To upgrade, download the new file, rename it, and update `base.html`.
- htmx partials are rendered from named blocks of the full-page template where possible
  (so one template file serves both the full page and the fragment).
- User-provided Markdown arrives already rendered and sanitized; output it with `|safe` only for
  fields documented as pre-rendered HTML (`*_html`). Never mark raw user input as safe.

## Code map

- The package is a library plus a thin binary, so tests in `tests/` can build the app.
  - `src/main.rs`: loads dotenv, parses the CLI, then sets up config, logging and DB. It then
    runs `cli::create_admin`, `cli::backup` or `serve()`.
  - `src/lib.rs`: the modules, `AppState` (`AppState::new(db, tz)` loads the templates),
    `router(state)` with every route, `serve()` (bind, graceful shutdown), and the small
    handlers `index` (dashboard), `markdown_preview` and `healthz`.
- `src/cli.rs`: clap `Cli`/`Command` and `create_admin()`, which reads the password from stdin
  and inserts an admin user.
- `src/auth/`:
  - `password.rs`: argon2 `hash`/`verify` in `spawn_blocking`.
  - `session.rs`: `create` (random token, 30-day expiry, deletes expired rows) and `remove`.
  - `extractor.rs`: `CurrentUser`. A handler argument of this type requires login, and its
    absence produces `AppError::Unauthorized`, which redirects to `/login`.
  - `handlers.rs`: `GET`/`POST /login`, `POST /logout`. The cookie is named `session`, with
    `Path=/`.
- Pages for logged-in users pass `current_user => Serde(&user)`; `base.html` uses it for the nav.
- `src/campaigns/`: campaign CRUD, members, invites management, GM change. `campaigns::load_for`
  loads a campaign plus the user's `Access` (404 for non-members) and is the permission entry
  point for every campaign-scoped feature module.
- `src/invites/`: `GET/POST /invite/{token}` (show, join) and `/invite/{token}/register`.
- `src/pages/`: campaign wiki pages, nested at `/campaigns/{id}/pages`.
- `src/comments/`, `src/polls/`: comments and polls (R7, R8) on a campaign or a game session.
  Each exposes `views(state, campaign_id, session_id, user, access)` for the page that embeds it;
  the htmx endpoints answer with the partials `comments/_comment.html` / `polls/_poll.html`.
  Partials (`_*.html`) are included by pages and also rendered alone as fragments.
- `src/markdown.rs`: `render()` (Markdown → sanitized HTML) and `slugify()`.
- `src/scheduling/`: scheduling polls (`/campaigns/{id}/schedule/new`, `/schedule/{poll_id}`),
  htmx voting grid (`PUT .../vote` returns the `grid` block), decide → game session.
  `pattern.rs`: regular slots ("Fri, Sat at 18:00, every week") → candidate dates, with unit
  tests; managed at `/campaigns/{id}/schedule/patterns`.
- `src/game_sessions/`: game sessions (`/sessions/{id}`: edit, attendance, entries), entries
  (`/entries/{id}`), history (`/campaigns/{id}/history`), upcoming lists for dashboard/campaign.
- `src/time.rs`: UTC DB strings ↔ `chrono`; `parse_local` for `datetime-local` inputs in
  `DISPLAY_TZ`; `When`, the struct templates get for any timestamp (`iso`, `date`, `time`, `full`,
  `input`, `is_past`).
- `AppState` (in `lib.rs`) holds `templates`, `db` and `tz`. `state.render(name, ctx)` renders a
  full page, `state.render_block(name, block, ctx)` one block (htmx fragments).
- `src/config.rs`: `Config::from_env()`. `DATABASE_URL` is required, `BIND_ADDR` defaults to
  `0.0.0.0:8080`, `DISPLAY_TZ` defaults to `UTC`. `.env` is optional (dev only); see `.env.example`.
- `src/db.rs`: `connect()` builds the SQLite pool (WAL, `synchronous=NORMAL`, max 4 connections)
  and runs the embedded migrations.
- `src/error.rs`: `AppError` (thiserror) + `IntoResponse`. Internal errors (`Template`,
  `Database`, `Internal` wrapping `anyhow::Error`) are logged and returned as a generic 500; the
  response body never contains internal error text. `NotFound` returns a 404, and `Unauthorized`
  redirects to `/login`.
- `build.rs`: makes cargo rebuild when `migrations/` changes (needed by `sqlx::migrate!()`), and
  sets `APP_VERSION` (`0.1.0+<git commit>`), shown in the footer and by `GET /healthz`.
- `deploy/`: systemd units (hardened service, nightly backup timer) and the env file example.
  `scripts/deploy.sh` builds and deploys. Runbook: `docs/DEPLOY.md`.

## Commands

```sh
# First-time setup (fresh clone). The sqlx query macros connect to the dev DB at compile time,
# so the DB must exist and be migrated BEFORE the first `cargo build`.
cp .env.example .env
mkdir -p data
cargo install --locked sqlx-cli --no-default-features --features sqlite
sqlx database setup            # create data/dev.db and run migrations

cargo run                      # dev server on 0.0.0.0:8080 (run from repo root: templates/ and
                               # static/ are resolved relative to the working directory)
RUST_LOG=debug cargo run       # override the default log filter "info,tower_http=debug"
cargo run -- create-admin <username> "<Display Name>"   # create an admin (prompts for password)
cargo run -- backup <file>     # consistent copy of the DB (VACUUM INTO), safe while serving
cargo check                    # fast type check
cargo clippy --all-targets     # lints, keep warnings at zero
cargo fmt                      # format
cargo test                     # unit tests + HTTP end-to-end tests (tests/e2e/), ~5 s; each
                               # #[sqlx::test] gets its own fresh, migrated SQLite DB
cargo test --test e2e invites  # one e2e module
cargo test --test browser -- --ignored   # browser tests (tests/browser/): headless Firefox via
                               # geckodriver (`cargo install geckodriver`); FIREFOX_BINARY if
                               # Firefox isn't on the PATH (the Flatpak build is found)
sqlx migrate add <name>        # new migration (needs: cargo install sqlx-cli)
sqlx migrate run               # apply migrations (the app also runs them on startup)
sqlx database reset            # dev only: drop, recreate and re-migrate data/dev.db

# Release build for the Pi (64-bit Raspberry Pi OS); runs locally via qemu-aarch64 (binfmt)
cargo zigbuild --release --target aarch64-unknown-linux-musl
PI_HOST=pi@raspberrypi HEALTH_URL=http://<vpn-ip>:8080/healthz scripts/deploy.sh   # see docs/DEPLOY.md
```

## Tests

- `tests/e2e/`: one test binary; `common.rs` holds the fixtures (`spawn_app(db)` starts the real
  router on a random port; `app.user("name")` creates and logs in a user with a cookie-keeping
  `reqwest` client that doesn't follow redirects; `Resp` has `scraper` helpers such as
  `text(css)`, `count(css)`, `attr(css, name)`, `error()`, `redirect()`). Add a test for every
  new page or permission rule; every template branch should render at least once, because a
  template mistake only fails at runtime (as a 500).
- `tests/browser/`: only for behaviour that needs JavaScript (htmx swaps, Alpine state).
  `Browser::mark_page()` + `still_same_page()` prove a swap didn't reload the page.

## Conventions

- Git commit messages: **no `Co-Authored-By` or other AI attribution lines.** Keep commits
  split by author where practical (user-written Rust vs agent-written frontend).

- "Session" is ambiguous here. Use **`game_session`** for a played/planned TTRPG session and
  **`auth_session`** for a login session, in code, tables and routes alike.
- All timestamps are stored in UTC in the DB and converted to the display time zone at render time.
- Schema changes go through `sqlx` migrations only. Never edit a migration that has been deployed
  to the Pi (sqlx checksums them and refuses to start); locally, edit + `sqlx database reset` is OK.
- SQLite schema style: `STRICT` tables; booleans as `INTEGER NOT NULL CHECK (x IN (0, 1))`;
  timestamps as UTC `TEXT` (`strftime('%Y-%m-%dT%H:%M:%SZ', 'now')`); nullability chosen
  deliberately, since nullable columns become `Option<T>` in Rust.
- Errors: handlers return `Result<_, AppError>` and use plain `?` (`#[from]` conversions).
  Startup/glue code uses `anyhow::Result` with `.context(...)` so failures name what failed.
  `Display` messages describe only their own layer; the cause comes from `source()`.
- Authorization checks live in the backend; templates may hide buttons, but must not be the only check.
