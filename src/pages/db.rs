use super::model::{Page, PageForm, PageSummary};
use crate::markdown::slugify;
use crate::notifications::{
    db::notify,
    model::{Kind, Notice, Target},
};
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

/// Inserts a page, tells the other members, and returns its slug. The slug comes from the title
/// and never changes afterwards, so links to a page survive renaming it. Duplicates get `-2`,
/// `-3`, ….
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
    let mut tx = db.begin().await?;
    let page_id = sqlx::query!(
        "INSERT INTO campaign_pages (campaign_id, title, slug, body_md, created_by) VALUES (?, ?, ?, ?, ?)",
        campaign_id,
        form.title,
        slug,
        form.body_md,
        user_id
    )
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    notify(
        &mut tx,
        &page_notice(campaign_id, page_id, user_id, Kind::PageAdded),
    )
    .await?;
    tx.commit().await?;
    Ok(slug)
}

/// Saves an edited page of `campaign_id` and tells the other members.
pub async fn update(
    db: &SqlitePool,
    campaign_id: i64,
    page_id: i64,
    form: &PageForm,
    actor_id: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!(
        "UPDATE campaign_pages SET title = ?, body_md = ?, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE id = ?",
        form.title,
        form.body_md,
        page_id
    )
    .execute(&mut *tx)
    .await?;
    notify(
        &mut tx,
        &page_notice(campaign_id, page_id, actor_id, Kind::PageEdited),
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

fn page_notice(campaign_id: i64, page_id: i64, actor_id: i64, kind: Kind) -> Notice {
    Notice {
        campaign_id,
        actor_id,
        kind,
        target: Target {
            page_id: Some(page_id),
            ..Target::default()
        },
    }
}

/// Deletes a page and the notifications about it.
pub async fn delete(db: &SqlitePool, page_id: i64) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query!("DELETE FROM notifications WHERE page_id = ?", page_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query!("DELETE FROM campaign_pages WHERE id = ?", page_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
