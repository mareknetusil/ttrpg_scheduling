use super::model::{
    Attendee, Entry, EntryForm, EntryKind, GameSession, HistoryRow, SessionStatus, UpcomingRow,
};
use sqlx::sqlite::{SqliteConnection, SqlitePool};

pub async fn get(db: &SqlitePool, id: i64) -> Result<Option<GameSession>, sqlx::Error> {
    sqlx::query_as!(
        GameSession,
        r#"SELECT id, campaign_id, number, title, starts_at, status AS "status: SessionStatus"
           FROM game_sessions WHERE id = ?"#,
        id
    )
    .fetch_optional(db)
    .await
}

pub async fn attendees(db: &SqlitePool, session_id: i64) -> Result<Vec<Attendee>, sqlx::Error> {
    sqlx::query_as!(
        Attendee,
        "SELECT u.id AS user_id, u.display_name FROM game_session_attendance AS a
         JOIN users AS u ON u.id = a.user_id WHERE a.game_session_id = ? ORDER BY u.display_name",
        session_id
    )
    .fetch_all(db)
    .await
}

/// Planned sessions of one campaign that haven't started yet, soonest first.
pub async fn upcoming_for_campaign(
    db: &SqlitePool,
    campaign_id: i64,
) -> Result<Vec<GameSession>, sqlx::Error> {
    sqlx::query_as!(
        GameSession,
        r#"SELECT id, campaign_id, number, title, starts_at, status AS "status: SessionStatus"
           FROM game_sessions
           WHERE campaign_id = ? AND status = 'planned'
             AND starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-6 hours')
           ORDER BY starts_at"#,
        campaign_id
    )
    .fetch_all(db)
    .await
}

/// Planned upcoming sessions in all of the user's campaigns, soonest first (dashboard).
pub async fn upcoming_for_user(
    db: &SqlitePool,
    user_id: i64,
) -> Result<Vec<UpcomingRow>, sqlx::Error> {
    sqlx::query_as!(
        UpcomingRow,
        "SELECT s.id, s.campaign_id, c.name AS campaign_name, s.number, s.title, s.starts_at
         FROM game_sessions AS s
         JOIN campaigns AS c ON c.id = s.campaign_id
         JOIN campaign_members AS m ON m.campaign_id = s.campaign_id AND m.user_id = ?
         WHERE s.status = 'planned' AND c.archived_at IS NULL
           AND s.starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-6 hours')
         ORDER BY s.starts_at
         LIMIT 10",
        user_id
    )
    .fetch_all(db)
    .await
}

pub async fn history(db: &SqlitePool, campaign_id: i64) -> Result<Vec<HistoryRow>, sqlx::Error> {
    sqlx::query_as!(
        HistoryRow,
        r#"SELECT s.id, s.campaign_id, s.number, s.title, s.starts_at, s.status AS "status: SessionStatus",
                  (SELECT COUNT(*) FROM game_session_attendance WHERE game_session_id = s.id) AS "attendee_count!: i64",
                  (SELECT COUNT(*) FROM game_session_entries WHERE game_session_id = s.id) AS "entry_count!: i64"
           FROM game_sessions AS s WHERE s.campaign_id = ?
           ORDER BY s.starts_at DESC"#,
        campaign_id
    )
    .fetch_all(db)
    .await
}

/// Adds a session by hand (not from a poll) with the given attendance, in one transaction.
/// Members' ids only; others are ignored.
pub async fn create(
    db: &SqlitePool,
    campaign_id: i64,
    title: &str,
    starts_at: &str,
    status: SessionStatus,
    user_ids: &[i64],
) -> Result<i64, sqlx::Error> {
    let mut tx = db.begin().await?;
    // A placeholder number (unique for now); `renumber` puts it in date order.
    let id = sqlx::query_scalar!(
        r#"INSERT INTO game_sessions (campaign_id, number, title, starts_at, status)
           VALUES (?, (SELECT COALESCE(MAX(number), 0) + 1 FROM game_sessions WHERE campaign_id = ?), ?, ?, ?)
           RETURNING id AS "id!""#,
        campaign_id,
        campaign_id,
        title,
        starts_at,
        status
    )
    .fetch_one(&mut *tx)
    .await?;
    for user_id in user_ids {
        sqlx::query!(
            "INSERT OR IGNORE INTO game_session_attendance (game_session_id, user_id)
             SELECT ?, user_id FROM campaign_members WHERE campaign_id = ? AND user_id = ?",
            id,
            campaign_id,
            user_id
        )
        .execute(&mut *tx)
        .await?;
    }
    renumber(&mut tx, campaign_id).await?;
    tx.commit().await?;
    Ok(id)
}

