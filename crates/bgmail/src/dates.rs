//! Message dates shown in the list and the reader, in the local time zone.
//!
//! The formatting core works on explicit `NaiveDateTime`s so it is testable
//! without depending on the machine's zone; [`list_date`] / [`full_date`]
//! convert the stored unix seconds with the local offset first.

use chrono::{Datelike, Local, NaiveDateTime, TimeZone};

use crate::locale::Language;

/// Local wall-clock time of `unix` seconds (`None` for 0 / out of range).
fn local(unix: i64) -> Option<NaiveDateTime> {
    if unix <= 0 {
        return None;
    }
    Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|time| time.naive_local())
}

/// Compact date for the message list: time today, "Yesterday", weekday within
/// the last week, otherwise the short date in the language's order.
pub fn format_list_date(time: NaiveDateTime, now: NaiveDateTime, language: Language) -> String {
    let days = (now.date() - time.date()).num_days();
    match days {
        0 => time.format("%H:%M").to_string(),
        1 => match language {
            Language::English => "Yesterday".to_string(),
            Language::Portuguese => "Ontem".to_string(),
        },
        2..=6 => weekday(time.weekday().num_days_from_monday(), language).to_string(),
        _ => short_date(time, now, language),
    }
}

/// Full date for the reader header ("dd/mm/yyyy HH:MM" or "mm/dd/yyyy HH:MM").
pub fn format_full_date(time: NaiveDateTime, language: Language) -> String {
    match language {
        Language::English => time.format("%b %-d, %Y at %H:%M").to_string(),
        Language::Portuguese => time.format("%d/%m/%Y %H:%M").to_string(),
    }
}

/// "Mon, Nov 2, 2026 at 9:05 AM" / "seg., 2 de nov. de 2026 às 09:05".
pub fn format_quote_date(time: NaiveDateTime, language: Language) -> String {
    match language {
        Language::English => time.format("%a, %b %-d, %Y at %-I:%M %p").to_string(),
        Language::Portuguese => {
            const DAYS: [&str; 7] = ["seg.", "ter.", "qua.", "qui.", "sex.", "sáb.", "dom."];
            const MONTHS: [&str; 12] = [
                "jan.", "fev.", "mar.", "abr.", "mai.", "jun.", "jul.", "ago.", "set.", "out.",
                "nov.", "dez.",
            ];
            format!(
                "{}, {} de {} de {} às {}",
                DAYS[time.weekday().num_days_from_monday() as usize],
                time.day(),
                MONTHS[time.month0() as usize],
                time.year(),
                time.format("%H:%M"),
            )
        }
    }
}

fn short_date(time: NaiveDateTime, now: NaiveDateTime, language: Language) -> String {
    let same_year = time.year() == now.year();
    match (language, same_year) {
        (Language::English, true) => time.format("%b %-d").to_string(),
        (Language::English, false) => time.format("%-m/%-d/%y").to_string(),
        (Language::Portuguese, true) => time.format("%d/%m").to_string(),
        (Language::Portuguese, false) => time.format("%d/%m/%y").to_string(),
    }
}

fn weekday(index_from_monday: u32, language: Language) -> &'static str {
    const EN: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    const PT: [&str; 7] = [
        "Segunda", "Terça", "Quarta", "Quinta", "Sexta", "Sábado", "Domingo",
    ];
    let index = (index_from_monday as usize).min(6);
    match language {
        Language::English => EN[index],
        Language::Portuguese => PT[index],
    }
}

/// List date for stored unix seconds; falls back to `fallback` (the legacy
/// `received_at` text of local/demo messages) when the time is unknown.
pub fn list_date(unix: i64, fallback: &str, language: Language) -> String {
    match local(unix) {
        Some(time) => format_list_date(time, Local::now().naive_local(), language),
        None => fallback.to_string(),
    }
}

/// Reader date for stored unix seconds, with the same fallback.
pub fn full_date(unix: i64, fallback: &str, language: Language) -> String {
    match local(unix) {
        Some(time) => format_full_date(time, language),
        None => fallback.to_string(),
    }
}

