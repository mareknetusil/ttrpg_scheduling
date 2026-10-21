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
    /// Create an admin account (reads the password from stdin).
    CreateAdmin {
        username: String,
        display_name: String,
    },
    /// Write a consistent copy of the database to a new file (safe while the server runs).
    Backup { destination: std::path::PathBuf },
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

/// `VACUUM INTO` writes a consistent, compacted copy of the live database, even while the server
/// is writing (it reads one snapshot). So the Pi doesn't need the `sqlite3` command-line tool.
pub async fn backup(db: &SqlitePool, destination: &std::path::Path) -> anyhow::Result<()> {
    if destination.exists() {
        anyhow::bail!("{} already exists", destination.display());
    }
    let path = destination
        .to_str()
        .context("the backup path must be valid UTF-8")?;
    sqlx::query("VACUUM INTO ?")
        .bind(path)
        .execute(db)
        .await
        .with_context(|| format!("failed to write backup to {path}"))?;
    println!("backup written to {path}");
    Ok(())
}
