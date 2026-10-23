use super::model::Comment;
use sqlx::sqlite::SqlitePool;

/// Comments of a campaign (both ids `None`), of one game session or of one scheduling poll,
/// oldest first.
pub async fn list(
    db: &SqlitePool,
    campaign_id: i64,
    session_id: Option<i64>,
    poll_id: Option<i64>,
) -> Result<Vec<Comment>, sqlx::Error> {
    // `IS` compares NULL as a value, so one query covers both cases.
    sqlx::query_as!(
        Comment,
        "SELECT c.id, c.campaign_id, c.author_id, u.display_name AS author_name,
                c.body_md, c.created_at, c.edited_at
         FROM comments AS c JOIN users AS u ON u.id = c.author_id
         WHERE c.campaign_id = ? AND c.game_session_id IS ? AND c.schedule_poll_id IS ?
         ORDER BY c.created_at, c.id",
        campaign_id,
        session_id,
        poll_id
    )
    .fetch_all(db)
    .await
}

pub async fn get(db: &SqlitePool, id: i64) -> Result<Option<Comment>, sqlx::Error> {
    sqlx::query_as!(
        Comment,
        "SELECT c.id, c.campaign_id, c.author_id, u.display_name AS author_name,
                c.body_md, c.created_at, c.edited_at
         FROM comments AS c JOIN users AS u ON u.id = c.author_id
         WHERE c.id = ?",
        id
    )
    .fetch_optional(db)
    .await
}

pub async fn create(
    db: &SqlitePool,
    campaign_id: i64,
    session_id: Option<i64>,
    poll_id: Option<i64>,
    author_id: i64,
    body_md: &str,
) -> Result<i64, sqlx::Error> {
    let id = sqlx::query!(
        "INSERT INTO comments (campaign_id, game_session_id, schedule_poll_id, author_id, body_md)
         VALUES (?, ?, ?, ?, ?)",
        campaign_id,
        session_id,
        poll_id,
        author_id,
        body_md
    )
    .execute(db)
    .await?
    .last_insert_rowid();
    Ok(id)
}

pub async fn update(db: &SqlitePool, id: i64, body_md: &str) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE comments SET body_md = ?, edited_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
        body_md,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete(db: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM comments WHERE id = ?", id)
        .execute(db)
        .await?;
    Ok(())
}
