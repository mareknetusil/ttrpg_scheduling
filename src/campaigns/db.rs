use super::model::{Banner, Campaign, CampaignForm, CampaignSummary, Invite, Member};
use anyhow::Context;
use sqlx::sqlite::SqlitePool;

pub async fn create(db: &SqlitePool, form: &CampaignForm, user_id: i64) -> anyhow::Result<i64> {
    let mut tx = db.begin().await?;
    let new_id = sqlx::query!(
        "INSERT INTO campaigns (name, system, tagline, description_md, accent, gm_user_id, created_by) VALUES (?, ?, ?, ?, ?, ?, ?)",
        form.name,
        form.system,
        form.tagline,
        form.description_md,
        form.accent,
        user_id,
        user_id
    )
    .execute(&mut *tx)
    .await
    .context("failed to create campaign")?
    .last_insert_rowid();
    sqlx::query!(
        "INSERT INTO campaign_members (campaign_id, user_id) VALUES (?, ?)",
        new_id,
        user_id
    )
    .execute(&mut *tx)
    .await
    .context("failed to insert into campaign_members")?;
    tx.commit()
        .await
        .context("failed to commit a new campaign")?;
    Ok(new_id)
}

pub async fn update(db: &SqlitePool, id: i64, form: &CampaignForm) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE campaigns SET name = ?, system = ?, tagline = ?, description_md = ?, accent = ? WHERE id = ?",
        form.name,
        form.system,
        form.tagline,
        form.description_md,
        form.accent,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn for_user(db: &SqlitePool, user_id: i64) -> Result<Vec<CampaignSummary>, sqlx::Error> {
    sqlx::query_as!(
        CampaignSummary,
        r#"SELECT c.id, c.name, c.system, c.tagline, c.gm_user_id = m.user_id AS "is_gm: bool",
                  c.accent, b.updated_at AS "banner_version?"
           FROM campaign_members AS m
           JOIN campaigns AS c ON m.campaign_id = c.id
           LEFT JOIN campaign_banners AS b ON b.campaign_id = c.id
           WHERE m.user_id = ? AND c.archived_at IS NULL ORDER BY c.name"#,
        user_id
    )
    .fetch_all(db)
    .await
}

pub async fn get(db: &SqlitePool, id: i64) -> Result<Option<Campaign>, sqlx::Error> {
    sqlx::query_as!(
        Campaign,
        r#"SELECT c.id, c.name, c.system, c.tagline, c.description_md, c.gm_user_id,
                  u.display_name as gm_name, c.accent, b.updated_at AS "banner_version?"
           FROM campaigns AS c
           JOIN users AS u ON c.gm_user_id = u.id
           LEFT JOIN campaign_banners AS b ON b.campaign_id = c.id
           WHERE c.id = ?"#,
        id
    )
    .fetch_optional(db)
    .await
}

pub async fn is_member(
    db: &SqlitePool,
    campaign_id: i64,
    user_id: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM campaign_members WHERE campaign_id = ? AND user_id = ?) AS "is_member: bool""#,
        campaign_id,
        user_id
    ).fetch_one(db).await
}

pub async fn members(db: &SqlitePool, campaign_id: i64) -> Result<Vec<Member>, sqlx::Error> {
    sqlx::query_as!(
        Member,
        r#"SELECT u.id AS user_id, u.username, u.display_name, m.joined_at,
                  u.deactivated_at IS NOT NULL AS "deactivated: bool"
           FROM users AS u JOIN campaign_members as m ON m.user_id = u.id
           WHERE m.campaign_id = ? ORDER BY u.display_name"#,
        campaign_id
    )
    .fetch_all(db)
    .await
}

pub async fn invites(db: &SqlitePool, campaign_id: i64) -> Result<Vec<Invite>, sqlx::Error> {
    sqlx::query_as!(
        Invite,
        "SELECT id, token, expires_at, max_uses, use_count FROM campaign_invites WHERE campaign_id = ? AND expires_at > strftime('%Y-%m-%dT%H:%M:%SZ', 'now') AND use_count < max_uses ORDER BY created_at",
        campaign_id
    ).fetch_all(db).await
}

pub async fn create_invite(
    db: &SqlitePool,
    campaign_id: i64,
    user_id: i64,
    max_uses: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM campaign_invites WHERE expires_at <= strftime('%Y-%m-%dT%H:%M:%SZ', 'now') OR use_count >= max_uses"
    ).execute(db).await?;
    let token = crate::auth::token::generate();
    sqlx::query!(
        "INSERT INTO campaign_invites (campaign_id, token, created_by, max_uses) VALUES (?, ?, ?, ?) ",
        campaign_id,
        token,
        user_id,
        max_uses
    ).execute(db).await?;
    Ok(())
}

