//! End-to-end tests: the real app over HTTP, one fresh database per test (`#[sqlx::test]`
//! creates and migrates it). Run with `cargo test`.

mod common;

mod auth;
mod campaigns;
mod comments_polls;
mod invites;
mod pages;
mod scheduling;
mod sessions;
