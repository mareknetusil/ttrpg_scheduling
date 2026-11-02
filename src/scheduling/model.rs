use crate::campaigns::model::Member;
use crate::time::{self, When};
use chrono::{Datelike, Duration, NaiveDate};
use chrono_tz::Tz;
use std::collections::HashMap;

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

/// How a poll shows its candidate dates (see GLOSSARY.md). Stored as `TEXT`.
#[derive(
    Clone, Copy, PartialEq, Eq, Debug, Default, sqlx::Type, serde::Serialize, serde::Deserialize,
)]
#[sqlx(rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// Week rows, at most one candidate date per day
    Calendar,
    /// One row per candidate date. The default for a form without the field, like the old polls.
    #[default]
    List,
}

pub struct Poll {
    pub id: i64,
    pub campaign_id: i64,
    pub title: String,
    pub description_md: String,
    pub layout: Layout,
    pub status: PollStatus,
    pub deadline_at: Option<String>,
    pub decided_option_id: Option<i64>,
    pub game_session_id: Option<i64>,
}

impl Poll {
    /// Members can vote: the poll is open and its deadline (if any) hasn't passed. For display
    /// only (buttons, refreshing the grid); writes are guarded by the `voting_polls` view.
    pub fn voting_open(&self) -> bool {
        let deadline_passed = match self.deadline_at.as_deref().and_then(time::from_db) {
            Some(deadline) => deadline <= chrono::Utc::now(),
            None => false,
        };
        self.status == PollStatus::Open && !deadline_passed
    }
}

/// A poll prepared for templates.
#[derive(serde::Serialize)]
pub struct PollView {
    pub id: i64,
    pub title: String,
    pub layout: Layout,
    pub status: PollStatus,
    pub deadline: Option<When>,
    /// See `Poll::voting_open`
    pub voting_open: bool,
    pub decided_option_id: Option<i64>,
    pub game_session_id: Option<i64>,
}

impl PollView {
    pub fn new(poll: &Poll, tz: Tz) -> PollView {
        PollView {
            id: poll.id,
            title: poll.title.clone(),
            layout: poll.layout,
            status: poll.status,
            deadline: poll.deadline_at.as_deref().map(|d| When::from_db(d, tz)),
            voting_open: poll.voting_open(),
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
    /// See `Poll::voting_open`
    pub voting_open: bool,
    /// Options still in the future (past ones can't be voted on)
    pub option_count: i64,
    /// How many of those future options the current user has answered
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
    pub layout: Layout,
    #[serde(default)]
    pub deadline: String,
    #[serde(default)]
    pub options: Vec<String>,
}

/// A validated `NewPollForm`, with times converted to UTC DB strings.
pub struct NewPoll {
    pub title: String,
    pub description_md: String,
    pub layout: Layout,
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
        let mut times = Vec::new();
        for o in &self.options {
            let t = time::parse_local(o, tz)
                .ok_or_else(|| format!("“{o}” isn't a valid date and time."))?;
            times.push(t);
        }
        times.sort();
        times.dedup();
        if self.layout == Layout::Calendar {
            // One candidate date per day (in the group's time zone); the dates are sorted, so a
            // second one on the same day is right after the first
            for pair in times.windows(2) {
                let day = pair[0].with_timezone(&tz).date_naive();
                if pair[1].with_timezone(&tz).date_naive() == day {
                    return Err(format!(
                        "{} has two times. A calendar has one time per day: use the list layout for that.",
                        day.format("%a %-d %b")
                    ));
                }
            }
        }
        let options = times.into_iter().map(time::to_db).collect();
        Ok(NewPoll {
            title: self.title.clone(),
            description_md: self.description_md.clone(),
            layout: self.layout,
            deadline_at,
            options,
        })
    }
}

/// The time most candidate dates start at ("18:00"): a calendar shows it once, above the weeks,
/// and marks only the days that start at another time. Ties go to the time of the earliest
/// date (`times` are in date order). `None` without dates.
pub fn usual_time(times: &[&str]) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for t in times {
        *counts.entry(t).or_default() += 1;
    }
    let mut best: Option<(&str, usize)> = None;
    for t in times {
        let n = counts[t];
        if best.is_none_or(|(_, most)| n > most) {
            best = Some((t, n));
        }
    }
    best.map(|(t, _)| t.to_string())
}

