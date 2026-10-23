# TTRPG Scheduling: Design Document

Status: v1.0 (2026-10-21). Progress: M0–M11 done.

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
| R3 | The GM picks the final date. It does not have to be the most-voted one. The GM can also add a game session directly, without a poll (one already played, or a date already agreed). | Picking creates a `game_session`. |
| R4 | Every game session shows in the campaign history with date and attendees. The GM can edit attendance. | Attendance defaults to the "yes" voters (all members for a session added directly). |
| R5 | A game session can hold several content entries (recap, notes, loot, ...). | |
| R6 | A campaign has a rich description and extra GM-written pages. | Wiki-lite. |
| R7 | Members can comment on campaigns, game sessions and scheduling polls. | |
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
| Templates | `minijinja` | Jinja2 syntax you already know. Templates live outside Rust, so frontend agents never touch `.rs` files. Supports rendering single blocks (in v3: `render_captured(ctx)?.with_state_mut(|s| s.render_block(..))`, for htmx; wrapped as `AppState::render_block`). Alternative: `askama` (compile-time checked, but each template needs a Rust struct, which couples frontend and backend work). |
| Markdown | `pulldown-cmark` + `ammonia` | Render CommonMark (+ tables, strikethrough, task lists), then sanitize the HTML. |
| Passwords | `argon2` | Current standard password hash. |
| Time | `chrono` + `chrono-tz` | Well supported by sqlx. |
| Errors | `thiserror` + `anyhow` | Typed `AppError` (thiserror) that implements `IntoResponse` for handlers; `anyhow` + `.context()` for startup code. |
| Config | env vars (`dotenvy` loads an optional `.env` in dev) | `DATABASE_URL` (required), `BIND_ADDR` (default `0.0.0.0:8080`), `DISPLAY_TZ` (IANA name, default `UTC`). |
| Logging | `tracing` + `tracing-subscriber`, `tower-http` `TraceLayer` | Request logs. |
| Static files | `tower-http` `ServeDir` | |

### 3.2 Project layout

```
Cargo.toml
build.rs               # rebuild when migrations/ changes
migrations/            # sqlx SQL migrations (embedded into the binary)
src/
  main.rs              # parse CLI, startup, then create-admin, backup or serve
  lib.rs               # modules, AppState, router(), serve(), dashboard/preview/healthz handlers
  cli.rs               # clap Cli/Command, create_admin()
  config.rs            # Config::from_env()
  db.rs                # pool + migrations
  error.rs             # AppError + IntoResponse
  markdown.rs          # md -> sanitized html, slugify
  auth/
    mod.rs
    password.rs        # argon2 hash()/verify() via spawn_blocking
    session.rs         # create()/remove() rows in auth_sessions
    extractor.rs       # CurrentUser (FromRequestParts)
    handlers.rs        # GET/POST /login, POST /logout
  time.rs              # UTC DB strings, DISPLAY_TZ conversion, When (template timestamps)
  campaigns/           # one module per feature: mod.rs (router) + handlers.rs + db.rs (queries) + model.rs
  invites/             # accept an invite, register through one
  pages/               # campaign wiki pages
  scheduling/          # scheduling polls, voting grid, regular slots (pattern.rs)
  game_sessions/       # sessions, history, attendance, entries
  comments/
  polls/
tests/e2e/             # (M11) HTTP end-to-end tests against the real app; see §10.1
tests/browser/         # (M11) browser tests for htmx/Alpine (headless Firefox, #[ignore])
templates/
  base.html
  campaigns/...
static/
  css/style.css
  js/  (htmx-<ver>.min.js, alpine-<ver>.min.js: vendored, version in file name, no CDN)
deploy/                # systemd units, env file example (docs/DEPLOY.md)
scripts/deploy.sh      # build for the Pi and deploy
docs/
```

A single crate is enough. Split into a workspace only if compile times start to hurt.

## 4. Data model (SQLite)

All `*_at` columns hold UTC timestamps. `*_md` columns hold Markdown source; HTML is rendered on
read (cheap at this scale, and no cache invalidation to get wrong).

