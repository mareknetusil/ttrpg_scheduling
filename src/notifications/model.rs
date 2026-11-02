use crate::game_sessions::model::EntryKind;
use crate::time::{self, When};
use chrono::{DateTime, Utc};
use chrono_tz::Tz;

/// What happened. Stored as `TEXT` ('schedule_poll_opened', …), see the `CHECK` in the migration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, sqlx::Type, serde::Serialize)]
#[sqlx(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    SchedulePollOpened,
    SchedulePollDecided,
    SchedulePollCancelled,
    SchedulePollReopened,
    PollCreated,
    PollClosed,
    SessionAdded,
    SessionMoved,
    SessionCancelled,
    EntryAdded,
    EntryEdited,
    CommentAdded,
    PageAdded,
    PageEdited,
    CampaignEdited,
    GmChanged,
}

impl Kind {
    /// The kind an edit folds into while unseen: "Mark added a recap" stays that (with the time
    /// of the latest edit) instead of becoming a second notification. Other kinds fold only into
    /// themselves.
    pub fn folds_into(self) -> Kind {
        match self {
            Kind::EntryEdited => Kind::EntryAdded,
            Kind::PageEdited => Kind::PageAdded,
            Kind::SessionMoved => Kind::SessionAdded,
            other => other,
        }
    }
}

/// What a notification points to. Set the ids that apply (a comment on a game session sets
/// both `comment_id` and `game_session_id`), so the page that shows the thing can clear it.
#[derive(Default)]
pub struct Target {
    pub game_session_id: Option<i64>,
    pub schedule_poll_id: Option<i64>,
    pub poll_id: Option<i64>,
    pub entry_id: Option<i64>,
    pub comment_id: Option<i64>,
    pub page_id: Option<i64>,
}

/// One action, to be told to every other member of the campaign (`db::notify`).
pub struct Notice {
    pub campaign_id: i64,
    pub actor_id: i64,
    pub kind: Kind,
    pub target: Target,
}

/// A page that, when opened, marks the notifications about what it shows as seen.
pub enum Viewing {
    /// The scheduling poll and its comments
    SchedulePoll(i64),
    /// The game session, its entries, comments and polls
    GameSession(i64),
    /// The campaign overview: campaign details, campaign comments and campaign polls
    Campaign(i64),
    /// A campaign page (its id)
    CampaignPage(i64),
    /// The members page: GM changes
    Members(i64),
}

/// A notification with the names it needs, from `db::list`. The `?` fields come from LEFT
/// JOINs: `None` when the notification doesn't point to such a thing.
pub struct NotificationRow {
    pub id: i64,
    pub campaign_id: i64,
    pub campaign_name: String,
    pub actor_name: String,
    pub actor_id: i64,
    pub gm_user_id: i64,
    pub gm_name: String,
    pub kind: Kind,
    pub game_session_id: Option<i64>,
    pub session_number: Option<i64>,
    pub session_title: Option<String>,
    pub session_starts_at: Option<String>,
    pub schedule_poll_id: Option<i64>,
    pub schedule_poll_title: Option<String>,
    pub poll_id: Option<i64>,
    pub entry_id: Option<i64>,
    pub comment_id: Option<i64>,
    pub poll_question: Option<String>,
    pub entry_kind: Option<EntryKind>,
    pub comment_body: Option<String>,
    pub page_title: Option<String>,
    pub page_slug: Option<String>,
    pub updated_at: String,
    pub seen_at: Option<String>,
}

/// A notification prepared for templates: "{actor_name} {text}", linking to `url`.
#[derive(serde::Serialize)]
pub struct NotificationView {
    pub id: i64,
    pub campaign_id: i64,
    pub campaign_name: String,
    pub actor_name: String,
    /// What the actor did: "opened the scheduling poll “Session 5”"
    pub text: String,
    /// The start of a comment, for comment notifications
    pub excerpt: Option<String>,
    pub url: String,
    pub when: When,
    /// "3 h ago"
    pub ago: String,
    pub seen: bool,
}

