use crate::time::When;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;

#[derive(Clone, Copy, PartialEq, Eq, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Planned,
    Played,
    Cancelled,
}

pub struct GameSession {
    pub id: i64,
    pub campaign_id: i64,
    pub number: i64,
    pub title: String,
    pub starts_at: String,
    pub status: SessionStatus,
}

/// A game session prepared for templates (also used in lists).
#[derive(serde::Serialize)]
pub struct SessionView {
    pub id: i64,
    pub campaign_id: i64,
    pub number: i64,
    pub title: String,
    pub when: When,
    pub status: SessionStatus,
}

impl SessionView {
    pub fn new(s: &GameSession, tz: Tz) -> SessionView {
        SessionView {
            id: s.id,
            campaign_id: s.campaign_id,
            number: s.number,
            title: s.title.clone(),
            when: When::from_db(&s.starts_at, tz),
            status: s.status,
        }
    }
}

/// An upcoming session in a list across campaigns (dashboard).
pub struct UpcomingRow {
    pub id: i64,
    pub campaign_id: i64,
    pub campaign_name: String,
    pub number: i64,
    pub title: String,
    pub starts_at: String,
}

#[derive(serde::Serialize)]
pub struct UpcomingView {
    pub id: i64,
    pub campaign_id: i64,
    pub campaign_name: String,
    pub number: i64,
    pub title: String,
    pub when: When,
}

impl UpcomingView {
    pub fn new(r: UpcomingRow, tz: Tz) -> UpcomingView {
        UpcomingView {
            when: When::from_db(&r.starts_at, tz),
            id: r.id,
            campaign_id: r.campaign_id,
            campaign_name: r.campaign_name,
            number: r.number,
            title: r.title,
        }
    }
}

#[derive(serde::Serialize)]
pub struct Attendee {
    pub user_id: i64,
    pub display_name: String,
}

/// One row of the campaign history.
pub struct HistoryRow {
    pub id: i64,
    pub campaign_id: i64,
    pub number: i64,
    pub title: String,
    pub starts_at: String,
    pub status: SessionStatus,
    pub attendee_count: i64,
    pub entry_count: i64,
}

#[derive(serde::Serialize)]
pub struct HistoryView {
    pub session: SessionView,
    pub attendee_count: i64,
    pub entry_count: i64,
}

impl HistoryView {
    pub fn new(r: HistoryRow, tz: Tz) -> HistoryView {
        HistoryView {
            session: SessionView {
                id: r.id,
                campaign_id: r.campaign_id,
                number: r.number,
                title: r.title,
                when: When::from_db(&r.starts_at, tz),
                status: r.status,
            },
            attendee_count: r.attendee_count,
            entry_count: r.entry_count,
        }
    }
}

/// `POST /campaigns/{id}/sessions` (add a session by hand) and `POST /sessions/{id}` (edit).
/// `status` is only used when editing (a new session's status follows from its date), and
/// `user_ids` (the attendance, repeated) only when adding.
#[derive(serde::Deserialize, serde::Serialize)]
pub struct SessionForm {
    #[serde(default)]
    pub title: String,
    pub starts_at: String,
    pub status: Option<SessionStatus>,
    #[serde(default)]
    pub user_ids: Vec<i64>,
}

impl SessionForm {
    /// Returns the start time.
    pub fn validate(&mut self, tz: Tz) -> Result<DateTime<Utc>, &'static str> {
        self.title = self.title.trim().to_string();
        if self.title.chars().count() > 100 {
            return Err("The title can be at most 100 characters long.");
        }
        crate::time::parse_local(&self.starts_at, tz).ok_or("Fill in a valid date and time.")
    }
}

/// `POST /sessions/{id}/attendance`: the checked members (repeated `user_ids`).
#[derive(serde::Deserialize)]
pub struct AttendanceForm {
    #[serde(default)]
    pub user_ids: Vec<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Recap,
    Notes,
    Loot,
    Other,
}

pub struct Entry {
    pub id: i64,
    pub game_session_id: i64,
    pub kind: EntryKind,
    pub title: String,
    pub body_md: String,
    pub author_id: i64,
    pub author_name: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(serde::Serialize)]
pub struct EntryView {
    pub id: i64,
    pub kind: EntryKind,
    pub title: String,
    pub body_md: String,
    pub body_html: String,
    pub author_name: String,
    pub created: When,
    pub edited: bool,
    pub can_edit: bool,
}

impl EntryView {
    pub fn new(e: &Entry, can_edit: bool, tz: Tz) -> EntryView {
        EntryView {
            id: e.id,
            kind: e.kind,
            title: e.title.clone(),
            body_md: e.body_md.clone(),
            body_html: crate::markdown::render(&e.body_md),
            author_name: e.author_name.clone(),
            created: When::from_db(&e.created_at, tz),
            edited: e.updated_at != e.created_at,
            can_edit,
        }
    }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct EntryForm {
    pub kind: EntryKind,
    #[serde(default)]
    pub title: String,
    pub body_md: String,
}

impl EntryForm {
    pub fn trim(&mut self) {
        self.title = self.title.trim().to_string();
        self.body_md = self.body_md.trim().to_string();
    }

    pub fn error(&self) -> Option<&'static str> {
        if self.body_md.is_empty() {
            Some("Write something first.")
        } else if self.title.chars().count() > 100 {
            Some("The title can be at most 100 characters long.")
        } else {
            None
        }
    }
}
