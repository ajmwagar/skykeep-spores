//! Skykeep Calendar: one week of events, drawn by the Spore.
//!
//! The same shape as `photos-view`: read records, lay out, rasterize one frame,
//! hand it over through `soil:present/surface.present-rgba`. There is no font in
//! a software rasterizer, so a block is its own label for now — colour is the
//! calendar, height is the duration, and the hour grid says when.

wit_bindgen::generate!({ path: "wit", world: "runner", generate_all });

mod paint;
mod text;

use serde_json::Value;
use paint::{Rgba, fill as rect, mix, rounded};
use text::Face;
use soil::present::surface;
use soil::records::records;

const EVENT_SCHEMA: &str = "fungos.calendar/event";
const PAGE: u32 = 256;
const SCAN: usize = 100_000;
const DAYS: u32 = 7;
const _UNUSED_BACKGROUND: [u8; 4] = [250, 250, 252, 0xff];
const _UNUSED_RULE: [u8; 4] = [226, 229, 234, 0xff];
const _UNUSED_HEADER: [u8; 4] = [244, 246, 249, 0xff];

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

const SURFACE: Rgba = [245, 246, 248, 0xff];
const CARD: Rgba = [255, 255, 255, 0xff];
const BORDER: Rgba = [228, 231, 236, 0xff];
const INK: Rgba = [17, 24, 39, 0xff];
const MUTED: Rgba = [107, 114, 128, 0xff];
const FAINT: Rgba = [156, 163, 175, 0xff];
const GRID: Rgba = [238, 240, 243, 0xff];
const NOW: Rgba = [232, 72, 85, 0xff];

const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September",
    "October", "November", "December",
];

/// Where one event lands: its lane inside a day, and how many lanes that day
/// needs, so two things at the same hour sit beside each other rather than on
/// top of each other.
struct Slot<'a> {
    event: &'a Event,
    day: u32,
    from: i64,
    to: i64,
    lane: u32,
    lanes: u32,
}

