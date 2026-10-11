use crate::auth::password;
use anyhow::Context;
use sqlx::sqlite::SqlitePool;
use std::io::Write;

#[derive(clap::Parser)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(clap::Subcommand)]
pub enum Command {
    CreateAdmin {
        username: String,
        display_name: String,
    },
}

pub async fn create_admin(
    db: &SqlitePool,
    username: String,
    display_name: String,
) -> anyhow::Result<()> {
    print!("Password: ");
    std::io::stdout().flush()?;
    let mut password = String::new();
    std::io::stdin().read_line(&mut password)?;
    let password = password.trim_end_matches(['\r', '\n']).to_string();
    if password.is_empty() {
        anyhow::bail!("password must not be empty")
    }
    let password_hash = password::hash(password).await?;
    let new_id = sqlx::query!(
        "INSERT INTO users (username, display_name, password_hash, is_admin) VALUES (?, ?, ?, 1)",
        username,
        display_name,
        password_hash
    )
    .execute(db)
    .await
    .context("failed to create user")?
    .last_insert_rowid();
    println!("created admin {username} (id {new_id})");
    Ok(())
}