/// The Monday of the week `date` is in.
pub fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(i64::from(date.weekday().num_days_from_monday()))
}

/// One day of a calendar poll's weeks.
#[derive(serde::Serialize, Debug, PartialEq)]
pub struct CalendarDay {
    /// "2026-11-06"
    pub date: String,
    /// "6"; "1 Dec" on the first of a month and on the first day shown
    pub label: String,
    /// The index of this day's candidate date in the poll's options, if it has one
    pub option: Option<usize>,
    /// Before today (in the group's time zone)
    pub is_past: bool,
    pub is_today: bool,
}

/// The weeks (Monday to Sunday) from the first candidate date to the last, every week in between
/// included. `dates` are the local days of the poll's options, in option order.
pub fn calendar_weeks(dates: &[NaiveDate], today: NaiveDate) -> Vec<Vec<CalendarDay>> {
    let (Some(first), Some(last)) = (dates.iter().min(), dates.iter().max()) else {
        return Vec::new();
    };
    let mut option_on: HashMap<NaiveDate, usize> = HashMap::new();
    for (i, d) in dates.iter().enumerate() {
        // Only an old list poll switched over could have two; the first one wins
        option_on.entry(*d).or_insert(i);
    }
    let start = monday_of(*first);
    let mut weeks = Vec::new();
    let mut monday = start;
    while monday <= *last {
        let week = (0..7)
            .map(|i| {
                let day = monday + Duration::days(i);
                let label = if day == start || day.day() == 1 {
                    day.format("%-d %b").to_string()
                } else {
                    day.day().to_string()
                };
                CalendarDay {
                    date: day.format("%Y-%m-%d").to_string(),
                    label,
                    option: option_on.get(&day).copied(),
                    is_past: day < today,
                    is_today: day == today,
                }
            })
            .collect();
        weeks.push(week);
        monday += Duration::days(7);
    }
    weeks
}

/// What a request to answer dates did (`db::set_vote`, `clear_vote`, `set_unanswered`, `paint`).
#[derive(Debug, PartialEq)]
pub enum Answered {
    Changed,
    /// Nothing changed: since the page was loaded the poll was decided or cancelled, or the
    /// deadline or the date(s) passed
    VotingClosed,
    /// Nothing changed: a date of another poll (a crafted request)
    NotThisPoll,
}

/// One row of the voting grid.
#[derive(serde::Serialize)]
pub struct OptionView {
    pub id: i64,
    pub when: When,
    pub yes: usize,
    pub maybe: usize,
    pub no: usize,
    /// One answer per member, in the order of `members`
    pub cells: Vec<Option<Answer>>,
    pub mine: Option<Answer>,
    pub is_best: bool,
    /// Every member answered, and nobody said no
    pub no_conflicts: bool,
    pub is_decided: bool,
}

/// A member who hasn't answered every upcoming date yet.
#[derive(serde::Serialize)]
pub struct WaitingOn {
    pub display_name: String,
    /// How many upcoming dates they haven't answered
    pub unanswered: usize,
}

/// The voting grid's numbers, worked out from the poll's rows (see `tally`).
pub struct Tally {
    pub options: Vec<OptionView>,
    /// Dates still ahead
    pub upcoming: usize,
    /// Dates still ahead that `user_id` hasn't answered
    pub unanswered: usize,
    pub waiting_on: Vec<WaitingOn>,
}

