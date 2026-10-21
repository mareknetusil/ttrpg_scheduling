use super::pattern::{Pattern, monday_of};
use crate::time::{self, When};
use chrono::{NaiveDate, NaiveTime};
use chrono_tz::Tz;

/// A vote. Stored as `TEXT` ('yes' | 'maybe' | 'no'), see the `CHECK` in the migration.
#[derive(Clone, Copy, PartialEq, Eq, Debug, sqlx::Type, serde::Serialize, serde::Deserialize)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    Yes,
    Maybe,
    No,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, sqlx::Type, serde::Serialize)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum PollStatus {
    Open,
    Decided,
    Cancelled,
}

pub struct Poll {
    pub id: i64,
    pub campaign_id: i64,
    pub title: String,
    pub description_md: String,
    pub status: PollStatus,
    pub deadline_at: Option<String>,
    pub decided_option_id: Option<i64>,
    pub game_session_id: Option<i64>,
}

/// A poll prepared for templates.
#[derive(serde::Serialize)]
pub struct PollView {
    pub id: i64,
    pub title: String,
    pub status: PollStatus,
    pub deadline: Option<When>,
    pub decided_option_id: Option<i64>,
    pub game_session_id: Option<i64>,
}

impl PollView {
    pub fn new(poll: &Poll, tz: Tz) -> PollView {
        PollView {
            id: poll.id,
            title: poll.title.clone(),
            status: poll.status,
            deadline: poll.deadline_at.as_deref().map(|d| When::from_db(d, tz)),
            decided_option_id: poll.decided_option_id,
            game_session_id: poll.game_session_id,
        }
    }
}

pub struct OptionRow {
    pub id: i64,
    pub starts_at: String,
}

pub struct VoteRow {
    pub option_id: i64,
    pub user_id: i64,
    pub answer: Answer,
}

/// A poll in a list (campaign page, dashboard).
#[derive(serde::Serialize)]
pub struct PollSummary {
    pub id: i64,
    pub campaign_id: i64,
    pub campaign_name: String,
    pub title: String,
    pub status: PollStatus,
    pub option_count: i64,
    /// How many of the options the current user has answered
    pub my_vote_count: i64,
}

/// `POST /campaigns/{id}/schedule`. `options` is the repeated `options` field (one
/// `datetime-local` value each), which needs `axum_extra::extract::Form`; axum's own `Form`
/// can't collect repeated keys into a `Vec`.
#[derive(serde::Deserialize, serde::Serialize)]
pub struct NewPollForm {
    pub title: String,
    #[serde(default)]
    pub description_md: String,
    #[serde(default)]
    pub deadline: String,
    #[serde(default)]
    pub options: Vec<String>,
}

/// A validated `NewPollForm`, with times converted to UTC DB strings.
pub struct NewPoll {
    pub title: String,
    pub description_md: String,
    pub deadline_at: Option<String>,
    pub options: Vec<String>,
}

pub const MAX_OPTIONS: usize = 40;

impl NewPollForm {
    pub fn trim(&mut self) {
        self.title = self.title.trim().to_string();
        self.description_md = self.description_md.trim().to_string();
        self.deadline = self.deadline.trim().to_string();
        self.options.retain(|o| !o.trim().is_empty());
    }

    pub fn validate(&self, tz: Tz) -> Result<NewPoll, String> {
        if self.title.is_empty() || self.title.chars().count() > 100 {
            return Err("Fill in a title (at most 100 characters).".to_string());
        }
        let deadline_at = if self.deadline.is_empty() {
            None
        } else {
            let t = time::parse_local(&self.deadline, tz)
                .ok_or("The deadline isn't a valid date and time.")?;
            Some(time::to_db(t))
        };
        if self.options.is_empty() {
            return Err("Add at least one date.".to_string());
        }
        if self.options.len() > MAX_OPTIONS {
            return Err(format!("At most {MAX_OPTIONS} dates per poll."));
        }
        let mut options = Vec::new();
        for o in &self.options {
            let t = time::parse_local(o, tz)
                .ok_or_else(|| format!("“{o}” isn't a valid date and time."))?;
            options.push(time::to_db(t));
        }
        options.sort();
        options.dedup();
        Ok(NewPoll {
            title: self.title.clone(),
            description_md: self.description_md.clone(),
            deadline_at,
            options,
        })
    }
}