pub async fn delete_invite(
    db: &SqlitePool,
    campaign_id: i64,
    invite_id: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM campaign_invites WHERE id = ? AND campaign_id = ?",
        invite_id,
        campaign_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

pub async fn remove_member(
    db: &SqlitePool,
    campaign_id: i64,
    user_id: i64,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM campaign_members WHERE campaign_id = ? AND user_id = ?",
        campaign_id,
        user_id
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

pub async fn set_gm(db: &SqlitePool, campaign_id: i64, user_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE campaigns SET gm_user_id = ? WHERE id = ?",
        user_id,
        campaign_id
    )
    .execute(db)
    .await?;
    Ok(())
}

/// A site admin takes the GM role: joins the campaign if needed, then becomes its GM. The old GM
/// stays a member. One transaction, so "the GM is a member" holds at commit.
pub async fn take_gm(db: &SqlitePool, campaign_id: i64, user_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!(
        "INSERT OR IGNORE INTO campaign_members (campaign_id, user_id) VALUES (?, ?)",
        campaign_id,
        user_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "UPDATE campaigns SET gm_user_id = ? WHERE id = ?",
        user_id,
        campaign_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Deletes a campaign and everything in it, in one transaction. The schema has no cascades (on
/// purpose: nothing disappears by accident), so this lists every table. Foreign keys are checked
/// at commit (`defer_foreign_keys`), because polls and their options point at each other.
pub async fn delete(db: &SqlitePool, campaign_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!("PRAGMA defer_foreign_keys = ON")
        .execute(&mut *tx)
        .await?;
    let statements = [
        "DELETE FROM poll_votes WHERE poll_option_id IN
           (SELECT o.id FROM poll_options AS o JOIN polls AS p ON p.id = o.poll_id WHERE p.campaign_id = ?)",
        "DELETE FROM poll_options WHERE poll_id IN (SELECT id FROM polls WHERE campaign_id = ?)",
        "DELETE FROM polls WHERE campaign_id = ?",
        "DELETE FROM comments WHERE campaign_id = ?",
        "DELETE FROM schedule_votes WHERE option_id IN
           (SELECT o.id FROM schedule_options AS o JOIN schedule_polls AS p ON p.id = o.poll_id WHERE p.campaign_id = ?)",
        "DELETE FROM schedule_options WHERE poll_id IN (SELECT id FROM schedule_polls WHERE campaign_id = ?)",
        "DELETE FROM schedule_polls WHERE campaign_id = ?",
        "DELETE FROM schedule_patterns WHERE campaign_id = ?",
        "DELETE FROM game_session_entries WHERE game_session_id IN (SELECT id FROM game_sessions WHERE campaign_id = ?)",
        "DELETE FROM game_session_attendance WHERE game_session_id IN (SELECT id FROM game_sessions WHERE campaign_id = ?)",
        "DELETE FROM game_sessions WHERE campaign_id = ?",
        "DELETE FROM campaign_pages WHERE campaign_id = ?",
        "DELETE FROM campaign_invites WHERE campaign_id = ?",
        "DELETE FROM campaign_banners WHERE campaign_id = ?",
        "DELETE FROM campaign_members WHERE campaign_id = ?",
        "DELETE FROM campaigns WHERE id = ?",
    ];
    for sql in statements {
        sqlx::query(sql).bind(campaign_id).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn banner(db: &SqlitePool, campaign_id: i64) -> Result<Option<Banner>, sqlx::Error> {
    sqlx::query_as!(
        Banner,
        "SELECT content_type, data FROM campaign_banners WHERE campaign_id = ?",
        campaign_id
    )
    .fetch_optional(db)
    .await
}

pub async fn set_banner(
    db: &SqlitePool,
    campaign_id: i64,
    content_type: &str,
    data: &[u8],
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO campaign_banners (campaign_id, content_type, data) VALUES (?, ?, ?)
         ON CONFLICT (campaign_id) DO UPDATE SET content_type = excluded.content_type,
             data = excluded.data, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')",
        campaign_id,
        content_type,
        data
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete_banner(db: &SqlitePool, campaign_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "DELETE FROM campaign_banners WHERE campaign_id = ?",
        campaign_id
    )
    .execute(db)
    .await?;
    Ok(())
}
