//! Skykeep Photos: the library iCloud filled in, as a fungOS Spore.
//!
//! Actions:
//!   view [<album-id>]   the library, or one album, by month
//!   albums              every album, with its count
//!   asset <id>          one photo, unabridged, with the content ids of its bytes
//!   help
//!
//! Three collections, because a photo is not a value:
//!
//! - `fungos.photos/asset@1` — one record per photo: when it was taken, what it
//!   is, and the content ids of its original, thumbnail and Live Photo part;
//! - `fungos.photos/album@1` — one record per album (or folder);
//! - `fungos.photos/album-item@1` — one record per membership, id `album/asset`.
//!
//! Membership is its own record on purpose. An album is edited far more often
//! than the photos in it are, and an album whose body held ten thousand ids
//! would be one record rewritten on every add — and a conflict between two
//! peers adding different photos to the same album.

wit_bindgen::generate!({ path: "wit", world: "photos", generate_all });

use serde_json::{Value, json};
use soil::records::records;

const ASSET_SCHEMA: &str = "fungos.photos/asset@1";
const ALBUM_SCHEMA: &str = "fungos.photos/album@1";
const ALBUM_ITEM_SCHEMA: &str = "fungos.photos/album-item@1";
const PAGE: u32 = 256;
/// Photos in one document. A month grid caps out; a library does not.
const MAX_ASSETS: usize = 2_400;
const MAX_ALBUMS: usize = 5_000;
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
            "albums" => print_json(&json!({ "albums": albums()? })),
            "asset" => {
                let id = args.get(1).ok_or("usage: asset <id>")?;
                let assets = assets()?;
                let asset = assets
                    .into_iter()
                    .find(|asset| asset.id == *id)
                    .ok_or_else(|| format!("no photo {id}"))?;
                print_json(&json!({ "asset": detail(&asset) }))
            }
            "help" => print_json(&json!({
                "actions": [
                    "view [<album-id>]   the library, or one album, by month",
                    "albums              every album, with its count",
                    "asset <id>          one photo, unabridged",
                    "help",
                ],
                "schemas": [ASSET_SCHEMA, ALBUM_SCHEMA, ALBUM_ITEM_SCHEMA],
                "writes": false,
            })),
            other => Err(format!("unknown action {other:?}; try help")),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The library, as records.

struct Asset {
    id: String,
    body: Value,
    taken_at: i64,
}

fn scan(schema: &str, max: usize) -> Result<Vec<(String, Value)>, String> {
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = records::scan(schema, cursor.as_deref(), PAGE)
            .map_err(|error| format!("scan {schema}: {error:?}"))?;
        for entry in page.entries {
            if entry.deleted || entry.schema != schema {
                continue;
            }
            if let Ok(body) = serde_json::from_slice::<Value>(&entry.body) {
                out.push((entry.key.id, body));
            }
        }
        if out.len() >= max {
            break;
        }
        match page.next {
            Some(next) if Some(next.as_str()) != cursor.as_deref() => cursor = Some(next),
            _ => break,
        }
    }
    Ok(out)
}

fn assets() -> Result<Vec<Asset>, String> {
    let mut assets: Vec<Asset> = scan(ASSET_SCHEMA, MAX_ASSETS)?
        .into_iter()
        .map(|(id, body)| {
            let taken_at = body.get("taken_at").and_then(Value::as_i64).unwrap_or(0);
            Asset { id, body, taken_at }
        })
        .collect();
    // Newest first, as Photos opens.
    assets.sort_by(|a, b| b.taken_at.cmp(&a.taken_at).then_with(|| a.id.cmp(&b.id)));
    assets.truncate(MAX_ASSETS);
    Ok(assets)
}

fn albums() -> Result<Vec<Value>, String> {
    let found = scan(ALBUM_SCHEMA, MAX_ALBUMS)?;
    let mut albums: Vec<(i64, Value)> = found
        .into_iter()
        .map(|(id, body)| {
            let position = body.get("position").and_then(Value::as_i64).unwrap_or(0);
            (
                position,
                json!({
                    "id": id,
                    "name": body.get("name").and_then(Value::as_str).unwrap_or_default(),
                    "folder": body.get("folder").and_then(Value::as_bool).unwrap_or(false),
                    "parent": body.get("parent").cloned().unwrap_or(Value::Null),
                    "count": body.get("count").and_then(Value::as_i64).unwrap_or(0),
                    "position": position,
                }),
            )
        })
        .collect();
    albums.sort_by_key(|(position, _)| *position);
    Ok(albums.into_iter().map(|(_, album)| album).collect())
}

/// Asset ids in one album, in the album's own order.
fn album_assets(album: &str) -> Result<Vec<String>, String> {
    let mut items: Vec<(i64, String)> = scan(ALBUM_ITEM_SCHEMA, MAX_ASSETS * 4)?
        .into_iter()
        .filter_map(|(_, body)| {
            let name = body
                .get("album")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if name != album {
                return None;
            }
            let asset = body.get("asset").and_then(Value::as_str)?;
            let position = body.get("position").and_then(Value::as_i64).unwrap_or(0);
            Some((position, asset.to_string()))
        })
        .collect();
    items.sort_by_key(|(position, _)| *position);
    Ok(items.into_iter().map(|(_, asset)| asset).collect())
}

// ---------------------------------------------------------------------------------------------
// The grid.

