use super::model::{
    Answer, NewPattern, NewPoll, OptionRow, PatternRow, Poll, PollStatus, PollSummary, VoteRow,
};
use sqlx::sqlite::SqlitePool;

pub async fn create(
    db: &SqlitePool,
    campaign_id: i64,
    user_id: i64,
    poll: &NewPoll,
) -> Result<i64, sqlx::Error> {
    let mut tx = db.begin().await?;
    let poll_id = sqlx::query!(
        "INSERT INTO schedule_polls (campaign_id, title, description_md, deadline_at, created_by) VALUES (?, ?, ?, ?, ?)",
        campaign_id,
        poll.title,
        poll.description_md,
        poll.deadline_at,
        user_id
    )
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    for starts_at in &poll.options {
        sqlx::query!(
            "INSERT INTO schedule_options (poll_id, starts_at) VALUES (?, ?)",
            poll_id,
            starts_at
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
        r#"SELECT id, campaign_id, title, description_md, status AS "status: PollStatus",
                  deadline_at, decided_option_id, game_session_id
           FROM schedule_polls WHERE id = ?"#,
        poll_id
    )
    .fetch_optional(db)
    .await
}

pub async fn options(db: &SqlitePool, poll_id: i64) -> Result<Vec<OptionRow>, sqlx::Error> {
    sqlx::query_as!(
        OptionRow,
        "SELECT id, starts_at FROM schedule_options WHERE poll_id = ? ORDER BY starts_at",
        poll_id
    )
    .fetch_all(db)
    .await
}

pub async fn votes(db: &SqlitePool, poll_id: i64) -> Result<Vec<VoteRow>, sqlx::Error> {
    sqlx::query_as!(
        VoteRow,
        r#"SELECT v.option_id, v.user_id, v.answer AS "answer: Answer"
           FROM schedule_votes AS v JOIN schedule_options AS o ON o.id = v.option_id
           WHERE o.poll_id = ?"#,
        poll_id
    )
    .fetch_all(db)
    .await
}

/// Records a vote, replacing an earlier one. Returns false if the option isn't part of this poll,
/// the poll isn't open, its deadline has passed or the option's date has passed (so a crafted
/// request can't vote on another poll's option either).
pub async fn set_vote(
    db: &SqlitePool,
    poll_id: i64,
    option_id: i64,
    user_id: i64,
    answer: Answer,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "INSERT INTO schedule_votes (option_id, user_id, answer)
         SELECT o.id, ?, ? FROM schedule_options AS o JOIN schedule_polls AS p ON p.id = o.poll_id
         WHERE o.id = ? AND p.id = ? AND p.status = 'open'
           AND (p.deadline_at IS NULL OR p.deadline_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
           AND o.starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
         ON CONFLICT (option_id, user_id) DO UPDATE
         SET answer = excluded.answer, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        user_id,
        answer,
        option_id,
        poll_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Saves an edited open poll. Options are matched by their time: unchanged ones keep their votes,
/// removed ones are deleted together with their votes, new ones are added. Returns false if the
/// poll isn't open (any more).
pub async fn update(db: &SqlitePool, poll_id: i64, poll: &NewPoll) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    let updated = sqlx::query!(
        "UPDATE schedule_polls SET title = ?, description_md = ?, deadline_at = ?
         WHERE id = ? AND status = 'open'",
        poll.title,
        poll.description_md,
        poll.deadline_at,
        poll_id
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    let existing = sqlx::query_as!(
        OptionRow,
        "SELECT id, starts_at FROM schedule_options WHERE poll_id = ?",
        poll_id
    )
    .fetch_all(&mut *tx)
    .await?;
    for option in &existing {
        if !poll.options.contains(&option.starts_at) {
            sqlx::query!("DELETE FROM schedule_votes WHERE option_id = ?", option.id)
                .execute(&mut *tx)
                .await?;
            sqlx::query!("DELETE FROM schedule_options WHERE id = ?", option.id)
                .execute(&mut *tx)
                .await?;
        }
    }
    for starts_at in &poll.options {
        if !existing.iter().any(|o| &o.starts_at == starts_at) {
            sqlx::query!(
                "INSERT INTO schedule_options (poll_id, starts_at) VALUES (?, ?)",
                poll_id,
                starts_at
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(true)
}

/// How many answers each option of a poll has, for the edit form's warning.
pub async fn vote_counts(db: &SqlitePool, poll_id: i64) -> Result<Vec<(String, i64)>, sqlx::Error> {
    let rows = sqlx::query!(
        r#"SELECT o.starts_at, COUNT(v.user_id) AS "count!: i64"
           FROM schedule_options AS o LEFT JOIN schedule_votes AS v ON v.option_id = o.id
           WHERE o.poll_id = ? GROUP BY o.id"#,
        poll_id
    )
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|r| (r.starts_at, r.count)).collect())
}

/// Decides the poll (R3) in one transaction: creates a game session of the campaign at the
/// option's time (numbered by date), pre-fills attendance with the members who voted yes, and marks the poll
/// decided. Returns the new game session id, or `None` if the poll isn't open or the option
/// isn't one of its options.
pub async fn decide(
    db: &SqlitePool,
    poll_id: i64,
    option_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    let mut tx = db.begin().await?;
    // The first statement writes (takes SQLite's write lock at once) and checks everything.
    let Some(session) = sqlx::query!(
        r#"INSERT INTO game_sessions (campaign_id, number, starts_at)
           SELECT p.campaign_id,
                  (SELECT COALESCE(MAX(number), 0) + 1 FROM game_sessions WHERE campaign_id = p.campaign_id),
                  o.starts_at
           FROM schedule_polls AS p JOIN schedule_options AS o ON o.poll_id = p.id
           WHERE p.id = ? AND o.id = ? AND p.status = 'open'
           RETURNING id AS "id!", campaign_id"#,
        poll_id,
        option_id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    sqlx::query!(
        "INSERT INTO game_session_attendance (game_session_id, user_id)
         SELECT ?, v.user_id FROM schedule_votes AS v
         JOIN campaign_members AS m ON m.user_id = v.user_id AND m.campaign_id = ?
         WHERE v.option_id = ? AND v.answer = 'yes'",
        session.id,
        session.campaign_id,
        option_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE schedule_polls SET status = 'decided', decided_option_id = ?, game_session_id = ? WHERE id = ?",
        option_id,
        session.id,
        poll_id
    )
    .execute(&mut *tx)
    .await?;
    crate::game_sessions::db::renumber(&mut tx, session.campaign_id).await?;
    tx.commit().await?;
    Ok(Some(session.id))
}

/// Returns false if the poll wasn't open.
pub async fn cancel(db: &SqlitePool, poll_id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE schedule_polls SET status = 'cancelled' WHERE id = ? AND status = 'open'",
        poll_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// The campaign's polls: open ones first, then the 10 most recent closed ones.
pub async fn for_campaign(
    db: &SqlitePool,
    campaign_id: i64,
    user_id: i64,
) -> Result<Vec<PollSummary>, sqlx::Error> {
    sqlx::query_as!(
        PollSummary,
        r#"SELECT p.id AS "id!", p.campaign_id, c.name AS campaign_name, p.title,
                  p.status AS "status: PollStatus",
                  (p.status = 'open' AND (p.deadline_at IS NULL OR p.deadline_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')))
                    AS "voting_open!: bool",
                  (SELECT COUNT(*) FROM schedule_options
                   WHERE poll_id = p.id AND starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) AS "option_count!: i64",
                  (SELECT COUNT(*) FROM schedule_votes AS v JOIN schedule_options AS o ON o.id = v.option_id
                   WHERE o.poll_id = p.id AND v.user_id = ?
                     AND o.starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) AS "my_vote_count!: i64"
           FROM schedule_polls AS p JOIN campaigns AS c ON c.id = p.campaign_id
           WHERE p.campaign_id = ?
           ORDER BY p.status = 'open' DESC, p.created_at DESC
           LIMIT 10 + (SELECT COUNT(*) FROM schedule_polls WHERE campaign_id = ? AND status = 'open')"#,
        user_id,
        campaign_id,
        campaign_id
    )
    .fetch_all(db)
    .await
}

/// Open polls in all of the user's (non-archived) campaigns, for the dashboard.
pub async fn open_for_user(db: &SqlitePool, user_id: i64) -> Result<Vec<PollSummary>, sqlx::Error> {
    sqlx::query_as!(
        PollSummary,
        r#"SELECT p.id AS "id!", p.campaign_id, c.name AS campaign_name, p.title,
                  p.status AS "status: PollStatus",
                  (p.status = 'open' AND (p.deadline_at IS NULL OR p.deadline_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')))
                    AS "voting_open!: bool",
                  (SELECT COUNT(*) FROM schedule_options
                   WHERE poll_id = p.id AND starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) AS "option_count!: i64",
                  (SELECT COUNT(*) FROM schedule_votes AS v JOIN schedule_options AS o ON o.id = v.option_id
                   WHERE o.poll_id = p.id AND v.user_id = ?
                     AND o.starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) AS "my_vote_count!: i64"
           FROM schedule_polls AS p
           JOIN campaigns AS c ON c.id = p.campaign_id
           JOIN campaign_members AS m ON m.campaign_id = p.campaign_id AND m.user_id = ?
           WHERE p.status = 'open' AND c.archived_at IS NULL
           ORDER BY p.created_at"#,
        user_id,
        user_id
    )
    .fetch_all(db)
    .await
}

/// The number the next game session of the campaign will get (for the default poll title).
pub async fn next_session_number(db: &SqlitePool, campaign_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COALESCE(MAX(number), 0) + 1 AS "n!: i64" FROM game_sessions WHERE campaign_id = ?"#,
        campaign_id
    )
    .fetch_one(db)
    .await
}

pub async fn patterns(db: &SqlitePool, campaign_id: i64) -> Result<Vec<PatternRow>, sqlx::Error> {
    sqlx::query_as!(
        PatternRow,
        "SELECT id, weekdays, start_time, every_n_weeks, anchor_date FROM schedule_patterns
         WHERE campaign_id = ? ORDER BY created_at",
        campaign_id
    )
    .fetch_all(db)
    .await
}

pub async fn pattern(
    db: &SqlitePool,
    campaign_id: i64,
    pattern_id: i64,
) -> Result<Option<PatternRow>, sqlx::Error> {
    sqlx::query_as!(
        PatternRow,
        "SELECT id, weekdays, start_time, every_n_weeks, anchor_date FROM schedule_patterns
         WHERE campaign_id = ? AND id = ?",
        campaign_id,
        pattern_id
    )
    .fetch_optional(db)
    .await
}

pub async fn create_pattern(
    db: &SqlitePool,
    campaign_id: i64,
    user_id: i64,
    p: &NewPattern,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO schedule_patterns (campaign_id, weekdays, start_time, every_n_weeks, anchor_date, created_by)
         VALUES (?, ?, ?, ?, ?, ?)",
        campaign_id,
        p.weekdays,
        p.start_time,
        p.every_n_weeks,
        p.anchor_date,
        user_id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete_pattern(
    db: &SqlitePool,
    campaign_id: i64,
    pattern_id: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM schedule_patterns WHERE campaign_id = ? AND id = ?",
        campaign_id,
        pattern_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}