```
users
  id, username (unique), display_name, password_hash, is_admin, created_at,
  deactivated_at NULL                  -- accounts are deactivated, never deleted

auth_sessions
  id (random token, primary key), user_id → users, created_at, expires_at

campaigns
  id, name, system (rule system), tagline, description_md, gm_user_id → users,
  created_by → users, created_at, archived_at NULL,
  accent ('' or a palette colour: the dashboard card's border)

campaign_banners                       -- optional card banner, 1 per campaign
  campaign_id (PK) → campaigns, content_type (png | jpeg | webp), data BLOB (≤ 1 MB), updated_at

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

schedule_patterns                      -- R2, "regular" scheduling (a campaign's regular slots)
  id, campaign_id, weekdays (bitmask, bit 0 = Monday), start_time ('HH:MM', local),
  every_n_weeks (1-4), anchor_date (Monday of a week with sessions), created_by, created_at

schedule_polls                         -- R2
  id, campaign_id, title, description_md, status ('open' | 'decided' | 'cancelled'),
  deadline_at NULL, created_by, created_at,
  decided_option_id NULL → schedule_options, game_session_id NULL → game_sessions

schedule_options
  id, poll_id, starts_at                 UNIQUE (poll_id, starts_at)

schedule_votes
  option_id → schedule_options, user_id → users, answer ('yes' | 'maybe' | 'no'), updated_at
  PK (option_id, user_id)

game_sessions                          -- R3, R4
  id, campaign_id, number (per campaign, UNIQUE with campaign_id), title ('' = none), starts_at,
  status ('planned' | 'played' | 'cancelled'), created_at

game_session_attendance                -- R4
  game_session_id, user_id, PK (game_session_id, user_id)

game_session_entries                   -- R5
  id, game_session_id, kind ('recap' | 'notes' | 'loot' | 'other'), title ('' = none), body_md,
  author_id, created_at, updated_at

comments                               -- R7
  id, campaign_id, game_session_id NULL, schedule_poll_id NULL, author_id, body_md,
  created_at, edited_at NULL           -- deleting removes the row; at most one of the two ids

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
  the same after creation. In the UI they're called "regular slots"; generation happens in Rust
  (`scheduling::pattern`), skipping past times and local times that don't exist (DST gap).
  `duration_minutes` and `active` were dropped: nothing uses a duration yet, and a slot that's no
  longer used is simply deleted.
- Deciding a poll (R3) is one DB transaction: set `decided_option_id`, create a `game_session`,
  pre-fill attendance with the "yes" voters, set `status = 'decided'`.
- Adding a session directly (`/campaigns/{id}/sessions/new`) is one transaction too. Its status
  follows from the date: past → `played`, future → `planned`. Attendance comes from checkboxes,
  all members ticked by default.
- Card banners are stored in the DB rather than as files: they're small, backups then cover
  them, and there's no upload directory to manage on the Pi. The type is taken from the file's
  first bytes (PNG, JPEG, WebP only; never SVG, which can carry scripts) and served with
  `X-Content-Type-Options: nosniff`.
- Enums are stored as `TEXT` with a `CHECK` constraint and mapped to Rust enums via `sqlx::Type`
  (`#[sqlx(rename_all = "lowercase")]`, and `AS "col: Type"` in the query macros).
- M6 left out `schedule_options.ends_at` and `schedule_votes.note` (not needed yet), and a
  `CHECK` keeps `schedule_polls.status = 'decided'` in sync with its two decision columns.
- Deciding a poll gives the session no title (the poll title is usually "Session 12", which
  would duplicate the number); the GM can add one on the session page.
- Sessions are numbered by date (cancelled ones included): `game_sessions::db::renumber` runs
  in the same transaction after every insert (poll decision, direct add) and date change, so a
  session backfilled for an earlier date takes its chronological number and later ones shift.
  Sessions are never deleted; the GM marks them cancelled. One exception: reopening a decided
  poll (`POST /schedule/{id}/reopen`, GM) deletes the session the decision created, but only while
  it's untouched (planned, no entries, comments or polls), so a wrong click or a date that fell
  through can be undone. Otherwise the GM moves or cancels the session on its page; the poll
  page always shows the session as it is now.
- Poll results are hidden from a member until they vote (to avoid bandwagoning), then shown with
  voter names; closed polls show results to everyone. Polls with `closes_at` in the past count as
  closed without a write.
- A scheduling poll's deadline closes voting: after `deadline_at` the poll stays `open` (the GM
  can still pick a date) but members can't vote, again without a write. Dates already in the
  past can't be voted on or picked, and don't count for "Most yes", the "Vote" badge or "you
  haven't answered". A vote refused because the poll changed since the page loaded answers with
  `HX-Refresh` so the page reloads and shows why.
- htmx failures are reported by a small handler in `base.html` (a notice with "Reload"); a
  request whose login expired is sent to `/login` instead of swapping the login page in.
- Votes count only for current members: the grid shows one column per member, and attendance
  is pre-filled only with yes-voters who are still members.

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