fn draw(width: u32, height: u32, week: &[&Event], monday: String) -> Vec<u8> {
    let mut canvas = vec![0u8; width as usize * height as usize * 4];
    rect(&mut canvas, width, height, 0, 0, width as i32, height as i32, SURFACE);

    let face = Face::regular();
    let strong = Face::medium();

    // The card: a hairline border with a white face inside it.
    let (cx, cy) = (18.0, 16.0);
    let (cw, ch) = (width as f32 - 36.0, height as f32 - 32.0);
    rounded(&mut canvas, width, height, cx, cy, cw, ch, 16.0, BORDER);
    rounded(&mut canvas, width, height, cx + 1.0, cy + 1.0, cw - 2.0, ch - 2.0, 15.0, CARD);

    // ---- header -----------------------------------------------------------
    let monday_days = civil_of(&monday).unwrap_or(0);
    let sunday = from_civil(monday_days + 6);
    let _ = (&sunday, first_day(&monday), last_day_of(&sunday));
    let label = format!("{} {}", month_name(&monday), year(&monday));
    strong.draw(&mut canvas, width, height, (cx + 26.0) as i32, (cy + 16.0) as i32, 19.0, INK, &label);
    let count = format!("{} events", week.len());
    let advance = strong.width(&label, 19.0);
    face.draw(&mut canvas, width, height, (cx + 36.0 + advance) as i32, (cy + 23.0) as i32, 10.5, FAINT, &count);

    let gutter = 62.0;
    let inner = cx + 26.0;
    let column_width = (cw - 52.0 - gutter) / DAYS as f32;
    let column_at = |day: u32| inner + gutter + day as f32 * column_width;
    let weekdays = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"];

    for day in 0..DAYS {
        let x = column_at(day);
        let centre = x + column_width / 2.0;
        let letter = weekdays[day as usize];
        let advance = face.width(letter, 9.5);
        face.draw(&mut canvas, width, height, (centre - advance / 2.0) as i32, (cy + 44.0) as i32, 9.5, FAINT, letter);
        let number = day_number(&from_civil(monday_days + day as i64));
        let advance = strong.width(&number, 14.0);
        strong.draw(&mut canvas, width, height, (centre - advance / 2.0) as i32, (cy + 58.0) as i32, 14.0, INK, &number);
    }

    // ---- all-day band -----------------------------------------------------
    let all_day_top = cy + 84.0;
    let mut band = all_day_top;
    let all_day: Vec<&Event> = week.iter().copied().filter(|event| event.all_day).collect();
    if !all_day.is_empty() {
        for (index, event) in all_day.iter().enumerate() {
            let Some(day) = day_index(&event.day, &monday) else { continue };
            let x = column_at(day as u32) + 3.0;
            let y = band + index as f32 * 20.0;
            let accent = calendar_colour(&event.calendar);
            let chip = mix(CARD, accent, 0.18);
            rounded(&mut canvas, width, height, x, y, column_width - 6.0, 17.0, 5.0, chip);
            rounded(&mut canvas, width, height, x, y, 3.0, 17.0, 1.5, accent);
            face.draw_clipped(&mut canvas, width, height, (x + 7.0) as i32, (y + 3.0) as i32, 9.0,
                column_width - 20.0, INK, &event.summary);
        }
        band += all_day.len() as f32 * 20.0 + 6.0;
    }

    // ---- grid -------------------------------------------------------------
    let grid_top = (band + 6.0).max(cy + 108.0);
    let grid_bottom = cy + ch - 14.0;
    let start_minute = 6 * 60;
    let hours = 18.0;
    let hour_height = (grid_bottom - grid_top) / hours;
    for mark in 0..=18 {
        let y = grid_top + mark as f32 * hour_height;
        let width_of_rule = if mark % 3 == 0 { 1.0 } else { 1.0 };
        rect(&mut canvas, width, height, inner as i32, y as i32, (cw - 52.0) as i32, width_of_rule as i32,
            if mark % 3 == 0 { GRID } else { mix(CARD, GRID, 0.5) });
        if mark % 3 == 0 && mark < 18 {
            let label = format!("{:02}:00", 6 + mark);
            let advance = face.width(&label, 9.5);
            face.draw(&mut canvas, width, height, (inner + gutter - 12.0 - advance) as i32,
                (y - 5.0) as i32, 9.5, FAINT, &label);
        }
    }
    for day in 1..DAYS {
        let x = column_at(day);
        rect(&mut canvas, width, height, x as i32, (cy + 40.0) as i32, 1, (grid_bottom - cy - 40.0) as i32, GRID);
    }

    // ---- events -----------------------------------------------------------
    let mut slots: Vec<Slot> = Vec::new();
    for event in week {
        let Some(day) = day_index(&event.day, &monday) else { continue };
        let from = if event.all_day { start_minute } else { event.from.max(start_minute) };
        let to = if event.all_day { from } else { event.to.min(start_minute + 18 * 60).max(from + 20) };
        if event.all_day { continue }
        slots.push(Slot { event, day: day as u32, from, to, lane: 0, lanes: 1 });
    }
    // Lanes: within a connected run of overlapping events, the first free one.
    slots.sort_by_key(|slot| (slot.day, slot.from, slot.to));
    let mut index = 0;
    while index < slots.len() {
        let day = slots[index].day;
        let mut end = index;
        let mut group_end = slots[index].to;
        while end + 1 < slots.len() && slots[end + 1].day == day && slots[end + 1].from < group_end {
            end += 1;
            group_end = group_end.max(slots[end].to);
        }
        let mut lane_ends: Vec<i64> = Vec::new();
        for slot in slots[index..=end].iter_mut() {
            let lane = lane_ends.iter().position(|end| *end <= slot.from).unwrap_or(lane_ends.len());
            if lane == lane_ends.len() { lane_ends.push(slot.to) } else { lane_ends[lane] = slot.to }
            slot.lane = lane as u32;
        }
        let lanes = lane_ends.len() as u32;
        for slot in slots[index..=end].iter_mut() { slot.lanes = lanes }
        index = end + 1;
    }

    for slot in &slots {
        let lane_width = (column_width - 6.0) / slot.lanes as f32;
        let x = column_at(slot.day) + 3.0 + lane_width * slot.lane as f32;
        let top = grid_top + (slot.from - start_minute) as f32 / 60.0 * hour_height;
        let bottom = grid_top + (slot.to - start_minute) as f32 / 60.0 * hour_height;
        let height_of = (bottom - top).max(16.0);
        let accent = calendar_colour(&slot.event.calendar);
        let chip = mix(CARD, accent, 0.16);
        let w = (lane_width - 3.0).max(8.0);
        rounded(&mut canvas, width, height, x, top, w, height_of, 5.0, chip);
        rounded(&mut canvas, width, height, x, top, 2.5, height_of, 1.25, accent);
        let room = w - 12.0;
        face.draw_clipped(&mut canvas, width, height, (x + 7.0) as i32, (top + 3.0) as i32, 9.5, room, INK, &slot.event.summary);
        if height_of >= 30.0 {
            let time = format!("{:02}:{:02}", slot.from / 60, slot.from % 60);
            face.draw_clipped(&mut canvas, width, height, (x + 7.0) as i32, (top + 16.0) as i32, 8.5, room, MUTED, &time);
        }
    }
    canvas
}

fn civil_of(date: &str) -> Option<i64> {
    let (year, month, day) = parts(date)?;
    Some(civil(year, month, day))
}

fn month_name(date: &str) -> &'static str {
    date.get(5..7)
        .and_then(|month| month.parse::<usize>().ok())
        .and_then(|month| MONTHS.get(month.saturating_sub(1)))
        .copied()
        .unwrap_or("")
}

fn year(date: &str) -> String {
    date.get(0..4).unwrap_or_default().to_string()
}

fn first_day(date: &str) -> String {
    day_number(date)
}

fn last_day_of(date: &str) -> String {
    day_number(date)
}

fn day_number(date: &str) -> String {
    date.get(8..10)
        .and_then(|day| day.parse::<u32>().ok())
        .map(|day| day.to_string())
        .unwrap_or_default()
}

/// A stable colour per calendar: the name, hashed.
fn calendar_colour(name: &str) -> Rgba {
    let mut hash: u32 = 2_166_136_261;
    for byte in name.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(16_777_619);
    }
    let hue = (hash % 360) as f32;
    let (r, g, b) = hsv(hue, 0.62, 0.82);
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
