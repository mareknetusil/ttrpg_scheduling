use super::model::{Kind, Notice, NotificationRow, Viewing};
use crate::game_sessions::model::EntryKind;
use sqlx::sqlite::{SqliteConnection, SqlitePool};

/// Tells every other member of the campaign about an action. Call it inside the transaction
/// that makes the change. A member who still has an unseen notification about the same thing by
/// the same actor (or the addition an edit folds into, see `Kind::folds_into`) gets that one
/// moved to now instead of a second one. Also deletes notifications older than 60 days.
pub async fn notify(conn: &mut SqliteConnection, n: &Notice) -> Result<(), sqlx::Error> {
    let t = &n.target;
    let folds_into = n.kind.folds_into();
    sqlx::query!(
        "UPDATE notifications SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE campaign_id = ? AND actor_id = ? AND seen_at IS NULL AND kind IN (?, ?)
           AND game_session_id IS ? AND schedule_poll_id IS ? AND poll_id IS ?
           AND entry_id IS ? AND comment_id IS ? AND page_id IS ?",
        n.campaign_id,
        n.actor_id,
        n.kind,
        folds_into,
        t.game_session_id,
        t.schedule_poll_id,
        t.poll_id,
        t.entry_id,
        t.comment_id,
        t.page_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "INSERT INTO notifications (user_id, campaign_id, actor_id, kind, game_session_id,
                                    schedule_poll_id, poll_id, entry_id, comment_id, page_id)
         SELECT m.user_id, m.campaign_id, ?, ?, ?, ?, ?, ?, ?, ?
         FROM campaign_members AS m
         WHERE m.campaign_id = ? AND m.user_id != ?
           AND NOT EXISTS (
             SELECT 1 FROM notifications AS x
             WHERE x.user_id = m.user_id AND x.campaign_id = m.campaign_id AND x.actor_id = ?
               AND x.seen_at IS NULL AND x.kind IN (?, ?)
               AND x.game_session_id IS ? AND x.schedule_poll_id IS ? AND x.poll_id IS ?
               AND x.entry_id IS ? AND x.comment_id IS ? AND x.page_id IS ?)",
        n.actor_id,
        n.kind,
        t.game_session_id,
        t.schedule_poll_id,
        t.poll_id,
        t.entry_id,
        t.comment_id,
        t.page_id,
        n.campaign_id,
        n.actor_id,
        n.actor_id,
        n.kind,
        folds_into,
        t.game_session_id,
        t.schedule_poll_id,
        t.poll_id,
        t.entry_id,
        t.comment_id,
        t.page_id
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        "DELETE FROM notifications
         WHERE updated_at < strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-60 days')"
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Marks the user's unseen notifications about what a page shows as seen. Returns how many.
pub async fn seen(db: &SqlitePool, user_id: i64, viewing: &Viewing) -> Result<u64, sqlx::Error> {
    let result = match *viewing {
        Viewing::SchedulePoll(id) => {
            sqlx::query!(
                "UPDATE notifications SET seen_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE user_id = ? AND seen_at IS NULL AND schedule_poll_id = ?",
                user_id,
                id
            )
            .execute(db)
            .await?
        }
        Viewing::GameSession(id) => {
            sqlx::query!(
                "UPDATE notifications SET seen_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE user_id = ? AND seen_at IS NULL AND game_session_id = ?",
                user_id,
                id
            )
            .execute(db)
            .await?
        }
        // Whatever belongs to the campaign itself: its details, and comments and polls that
        // aren't on a game session or scheduling poll
        Viewing::Campaign(id) => {
            sqlx::query!(
                "UPDATE notifications SET seen_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE user_id = ? AND seen_at IS NULL AND campaign_id = ? AND kind != 'gm_changed'
                   AND game_session_id IS NULL AND schedule_poll_id IS NULL
                   AND entry_id IS NULL AND page_id IS NULL",
                user_id,
                id
            )
            .execute(db)
            .await?
        }
        Viewing::CampaignPage(id) => {
            sqlx::query!(
                "UPDATE notifications SET seen_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE user_id = ? AND seen_at IS NULL AND page_id = ?",
                user_id,
                id
            )
            .execute(db)
            .await?
        }
        Viewing::Members(id) => {
            sqlx::query!(
                "UPDATE notifications SET seen_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
                 WHERE user_id = ? AND seen_at IS NULL AND campaign_id = ? AND kind = 'gm_changed'",
                user_id,
                id
            )
            .execute(db)
            .await?
        }
    };
    Ok(result.rows_affected())
}

/// "Mark all as seen".
pub async fn seen_all(db: &SqlitePool, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE notifications SET seen_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         WHERE user_id = ? AND seen_at IS NULL",
        user_id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// The user's unseen (`seen` false) or seen notifications, newest first, at most `limit`.
pub async fn list(
    db: &SqlitePool,
    user_id: i64,
    seen: bool,
    limit: i64,
) -> Result<Vec<NotificationRow>, sqlx::Error> {
    sqlx::query_as!(
        NotificationRow,
        r#"SELECT n.id, n.campaign_id, c.name AS campaign_name, a.display_name AS actor_name,
                  n.actor_id, c.gm_user_id, g.display_name AS gm_name, n.kind AS "kind: Kind",
                  n.game_session_id, s.number AS "session_number?", s.title AS "session_title?",
                  s.starts_at AS "session_starts_at?",
                  n.schedule_poll_id, sp.title AS "schedule_poll_title?",
                  n.poll_id, n.entry_id, n.comment_id,
                  p.question AS "poll_question?", e.kind AS "entry_kind?: EntryKind",
                  cm.body_md AS "comment_body?", pg.title AS "page_title?", pg.slug AS "page_slug?",
                  n.updated_at, n.seen_at
           FROM notifications AS n
           JOIN campaigns AS c ON c.id = n.campaign_id
           JOIN users AS a ON a.id = n.actor_id
           JOIN users AS g ON g.id = c.gm_user_id
           LEFT JOIN game_sessions AS s ON s.id = n.game_session_id
           LEFT JOIN schedule_polls AS sp ON sp.id = n.schedule_poll_id
           LEFT JOIN polls AS p ON p.id = n.poll_id
           LEFT JOIN game_session_entries AS e ON e.id = n.entry_id
           LEFT JOIN comments AS cm ON cm.id = n.comment_id
           LEFT JOIN campaign_pages AS pg ON pg.id = n.page_id
           WHERE n.user_id = ? AND (n.seen_at IS NOT NULL) = ?
           ORDER BY n.updated_at DESC, n.id DESC
           LIMIT ?"#,
        user_id,
        seen,
        limit
    )
    .fetch_all(db)
    .await
}

/// A member left or was removed: their notifications from that campaign go.
pub async fn forget_member(
    conn: &mut SqliteConnection,
    campaign_id: i64,
    user_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM notifications WHERE campaign_id = ? AND user_id = ?",
        campaign_id,
        user_id
    )
    .execute(conn)
    .await?;
    Ok(())
}