| Action | Site admin (not GM) | GM of campaign | Member | Non-member |
|---|:-:|:-:|:-:|:-:|
| Create campaign | ✓ | ✓ | ✓ | ✓ |
| View campaign, members, history, pages | ✓ | ✓ | ✓ | – |
| Edit campaign, pages, reassign GM | – | ✓ | – | – |
| Create invites, remove members | – | ✓ | – | – |
| Create / decide scheduling poll, regular slots | – | ✓ | – | – |
| Vote in scheduling poll / poll | – | ✓ | ✓ | – |
| Edit attendance, session details | – | ✓ | – | – |
| Add game session entries (recap...) | – | ✓ | ✓ | – |
| Edit/delete own session entry, own comment | – | ✓ | ✓ | – |
| Delete any session entry or comment | – | ✓ | – | – |
| Comment | – | ✓ | ✓ | – |
| Create / close polls | – | ✓ | – | – |
| **Take over the GM role** (joins as member, old GM stays a member) | ✓ | | | |
| **Delete the campaign** (type its exact name to confirm) | ✓ | – | – | – |
| **Admin tools** (`/admin`): all users and campaigns | ✓ | – | – | – |

- A site admin who is also a member or the GM has those rights as well; the table shows what
  being an admin adds. Running a campaign is the GM's alone (decided 2026-10-21): an admin who
  needs to run one takes the GM role over.
- Admin tools: create users, set a user's password (logs them out everywhere), make/remove
  admins, deactivate/reactivate accounts (never yourself, so the site keeps an admin).
  Accounts are never deleted; a deactivated user can't log in and their sessions end at once.
- Deleting a campaign removes everything in it in one transaction (the schema has no cascades;
  `campaigns::db::delete` lists every table). User accounts are kept.

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
POST /campaigns/{id}/gm/take                           # site admin takes over the GM role
GET  /campaigns/{id}/banner                            # the banner image (members/admins), ?v=<updated_at>
POST /campaigns/{id}/banner                            # multipart upload, field banner (GM)
DELETE /campaigns/{id}/banner                          # htmx (GM); HX-Redirect to the edit page
GET  /campaigns/{id}/pages/new, POST /campaigns/{id}/pages               # new page (GM/admin)
GET  /campaigns/{id}/pages/{slug}                      # view (members)
GET  /campaigns/{id}/pages/{slug}/edit, POST /campaigns/{id}/pages/{slug}   # edit (GM/admin)
DELETE /campaigns/{id}/pages/{slug}                    # htmx, HX-Redirect to the campaign
GET  /campaigns/{id}/history                           # list of game sessions
GET  /campaigns/{id}/sessions/new                      # add a session without a poll (GM)
POST /campaigns/{id}/sessions                          # fields title, starts_at, user_ids (repeated)

GET  /campaigns/{id}/schedule/new                      # ?pattern_id=&weeks= fills dates from a slot
POST /campaigns/{id}/schedule
GET  /campaigns/{id}/schedule/patterns, POST …/patterns              # regular slots (GM/admin)
DELETE /campaigns/{id}/schedule/patterns/{pattern_id}                # htmx
GET  /schedule/{poll_id}                               # voting grid
GET  /campaigns/{id}/schedule/slot-dates?pattern_id=…&weeks=…   # JSON dates of a regular slot (GM); the poll form's "Add"
POST /schedule/{poll_id}/reopen                       # undo a decision (GM); deletes the untouched session
GET  /schedule/{poll_id}/edit, POST /schedule/{poll_id}   # edit an open poll (GM); dates matched by time keep their votes
PUT  /schedule/{poll_id}/options/{opt_id}/vote         # htmx, field answer; (fragment) the whole grid or HX-Refresh
POST /schedule/{poll_id}/decide                        # field option_id; redirects to the new session
POST /schedule/{poll_id}/cancel

GET  /sessions/{id}                                    # game session page: entries, attendance, comments, polls
POST /sessions/{id}                                    # edit title, date, status (GM/admin)
POST /sessions/{id}/attendance                         # repeated user_ids (GM/admin)
POST /sessions/{id}/entries                            # add an entry (members)
GET  /entries/{id}/edit, POST /entries/{id}            # edit an entry (author or GM/admin)
DELETE /entries/{id}                                   # htmx

POST /campaigns/{id}/comments, POST /sessions/{id}/comments, POST /schedule/{id}/comments
                                                      # htmx, field body_md; (fragment) the new comment
GET  /comments/{id}/edit                               # (fragment) inline edit form (author)
GET  /comments/{id}, PUT /comments/{id}                # (fragment) the comment (cancel / save)
DELETE /comments/{id}                                  # htmx (author, or GM/admin)

GET  /campaigns/{id}/polls/new[?session_id=]           # new poll (GM/admin), for the campaign or a session
POST /campaigns/{id}/polls                             # fields …, options (repeated), game_session_id?
POST /polls/{id}/vote                                  # htmx, repeated option_ids; (fragment) the poll
POST /polls/{id}/close                                 # htmx (GM/admin); (fragment) the poll

POST /markdown/preview                                 # (fragment) for the editor preview tab
GET  /healthz                                          # "ok <version>"

