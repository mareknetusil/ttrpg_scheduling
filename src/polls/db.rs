use super::model::{NewPoll, OptionRow, Poll, VoteRow};
use sqlx::sqlite::SqlitePool;

pub async fn create(
    db: &SqlitePool,
    campaign_id: i64,
    session_id: Option<i64>,
    user_id: i64,
    poll: &NewPoll,
) -> Result<i64, sqlx::Error> {
    let mut tx = db.begin().await?;
    let poll_id = sqlx::query!(
        "INSERT INTO polls (campaign_id, game_session_id, question, description_md, multiple_choice, closes_at, created_by)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        campaign_id,
        session_id,
        poll.question,
        poll.description_md,
        poll.multiple_choice,
        poll.closes_at,
        user_id
    )
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    for (i, label) in poll.options.iter().enumerate() {
        let sort_order = i as i64;
        sqlx::query!(
            "INSERT INTO poll_options (poll_id, label, sort_order) VALUES (?, ?, ?)",
            poll_id,
            label,
            sort_order
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(poll_id)
}

pub async fn get(db: &SqlitePool, poll_id: i64) -> Result<Option<Poll>, sqlx::Error> {
    sqlx::query_as!(
        Poll,
        r#"SELECT id, campaign_id, question, description_md,
                  multiple_choice AS "multiple_choice: bool", closes_at, closed_at
           FROM polls WHERE id = ?"#,
        poll_id
    )
    .fetch_optional(db)
    .await
}

/// Polls of a campaign (`session_id = None`) or of one game session, newest first.
pub async fn list(
    db: &SqlitePool,
    campaign_id: i64,
    session_id: Option<i64>,
) -> Result<Vec<Poll>, sqlx::Error> {
    sqlx::query_as!(
        Poll,
        r#"SELECT id, campaign_id, question, description_md,
                  multiple_choice AS "multiple_choice: bool", closes_at, closed_at
           FROM polls WHERE campaign_id = ? AND game_session_id IS ?
           ORDER BY created_at DESC, id DESC"#,
        campaign_id,
        session_id
    )
    .fetch_all(db)
    .await
}

pub async fn options(db: &SqlitePool, poll_id: i64) -> Result<Vec<OptionRow>, sqlx::Error> {
    sqlx::query_as!(
        OptionRow,
        "SELECT id, label FROM poll_options WHERE poll_id = ? ORDER BY sort_order",
        poll_id
    )
    .fetch_all(db)
    .await
}

pub async fn votes(db: &SqlitePool, poll_id: i64) -> Result<Vec<VoteRow>, sqlx::Error> {
    sqlx::query_as!(
        VoteRow,
        "SELECT v.poll_option_id, v.user_id, u.display_name
         FROM poll_votes AS v
         JOIN poll_options AS o ON o.id = v.poll_option_id
         JOIN users AS u ON u.id = v.user_id
         WHERE o.poll_id = ? ORDER BY u.display_name",
        poll_id
    )
    .fetch_all(db)
    .await
}

/// Replaces the user's votes in this poll. Option ids from other polls are ignored.
pub async fn set_votes(
    db: &SqlitePool,
    poll_id: i64,
    user_id: i64,
    option_ids: &[i64],
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!(
        "DELETE FROM poll_votes WHERE user_id = ?
         AND poll_option_id IN (SELECT id FROM poll_options WHERE poll_id = ?)",
        user_id,
        poll_id
    )
    .execute(&mut *tx)
    .await?;
    for option_id in option_ids {
        sqlx::query!(
            "INSERT OR IGNORE INTO poll_votes (poll_option_id, user_id)
             SELECT id, ? FROM poll_options WHERE id = ? AND poll_id = ?",
            user_id,
            option_id,
            poll_id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn close(db: &SqlitePool, poll_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE polls SET closed_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ? AND closed_at IS NULL",
        poll_id
    )
    .execute(db)
    .await?;
    Ok(())
}
