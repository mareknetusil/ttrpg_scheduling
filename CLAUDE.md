# TTRPG Scheduling

A small self-hosted web app for scheduling tabletop RPG sessions within a group of friends.
It is reachable only over a VPN. Inspired by RollRota; Doodle and Hyperfictive were tried and rejected.

**Production target: Raspberry Pi 3B+ (64-bit Raspberry Pi OS, aarch64, 1 GB RAM).** Never build on the Pi.
Cross-compile on the x86_64 desktop with `cargo-zigbuild` to a static musl target (see
`docs/DESIGN.md` §8). Keep that in mind when choosing dependencies: avoid crates that need
system C libraries (prefer pure Rust or `bundled` features, e.g. rustls instead of OpenSSL).

See `docs/DESIGN.md` for requirements, architecture, data model and the milestone plan.

## Stack

- **Backend:** Rust: `axum` (HTTP), `tokio` (async runtime), `sqlx` + SQLite (DB and migrations),
  `minijinja` (server-side templates), `pulldown-cmark` + `ammonia` (Markdown to sanitized HTML),
  `argon2` (password hashing), `chrono` / `chrono-tz` (time handling).
- **Frontend:** server-rendered HTML + htmx + Alpine.js + plain CSS. No JS build step, no SPA.

## How we work together (important)

The owner is an experienced Python + htmx + Alpine.js developer who is **learning Rust through
this project**. The repo has two modes of collaboration.

### Backend (`src/`, `Cargo.toml`, `migrations/`): guided / teaching mode

- **Do not write complete backend implementations unless the user explicitly asks you to**
  (e.g. "just write it", "implement this for me"). The goal is for the user to write the Rust.
- Default flow for a backend task:
  1. Explain the concept(s) involved and how they map to what the user already knows from Python
     (e.g. `Result` + `?` vs exceptions, traits vs protocols/ABCs, extractors vs FastAPI dependencies).
  2. Propose the shape: module layout, types, function signatures, possibly a skeleton with `todo!()`.
  3. Let the user implement. Give hints step by step, smallest useful hint first.
  4. Review the user's code: correctness, idiomatic Rust, error handling, clippy-style feedback.
     Explain *why*, not only *what*.
- When the compiler complains, **explain the error message** (borrowing, lifetimes, trait bounds)
  before suggesting a fix. Point to the relevant Rust Book / crate docs section when useful.
- Fine to write without asking: `Cargo.toml` dependency lines, config files, small boilerplate
  the user has already written once before, and test scaffolding the user asks for.
- If unsure whether the user wants to write something themselves, ask.
- Prefer simple, explicit Rust over clever abstractions. No macros, generics or traits added just
  for elegance; introduce them when they solve a problem the user has actually hit.

### Frontend (`templates/`, `static/`): vibe-code mode

- Write templates, CSS and small Alpine/htmx snippets freely.
- **Do not modify Rust code during frontend work.** The contract between backend and frontend is
  the template context (the variables each handler passes to a template) and the routes listed
  in `docs/DESIGN.md`. If a template needs data the handler does not provide, say so and describe
  the needed field rather than editing the handler.
- htmx partials are rendered from named blocks of the full-page template where possible
  (so one template file serves both the full page and the fragment).
- User-provided Markdown arrives already rendered and sanitized; output it with `|safe` only for
  fields documented as pre-rendered HTML (`*_html`). Never mark raw user input as safe.

## Commands

```sh
cargo run                      # run dev server
cargo check                    # fast type check
cargo clippy --all-targets     # lints, keep warnings at zero
cargo fmt                      # format
cargo test                     # tests (DB tests use sqlx::test with a temp SQLite DB)
sqlx migrate add <name>        # new migration (needs: cargo install sqlx-cli)
sqlx migrate run               # apply migrations

# Release build for the Pi (64-bit Raspberry Pi OS)
cargo zigbuild --release --target aarch64-unknown-linux-musl
```

## Conventions

- "Session" is ambiguous here. Use **`game_session`** for a played/planned TTRPG session and
  **`auth_session`** for a login session, in code, tables and routes alike.
- All timestamps are stored in UTC in the DB and converted to the display time zone at render time.
- Schema changes go through `sqlx` migrations only; never edit an already-applied migration.
- Errors: handlers return `Result<_, AppError>`; `AppError` implements `IntoResponse`.
- Authorization checks live in the backend; templates may hide buttons, but must not be the only check.