/// Works out the voting grid: per date the answers of each member (in the order of `members`)
/// and the counts, "most yes" and "no conflicts" markers, and who still has dates to answer.
/// Past dates show their answers but get no markers and count as nothing to answer.
pub fn tally(
    options: &[OptionRow],
    members: &[Member],
    votes: &[VoteRow],
    user_id: i64,
    decided_option_id: Option<i64>,
    tz: Tz,
) -> Tally {
    let mut answers: HashMap<(i64, i64), Answer> = HashMap::new();
    for v in votes {
        answers.insert((v.option_id, v.user_id), v.answer);
    }
    let mut options: Vec<OptionView> = options
        .iter()
        .map(|o| {
            let when = When::from_db(&o.starts_at, tz);
            let cells: Vec<Option<Answer>> = members
                .iter()
                .map(|m| answers.get(&(o.id, m.user_id)).copied())
                .collect();
            let count = |a: Answer| cells.iter().filter(|c| **c == Some(a)).count();
            OptionView {
                id: o.id,
                yes: count(Answer::Yes),
                maybe: count(Answer::Maybe),
                no: count(Answer::No),
                mine: answers.get(&(o.id, user_id)).copied(),
                no_conflicts: !when.is_past
                    && !members.is_empty()
                    && cells
                        .iter()
                        .all(|c| matches!(c, Some(Answer::Yes | Answer::Maybe))),
                cells,
                is_best: false,
                is_decided: decided_option_id == Some(o.id),
                when,
            }
        })
        .collect();
    // "Most yes" (ties broken by maybe) among the dates still ahead, only once someone said yes.
    // Ties all get the marker.
    let best = options
        .iter()
        .filter(|o| !o.when.is_past)
        .map(|o| (o.yes, o.maybe))
        .max();
    if let Some(best) = best.filter(|(yes, _)| *yes > 0) {
        for o in &mut options {
            o.is_best = !o.when.is_past && (o.yes, o.maybe) == best;
        }
    }
    let upcoming = options.iter().filter(|o| !o.when.is_past).count();
    let unanswered = options
        .iter()
        .filter(|o| !o.when.is_past && o.mine.is_none())
        .count();
    let waiting_on = members
        .iter()
        .enumerate()
        .map(|(i, m)| WaitingOn {
            display_name: m.display_name.clone(),
            unanswered: options
                .iter()
                .filter(|o| !o.when.is_past && o.cells[i].is_none())
                .count(),
        })
        .filter(|w| w.unanswered > 0)
        .collect();
    Tally {
        options,
        upcoming,
        unanswered,
        waiting_on,
    }
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
            layout: Layout::List,
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

    #[test]
    fn a_calendar_has_one_time_per_day() {
        let tz = chrono_tz::Europe::Prague;
        let mut f = form(&["2026-11-07T14:00", "2026-11-07T19:00"]);
        assert!(f.validate(tz).is_ok(), "a list can");
        f.layout = Layout::Calendar;
        assert_eq!(
            f.validate(tz).err().unwrap(),
            "Sat 7 Nov has two times. A calendar has one time per day: use the list layout for that."
        );
        // 00:30 local is still the 8th, although it's the 7th in UTC
        f.options = vec!["2026-11-07T19:00".into(), "2026-11-08T00:30".into()];
        assert!(f.validate(tz).is_ok());
    }

    #[test]
    fn usual_time_is_the_most_common() {
        assert_eq!(usual_time(&[]), None);
        assert_eq!(
            usual_time(&["14:00", "18:00", "18:00"]).as_deref(),
            Some("18:00")
        );
        // a tie: the earliest date's time
        assert_eq!(usual_time(&["19:00", "18:00"]).as_deref(), Some("19:00"));
    }

    #[test]
    fn calendar_weeks_cover_every_week_from_first_to_last() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        // Fri 27 Nov and Tue 8 Dec 2026: three weeks, Mon 23 Nov to Sun 13 Dec
        let weeks = calendar_weeks(&[d("2026-11-27"), d("2026-12-08")], d("2026-11-25"));
        assert_eq!(weeks.len(), 3);
        assert!(weeks.iter().all(|w| w.len() == 7));
        let first = &weeks[0][0];
        assert_eq!(
            (first.date.as_str(), first.label.as_str()),
            ("2026-11-23", "23 Nov")
        );
        assert!(first.is_past);
        assert!(weeks[0][2].is_today);
        assert_eq!(weeks[0][4].option, Some(0));
        assert_eq!(weeks[1][1].label, "1 Dec", "the first of a month names it");
        assert_eq!(weeks[1][2].label, "2");
        assert!(
            weeks[1].iter().all(|d| d.option.is_none()),
            "a week between"
        );
        assert_eq!(weeks[2][1].option, Some(1));
        assert_eq!(weeks[2][6].date, "2026-12-13");
        assert!(calendar_weeks(&[], d("2026-11-25")).is_empty());
    }

    fn member(user_id: i64, name: &str) -> Member {
        Member {
            user_id,
            username: name.to_lowercase(),
            display_name: name.to_string(),
            joined_at: "2026-01-01T00:00:00Z".to_string(),
            deactivated: false,
        }
    }

    fn row(id: i64, starts_at: &str) -> OptionRow {
        OptionRow {
            id,
            starts_at: starts_at.to_string(),
        }
    }

    fn votes(answers: &[(i64, i64, Answer)]) -> Vec<VoteRow> {
        answers
            .iter()
            .map(|&(option_id, user_id, answer)| VoteRow {
                option_id,
                user_id,
                answer,
            })
            .collect()
    }

    use Answer::{Maybe, No, Yes};
    const PAST: &str = "2020-01-10T17:00:00Z";
    const LATER: &str = "2030-01-10T17:00:00Z";
    const LATER2: &str = "2030-01-11T17:00:00Z";
    const LATER3: &str = "2030-01-12T17:00:00Z";

    #[test]
    fn most_yes_needs_a_yes_and_ignores_past_dates() {
        let members = [member(1, "Gina"), member(2, "Mark")];
        let rows = [row(1, PAST), row(2, LATER), row(3, LATER2)];
        let tz = chrono_tz::UTC;

        // nobody said yes yet: no marker, even with maybes
        let t = tally(&rows, &members, &votes(&[(2, 1, Maybe)]), 1, None, tz);
        assert!(t.options.iter().all(|o| !o.is_best));

        // the past date has the most yes, but only future dates compete
        let v = votes(&[
            (1, 1, Yes),
            (1, 2, Yes),
            (2, 1, Yes),
            (3, 2, Yes),
            (3, 1, Maybe),
        ]);
        let t = tally(&rows, &members, &v, 1, None, tz);
        let best: Vec<bool> = t.options.iter().map(|o| o.is_best).collect();
        assert_eq!(
            best,
            [false, false, true],
            "a tie on yes is broken by maybe"
        );
        assert_eq!(
            (t.options[0].yes, t.options[2].yes, t.options[2].maybe),
            (2, 1, 1)
        );

        // a real tie marks both
        let v = votes(&[(2, 1, Yes), (3, 2, Yes)]);
        let t = tally(&rows, &members, &v, 1, None, tz);
        let best: Vec<bool> = t.options.iter().map(|o| o.is_best).collect();
        assert_eq!(best, [false, true, true]);
    }

    #[test]
    fn no_conflicts_needs_every_member_and_a_future_date() {
        let members = [member(1, "Gina"), member(2, "Mark")];
        let rows = [row(1, PAST), row(2, LATER), row(3, LATER2), row(4, LATER3)];
        let v = votes(&[
            (1, 1, Yes),
            (1, 2, Yes),
            (2, 1, Yes),
            (2, 2, Maybe),
            (3, 1, Yes),
            (3, 2, No),
            (4, 1, Yes),
        ]);
        let t = tally(&rows, &members, &v, 1, Some(2), chrono_tz::UTC);
        let ok: Vec<bool> = t.options.iter().map(|o| o.no_conflicts).collect();
        assert_eq!(ok, [false, true, false, false]);
        assert_eq!(t.options[1].cells, [Some(Yes), Some(Maybe)]);
        assert_eq!(t.options[3].cells, [Some(Yes), None]);
        assert!(t.options[1].is_decided && !t.options[2].is_decided);
        // nobody to ask, no conflicts to claim
        let t = tally(&rows, &[], &v, 1, None, chrono_tz::UTC);
        assert!(t.options.iter().all(|o| !o.no_conflicts));
    }

    #[test]
    fn waiting_on_counts_only_dates_still_ahead() {
        let members = [member(1, "Gina"), member(2, "Mark"), member(3, "Nina")];
        let rows = [row(1, PAST), row(2, LATER), row(3, LATER2)];
        // Gina answered both future dates, Mark one, Nina none (only the past one)
        let v = votes(&[(2, 1, Yes), (3, 1, No), (2, 2, Maybe), (1, 3, Yes)]);
        let t = tally(&rows, &members, &v, 2, None, chrono_tz::UTC);
        assert_eq!(t.upcoming, 2);
        assert_eq!(t.unanswered, 1, "Mark's own");
        assert_eq!(t.options[1].mine, Some(Maybe));
        let waiting: Vec<(&str, usize)> = t
            .waiting_on
            .iter()
            .map(|w| (w.display_name.as_str(), w.unanswered))
            .collect();
        assert_eq!(waiting, [("Mark", 1), ("Nina", 2)]);
    }
}
