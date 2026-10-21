use super::model::{CampaignContents, CampaignRow, UserRow};
use sqlx::sqlite::SqlitePool;

pub async fn users(db: &SqlitePool) -> Result<Vec<UserRow>, sqlx::Error> {
    sqlx::query_as!(
        UserRow,
        r#"SELECT u.id, u.username, u.display_name, u.is_admin AS "is_admin: bool",
                  u.deactivated_at IS NOT NULL AS "deactivated: bool", u.created_at,
                  (SELECT COUNT(*) FROM campaign_members WHERE user_id = u.id) AS "campaign_count!: i64"
           FROM users AS u ORDER BY u.deactivated_at IS NOT NULL, u.display_name"#
    )
    .fetch_all(db)
    .await
}

pub async fn user(db: &SqlitePool, id: i64) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as!(
        UserRow,
        r#"SELECT u.id, u.username, u.display_name, u.is_admin AS "is_admin: bool",
                  u.deactivated_at IS NOT NULL AS "deactivated: bool", u.created_at,
                  (SELECT COUNT(*) FROM campaign_members WHERE user_id = u.id) AS "campaign_count!: i64"
           FROM users AS u WHERE u.id = ?"#,
        id
    )
    .fetch_optional(db)
    .await
}

pub async fn create_user(
    db: &SqlitePool,
    username: &str,
    display_name: &str,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO users (username, display_name, password_hash) VALUES (?, ?, ?)",
        username,
        display_name,
        password_hash
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Sets a new password and logs the user out everywhere.
pub async fn set_password(
    db: &SqlitePool,
    id: i64,
    password_hash: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!(
        "UPDATE users SET password_hash = ? WHERE id = ?",
        password_hash,
        id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!("DELETE FROM auth_sessions WHERE user_id = ?", id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

pub async fn set_admin(db: &SqlitePool, id: i64, is_admin: bool) -> Result<(), sqlx::Error> {
    sqlx::query!("UPDATE users SET is_admin = ? WHERE id = ?", is_admin, id)
        .execute(db)
        .await?;
    Ok(())
}

/// Deactivating also ends the user's sessions, so it takes effect at once.
pub async fn set_active(db: &SqlitePool, id: i64, active: bool) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    if active {
        sqlx::query!("UPDATE users SET deactivated_at = NULL WHERE id = ?", id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query!(
            "UPDATE users SET deactivated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
             WHERE id = ? AND deactivated_at IS NULL",
            id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query!("DELETE FROM auth_sessions WHERE user_id = ?", id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

pub async fn campaigns(db: &SqlitePool) -> Result<Vec<CampaignRow>, sqlx::Error> {
    sqlx::query_as!(
        CampaignRow,
        r#"SELECT c.id, c.name, c.system, u.display_name AS gm_name, c.created_at,
                  (SELECT COUNT(*) FROM campaign_members WHERE campaign_id = c.id) AS "member_count!: i64",
                  (SELECT COUNT(*) FROM game_sessions WHERE campaign_id = c.id) AS "session_count!: i64"
           FROM campaigns AS c JOIN users AS u ON u.id = c.gm_user_id
           ORDER BY c.name"#
    )
    .fetch_all(db)
    .await
}

pub async fn contents(db: &SqlitePool, campaign_id: i64) -> Result<CampaignContents, sqlx::Error> {
    sqlx::query_as!(
        CampaignContents,
        r#"SELECT
             (SELECT COUNT(*) FROM campaign_members WHERE campaign_id = ?1) AS "members!: i64",
             (SELECT COUNT(*) FROM game_sessions WHERE campaign_id = ?1) AS "sessions!: i64",
             (SELECT COUNT(*) FROM campaign_pages WHERE campaign_id = ?1) AS "pages!: i64",
             (SELECT COUNT(*) FROM polls WHERE campaign_id = ?1)
               + (SELECT COUNT(*) FROM schedule_polls WHERE campaign_id = ?1) AS "polls!: i64",
             (SELECT COUNT(*) FROM comments WHERE campaign_id = ?1) AS "comments!: i64""#,
        campaign_id
    )
    .fetch_one(db)
    .await
}
