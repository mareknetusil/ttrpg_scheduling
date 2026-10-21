//! End-to-end tests: the real app over HTTP, one fresh database per test (`#[sqlx::test]`
//! creates and migrates it). Run with `cargo test`.

mod common;

mod admin;
mod auth;
mod campaigns;
mod card_style;
mod comments_polls;
mod invites;
mod pages;
mod scheduling;
mod sessions;