pub struct PatternRow {
    pub id: i64,
    pub weekdays: i64,
    pub start_time: String,
    pub every_n_weeks: i64,
    pub anchor_date: String,
}

impl PatternRow {
    /// `None` only if the row is malformed, which the CHECKs and `PatternForm` prevent.
    pub fn pattern(&self) -> Option<Pattern> {
        Some(Pattern {
            weekdays: u8::try_from(self.weekdays).ok()?,
            start_time: NaiveTime::parse_from_str(&self.start_time, "%H:%M").ok()?,
            every_n_weeks: u32::try_from(self.every_n_weeks).ok()?,
            anchor: NaiveDate::parse_from_str(&self.anchor_date, "%Y-%m-%d").ok()?,
        })
    }
}

#[derive(serde::Serialize)]
pub struct PatternView {
    pub id: i64,
    pub description: String,
}

impl PatternView {
    pub fn new(row: &PatternRow) -> Option<PatternView> {
        Some(PatternView {
            id: row.id,
            description: row.pattern()?.describe(),
        })
    }
}

/// `POST /campaigns/{id}/schedule/patterns`. `weekdays` are repeated checkbox values 0 (Monday)
/// to 6 (Sunday); `first_week` is any date in the first week with sessions.
#[derive(serde::Deserialize, serde::Serialize)]
pub struct PatternForm {
    #[serde(default)]
    pub weekdays: Vec<u8>,
    pub start_time: String,
    pub every_n_weeks: u32,
    #[serde(default)]
    pub first_week: String,
}

/// A validated `PatternForm`, ready to insert.
pub struct NewPattern {
    pub weekdays: i64,
    pub start_time: String,
    pub every_n_weeks: i64,
    pub anchor_date: String,
}

impl PatternForm {
    pub fn validate(&self, today: NaiveDate) -> Result<NewPattern, &'static str> {
        let mut weekdays: u8 = 0;
        for day in &self.weekdays {
            if *day > 6 {
                return Err("Pick days of the week.");
            }
            weekdays |= 1 << day;
        }
        if weekdays == 0 {
            return Err("Pick at least one day of the week.");
        }
        let start_time = NaiveTime::parse_from_str(self.start_time.trim(), "%H:%M")
            .map_err(|_| "Fill in a start time.")?;
        if !(1..=4).contains(&self.every_n_weeks) {
            return Err("Pick how often: every 1 to 4 weeks.");
        }
        let first_week = if self.first_week.trim().is_empty() {
            today
        } else {
            NaiveDate::parse_from_str(self.first_week.trim(), "%Y-%m-%d")
                .map_err(|_| "The first week isn't a valid date.")?
        };
        Ok(NewPattern {
            weekdays: i64::from(weekdays),
            start_time: start_time.format("%H:%M").to_string(),
            every_n_weeks: i64::from(self.every_n_weeks),
            anchor_date: monday_of(first_week).format("%Y-%m-%d").to_string(),
        })
    }
}

/// `GET /campaigns/{id}/schedule/new?pattern_id=…&weeks=…` fills the dates from a pattern.
#[derive(serde::Deserialize)]
pub struct GenerateQuery {
    pub pattern_id: Option<i64>,
    pub weeks: Option<u32>,
}

#[derive(serde::Deserialize)]
pub struct VoteForm {
    pub answer: Answer,
}

#[derive(serde::Deserialize)]
pub struct DecideForm {
    pub option_id: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(options: &[&str]) -> NewPollForm {
        NewPollForm {
            title: "Next session".to_string(),
            description_md: String::new(),
            deadline: String::new(),
            options: options.iter().map(|o| o.to_string()).collect(),
        }
    }

    #[test]
    fn validates_and_converts_options() {
        let tz = chrono_tz::Europe::Prague;
        let poll = form(&["2026-11-07T18:00", "2026-10-24T18:00", "2026-10-24T18:00"])
            .validate(tz)
            .unwrap();
        // sorted, deduplicated, UTC
        assert_eq!(
            poll.options,
            ["2026-10-24T16:00:00Z", "2026-11-07T17:00:00Z"]
        );
        assert!(form(&[]).validate(tz).is_err());
        assert!(form(&["soon"]).validate(tz).is_err());
    }
}