/// Date of a reply's attribution line, as Gmail writes it (`fallback` for
/// unknown times).
pub fn quote_date(unix: i64, fallback: &str, language: Language) -> String {
    match local(unix) {
        Some(time) => format_quote_date(time, language),
        None => fallback.to_string(),
    }
}

/// Local "HH:MM" of `unix` seconds (empty for unknown times).
pub fn clock_time(unix: i64) -> String {
    local(unix)
        .map(|time| time.format("%H:%M").to_string())
        .unwrap_or_default()
}

/// Unix seconds now.
pub fn now_unix() -> i64 {
    Local::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    #[test]
    fn today_shows_the_time() {
        let now = at(2026, 10, 31, 18, 0);
        assert_eq!(
            format_list_date(at(2026, 10, 31, 9, 5), now, Language::English),
            "09:05"
        );
    }

    #[test]
    fn yesterday_is_localized() {
        let now = at(2026, 10, 31, 0, 30);
        let time = at(2026, 10, 30, 23, 59);
        assert_eq!(format_list_date(time, now, Language::English), "Yesterday");
        assert_eq!(format_list_date(time, now, Language::Portuguese), "Ontem");
    }

    #[test]
    fn last_week_shows_the_weekday() {
        let now = at(2026, 10, 31, 12, 0); // Saturday
        let time = at(2026, 10, 27, 12, 0); // Tuesday
        assert_eq!(format_list_date(time, now, Language::English), "Tuesday");
        assert_eq!(format_list_date(time, now, Language::Portuguese), "Terça");
    }

    #[test]
    fn older_dates_use_short_dates() {
        let now = at(2026, 10, 31, 12, 0);
        assert_eq!(
            format_list_date(at(2026, 3, 4, 8, 0), now, Language::English),
            "Mar 4"
        );
        assert_eq!(
            format_list_date(at(2026, 3, 4, 8, 0), now, Language::Portuguese),
            "04/03"
        );
        assert_eq!(
            format_list_date(at(2024, 3, 4, 8, 0), now, Language::English),
            "3/4/24"
        );
        assert_eq!(
            format_list_date(at(2024, 3, 4, 8, 0), now, Language::Portuguese),
            "04/03/24"
        );
    }

    #[test]
    fn future_dates_fall_back_to_short_dates() {
        let now = at(2026, 10, 31, 12, 0);
        assert_eq!(
            format_list_date(at(2026, 11, 2, 8, 0), now, Language::Portuguese),
            "02/11"
        );
    }

    #[test]
    fn full_date_formats_per_language() {
        let time = at(2026, 10, 31, 14, 7);
        assert_eq!(
            format_full_date(time, Language::English),
            "Oct 31, 2026 at 14:07"
        );
        assert_eq!(
            format_full_date(time, Language::Portuguese),
            "31/10/2026 14:07"
        );
    }

    #[test]
    fn quote_date_follows_gmail_per_language() {
        let time = at(2026, 11, 2, 9, 5);
        assert_eq!(
            format_quote_date(time, Language::English),
            "Mon, Nov 2, 2026 at 9:05 AM"
        );
        assert_eq!(
            format_quote_date(time, Language::Portuguese),
            "seg., 2 de nov. de 2026 às 09:05"
        );
    }

    #[test]
    fn unknown_times_use_the_fallback() {
        assert_eq!(list_date(0, "09:42", Language::English), "09:42");
        assert_eq!(full_date(-5, "Today", Language::English), "Today");
        assert!(local(0).is_none());
    }

    #[test]
    fn known_times_are_formatted() {
        let now = now_unix();
        assert!(now > 1_700_000_000);
        assert_ne!(list_date(now, "fallback", Language::English), "fallback");
        assert!(full_date(now, "fallback", Language::Portuguese).contains('/'));
    }

    #[test]
    fn clock_time_is_hours_and_minutes() {
        assert_eq!(clock_time(0), "");
        assert_eq!(clock_time(now_unix()).len(), 5);
    }

    #[test]
    fn weekday_index_is_clamped() {
        assert_eq!(weekday(9, Language::English), "Sunday");
    }
}