/// Numbers the campaign's sessions 1, 2, 3, … by date (cancelled ones included), so a session
/// added for an earlier date gets the right number. Run after every insert or date change.
pub async fn renumber(conn: &mut SqliteConnection, campaign_id: i64) -> Result<(), sqlx::Error> {
    // Negate first: `UNIQUE (campaign_id, number)` is checked row by row, so moving numbers in
    // place could collide halfway through.
    sqlx::query!(
        "UPDATE game_sessions SET number = -number WHERE campaign_id = ?",
        campaign_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "UPDATE game_sessions SET number = r.n
         FROM (SELECT id, ROW_NUMBER() OVER (ORDER BY starts_at, id) AS n
               FROM game_sessions WHERE campaign_id = ?) AS r
         WHERE game_sessions.id = r.id",
        campaign_id
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub async fn update(
    db: &SqlitePool,
    session: &GameSession,
    title: &str,
    starts_at: &str,
    status: SessionStatus,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!(
        "UPDATE game_sessions SET title = ?, starts_at = ?, status = ? WHERE id = ?",
        title,
        starts_at,
        status,
        session.id
    )
    .execute(&mut *tx)
    .await?;
    if starts_at != session.starts_at {
        renumber(&mut tx, session.campaign_id).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Replaces the attendance. Ids that aren't members of the campaign are ignored.
pub async fn set_attendance(
    db: &SqlitePool,
    session_id: i64,
    campaign_id: i64,
    user_ids: &[i64],
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!(
        "DELETE FROM game_session_attendance WHERE game_session_id = ?",
        session_id
    )
    .execute(&mut *tx)
    .await?;
    for user_id in user_ids {
        sqlx::query!(
            "INSERT OR IGNORE INTO game_session_attendance (game_session_id, user_id)
             SELECT ?, user_id FROM campaign_members WHERE campaign_id = ? AND user_id = ?",
            session_id,
            campaign_id,
            user_id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn entries(db: &SqlitePool, session_id: i64) -> Result<Vec<Entry>, sqlx::Error> {
    sqlx::query_as!(
        Entry,
        r#"SELECT e.id, e.game_session_id, e.kind AS "kind: EntryKind", e.title, e.body_md,
                  e.author_id, u.display_name AS author_name, e.created_at, e.updated_at
           FROM game_session_entries AS e JOIN users AS u ON u.id = e.author_id
           WHERE e.game_session_id = ? ORDER BY e.created_at, e.id"#,
        session_id
    )
    .fetch_all(db)
    .await
}

pub async fn entry(db: &SqlitePool, id: i64) -> Result<Option<Entry>, sqlx::Error> {
    sqlx::query_as!(
        Entry,
        r#"SELECT e.id, e.game_session_id, e.kind AS "kind: EntryKind", e.title, e.body_md,
                  e.author_id, u.display_name AS author_name, e.created_at, e.updated_at
           FROM game_session_entries AS e JOIN users AS u ON u.id = e.author_id
           WHERE e.id = ?"#,
        id
    )
    .fetch_optional(db)
    .await
}

pub async fn create_entry(
    db: &SqlitePool,
    session_id: i64,
    author_id: i64,
    form: &EntryForm,
) -> Result<i64, sqlx::Error> {
    let id = sqlx::query!(
        "INSERT INTO game_session_entries (game_session_id, kind, title, body_md, author_id) VALUES (?, ?, ?, ?, ?)",
        session_id,
        form.kind,
        form.title,
        form.body_md,
        author_id
    )
    .execute(db)
    .await?
    .last_insert_rowid();
    Ok(id)
}

pub async fn update_entry(db: &SqlitePool, id: i64, form: &EntryForm) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE game_session_entries SET kind = ?, title = ?, body_md = ?,
         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
        form.kind,
        form.title,
        form.body_md,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete_entry(db: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM game_session_entries WHERE id = ?", id)
        .execute(db)
        .await?;
    Ok(())
}
