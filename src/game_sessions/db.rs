use super::model::{
    Attendee, Entry, EntryForm, EntryKind, GameSession, HistoryRow, SessionStatus, UpcomingRow,
};
use crate::notifications::{
    db::notify,
    model::{Kind, Notice, Target},
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

/// Adds a session by hand (not from a poll) with the given attendance, in one transaction, and
/// tells the members (`actor_id` added it). Members' ids only; others are ignored.
pub async fn create(
    db: &SqlitePool,
    campaign_id: i64,
    title: &str,
    starts_at: &str,
    status: SessionStatus,
    user_ids: &[i64],
    actor_id: i64,
) -> Result<i64, sqlx::Error> {
    let mut tx = db.begin().await?;
    let id = insert(&mut tx, campaign_id, title, starts_at, status, user_ids).await?;
    notify(
        &mut tx,
        &Notice {
            campaign_id,
            actor_id,
            kind: Kind::SessionAdded,
            target: Target {
                game_session_id: Some(id),
                ..Target::default()
            },
        },
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Inserts a game session with the given attendance (members' ids only; others are ignored) and
/// renumbers the campaign's sessions, inside the caller's transaction. Doesn't notify: the caller
/// knows why the session exists (added by hand, or a scheduling poll was decided).
pub async fn insert(
    conn: &mut SqliteConnection,
    campaign_id: i64,
    title: &str,
    starts_at: &str,
    status: SessionStatus,
    user_ids: &[i64],
) -> Result<i64, sqlx::Error> {
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
    .fetch_one(&mut *conn)
    .await?;
    for user_id in user_ids {
        sqlx::query!(
            "INSERT OR IGNORE INTO game_session_attendance (game_session_id, user_id)
             SELECT ?, user_id FROM campaign_members WHERE campaign_id = ? AND user_id = ?",
            id,
            campaign_id,
            user_id
        )
        .execute(&mut *conn)
        .await?;
    }
    renumber(conn, campaign_id).await?;
    Ok(id)
}

/// The number the next game session of the campaign will get (for a default poll title).
pub async fn next_number(db: &SqlitePool, campaign_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COALESCE(MAX(number), 0) + 1 AS "n!: i64" FROM game_sessions WHERE campaign_id = ?"#,
        campaign_id
    )
    .fetch_one(db)
    .await
}

/// Is the game session untouched: still planned, with no entries, comments or polls? Only then
/// may it be deleted (`delete_if_untouched`). Attendance and edits to its date or title don't
/// count: deleting it is how a scheduling poll is reopened when the date fell through.
pub async fn untouched(conn: &mut SqliteConnection, id: i64) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT (status = 'planned'
                   AND NOT EXISTS (SELECT 1 FROM game_session_entries WHERE game_session_id = s.id)
                   AND NOT EXISTS (SELECT 1 FROM comments WHERE game_session_id = s.id)
                   AND NOT EXISTS (SELECT 1 FROM polls WHERE game_session_id = s.id)) AS "ok!: bool"
           FROM game_sessions AS s WHERE s.id = ?"#,
        id
    )
    .fetch_optional(conn)
    .await
    .map(|ok| ok.unwrap_or(false))
}

/// Deletes the game session (with its attendance and notifications) and renumbers the rest, inside
/// the caller's transaction, but only if it's `untouched`. The one exception to "game sessions
/// are never deleted". Returns false (and changes nothing) otherwise. Doesn't notify. The caller
/// must first clear anything that points at the session (a decided poll's `game_session_id`).
pub async fn delete_if_untouched(
    conn: &mut SqliteConnection,
    id: i64,
) -> Result<bool, sqlx::Error> {
    if !untouched(conn, id).await? {
        return Ok(false);
    }
    sqlx::query!(
        "DELETE FROM game_session_attendance WHERE game_session_id = ?",
        id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!("DELETE FROM notifications WHERE game_session_id = ?", id)
        .execute(&mut *conn)
        .await?;
    let campaign_id = sqlx::query_scalar!(
        "DELETE FROM game_sessions WHERE id = ? RETURNING campaign_id",
        id
    )
    .fetch_one(&mut *conn)
    .await?;
    renumber(conn, campaign_id).await?;
    Ok(true)
}

/// Numbers the campaign's sessions 1, 2, 3, … by date (cancelled ones included), so a session
/// added for an earlier date gets the right number. Run after every insert, delete or date change
/// (all of which happen in this module).
async fn renumber(conn: &mut SqliteConnection, campaign_id: i64) -> Result<(), sqlx::Error> {
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

/// Saves title, date and status. Cancelling the session or moving it to another date tells the
/// members (`actor_id` did it).
pub async fn update(
    db: &SqlitePool,
    session: &GameSession,
    title: &str,
    starts_at: &str,
    status: SessionStatus,
    actor_id: i64,
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
    let kind = if status == SessionStatus::Cancelled && session.status != SessionStatus::Cancelled {
        Some(Kind::SessionCancelled)
    } else if starts_at != session.starts_at {
        Some(Kind::SessionMoved)
    } else {
        None
    };
    if let Some(kind) = kind {
        notify(
            &mut tx,
            &Notice {
                campaign_id: session.campaign_id,
                actor_id,
                kind,
                target: Target {
                    game_session_id: Some(session.id),
                    ..Target::default()
                },
            },
        )
        .await?;
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

/// Adds an entry to a session of `campaign_id` and tells the other members.
pub async fn create_entry(
    db: &SqlitePool,
    campaign_id: i64,
    session_id: i64,
    author_id: i64,
    form: &EntryForm,
) -> Result<i64, sqlx::Error> {
    let mut tx = db.begin().await?;
    let id = sqlx::query!(
        "INSERT INTO game_session_entries (game_session_id, kind, title, body_md, author_id) VALUES (?, ?, ?, ?, ?)",
        session_id,
        form.kind,
        form.title,
        form.body_md,
        author_id
    )
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    notify(
        &mut tx,
        &entry_notice(campaign_id, session_id, id, author_id, Kind::EntryAdded),
    )
    .await?;
    tx.commit().await?;
    Ok(id)
}

/// Saves an edited entry (of a session of `campaign_id`) and tells the members (`actor_id`
/// edited it: the author, or the GM).
pub async fn update_entry(
    db: &SqlitePool,
    campaign_id: i64,
    entry: &Entry,
    form: &EntryForm,
    actor_id: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!(
        "UPDATE game_session_entries SET kind = ?, title = ?, body_md = ?,
         updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
        form.kind,
        form.title,
        form.body_md,
        entry.id
    )
    .execute(&mut *tx)
    .await?;
    let notice = entry_notice(
        campaign_id,
        entry.game_session_id,
        entry.id,
        actor_id,
        Kind::EntryEdited,
    );
    notify(&mut tx, &notice).await?;
    tx.commit().await?;
    Ok(())
}

fn entry_notice(
    campaign_id: i64,
    session_id: i64,
    entry_id: i64,
    actor_id: i64,
    kind: Kind,
) -> Notice {
    Notice {
        campaign_id,
        actor_id,
        kind,
        target: Target {
            game_session_id: Some(session_id),
            entry_id: Some(entry_id),
            ..Target::default()
        },
    }
}

/// Deletes an entry and the notifications about it.
pub async fn delete_entry(db: &SqlitePool, id: i64) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!("DELETE FROM notifications WHERE entry_id = ?", id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM game_session_entries WHERE id = ?", id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