fn view(album: Option<&str>) -> Result<Value, String> {
    let albums = albums()?;
    let name = album.and_then(|id| {
        albums
            .iter()
            .find(|album| album["id"] == id)
            .and_then(|album| album["name"].as_str())
            .map(str::to_string)
    });
    let mut assets = assets()?;
    if let Some(id) = album {
        let order = album_assets(id)?;
        assets.retain(|asset| order.iter().any(|wanted| wanted == &asset.id));
        assets.sort_by_key(|asset| {
            order
                .iter()
                .position(|wanted| wanted == &asset.id)
                .unwrap_or(usize::MAX)
        });
    }

    // Grouped by the month it was taken, newest month first: the grid Photos
    // opens on, and the only grouping the records can answer without a clock.
    let mut months: Vec<(i64, String, Vec<Value>)> = Vec::new();
    for asset in &assets {
        let (month, label) = month_of(asset.taken_at);
        match months.iter_mut().find(|(key, _, _)| *key == month) {
            Some((_, _, list)) => list.push(tile(asset)),
            None => months.push((month, label, vec![tile(asset)])),
        }
    }

    Ok(json!({
        "source": "iCloud",
        "count": assets.len(),
        "album": album,
        "album_name": name,
        "albums": albums,
        "months": months
            .into_iter()
            .map(|(key, label, assets)| json!({ "key": key, "label": label, "assets": assets }))
            .collect::<Vec<_>>(),
    }))
}

fn tile(asset: &Asset) -> Value {
    let body = &asset.body;
    let width = body.get("width").and_then(Value::as_i64).unwrap_or(0);
    let height = body.get("height").and_then(Value::as_i64).unwrap_or(0);
    let aspect = if width > 0 && height > 0 {
        (width as f64 / height as f64 * 1000.0).round() / 1000.0
    } else {
        1.0
    };
    let thumb = body
        .get("thumb")
        .and_then(|thumb| thumb.get("content_id"))
        .and_then(Value::as_str);
    let content = body
        .get("content")
        .and_then(|content| content.get("content_id"))
        .and_then(Value::as_str);
    json!({
        "id": asset.id,
        "kind": body.get("kind").cloned().unwrap_or(json!("image")),
        "filename": body.get("filename").cloned().unwrap_or(Value::Null),
        "day": day_of(asset.taken_at),
        "taken_at": asset.taken_at,
        "aspect": aspect,
        "favorite": body.get("favorite").and_then(Value::as_bool).unwrap_or(false),
        "duration": body.get("duration").cloned().unwrap_or(Value::Null),
        "thumb": thumb,
        "content": content,
    })
}

fn detail(asset: &Asset) -> Value {
    let body = &asset.body;
    json!({
        "id": asset.id,
        "filename": body.get("filename").cloned().unwrap_or(Value::Null),
        "kind": body.get("kind").cloned().unwrap_or(json!("image")),
        "taken_at": asset.taken_at,
        "title": body.get("title").cloned().unwrap_or(Value::Null),
        "caption": body.get("caption").cloned().unwrap_or(Value::Null),
        "width": body.get("width").cloned().unwrap_or(Value::Null),
        "height": body.get("height").cloned().unwrap_or(Value::Null),
        "uti": body.get("uti").cloned().unwrap_or(Value::Null),
        "size": body.get("size").cloned().unwrap_or(Value::Null),
        "rating": body.get("rating").cloned().unwrap_or(Value::Null),
        "keywords": body.get("keywords").cloned().unwrap_or(json!([])),
        "latitude": body.get("latitude").cloned().unwrap_or(Value::Null),
        "longitude": body.get("longitude").cloned().unwrap_or(Value::Null),
        "favorite": body.get("favorite").and_then(Value::as_bool).unwrap_or(false),
        "hidden": body.get("hidden").and_then(Value::as_bool).unwrap_or(false),
        "content": body.get("content").cloned().unwrap_or(Value::Null),
        "thumb": body.get("thumb").cloned().unwrap_or(Value::Null),
        "live": body.get("live").cloned().unwrap_or(Value::Null),
    })
}

// ---------------------------------------------------------------------------------------------
// Dates, without a date library.

/// The month a unix timestamp falls in, as `(YYYYMM, "March 2026")`.
fn month_of(seconds: i64) -> (i64, String) {
    let (year, month, _) = civil_from_days(seconds.div_euclid(86_400));
    (
        year * 100 + i64::from(month),
        format!("{} {year}", MONTHS[(month - 1) as usize]),
    )
}

/// The day of a unix timestamp, as `12 Mar`.
fn day_of(seconds: i64) -> String {
    let (_, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let name = MONTHS[(month - 1) as usize];
    format!("{day} {}", name.get(..3).unwrap_or(name))
}

/// Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
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
    fn a_timestamp_names_its_month_and_day() {
        // 2026-03-20T09:46:40Z
        let seconds = 1_774_000_000;
        assert_eq!(month_of(seconds), (202603, "March 2026".to_string()));
        assert_eq!(day_of(seconds), "20 Mar");
    }

    #[test]
    fn a_timestamp_before_the_epoch_still_works() {
        // 1969-12-31T23:59:59Z
        assert_eq!(month_of(-1), (196912, "December 1969".to_string()));
        assert_eq!(day_of(-1), "31 Dec");
    }
}
