//! Skykeep Calendar: one week of events, drawn by the Spore.
//!
//! The same shape as `photos-view`: read records, lay out, rasterize one frame,
//! hand it over through `soil:present/surface.present-rgba`. There is no font in
//! a software rasterizer, so a block is its own label for now — colour is the
//! calendar, height is the duration, and the hour grid says when.

wit_bindgen::generate!({ path: "wit", world: "runner", generate_all });

mod text;

use serde_json::Value;
use text::Face;
use soil::present::surface;
use soil::records::records;

const EVENT_SCHEMA: &str = "fungos.calendar/event";
const PAGE: u32 = 256;
const SCAN: usize = 100_000;
const DAYS: u32 = 7;
const BACKGROUND: [u8; 4] = [250, 250, 252, 0xff];
const RULE: [u8; 4] = [226, 229, 234, 0xff];
const HEADER: [u8; 4] = [244, 246, 249, 0xff];

struct Event {
    summary: String,
    /// `YYYY-MM-DD`, the day it starts on.
    day: String,
    /// Minutes from midnight.
    from: i64,
    to: i64,
    all_day: bool,
    calendar: String,
}

fn main() -> Result<(), String> {
    let mut drawn: Option<(u32, u32)> = None;
    loop {
        let (width, height) = surface::size();
        if drawn != Some((width, height)) {
            let (events, note) = load();
            if !note.is_empty() {
                eprintln!("calendar-view: {note}");
            }
            let (week, start) = week_of(&events);
            let pixels = draw(width, height, &week, start);
            surface::present_rgba(&surface::RgbaFrame { width, height, pixels })
                .map_err(|error| format!("present: {error}"))?;
            eprintln!("calendar-view: {} events in one week", week.len());
            drawn = Some((width, height));
        }
        if !soil::present::input::interactive() {
            return Ok(());
        }
        let _ = soil::present::input::poll(250);
    }
}

fn load() -> (Vec<Event>, String) {
    let mut events = Vec::new();
    let mut note = String::new();
    let mut cursor: Option<String> = None;
    let mut scanned = 0usize;
    loop {
        let page = match records::scan(EVENT_SCHEMA, cursor.as_deref(), PAGE) {
            Ok(page) => page,
            Err(error) => {
                note = format!("records: {error:?}");
                break;
            }
        };
        for entry in page.entries {
            scanned += 1;
            let Ok(body) = serde_json::from_slice::<Value>(&entry.body) else {
                continue;
            };
            let Some(start) = body.get("start").and_then(Value::as_str) else {
                continue;
            };
            let Some((day, minute)) = split_time(start) else {
                continue;
            };
            let end = body.get("end").and_then(Value::as_str).and_then(split_time);
            let to = match end {
                Some((end_day, end_minute)) if end_day == day => end_minute,
                _ => minute + 60,
            };
            events.push(Event {
                summary: body
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                day,
                from: minute,
                to: to.max(minute + 15),
                all_day: body.get("all_day").and_then(Value::as_bool).unwrap_or(false),
                calendar: body
                    .get("categories")
                    .and_then(Value::as_array)
                    .and_then(|list| list.first())
                    .and_then(Value::as_str)
                    .unwrap_or("Other")
                    .to_string(),
            });
        }
        match page.next {
            Some(next) if Some(next.as_str()) != cursor.as_deref() => cursor = Some(next),
            _ => break,
        }
        if scanned >= SCAN {
            break;
        }
    }
    (events, note)
}

/// `2026-10-24T17:30:00Z` → (`2026-10-24`, minutes).
fn split_time(value: &str) -> Option<(String, i64)> {
    let date = value.get(0..10)?.to_string();
    let hour: i64 = value.get(11..13)?.parse().ok()?;
    let minute: i64 = value.get(14..16)?.parse().ok()?;
    Some((date, hour * 60 + minute))
}

/// The busiest week in the read, and that week's Monday.
///
/// Not "this week": a store that stopped syncing months ago has an empty this
/// week, and a frame of nothing does not say whether the Spore works. The week
/// with the most events is the one that shows what a week looks like.
fn week_of(events: &[Event]) -> (Vec<&Event>, String) {
    if events.is_empty() {
        return (Vec::new(), "1970-01-01".to_string());
    }
    let mut weeks: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for event in events {
        *weeks.entry(monday_of(&event.day)).or_default() += 1;
    }
    let Some((monday, _)) = weeks.iter().max_by_key(|(_, count)| **count) else {
        return (Vec::new(), "1970-01-01".to_string());
    };
    let monday = monday.clone();
    let week: Vec<&Event> = events
        .iter()
        .filter(|event| day_index(&event.day, &monday).is_some_and(|day| day < DAYS as i64))
        .collect();
    (week, monday)
}

/// Days since the Monday of `date`'s week, as a date string, without a calendar
/// library: the store's dates are already `YYYY-MM-DD`, and a civil-day count is
/// arithmetic on that.
fn day_index(date: &str, monday: &str) -> Option<i64> {
    let (year, month, day) = parts(date)?;
    let (myear, mmonth, mday) = parts(monday)?;
    let days = civil(year, month, day) - civil(myear, mmonth, mday);
    (0..7).contains(&days).then_some(days)
}

