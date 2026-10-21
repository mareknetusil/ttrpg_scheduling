use super::model::{InviteInfo, JoinOutcome};
use sqlx::{Sqlite, Transaction, sqlite::SqlitePool};

pub async fn find(db: &SqlitePool, token: &str) -> Result<Option<InviteInfo>, sqlx::Error> {
    sqlx::query_as!(
        InviteInfo,
        "SELECT c.id AS campaign_id, c.name AS campaign_name, u.display_name AS gm_name FROM campaigns AS c JOIN users AS u ON c.gm_user_id = u.id JOIN campaign_invites AS i ON c.id = i.campaign_id WHERE token = ? AND i.use_count < i.max_uses AND i.expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        token
    ).fetch_optional(db).await
}

/// Validates the invite and counts one use, as the transaction's first statement (takes the write
/// lock at once). Returns the campaign id, or `None` if the invite is invalid, expired or used up.
async fn consume(
    tx: &mut Transaction<'_, Sqlite>,
    token: &str,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar!(
        "UPDATE campaign_invites SET use_count = use_count + 1
         WHERE token = ? AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now') AND use_count < max_uses
         RETURNING campaign_id",
        token
    )
    .fetch_optional(&mut **tx)
    .await
}

async fn add_member(
    tx: &mut Transaction<'_, Sqlite>,
    campaign_id: i64,
    user_id: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO campaign_members (campaign_id, user_id) VALUES (?, ?)",
        campaign_id,
        user_id
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn join(db: &SqlitePool, token: &str, user_id: i64) -> Result<JoinOutcome, sqlx::Error> {
    let mut tx = db.begin().await?;
    let Some(campaign_id) = consume(&mut tx, token).await? else {
        return Ok(JoinOutcome::InvalidInvite);
    };
    let already_member = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM campaign_members WHERE campaign_id = ? AND user_id = ?) AS "is_member: bool""#,
        campaign_id,
        user_id
    )
    .fetch_one(&mut *tx)
    .await?;
    if already_member {
        // Dropping `tx` rolls back, so the use isn't counted.
        return Ok(JoinOutcome::AlreadyMember(campaign_id));
    }
    add_member(&mut tx, campaign_id, user_id).await?;
    tx.commit().await?;
    Ok(JoinOutcome::Joined(campaign_id))
}

/// Creates the user and joins the campaign in one transaction. Returns `(user_id, campaign_id)`,
/// or `None` if the invite is invalid. A taken username comes back as a unique-violation error.
pub async fn register(
    db: &SqlitePool,
    token: &str,
    username: &str,
    display_name: &str,
    password_hash: &str,
) -> Result<Option<(i64, i64)>, sqlx::Error> {
    let mut tx = db.begin().await?;
    let Some(campaign_id) = consume(&mut tx, token).await? else {
        return Ok(None);
    };
    let user_id = sqlx::query!(
        "INSERT INTO users (username, display_name, password_hash) VALUES (?, ?, ?)",
        username,
        display_name,
        password_hash
    )
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    add_member(&mut tx, campaign_id, user_id).await?;
    tx.commit().await?;
    Ok(Some((user_id, campaign_id)))
}
