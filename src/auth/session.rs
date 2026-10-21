use super::token;
use axum_extra::extract::cookie::{Cookie, SameSite};
use sqlx::sqlite::SqlitePool;

/// The session cookie for `token`. Long-lived; the real limit is `expires_at` in the DB.
pub fn cookie(token: String) -> Cookie<'static> {
    Cookie::build(("session", token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .permanent()
        .build()
}

pub async fn create(db: &SqlitePool, user_id: i64) -> Result<String, sqlx::Error> {
    sqlx::query!(
        "DELETE FROM auth_sessions WHERE expires_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now')"
    )
    .execute(db)
    .await?;
    let token = token::generate();
    sqlx::query!(
        "INSERT INTO auth_sessions (id, user_id, expires_at) VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+30 days'))",
        token,
        user_id
    ).execute(db).await?;
    Ok(token)
}

pub async fn remove(db: &SqlitePool, token: &str) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM auth_sessions WHERE id=?", token)
        .execute(db)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test]
    async fn create_inserts_session(db: SqlitePool) -> sqlx::Result<()> {
        // Arrange: auth_session.user_id is a foreign key, so a user must exist first
        let user_id = sqlx::query!(
            "INSERT INTO users (username, display_name, password_hash) VALUES ('alice', 'Alice', 'x')").execute(&db).await?.last_insert_rowid();

        // Act
        let token = create(&db, user_id).await?;

        let row = sqlx::query!(
            "SELECT user_id, expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now') AS in_future
            FROM auth_sessions WHERE id = ?",
            token
        )
        .fetch_one(&db)
        .await?;
        // Assert
        assert_eq!(row.user_id, user_id);
        assert_eq!(row.in_future, 1);

        Ok(())
    }
}
