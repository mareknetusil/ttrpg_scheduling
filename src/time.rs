//! Time handling. The DB stores UTC as `TEXT` (`2026-10-24T16:00:00Z`); the UI shows and accepts
//! times in the one display time zone (`DISPLAY_TZ`).

use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;

/// The DB format, the same as `strftime('%Y-%m-%dT%H:%M:%SZ', 'now')` in SQL.
const DB_FORMAT: &str = "%Y-%m-%dT%H:%M:%SZ";

pub fn to_db(t: DateTime<Utc>) -> String {
    t.format(DB_FORMAT).to_string()
}

pub fn from_db(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s, DB_FORMAT)
        .ok()
        .map(|naive| naive.and_utc())
}

/// Parses the value of an `<input type="datetime-local">` (`2026-10-24T18:00`, seconds
/// optional) as a time in `tz`. In the autumn DST overlap the earlier of the two instants wins;
/// a time skipped by the spring change is `None`.
pub fn parse_local(s: &str, tz: Tz) -> Option<DateTime<Utc>> {
    let naive = NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M")
        .or_else(|_| NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M:%S"))
        .ok()?;
    tz.from_local_datetime(&naive)
        .earliest()
        .map(|t| t.with_timezone(&Utc))
}

/// A point in time prepared for templates: `<time datetime="{{ t.iso }}">{{ t.full }}</time>`.
#[derive(serde::Serialize, Clone)]
pub struct When {
    /// UTC, for `<time datetime>` and sorting
    pub iso: String,
    /// "Fri 24 Oct"
    pub date: String,
    /// "Fri 24 Oct 2026"
    pub date_long: String,
    /// "18:00"
    pub time: String,
    /// "Fri 24 Oct 2026, 18:00"
    pub full: String,
    /// "2026-10-24T18:00", the value for an `<input type="datetime-local">`
    pub input: String,
    pub is_past: bool,
}

impl When {
    pub fn new(t: DateTime<Utc>, tz: Tz) -> When {
        let local = t.with_timezone(&tz);
        When {
            iso: to_db(t),
            date: local.format("%a %-d %b").to_string(),
            date_long: local.format("%a %-d %b %Y").to_string(),
            time: local.format("%H:%M").to_string(),
            full: local.format("%a %-d %b %Y, %H:%M").to_string(),
            input: local.format("%Y-%m-%dT%H:%M").to_string(),
            is_past: t < Utc::now(),
        }
    }

    /// From a DB timestamp. A malformed value (which the app never writes) shows as-is rather
    /// than failing the whole page.
    pub fn from_db(s: &str, tz: Tz) -> When {
        match from_db(s) {
            Some(t) => When::new(t, tz),
            None => When {
                iso: s.to_string(),
                date: s.to_string(),
                date_long: s.to_string(),
                time: String::new(),
                full: s.to_string(),
                input: String::new(),
                is_past: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRAGUE: Tz = chrono_tz::Europe::Prague;

    #[test]
    fn db_round_trip() {
        let t = from_db("2026-10-24T16:00:00Z").unwrap();
        assert_eq!(to_db(t), "2026-10-24T16:00:00Z");
        assert!(from_db("garbage").is_none());
    }

    #[test]
    fn local_input_to_utc() {
        // Summer time (UTC+2) and winter time (UTC+1)
        assert_eq!(
            to_db(parse_local("2026-10-24T18:00", PRAGUE).unwrap()),
            "2026-10-24T16:00:00Z"
        );
        assert_eq!(
            to_db(parse_local("2026-11-07T18:00", PRAGUE).unwrap()),
            "2026-11-07T17:00:00Z"
        );
        // 02:30 doesn't exist on the spring-forward night
        assert!(parse_local("2027-03-28T02:30", PRAGUE).is_none());
        assert!(parse_local("tomorrow", PRAGUE).is_none());
    }

    #[test]
    fn when_formats_in_display_zone() {
        let w = When::from_db("2026-10-24T16:00:00Z", PRAGUE);
        assert_eq!(w.full, "Sat 24 Oct 2026, 18:00");
        assert_eq!(w.input, "2026-10-24T18:00");
        assert_eq!(w.date, "Sat 24 Oct");
    }
}
