use super::model::{CampaignForm, CampaignSummary};
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

pub async fn for_user(db: &SqlitePool, user_id: i64) -> Result<Vec<CampaignSummary>, sqlx::Error> {
    sqlx::query_as!(
        CampaignSummary,
        r#"SELECT c.id, c.name, c.system, c.tagline, c.gm_user_id = m.user_id AS "is_gm: bool" FROM campaign_members AS m JOIN campaigns AS c ON m.campaign_id = c.id WHERE m.user_id = ? AND c.archived_at IS NULL ORDER BY c.name"#,
        user_id
    ).fetch_all(db)
    .await
}
