//! Skykeep Calendar: the weeks iCloud filled in, as a fungOS Spore.
//!
//! Actions:
//!   view [<YYYY-MM-DD>]   the week containing that day, or the first week held
//!   list                 calendars and how many events each holds
//!   event <uid>          one event, unabridged
//!   help
//!
//! The collection is `fungos.calendar/event`: iCalendar-shaped, with `start` and
//! `end` as RFC 3339 UTC strings. That shape is why this module carries no date
//! library — a fixed-width UTC timestamp sorts and compares as a string, so the
//! week window is a prefix and the day order is the string order.
//!
//! Weekday arithmetic is Howard Hinnant's `days_from_civil` / `civil_from_days`
//! (chrono-compatible algorithms, public domain), kept to the twenty lines a
//! Spore needs instead of a calendar crate.

wit_bindgen::generate!({ path: "wit", world: "calendar", generate_all });

use serde_json::{Value, json};
use soil::records::records;

const EVENT_SCHEMA: &str = "fungos.calendar/event";
const PAGE: u32 = 256;
const MAX: usize = 50_000;
const DAYS: [&str; 7] = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

struct Component;

export!(Component);

impl Guest for Component {
    fn run(args: Vec<String>) -> Result<(), String> {
        match args.first().map(String::as_str).unwrap_or("view") {
            "view" => print_json(&view(args.get(1).map(String::as_str))?),
            "list" => print_json(&json!({
                "calendars": calendars(&load()?),
                "count": load()?.len(),
            })),
            "event" => {
                let uid = args.get(1).ok_or("usage: event <uid>")?;
                let events = load()?;
                let event = events
                    .iter()
                    .find(|event| event.uid == *uid)
                    .ok_or_else(|| format!("no event {uid}"))?;
                print_json(&json!({ "event": detail(event) }))
            }
            "help" => print_json(&json!({
                "actions": [
                    "view [<YYYY-MM-DD>]   the week containing that day, or the first week held",
                    "list                 calendars and how many events each holds",
                    "event <uid>          one event, unabridged",
                    "help",
                ],
                "schema": EVENT_SCHEMA,
                "writes": false,
            })),
            other => Err(format!("unknown action {other:?}; try help")),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The calendar, as records.

struct Event {
    uid: String,
    body: Value,
    /// `YYYY-MM-DD`, the day it starts on.
    day: String,
    /// `HH:MM`, empty for an all-day event.
    time: String,
    all_day: bool,
}

fn load() -> Result<Vec<Event>, String> {
    let mut events = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = records::scan(EVENT_SCHEMA, cursor.as_deref(), PAGE)
            .map_err(|error| format!("scan events: {error:?}"))?;
        for entry in page.entries {
            if entry.deleted || entry.schema != EVENT_SCHEMA {
                continue;
            }
            let Ok(body) = serde_json::from_slice::<Value>(&entry.body) else {
                continue;
            };
            let start = body
                .get("start")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let Some(day) = start.get(..10) else {
                continue;
            };
            let day = day.to_string();
            let all_day = body
                .get("all_day")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let time = if all_day {
                String::new()
            } else {
                start.get(11..16).unwrap_or_default().to_string()
            };
            events.push(Event {
                uid: entry.key.id,
                body,
                day,
                time,
                all_day,
            });
        }
        if events.len() >= MAX {
            break;
        }
        match page.next {
            Some(next) if Some(next.as_str()) != cursor.as_deref() => cursor = Some(next),
            _ => break,
        }
    }
    events.sort_by(|a, b| {
        let left = a
            .body
            .get("start")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let right = b
            .body
            .get("start")
            .and_then(Value::as_str)
            .unwrap_or_default();
        left.cmp(right).then_with(|| a.uid.cmp(&b.uid))
    });
    Ok(events)
}

fn text(body: &Value, key: &str) -> String {
    body.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn summary(event: &Event) -> Value {
    let calendar = event
        .body
        .get("categories")
        .and_then(Value::as_array)
        .and_then(|names| names.first())
        .and_then(Value::as_str)
        .unwrap_or("");
    json!({
        "id": event.uid,
        "title": text(&event.body, "summary"),
        "time": event.time,
        "all_day": event.all_day,
        "location": text(&event.body, "location"),
        "calendar": calendar,
    })
}

fn detail(event: &Event) -> Value {
    let body = &event.body;
    json!({
        "id": event.uid,
        "uid": text(body, "uid"),
        "title": text(body, "summary"),
        "description": text(body, "description"),
        "location": text(body, "location"),
        "start": text(body, "start"),
        "end": text(body, "end"),
        "all_day": event.all_day,
        "timezone": text(body, "timezone"),
        "url": text(body, "url"),
        "calendar": text(body, "categories"),
        "rrule": text(body, "rrule"),
    })
}

/// Calendars an event names, and how many events each holds.
///
/// The records carry the calendar as `categories[0]` rather than as a second
/// collection: an event is a whole record, and a calendar that disagrees with
/// its events is a bug nobody asks for.
fn calendars(events: &[Event]) -> Vec<Value> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for event in events {
        for name in event
            .body
            .get("categories")
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
        {
            match counts.iter_mut().find(|(existing, _)| *existing == name) {
                Some((_, count)) => *count += 1,
                None => counts.push((name, 1)),
            }
        }
    }
    counts.sort_by(|a, b| a.0.cmp(&b.0));
    counts
        .into_iter()
        .map(|(name, count)| json!({ "name": name, "count": count }))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The week.

fn view(day: Option<&str>) -> Result<Value, String> {
    let events = load()?;
    // A week needs a day. The caller names one; with none, the first day the
    // library holds is the one week this Spore can be sure is worth drawing.
    let anchor = day
        .and_then(parse_date)
        .or_else(|| events.first().and_then(|event| parse_date(&event.day)))
        .ok_or_else(|| "no events; pass a day (view <YYYY-MM-DD>)".to_string())?;
    let weekday = weekday_of(anchor);
    let start = add_days(anchor, -i64::from(weekday));

    let mut days = Vec::with_capacity(7);
    for index in 0..7 {
        let date = add_days(start, index);
        let label = iso(date);
        let mut day_events = Vec::new();
        let mut all_day = Vec::new();
        for event in &events {
            if event.day != label {
                continue;
            }
            if event.all_day {
                all_day.push(summary(event));
            } else {
                day_events.push(summary(event));
            }
        }
        days.push(json!({
            "date": label,
            "label": DAYS[index as usize],
            "day": date.2,
            "month": MONTHS[(date.1 - 1) as usize],
            "events": day_events,
            "all_day": all_day,
        }));
    }

    let end = add_days(start, 6);
    Ok(json!({
        "source": "iCloud",
        "count": events.len(),
        "week": {
            "start": iso(start),
            "end": iso(end),
            "label": week_label(start, end),
            // The neighbours, so the view can offer a week either side without
            // doing date arithmetic of its own.
            "prev": iso(add_days(start, -7)),
            "next": iso(add_days(start, 7)),
        },
        "days": days,
        "calendars": calendars(&events),
    }))
}

fn week_label(start: (i64, u32, u32), end: (i64, u32, u32)) -> String {
    let from = MONTHS[(start.1 - 1) as usize];
    let to = MONTHS[(end.1 - 1) as usize];
    if start.1 == end.1 {
        format!("{} - {} {} {}", start.2, end.2, from, start.0)
    } else if start.0 == end.0 {
        format!("{} {} - {} {} {}", start.2, from, end.2, to, start.0)
    } else {
        format!(
            "{} {} {} - {} {} {}",
            start.2, from, start.0, end.2, to, end.0
        )
    }
}

/// `YYYY-MM-DD` as a civil date.
fn parse_date(value: &str) -> Option<(i64, u32, u32)> {
    let year: i64 = value.get(..4)?.parse().ok()?;
    let month: u32 = value.get(5..7)?.parse().ok()?;
    let day: u32 = value.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some((year, month, day))
}

fn iso(date: (i64, u32, u32)) -> String {
    format!("{:04}-{:02}-{:02}", date.0, date.1, date.2)
}

fn add_days(date: (i64, u32, u32), days: i64) -> (i64, u32, u32) {
    civil_from_days(days_from_civil(date) + days)
}

/// Monday is 0.
fn weekday_of(date: (i64, u32, u32)) -> u32 {
    // 1970-01-01 was a Thursday, so day zero is Monday + 3.
    (days_from_civil(date) + 3).rem_euclid(7) as u32
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithms).
fn days_from_civil(date: (i64, u32, u32)) -> i64 {
    let (year, month, day) = date;
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn print_json(value: &Value) -> Result<(), String> {
    println!("{value}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_arithmetic_round_trips() {
        for date in [(2026, 3, 16), (1970, 1, 1), (2024, 2, 29), (1969, 12, 31)] {
            let days = days_from_civil(date);
            assert_eq!(civil_from_days(days), date);
        }
    }

    #[test]
    fn a_week_starts_on_monday() {
        // 2026-03-16 is a Monday; 2026-03-22 is the Sunday after it.
        assert_eq!(weekday_of((2026, 3, 16)), 0);
        assert_eq!(weekday_of((2026, 3, 22)), 6);
        assert_eq!(add_days((2026, 3, 16), 6), (2026, 3, 22));
        assert_eq!(add_days((2026, 3, 1), -1), (2026, 2, 28));
    }

    #[test]
    fn a_week_label_reads_like_calendar() {
        assert_eq!(
            week_label((2026, 3, 16), (2026, 3, 22)),
            "16 - 22 March 2026"
        );
        assert_eq!(
            week_label((2026, 3, 30), (2026, 4, 5)),
            "30 March - 5 April 2026"
        );
    }

    #[test]
    fn a_date_is_three_numbers() {
        assert_eq!(parse_date("2026-03-16"), Some((2026, 3, 16)));
        assert_eq!(parse_date("2026-13-16"), None);
        assert_eq!(iso((2026, 3, 6)), "2026-03-06");
    }
}
