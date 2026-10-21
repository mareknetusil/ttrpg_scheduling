use crate::time::When;
use chrono_tz::Tz;

pub struct Comment {
    pub id: i64,
    pub campaign_id: i64,
    pub author_id: i64,
    pub author_name: String,
    pub body_md: String,
    pub created_at: String,
    pub edited_at: Option<String>,
}

#[derive(serde::Serialize)]
pub struct CommentView {
    pub id: i64,
    pub author_name: String,
    pub body_md: String,
    pub body_html: String,
    pub created: When,
    pub edited: bool,
    pub can_edit: bool,
    pub can_delete: bool,
}

impl CommentView {
    pub fn new(c: &Comment, can_edit: bool, can_delete: bool, tz: Tz) -> CommentView {
        CommentView {
            id: c.id,
            author_name: c.author_name.clone(),
            body_md: c.body_md.clone(),
            body_html: crate::markdown::render(&c.body_md),
            created: When::from_db(&c.created_at, tz),
            edited: c.edited_at.is_some(),
            can_edit,
            can_delete,
        }
    }
}

#[derive(serde::Deserialize)]
pub struct CommentForm {
    pub body_md: String,
}

pub const MAX_LEN: usize = 10_000;
