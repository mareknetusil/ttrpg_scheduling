use super::model::{Page, PageForm, PageSummary};
use crate::markdown::slugify;
use sqlx::sqlite::SqlitePool;

pub async fn list(db: &SqlitePool, campaign_id: i64) -> Result<Vec<PageSummary>, sqlx::Error> {
    sqlx::query_as!(
        PageSummary,
        "SELECT title, slug FROM campaign_pages WHERE campaign_id = ? ORDER BY sort_order, title",
        campaign_id
    )
    .fetch_all(db)
    .await
}

pub async fn get(
    db: &SqlitePool,
    campaign_id: i64,
    slug: &str,
) -> Result<Option<Page>, sqlx::Error> {
    sqlx::query_as!(
        Page,
        "SELECT id, title, slug, body_md, updated_at FROM campaign_pages WHERE campaign_id = ? AND slug = ?",
        campaign_id,
        slug
    )
    .fetch_optional(db)
    .await
}

async fn slug_taken(db: &SqlitePool, campaign_id: i64, slug: &str) -> Result<bool, sqlx::Error> {
    if slug == "new" {
        return Ok(true);
    }
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM campaign_pages WHERE campaign_id = ? AND slug = ?) AS "taken: bool""#,
        campaign_id,
        slug
    )
    .fetch_one(db)
    .await
}

/// Inserts a page and returns its slug. The slug comes from the title and never changes
/// afterwards, so links to a page survive renaming it. Duplicates get `-2`, `-3`, ….
pub async fn create(
    db: &SqlitePool,
    campaign_id: i64,
    user_id: i64,
    form: &PageForm,
) -> Result<String, sqlx::Error> {
    let mut base = slugify(&form.title);
    if base.is_empty() {
        base = "page".to_string();
    }
    let mut slug = base.clone();
    let mut n = 2;
    while slug_taken(db, campaign_id, &slug).await? {
        slug = format!("{base}-{n}");
        n += 1;
    }
    sqlx::query!(
        "INSERT INTO campaign_pages (campaign_id, title, slug, body_md, created_by) VALUES (?, ?, ?, ?, ?)",
        campaign_id,
        form.title,
        slug,
        form.body_md,
        user_id
    )
    .execute(db)
    .await?;
    Ok(slug)
}

pub async fn update(db: &SqlitePool, page_id: i64, form: &PageForm) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "UPDATE campaign_pages SET title = ?, body_md = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
        form.title,
        form.body_md,
        page_id
    )
    .execute(db)
    .await?;
    Ok(())
}

pub async fn delete(db: &SqlitePool, page_id: i64) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM campaign_pages WHERE id = ?", page_id)
        .execute(db)
        .await?;
    Ok(())
}