impl NotificationView {
    pub fn new(row: NotificationRow, tz: Tz) -> NotificationView {
        let session = match row.session_number {
            Some(number) => match row.session_title.as_deref() {
                Some(title) if !title.is_empty() => format!("session {number} “{title}”"),
                _ => format!("session {number}"),
            },
            None => "a game session".to_string(),
        };
        let session_date = row
            .session_starts_at
            .as_deref()
            .map(|s| When::from_db(s, tz).full)
            .unwrap_or_default();
        let poll_title = row
            .schedule_poll_title
            .as_deref()
            .unwrap_or("a scheduling poll");
        let question = row.poll_question.as_deref().unwrap_or("a poll");
        let page = row.page_title.as_deref().unwrap_or("a page");
        let (an_entry, the_entry) = match row.entry_kind {
            Some(EntryKind::Recap) => ("a recap", "the recap"),
            Some(EntryKind::Notes) => ("notes", "the notes"),
            Some(EntryKind::Loot) => ("loot", "the loot"),
            Some(EntryKind::Other) | None => ("an entry", "an entry"),
        };
        let text = match row.kind {
            Kind::SchedulePollOpened => format!("opened the scheduling poll “{poll_title}”"),
            Kind::SchedulePollDecided => format!("picked the date in “{poll_title}”"),
            Kind::SchedulePollCancelled => format!("cancelled the scheduling poll “{poll_title}”"),
            Kind::SchedulePollReopened => format!("reopened voting in “{poll_title}”"),
            Kind::PollCreated => format!("asked “{question}”"),
            Kind::PollClosed => format!("closed the poll “{question}”"),
            Kind::SessionAdded => format!("added {session}, {session_date}"),
            Kind::SessionMoved => format!("moved {session} to {session_date}"),
            Kind::SessionCancelled => format!("cancelled {session}"),
            Kind::EntryAdded => format!("added {an_entry} to {session}"),
            Kind::EntryEdited => format!("edited {the_entry} of {session}"),
            Kind::CommentAdded => {
                if row.schedule_poll_id.is_some() {
                    format!("commented on “{poll_title}”")
                } else if row.game_session_id.is_some() {
                    format!("commented on {session}")
                } else {
                    "commented on the campaign".to_string()
                }
            }
            Kind::PageAdded => format!("added the page “{page}”"),
            Kind::PageEdited => format!("edited the page “{page}”"),
            Kind::CampaignEdited => "edited the campaign details".to_string(),
            Kind::GmChanged if row.gm_user_id == row.actor_id => "took over as GM".to_string(),
            Kind::GmChanged => format!("made {} the GM", row.gm_name),
        };
        // The page, then the item on it
        let page_url = if let Some(slug) = &row.page_slug {
            format!("/campaigns/{}/pages/{slug}", row.campaign_id)
        } else if let Some(id) = row.schedule_poll_id {
            format!("/schedule/{id}")
        } else if let Some(id) = row.game_session_id {
            format!("/sessions/{id}")
        } else if row.kind == Kind::GmChanged {
            format!("/campaigns/{}/members", row.campaign_id)
        } else {
            format!("/campaigns/{}", row.campaign_id)
        };
        let url = if let Some(id) = row.comment_id {
            format!("{page_url}#comment-{id}")
        } else if let Some(id) = row.entry_id {
            format!("{page_url}#entry-{id}")
        } else if let Some(id) = row.poll_id {
            format!("{page_url}#poll-{id}")
        } else {
            page_url
        };
        NotificationView {
            id: row.id,
            campaign_id: row.campaign_id,
            campaign_name: row.campaign_name,
            actor_name: row.actor_name,
            text,
            excerpt: row.comment_body.as_deref().map(excerpt),
            url,
            ago: time::from_db(&row.updated_at)
                .map(|t| ago(t, Utc::now()))
                .unwrap_or_default(),
            when: When::from_db(&row.updated_at, tz),
            seen: row.seen_at.is_some(),
        }
    }
}

/// How long ago `t` was, roughly: "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago".
fn ago(t: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - t).num_minutes();
    match minutes {
        ..1 => "just now".to_string(),
        1..60 => format!("{minutes} min ago"),
        60..1440 => format!("{} h ago", minutes / 60),
        1440..2880 => "yesterday".to_string(),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// The start of a comment on one line, at most 100 characters.
fn excerpt(body_md: &str) -> String {
    let one_line = body_md.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= 100 {
        one_line
    } else {
        let cut: String = one_line.chars().take(99).collect();
        format!("{}…", cut.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_fold_into_the_addition() {
        assert_eq!(Kind::EntryEdited.folds_into(), Kind::EntryAdded);
        assert_eq!(Kind::SessionMoved.folds_into(), Kind::SessionAdded);
        assert_eq!(Kind::CommentAdded.folds_into(), Kind::CommentAdded);
    }

    #[test]
    fn ago_is_rough() {
        let now = time::from_db("2026-11-02T12:00:00Z").unwrap();
        let at = |s: &str| ago(time::from_db(s).unwrap(), now);
        assert_eq!(at("2026-11-02T11:59:40Z"), "just now");
        assert_eq!(at("2026-11-02T11:55:00Z"), "5 min ago");
        assert_eq!(at("2026-11-02T09:00:00Z"), "3 h ago");
        assert_eq!(at("2026-11-01T10:00:00Z"), "yesterday");
        assert_eq!(at("2026-10-29T12:00:00Z"), "4 days ago");
    }

    #[test]
    fn excerpts_are_one_short_line() {
        assert_eq!(excerpt("See you\n\non *Friday*"), "See you on *Friday*");
        let long = "word ".repeat(40);
        let e = excerpt(&long);
        assert_eq!(e.chars().count(), 100);
        assert!(e.ends_with("word…"), "{e}");
    }
}