GET  /admin                                            # → /admin/users (site admins only, else 404)
GET  /admin/users, POST /admin/users                   # list, create a user
PUT  /admin/users/{id}/admin, PUT /admin/users/{id}/active   # htmx, value=1|0; (fragment) the row
POST /admin/users/{id}/password                        # htmx; (fragment) the row
GET  /admin/campaigns                                  # all campaigns
GET  /admin/campaigns/{id}/delete                      # confirmation page
DELETE /admin/campaigns/{id}?confirm_name=…            # htmx; HX-Redirect, or an error fragment
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
- Editor: a `<textarea>` with Write/Preview tabs (Alpine + `hx-post="/markdown/preview"`), the
  `markdown_field` macro in `templates/_markdown.html`. The preview endpoint renders whichever
  single form field it receives (`hx-include` of the textarea).
- Task-list checkboxes survive sanitizing: `<input>` is allowed with only `type="checkbox"`,
  `checked` and a forced `disabled`.
- Page slugs come from the title (accents transliterated with `deunicode`), are unique per
  campaign (`-2`, `-3`, …; "new" is reserved for the route) and never change after creation.

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
- A small deploy script (`scripts/deploy.sh`): zigbuild → backup → rsync → `systemctl restart`
  → health check. Units and env file are in `deploy/`; the full runbook is `docs/DEPLOY.md`.
- The data lives in `/var/lib/ttrpg-scheduling` (systemd `StateDirectory=`), not next to the
  binary, so a deploy never touches it.

### 8.4 Pi-specific considerations

- **SD card wear and corruption:** SQLite in WAL mode with `synchronous=NORMAL`. Write volume is
  tiny anyway. Backups must leave the card: nightly `ttrpg-scheduling backup <file>` (SQLite
  `VACUUM INTO`, so no `sqlite3` package is needed) via a systemd timer, then copy it off the Pi
  (e.g. pull from the desktop/NAS with rsync).
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
| M5 | Markdown rendering + campaign pages | `&str` vs `String`, ownership through function boundaries | done |
| M6 | Scheduling polls: manual options, voting grid (htmx), decide → game session | transactions, `chrono`, `HashMap`/grouping, iterators | done |
| M7 | Regular patterns → generated candidate dates | iterators, `chrono` date arithmetic, unit tests | done |
| M8 | Game session history, attendance, entries | putting it together; refactoring shared code | done |
| M9 | Comments and polls | generic helpers if (and only if) duplication appears | done |
| M10 | systemd hardening, backups, deploy script polish | release profile, `cfg` attributes, build scripts | done |
| M11 | Automated end-to-end tests: HTTP-level tests of the real app, then a few browser tests for the htmx/Alpine flows (see §10.1) | library + binary crate (`src/lib.rs`), `tests/` integration tests, test fixtures and helpers, `reqwest` + cookie store, `scraper` (CSS selectors on HTML), WebDriver from Rust (`thirtyfour`/`fantoccini`) | done |

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

**What was built (2026-10-21):**

- `src/lib.rs` + a thin `src/main.rs`; `router(AppState::new(db, tz))` is what both `serve()`
  and the tests run.
- `tests/e2e/` (one test binary, 26 tests, ~3 s): auth and `next`, campaigns and the permission
  matrix (404s for non-members, member vs GM vs admin), invites and registration, members/GM
  changes, pages and slugs, scheduling (validation, voting fragments, decide → session +
  attendance, cancel, regular slots), sessions (history, edit, attendance, entries), comments
  and polls. Each test gets its own migrated SQLite DB from `#[sqlx::test]`.
- `tests/browser/` (4 tests, `#[ignore]`d, ~50 s): the voting grid swap, the Markdown preview
  tab and the Alpine date list, posting and deleting comments (with the `hx-confirm` dialog),
  and the invite page's switch to registration. geckodriver is started per test on a free port.
- The browser tests found one real bug the HTTP tests couldn't: inside a form, htmx sent every
  form field to `/markdown/preview`, which rendered whichever came first (fixed with
  `hx-params`).
- Compared with Playwright: thirtyfour needs a separately running driver (started by the test
  helper here) and explicit waits via `query(..).first()` / `wait_until()`, where Playwright
  auto-waits on every action. The HTTP layer, which is the bulk of the tests, needs no browser
  at all.

## 11. Later ideas

- iCal feed per user (`/calendar/<token>.ics`) so decided sessions appear in phone calendars.
- Discord webhook notification when a poll opens or a date is decided.
- Reminders before the vote deadline.
- Scheduling polls by day, not only by exact start time: a candidate can be a whole day
  ("free any time on Sat 24 Oct"), and a member can answer for the day, optionally with a time
  window or a note ("from 19:00"). Today every option is one `starts_at`, which makes "any time
  that day" impossible to express. Picking a day-option would then ask the GM for the start time.
- File/image uploads for campaign pages (maps, handouts).
- Embedding templates/static into the binary for a single-file deploy.