fn monday_of(date: &str) -> String {
    let Some((year, month, day)) = parts(date) else {
        return date.to_string();
    };
    // 1970-01-01 was a Thursday, so day 0 of the civil count is Thursday.
    let weekday = (civil(year, month, day) + 3).rem_euclid(7);
    from_civil(civil(year, month, day) - weekday)
}

fn parts(date: &str) -> Option<(i64, i64, i64)> {
    Some((
        date.get(0..4)?.parse().ok()?,
        date.get(5..7)?.parse().ok()?,
        date.get(8..10)?.parse().ok()?,
    ))
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn from_civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    format!("{year:04}-{month:02}-{day:02}")
}

fn draw(width: u32, height: u32, week: &[&Event], monday: String) -> Vec<u8> {
    let mut canvas = vec![0u8; width as usize * height as usize * 4];
    for pixel in canvas.chunks_exact_mut(4) {
        pixel.copy_from_slice(&BACKGROUND);
    }
    let face = Face::regular();
    let strong = Face::medium();
    let ink = [31u8, 35, 40, 0xff];
    let muted = [110u8, 116, 128, 0xff];

    let gutter = 44u32;
    let header = 44u32;
    fill(&mut canvas, width, height, 0, 0, width, header, HEADER);
    let column_at = |day: u32| gutter + day * (width - gutter) / DAYS;

    // Day letters, one per column, centred.
    let initials = ["M", "T", "W", "T", "F", "S", "S"];
    for day in 0..DAYS {
        let left = column_at(day) as f32;
        let right = column_at(day + 1) as f32;
        let room = right - left;
        let letter = initials[day as usize];
        let advance = strong.width(letter, 15.0);
        strong.draw(
            &mut canvas,
            width,
            height,
            (left + (room - advance) / 2.0) as i32,
            14,
            15.0,
            ink,
            letter,
        );
    }

    let start_minute = 6 * 60;
    let area = height.saturating_sub(header + 8);
    // Hour labels every three hours, and the rules that line up with them.
    for mark in 0..7 {
        let hour = 6 + mark * 3;
        let y = header + (mark as u32 * 3) * area / 18;
        let label = format!("{hour:02}:00");
        face.draw(
            &mut canvas,
            width,
            height,
            6,
            y as i32 + 1,
            11.0,
            muted,
            &label,
        );
        fill(&mut canvas, width, height, gutter, y, width - gutter, 1, RULE);
    }
    for day in 0..DAYS {
        fill(&mut canvas, width, height, column_at(day), header, 1, area, RULE);
    }

    for event in week {
        let Some(day) = day_index(&event.day, &monday) else {
            continue;
        };
        let from = if event.all_day {
            start_minute
        } else {
            event.from.max(start_minute)
        };
        let to = if event.all_day {
            from + 60
        } else {
            event.to.min(start_minute + 18 * 60).max(from + 20)
        };
        let top = header + ((from - start_minute) * area as i64 / (18 * 60)) as u32;
        let bottom = header + ((to - start_minute) * area as i64 / (18 * 60)) as u32;
        let left = column_at(day as u32) + 2;
        let right = column_at(day as u32 + 1);
        let block_height = (bottom.saturating_sub(top)).max(14);
        let colour = calendar_colour(&event.calendar);
        fill(
            &mut canvas,
            width,
            height,
            left,
            top,
            (right.saturating_sub(4)).saturating_sub(left),
            block_height,
            colour,
        );
        // A block says what it is when there is room for a line of type.
        if block_height >= 15 {
            let room = (right.saturating_sub(left) as f32) - 10.0;
            face.draw_clipped(
                &mut canvas,
                width,
                height,
                left as i32 + 4,
                top as i32 + 3,
                10.5,
                room,
                [255, 255, 255, 0xff],
                &event.summary,
            );
        }
    }
    canvas
}

/// A stable colour per calendar: the name, hashed.
fn calendar_colour(name: &str) -> [u8; 4] {
    let mut hash: u32 = 2_166_136_261;
    for byte in name.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
    }
    let hue = (hash % 360) as f32;
    let (r, g, b) = hsv(hue, 0.55, 0.86);
    [r, g, b, 0xff]
}

fn hsv(hue: f32, saturation: f32, value: f32) -> (u8, u8, u8) {
    let c = value * saturation;
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs());
    let m = value - c;
    let (r, g, b) = match (hue / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

fn fill(canvas: &mut [u8], width: u32, height: u32, x0: u32, y0: u32, w: u32, h: u32, colour: [u8; 4]) {
    for y in y0..y0.saturating_add(h).min(height) {
        for x in x0..x0.saturating_add(w).min(width) {
            let at = ((y * width + x) as usize) * 4;
            canvas[at..at + 4].copy_from_slice(&colour);
        }
    }
}

struct Component;

impl Guest for Component {
    async fn run() -> u32 {
        match main() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("calendar-view: {error}");
                1
            }
        }
    }
}

export!(Component);
