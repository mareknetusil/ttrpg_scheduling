//! Regular slots ("Fridays at 18:00, every 2 weeks") and the candidate dates they generate (M7).

use chrono::{DateTime, Datelike, Days, NaiveDate, NaiveTime, TimeZone, Utc, Weekday};
use chrono_tz::Tz;

pub const WEEKDAYS: [Weekday; 7] = [
    Weekday::Mon,
    Weekday::Tue,
    Weekday::Wed,
    Weekday::Thu,
    Weekday::Fri,
    Weekday::Sat,
    Weekday::Sun,
];

pub struct Pattern {
    /// Bit 0 = Monday … bit 6 = Sunday
    pub weekdays: u8,
    /// Local time in the display time zone
    pub start_time: NaiveTime,
    pub every_n_weeks: u32,
    /// The Monday of a week that has sessions; with `every_n_weeks > 1`, weeks are counted from it
    pub anchor: NaiveDate,
}

impl Pattern {
    pub fn has_weekday(&self, day: Weekday) -> bool {
        self.weekdays & (1 << day.num_days_from_monday()) != 0
    }

    /// Is `date` in a week that has sessions?
    fn in_active_week(&self, date: NaiveDate) -> bool {
        let monday = date - Days::new(u64::from(date.weekday().num_days_from_monday()));
        let weeks = (monday - self.anchor).num_weeks();
        weeks.rem_euclid(i64::from(self.every_n_weeks)) == 0
    }

    /// Candidate start times in the next `weeks` weeks after `now`, in order. Times that are
    /// already past, or that don't exist locally (DST gap), are skipped.
    pub fn generate(&self, now: DateTime<Utc>, weeks: u32, tz: Tz) -> Vec<DateTime<Utc>> {
        let today = now.with_timezone(&tz).date_naive();
        (0..u64::from(weeks) * 7)
            .filter_map(|offset| today.checked_add_days(Days::new(offset)))
            .filter(|date| self.has_weekday(date.weekday()) && self.in_active_week(*date))
            .filter_map(|date| {
                tz.from_local_datetime(&date.and_time(self.start_time))
                    .earliest()
            })
            .map(|t| t.with_timezone(&Utc))
            .filter(|t| *t > now)
            .collect()
    }

    /// "Fri, Sat at 18:00, every week"
    pub fn describe(&self) -> String {
        let days: Vec<String> = WEEKDAYS
            .iter()
            .filter(|d| self.has_weekday(**d))
            .map(|d| d.to_string())
            .collect();
        let every = match self.every_n_weeks {
            1 => "every week".to_string(),
            n => format!("every {n} weeks"),
        };
        format!(
            "{} at {}, {every}",
            days.join(", "),
            self.start_time.format("%H:%M")
        )
    }
}

/// The Monday of the week containing `date`.
pub fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Days::new(u64::from(date.weekday().num_days_from_monday()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::to_db;

    const PRAGUE: Tz = chrono_tz::Europe::Prague;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn fri_sat_18(every_n_weeks: u32, anchor: &str) -> Pattern {
        Pattern {
            weekdays: (1 << 4) | (1 << 5),
            start_time: NaiveTime::from_hms_opt(18, 0, 0).unwrap(),
            every_n_weeks,
            anchor: date(anchor),
        }
    }

    fn generated(p: &Pattern, now: &str, weeks: u32) -> Vec<String> {
        let now = crate::time::from_db(now).unwrap();
        p.generate(now, weeks, PRAGUE)
            .into_iter()
            .map(to_db)
            .collect()
    }

    #[test]
    fn weekly_two_days_across_dst_change() {
        // Wednesday 21 Oct 2026; DST ends on Sunday 25 Oct, so the UTC hour shifts
        let dates = generated(&fri_sat_18(1, "2026-10-19"), "2026-10-21T10:00:00Z", 2);
        assert_eq!(
            dates,
            [
                "2026-10-23T16:00:00Z",
                "2026-10-24T16:00:00Z",
                "2026-10-30T17:00:00Z",
                "2026-10-31T17:00:00Z",
            ]
        );
    }

    #[test]
    fn every_other_week_counts_from_anchor() {
        // Anchor week starts Mon 26 Oct, so the week of 19 Oct is skipped, 26 Oct is used
        let dates = generated(&fri_sat_18(2, "2026-10-26"), "2026-10-21T10:00:00Z", 4);
        assert_eq!(
            dates,
            [
                "2026-10-30T17:00:00Z",
                "2026-10-31T17:00:00Z",
                "2026-11-13T17:00:00Z",
                "2026-11-14T17:00:00Z",
            ]
        );
    }

    #[test]
    fn skips_times_already_past_today() {
        // Friday 23 Oct, 19:00 local: today's 18:00 is gone
        let dates = generated(&fri_sat_18(1, "2026-10-19"), "2026-10-23T17:00:00Z", 1);
        assert_eq!(dates, ["2026-10-24T16:00:00Z"]);
    }

    #[test]
    fn describes_itself() {
        assert_eq!(
            fri_sat_18(1, "2026-10-19").describe(),
            "Fri, Sat at 18:00, every week"
        );
        assert_eq!(
            fri_sat_18(2, "2026-10-19").describe(),
            "Fri, Sat at 18:00, every 2 weeks"
        );
        assert_eq!(monday_of(date("2026-10-25")), date("2026-10-19"));
    }
}
