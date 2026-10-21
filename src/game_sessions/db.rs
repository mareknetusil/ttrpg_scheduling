use super::model::{Attendee, GameSession, SessionStatus, UpcomingRow};
use sqlx::sqlite::SqlitePool;

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
