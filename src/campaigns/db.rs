use super::model::{Campaign, CampaignForm, CampaignSummary, Invite, Member};
use anyhow::Context;
use sqlx::sqlite::SqlitePool;

pub async fn create(db: &SqlitePool, form: &CampaignForm, user_id: i64) -> anyhow::Result<i64> {
    let mut tx = db.begin().await?;
    let new_id = sqlx::query!(
        "INSERT INTO campaigns (name, system, tagline, description_md, gm_user_id, created_by) VALUES (?, ?, ?, ?, ?, ?)",
        form.name,
        form.system,
        form.tagline,
        form.description_md,
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
        "UPDATE campaigns SET name = ?, system = ?, tagline = ?, description_md = ? WHERE id = ?",
        form.name,
        form.system,
        form.tagline,
        form.description_md,
        id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn for_user(db: &SqlitePool, user_id: i64) -> Result<Vec<CampaignSummary>, sqlx::Error> {
    sqlx::query_as!(
        CampaignSummary,
        r#"SELECT c.id, c.name, c.system, c.tagline, c.gm_user_id = m.user_id AS "is_gm: bool" FROM campaign_members AS m JOIN campaigns AS c ON m.campaign_id = c.id WHERE m.user_id = ? AND c.archived_at IS NULL ORDER BY c.name"#,
        user_id
    ).fetch_all(db)
    .await
}

pub async fn get(db: &SqlitePool, id: i64) -> Result<Option<Campaign>, sqlx::Error> {
    sqlx::query_as!(
        Campaign,
        r#"SELECT c.id, c.name, c.system, c.tagline, c.description_md, c.gm_user_id, u.display_name as gm_name FROM campaigns AS c JOIN users AS u ON c.gm_user_id = u.id WHERE c.id = ?"#,
        id
    ).fetch_optional(db).await
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
        "SELECT u.id AS user_id, u.username, u.display_name, m.joined_at FROM users AS u JOIN campaign_members as m ON m.user_id = u.id WHERE m.campaign_id = ? ORDER BY u.display_name",
        campaign_id
    ).fetch_all(db).await
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
