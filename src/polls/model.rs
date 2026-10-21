use crate::time::{self, When};
use chrono_tz::Tz;

pub struct Poll {
    pub id: i64,
    pub campaign_id: i64,
    pub question: String,
    pub description_md: String,
    pub multiple_choice: bool,
    pub closes_at: Option<String>,
    pub closed_at: Option<String>,
}

impl Poll {
    /// Closed by the GM, or past its automatic close time.
    pub fn is_closed(&self) -> bool {
        self.closed_at.is_some()
            || self
                .closes_at
                .as_deref()
                .and_then(time::from_db)
                .is_some_and(|t| t <= chrono::Utc::now())
    }
}

pub struct OptionRow {
    pub id: i64,
    pub label: String,
}

pub struct VoteRow {
    pub poll_option_id: i64,
    pub user_id: i64,
    pub display_name: String,
}

#[derive(serde::Serialize)]
pub struct OptionView {
    pub id: i64,
    pub label: String,
    pub count: usize,
    /// Percent of voters who picked this option (0-100), for the result bar
    pub percent: usize,
    pub voters: Vec<String>,
    pub mine: bool,
}

#[derive(serde::Serialize)]
pub struct PollView {
    pub id: i64,
    pub question: String,
    pub description_html: String,
    pub multiple_choice: bool,
    pub is_closed: bool,
    pub closes: Option<When>,
    pub options: Vec<OptionView>,
    pub voter_count: usize,
    pub has_voted: bool,
    pub can_vote: bool,
    pub can_manage: bool,
}

/// `POST /campaigns/{id}/polls`. `options` is the repeated option field, `game_session_id` set
/// for a session poll. Needs `axum_extra::extract::Form` for the `Vec`.
#[derive(serde::Deserialize, serde::Serialize)]
pub struct NewPollForm {
    pub question: String,
    #[serde(default)]
    pub description_md: String,
    /// A checkbox: present ("on") when checked
    #[serde(default)]
    pub multiple_choice: Option<String>,
    #[serde(default)]
    pub closes_at: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub game_session_id: Option<i64>,
}

pub struct NewPoll {
    pub question: String,
    pub description_md: String,
    pub multiple_choice: bool,
    pub closes_at: Option<String>,
    pub options: Vec<String>,
}

impl NewPollForm {
    pub fn validate(&mut self, tz: Tz) -> Result<NewPoll, &'static str> {
        self.question = self.question.trim().to_string();
        self.description_md = self.description_md.trim().to_string();
        self.options = self
            .options
            .iter()
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect();
        if self.question.is_empty() || self.question.chars().count() > 200 {
            return Err("Fill in a question (at most 200 characters).");
        }
        if self.options.len() < 2 {
            return Err("Add at least two options.");
        }
        if self.options.len() > 20 || self.options.iter().any(|o| o.chars().count() > 200) {
            return Err("At most 20 options of at most 200 characters each.");
        }
        let closes_at = if self.closes_at.trim().is_empty() {
            None
        } else {
            let t = time::parse_local(&self.closes_at, tz)
                .ok_or("The closing time isn't a valid date and time.")?;
            Some(time::to_db(t))
        };
        Ok(NewPoll {
            question: self.question.clone(),
            description_md: self.description_md.clone(),
            multiple_choice: self.multiple_choice.is_some(),
            closes_at,
            options: self.options.clone(),
        })
    }
}

/// `POST /polls/{id}/vote`: the checked options (repeated `option_ids`); empty withdraws the vote.
#[derive(serde::Deserialize)]
pub struct VoteForm {
    #[serde(default)]
    pub option_ids: Vec<i64>,
}

#[derive(serde::Deserialize)]
pub struct NewPollQuery {
    pub session_id: Option<i64>,
}
