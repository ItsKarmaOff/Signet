//! Day grouping and date formatting of the Library (`Intl.RelativeTimeFormat` / `Intl.DateTimeFormat` in `Library.tsx`),
//! for the two interface languages.

use chrono::{DateTime, Datelike, NaiveDate, TimeZone, Timelike};
use signet_core::i18n::Locale;

const DAY_MS: i64 = 86_400_000;

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn date_in<Tz: TimeZone>(ts: i64, tz: &Tz) -> Option<NaiveDate> {
    tz.timestamp_millis_opt(ts).single().map(|d| d.date_naive())
}

/// Start of the local day containing `ts` (ms), used as the grouping key.
pub fn start_of_day<Tz: TimeZone>(ts: i64, tz: &Tz) -> i64 {
    date_in(ts, tz)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|n| tz.from_local_datetime(&n).earliest())
        .map(|d: DateTime<Tz>| d.timestamp_millis())
        .unwrap_or(ts - ts.rem_euclid(DAY_MS))
}

const EN_DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const FR_DAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
const EN_MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const FR_MONTHS: [&str; 12] = ["janvier", "f\u{e9}vrier", "mars", "avril", "mai", "juin", "juillet", "ao\u{fb}t", "septembre", "octobre", "novembre", "d\u{e9}cembre"];

/// "Today", "Yesterday", then "Tuesday, October 21" (with the year if it is not the current one).
pub fn day_label<Tz: TimeZone>(ts: i64, now: i64, locale: Locale, tz: &Tz) -> String {
    let (Some(d), Some(today)) = (date_in(ts, tz), date_in(now, tz)) else { return String::new() };
    let diff = today.signed_duration_since(d).num_days();
    if diff <= 1 {
        let word = match (locale, diff.max(0)) {
            (Locale::En, 0) => "today",
            (Locale::En, _) => "yesterday",
            (Locale::Fr, 0) => "aujourd\u{2019}hui",
            (Locale::Fr, _) => "hier",
        };
        return capitalize(word);
    }
    let same_year = d.year() == today.year();
    let wd = d.weekday().num_days_from_monday() as usize;
    let m = d.month0() as usize;
    match locale {
        Locale::En => {
            let base = format!("{}, {} {}", EN_DAYS[wd], EN_MONTHS[m], d.day());
            if same_year { base } else { format!("{base}, {}", d.year()) }
        }
        Locale::Fr => {
            let base = format!("{} {} {}", FR_DAYS[wd], d.day(), FR_MONTHS[m]);
            if same_year { base } else { format!("{base} {}", d.year()) }
        }
    }
}

/// "09:41 AM" in English, "09:41" in French.
pub fn time_of<Tz: TimeZone>(ts: i64, locale: Locale, tz: &Tz) -> String {
    let Some(t) = tz.timestamp_millis_opt(ts).single() else { return String::new() };
    match locale {
        Locale::Fr => format!("{:02}:{:02}", t.hour(), t.minute()),
        Locale::En => {
            let (pm, h) = t.hour12();
            format!("{:02}:{:02} {}", h, t.minute(), if pm { "PM" } else { "AM" })
        }
    }
}

/// A day of a list sorted by descending date.
#[derive(Debug, PartialEq)]
pub struct DayGroup<T> {
    pub key: i64,
    pub label: String,
    pub items: Vec<T>,
}

/// Groups a list already sorted by descending day.
pub fn by_day<T, Tz: TimeZone>(items: Vec<T>, at: impl Fn(&T) -> i64, now: i64, locale: Locale, tz: &Tz) -> Vec<DayGroup<T>> {
    let mut groups: Vec<DayGroup<T>> = Vec::new();
    for item in items {
        let ts = at(&item);
        let key = start_of_day(ts, tz);
        match groups.last_mut() {
            Some(last) if last.key == key => last.items.push(item),
            _ => groups.push(DayGroup { key, label: day_label(ts, now, locale, tz), items: vec![item] }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn ms(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
        Utc.with_ymd_and_hms(y, m, d, h, min, 0).unwrap().timestamp_millis()
    }

    #[test]
    fn labels_today_yesterday_then_dates() {
        let now = ms(2026, 11, 6, 15, 0);
        assert_eq!(day_label(ms(2026, 11, 6, 1, 0), now, Locale::En, &Utc), "Today");
        assert_eq!(day_label(ms(2026, 11, 5, 23, 0), now, Locale::En, &Utc), "Yesterday");
        assert_eq!(day_label(ms(2026, 11, 6, 1, 0), now, Locale::Fr, &Utc), "Aujourd\u{2019}hui");
        assert_eq!(day_label(ms(2026, 11, 5, 23, 0), now, Locale::Fr, &Utc), "Hier");
        assert_eq!(day_label(ms(2026, 10, 21, 9, 0), now, Locale::En, &Utc), "Wednesday, October 21");
        assert_eq!(day_label(ms(2026, 10, 21, 9, 0), now, Locale::Fr, &Utc), "mercredi 21 octobre");
        assert_eq!(day_label(ms(2025, 10, 21, 9, 0), now, Locale::En, &Utc), "Tuesday, October 21, 2025");
        assert_eq!(day_label(ms(2025, 10, 21, 9, 0), now, Locale::Fr, &Utc), "mardi 21 octobre 2025");
    }

    #[test]
    fn times_follow_the_language() {
        assert_eq!(time_of(ms(2026, 11, 6, 9, 5), Locale::En, &Utc), "09:05 AM");
        assert_eq!(time_of(ms(2026, 11, 6, 21, 41), Locale::En, &Utc), "09:41 PM");
        assert_eq!(time_of(ms(2026, 11, 6, 21, 41), Locale::Fr, &Utc), "21:41");
    }

    #[test]
    fn groups_consecutive_entries_of_a_day() {
        let now = ms(2026, 11, 6, 15, 0);
        let items = vec![ms(2026, 11, 6, 12, 0), ms(2026, 11, 6, 8, 0), ms(2026, 11, 5, 20, 0)];
        let groups = by_day(items, |t| *t, now, Locale::En, &Utc);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].label, "Today");
        assert_eq!(groups[0].items.len(), 2);
        assert_eq!(groups[1].label, "Yesterday");
        assert_eq!(groups[0].key, ms(2026, 11, 6, 0, 0));
    }
}
