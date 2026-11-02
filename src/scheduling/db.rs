use super::model::{Answer, Layout, NewPoll, OptionRow, Poll, PollStatus, PollSummary, VoteRow};
use crate::game_sessions::{self, model::SessionStatus};
use crate::notifications::{
    db::notify,
    model::{Kind, Notice, Target},
};
use crate::time;
use chrono_tz::Tz;
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;

pub async fn create(
    db: &SqlitePool,
    campaign_id: i64,
    user_id: i64,
    poll: &NewPoll,
) -> Result<i64, sqlx::Error> {
    let mut tx = db.begin().await?;
    let poll_id = sqlx::query!(
        "INSERT INTO schedule_polls (campaign_id, title, description_md, layout, deadline_at, created_by)
         VALUES (?, ?, ?, ?, ?, ?)",
        campaign_id,
        poll.title,
        poll.description_md,
        poll.layout,
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
    notify(
        &mut tx,
        &Notice {
            campaign_id,
            actor_id: user_id,
            kind: Kind::SchedulePollOpened,
            target: Target {
                schedule_poll_id: Some(poll_id),
                ..Target::default()
            },
        },
    )
    .await?;
    tx.commit().await?;
    Ok(poll_id)
}

pub async fn get(db: &SqlitePool, poll_id: i64) -> Result<Option<Poll>, sqlx::Error> {
    sqlx::query_as!(
        Poll,
        r#"SELECT id, campaign_id, title, description_md, layout AS "layout: Layout",
                  status AS "status: PollStatus",
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

/// Saves an edited open poll. Existing options are matched to the new ones: by their exact time
/// in a list poll, by their day (in `tz`) in a calendar poll, so a day whose time moved keeps its
/// answers. Matched ones keep their votes (and get the new time), unmatched old ones are deleted
/// together with their votes, unmatched new ones are added. Returns false if the poll isn't open
/// (any more).
pub async fn update(
    db: &SqlitePool,
    poll_id: i64,
    poll: &NewPoll,
    tz: Tz,
) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    let updated = sqlx::query!(
        "UPDATE schedule_polls SET title = ?, description_md = ?, layout = ?, deadline_at = ?
         WHERE id = ? AND status = 'open'",
        poll.title,
        poll.description_md,
        poll.layout,
        poll.deadline_at,
        poll_id
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Ok(false);
    }
    let match_key = |starts_at: &str| match poll.layout {
        Layout::List => starts_at.to_string(),
        Layout::Calendar => match time::from_db(starts_at) {
            Some(t) => t.with_timezone(&tz).date_naive().to_string(),
            None => starts_at.to_string(),
        },
    };
    // key -> the new time; a key is removed once an existing option took it
    let mut unmatched: HashMap<String, &String> =
        poll.options.iter().map(|o| (match_key(o), o)).collect();
    let existing = sqlx::query_as!(
        OptionRow,
        "SELECT id, starts_at FROM schedule_options WHERE poll_id = ? ORDER BY starts_at",
        poll_id
    )
    .fetch_all(&mut *tx)
    .await?;
    let mut moved = Vec::new();
    for option in &existing {
        match unmatched.remove(&match_key(&option.starts_at)) {
            Some(starts_at) if *starts_at != option.starts_at => moved.push((option.id, starts_at)),
            Some(_) => {}
            None => {
                sqlx::query!("DELETE FROM schedule_votes WHERE option_id = ?", option.id)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query!("DELETE FROM schedule_options WHERE id = ?", option.id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
    }
    // After the deletes, so a moved time can't collide with a removed option's (UNIQUE)
    for (option_id, starts_at) in moved {
        sqlx::query!(
            "UPDATE schedule_options SET starts_at = ? WHERE id = ?",
            starts_at,
            option_id
        )
        .execute(&mut *tx)
        .await?;
    }
    for starts_at in unmatched.into_values() {
        sqlx::query!(
            "INSERT INTO schedule_options (poll_id, starts_at) VALUES (?, ?)",
            poll_id,
            starts_at
        )
        .execute(&mut *tx)
        .await?;
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

/// Answers every future option the user hasn't answered yet ("answer the rest"). Like `set_vote`,
/// only while voting is open; returns false if it isn't.
pub async fn set_unanswered(
    db: &SqlitePool,
    poll_id: i64,
    user_id: i64,
    answer: Answer,
) -> Result<bool, sqlx::Error> {
    let open = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "n!: i64" FROM schedule_polls
           WHERE id = ? AND status = 'open'
             AND (deadline_at IS NULL OR deadline_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))"#,
        poll_id
    )
    .fetch_one(db)
    .await?;
    if open == 0 {
        return Ok(false);
    }
    sqlx::query!(
        "INSERT INTO schedule_votes (option_id, user_id, answer)
         SELECT o.id, ?, ? FROM schedule_options AS o
         WHERE o.poll_id = ? AND o.starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
           AND NOT EXISTS (SELECT 1 FROM schedule_votes AS v WHERE v.option_id = o.id AND v.user_id = ?)",
        user_id,
        answer,
        poll_id,
        user_id
    )
    .execute(db)
    .await?;
    Ok(true)
}

/// The brush of a calendar poll: gives `answer` to each option in `option_ids` (one day, or every
/// day of a weekday), or, if they all have that answer already, clears them, so tapping a
/// painted day again undoes it. Only this poll's future options count. Like `set_vote`, only
/// while voting is open; returns false (and changes nothing) if it isn't, or if none of the
/// options can be voted on.
pub async fn paint(
    db: &SqlitePool,
    poll_id: i64,
    user_id: i64,
    option_ids: &[i64],
    answer: Answer,
) -> Result<bool, sqlx::Error> {
    let open = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "n!: i64" FROM schedule_polls
           WHERE id = ? AND status = 'open'
             AND (deadline_at IS NULL OR deadline_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))"#,
        poll_id
    )
    .fetch_one(db)
    .await?;
    if open == 0 {
        return Ok(false);
    }
    let mut targets = Vec::new();
    let mut all_have_it = true;
    for option_id in option_ids {
        let Some(row) = sqlx::query!(
            r#"SELECT v.answer AS "answer?: Answer"
               FROM schedule_options AS o
               LEFT JOIN schedule_votes AS v ON v.option_id = o.id AND v.user_id = ?
               WHERE o.id = ? AND o.poll_id = ?
                 AND o.starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')"#,
            user_id,
            option_id,
            poll_id
        )
        .fetch_optional(db)
        .await?
        else {
            continue;
        };
        all_have_it = all_have_it && row.answer == Some(answer);
        targets.push(*option_id);
    }
    if targets.is_empty() {
        return Ok(false);
    }
    let mut tx = db.begin().await?;
    for option_id in &targets {
        if all_have_it {
            sqlx::query!(
                "DELETE FROM schedule_votes WHERE option_id = ? AND user_id = ?",
                option_id,
                user_id
            )
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query!(
                "INSERT INTO schedule_votes (option_id, user_id, answer) VALUES (?, ?, ?)
                 ON CONFLICT (option_id, user_id) DO UPDATE
                 SET answer = excluded.answer, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
                option_id,
                user_id,
                answer
            )
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(true)
}

/// Removes the user's answer to an option (back to "no answer"), under the same conditions as
/// `set_vote`. Returns false if the option can't be voted on (any more).
pub async fn clear_vote(
    db: &SqlitePool,
    poll_id: i64,
    option_id: i64,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    let votable = sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "n!: i64"
           FROM schedule_options AS o JOIN schedule_polls AS p ON p.id = o.poll_id
           WHERE o.id = ? AND p.id = ? AND p.status = 'open'
             AND (p.deadline_at IS NULL OR p.deadline_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
             AND o.starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')"#,
        option_id,
        poll_id
    )
    .fetch_one(db)
    .await?;
    if votable == 0 {
        return Ok(false);
    }
    sqlx::query!(
        "DELETE FROM schedule_votes WHERE option_id = ? AND user_id = ?",
        option_id,
        user_id
    )
    .execute(db)
    .await?;
    Ok(true)
}

/// How many open polls the campaign has (each is probably for a different next session).
pub async fn open_poll_count(db: &SqlitePool, campaign_id: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT COUNT(*) AS "n!: i64" FROM schedule_polls WHERE campaign_id = ? AND status = 'open'"#,
        campaign_id
    )
    .fetch_one(db)
    .await
}

/// Decides the poll (R3) in one transaction: creates a game session of the campaign at the
/// option's time (numbered by date), pre-fills attendance with the members who voted yes, and
/// marks the poll decided, and tells the members (`actor_id` decided). Returns the new game
/// session id, or `None` if the poll isn't open, the option isn't one of its options, or its
/// date has passed.
pub async fn decide(
    db: &SqlitePool,
    poll_id: i64,
    option_id: i64,
    actor_id: i64,
) -> Result<Option<i64>, sqlx::Error> {
    // IMMEDIATE takes SQLite's write lock at once, so nothing changes between the checks below
    // and the writes (e.g. a double click deciding twice).
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let Some(option) = sqlx::query!(
        "SELECT p.campaign_id, o.starts_at
         FROM schedule_polls AS p JOIN schedule_options AS o ON o.poll_id = p.id
         WHERE p.id = ? AND o.id = ? AND p.status = 'open'
           AND o.starts_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        poll_id,
        option_id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(None);
    };
    let yes_voters = sqlx::query_scalar!(
        "SELECT user_id FROM schedule_votes WHERE option_id = ? AND answer = 'yes'",
        option_id
    )
    .fetch_all(&mut *tx)
    .await?;
    let session_id = game_sessions::db::insert(
        &mut tx,
        option.campaign_id,
        "",
        &option.starts_at,
        SessionStatus::Planned,
        &yes_voters,
    )
    .await?;
    sqlx::query!(
        "UPDATE schedule_polls SET status = 'decided', decided_option_id = ?, game_session_id = ? WHERE id = ?",
        option_id,
        session_id,
        poll_id
    )
    .execute(&mut *tx)
    .await?;
    notify(
        &mut tx,
        &Notice {
            campaign_id: option.campaign_id,
            actor_id,
            kind: Kind::SchedulePollDecided,
            target: Target {
                schedule_poll_id: Some(poll_id),
                ..Target::default()
            },
        },
    )
    .await?;
    tx.commit().await?;
    Ok(Some(session_id))
}

/// Undoes a decision (a wrong click, or the date fell through): the poll is open again, with its
/// votes, and the game session the decision created is deleted, if it's still untouched
/// (`game_sessions::db::delete_if_untouched`). Returns false (and changes nothing) if the poll
/// isn't decided or the session isn't untouched. Tells the members (`actor_id` reopened it).
pub async fn reopen(db: &SqlitePool, poll_id: i64, actor_id: i64) -> Result<bool, sqlx::Error> {
    // IMMEDIATE: the write lock at once, so the poll can't change between check and writes.
    let mut tx = db.begin_with("BEGIN IMMEDIATE").await?;
    let Some(row) = sqlx::query!(
        r#"SELECT game_session_id AS "session_id!", campaign_id FROM schedule_polls
           WHERE id = ? AND status = 'decided'"#,
        poll_id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(false);
    };
    // The poll lets go of the session first (it holds a foreign key to it).
    sqlx::query!(
        "UPDATE schedule_polls SET status = 'open', decided_option_id = NULL, game_session_id = NULL
         WHERE id = ?",
        poll_id
    )
    .execute(&mut *tx)
    .await?;
    if !game_sessions::db::delete_if_untouched(&mut tx, row.session_id).await? {
        // dropping `tx` rolls back the poll update
        return Ok(false);
    }
    notify(
        &mut tx,
        &Notice {
            campaign_id: row.campaign_id,
            actor_id,
            kind: Kind::SchedulePollReopened,
            target: Target {
                schedule_poll_id: Some(poll_id),
                ..Target::default()
            },
        },
    )
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Cancels an open poll and tells the members (`actor_id` cancelled it). Returns false if the
/// poll wasn't open.
pub async fn cancel(db: &SqlitePool, poll_id: i64, actor_id: i64) -> Result<bool, sqlx::Error> {
    let mut tx = db.begin().await?;
    let Some(campaign_id) = sqlx::query_scalar!(
        "UPDATE schedule_polls SET status = 'cancelled' WHERE id = ? AND status = 'open'
         RETURNING campaign_id",
        poll_id
    )
    .fetch_optional(&mut *tx)
    .await?
    else {
        return Ok(false);
    };
    notify(
        &mut tx,
        &Notice {
            campaign_id,
            actor_id,
            kind: Kind::SchedulePollCancelled,
            target: Target {
                schedule_poll_id: Some(poll_id),
                ..Target::default()
            },
        },
    )
    .await?;
    tx.commit().await?;
    Ok(true)
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

/// The start times of the options of the campaign's most recent scheduling poll, in date order:
/// the new-poll form starts with that poll's usual time.
pub async fn latest_poll_starts(
    db: &SqlitePool,
    campaign_id: i64,
) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar!(
        "SELECT starts_at FROM schedule_options WHERE poll_id =
           (SELECT id FROM schedule_polls WHERE campaign_id = ? ORDER BY created_at DESC, id DESC LIMIT 1)
         ORDER BY starts_at",
        campaign_id
    )
    .fetch_all(db)
    .await
}
