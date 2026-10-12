use sqlx::sqlite::SqlitePool;

pub async fn create(db: &SqlitePool, user_id: i64) -> Result<String, sqlx::Error> {
    let bytes: [u8; 32] = rand::random();
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query!(
        "INSERT INTO auth_sessions (id, user_id, expires_at) VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '+30 days'))",
        token,
        user_id
    ).execute(db).await?;
    Ok(token)
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

        // Assert
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));

        let row = sqlx::query!(
            "SELECT user_id, expires_at > strftime('%Y-%m-%dT%H%M%SZ', 'now') AS in_future
            FROM auth_sessions WHERE id = ?",
            token
        )
        .fetch_one(&db)
        .await?;
        assert_eq!(row.user_id, user_id);
        assert_eq!(row.in_future, 1);

        Ok(())
    }
}
