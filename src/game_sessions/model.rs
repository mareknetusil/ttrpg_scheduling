use crate::time::When;
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
