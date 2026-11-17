//! macOS as a source, not a login.
//!
//! The Mac is signed into iCloud in System Settings, so Contacts and Photos are already
//! mirrored to disk. Reading them is a local read of an undocumented schema — no Apple ID, no
//! SRP, no session — and it is the difference between "ingest your stuff" being a login
//! problem and being a file problem.
//!
//! The schema moves with every macOS release, so nothing here assumes a column exists: the
//! table is inspected once and a missing column reads as null. A missing *table* is the loud
//! failure — an address book we cannot read is not an empty address book.
//!
//! Photos are the same deal with one twist: the originals may not be here. A library on
//! Optimize Storage keeps every photo's *preview* on disk (about a megabyte each in
//! `resources/derivatives`) and the original in iCloud. So the record carries the preview's
//! content id, the original's size, and the CloudKit id to hydrate the rest from — see
//! `docs/photos-store.md`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

use crate::{
    asset_body, ingest, AssetDraft, Batch, Record, ALBUM_ITEM_SCHEMA, ALBUM_SCHEMA, ASSET_SCHEMA,
    CONTACT_SCHEMA, EVENT_SCHEMA,
};

/// The address book's SQLite file, by the name macOS has used for years.
const ADDRESS_BOOK: &str = "AddressBook-v22.abcddb";

/// A photo library's SQLite file, inside the `.photoslibrary` bundle.
const PHOTOS_DATABASE: &str = "database/Photos.sqlite";

/// Where a library keeps its previews.
const PREVIEWS: &str = "resources/derivatives";

/// What a photo pass writes and how hard it works.
pub struct PhotoOptions<'a> {
    pub book: &'a str,
    /// Hash every preview read. Off is a metadata-only pass.
    pub phash: bool,
}

impl Default for PhotoOptions<'_> {
    fn default() -> Self {
        Self {
            book: "photos",
            phash: true,
        }
    }
}

/// Read every person in a macOS address book directory.
pub fn contacts_from_address_book(dir: &Path, book: &str) -> Result<Vec<Record>> {
    let path = dir.join(ADDRESS_BOOK);
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open {} (read-only)", path.display()))?;

    let columns = columns(&conn, "ZABCDRECORD")?;
    if columns.is_empty() {
        anyhow::bail!(
            "{} has no ZABCDRECORD table: not an AddressBook database",
            path.display()
        );
    }

    // A column the running macOS does not have reads as null; the body keeps its shape
    // either way, because the contacts book validates every field.
    let pick = |name: &str| -> String {
        if columns.iter().any(|column| column == name) {
            name.to_string()
        } else {
            "NULL".to_string()
        }
    };
    // `ZUNIQUEID` is the address book's own stable id; without it a row number is
    // only unique inside this database, so it is namespaced to say so.
    let unique = if columns.iter().any(|column| column == "ZUNIQUEID") {
        "CAST(ZUNIQUEID AS TEXT)".to_string()
    } else {
        "'macos-' || CAST(Z_PK AS TEXT)".to_string()
    };
    let sql = format!(
        "SELECT Z_PK, {unique}, {}, {}, {}, {}, {}, {}, {} FROM ZABCDRECORD",
        pick("ZFIRSTNAME"),
        pick("ZMIDDLENAME"),
        pick("ZLASTNAME"),
        pick("ZORGANIZATION"),
        pick("ZJOBTITLE"),
        pick("ZNOTE"),
        pick("ZBIRTHDAY"),
    );

    let mut statement = conn.prepare(&sql).context("read ZABCDRECORD")?;
    let rows = statement
        .query_map([], |row| {
            Ok(Person {
                pk: row.get(0)?,
                uid: match row.get::<_, Option<String>>(1)? {
                    Some(uid) if !uid.is_empty() => uid,
                    _ => format!("macos-{}", row.get::<_, i64>(0)?),
                },
                first: row.get(2)?,
                middle: row.get(3)?,
                last: row.get(4)?,
                organization: row.get(5)?,
                title: row.get(6)?,
                note: row.get(7)?,
                birthday: row.get::<_, Option<f64>>(8)?.map(core_data_date),
            })
        })
        .context("walk ZABCDRECORD")?;

    let mut people = Vec::new();
    for person in rows {
        people.push(person?);
    }

    let emails = values(&conn, "ZABCDEMAILADDRESS", "ZADDRESS")?;
    let phones = values(&conn, "ZABCDPHONENUMBER", "ZFULLNUMBER")?;

    let mut records = Vec::with_capacity(people.len());
    for person in people {
        // A record with no name, no organization and no method is a shell — a group,
        // a leftover, something Contacts itself does not draw. It is not a person.
        let empty = [
            &person.first,
            &person.middle,
            &person.last,
            &person.organization,
        ]
        .into_iter()
        .all(|value| value.as_deref().unwrap_or_default().trim().is_empty());
        if empty {
            continue;
        }
        let email = emails
            .iter()
            .filter(|(owner, _)| *owner == person.pk)
            .map(|(_, value)| json!({ "label": "home", "value": value }))
            .collect::<Vec<_>>();
        let phone = phones
            .iter()
            .filter(|(owner, _)| *owner == person.pk)
            .map(|(_, value)| json!({ "label": "mobile", "value": value }))
            .collect::<Vec<_>>();
        records.push(Record {
            book: book.to_string(),
            id: person.uid.clone(),
            schema: CONTACT_SCHEMA.to_string(),
            body: body(&person, email, phone),
        });
    }
    records.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(records)
}

struct Person {
    pk: i64,
    uid: String,
    first: Option<String>,
    middle: Option<String>,
    last: Option<String>,
    organization: Option<String>,
    title: Option<String>,
    note: Option<String>,
    birthday: Option<String>,
}

// ---------------------------------------------------------------------------------------------
// Photos.

/// Read a `.photoslibrary` bundle: every live asset, its album membership, and the previews
/// this Mac already holds.
///
/// Read-only, and safe to run while Photos is open: the connection takes no write lock, and
/// the database is opened as it stands (not `immutable`) so a library mid-sync is read as of
/// its last commit rather than a stale page before the write-ahead log.
pub fn photos_from_library(library: &Path, options: &PhotoOptions) -> Result<Batch> {
    let path = library.join(PHOTOS_DATABASE);
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open {} (read-only)", path.display()))?;

    let columns = columns(&conn, "ZASSET")?;
    if columns.is_empty() {
        anyhow::bail!(
            "{} has no ZASSET table: not a Photos database",
            path.display()
        );
    }

    let assets = assets(&conn, &columns)?;
    let people = people(&conn)?;
    let previews = previews(&library.join(PREVIEWS));

    let mut batch = Batch::default();
    for asset in &assets {
        let preview = previews.get(&asset.uuid);
        let mut content = Vec::new();
        let thumb = match preview {
            Some(path) => Some(ingest(path, "image/jpeg", &mut content, options.phash)?),
            None => None,
        };
        // The hash describes the preview, not the asset: a video's poster frame is a picture
        // like any other, and hashing it is how a video is found from a still.
        let phash = thumb.as_ref().and_then(|thumb| thumb.phash.clone());
        let mut sources = vec![json!({
            "provider": "photos-library",
            "id": asset.uuid,
        })];
        if let Some(cloud) = &asset.cloud {
            sources.push(json!({ "provider": "icloud", "id": cloud }));
        }
        let body = asset_body(AssetDraft {
            filename: asset.filename.clone().filter(|name| !name.is_empty()),
            kind: if asset.kind == 1 { "video" } else { "image" },
            taken_at: asset.taken_at,
            added_at: asset.added_at,
            width: asset.width,
            height: asset.height,
            duration: if asset.kind == 1 {
                asset.duration.filter(|seconds| *seconds > 0.0)
            } else {
                // Photos writes 0 for a still; a still has no duration, it is not 0 seconds long.
                None
            },
            uti: asset.uti.clone(),
            size: asset.size,
            favorite: asset.favorite,
            hidden: asset.hidden,
            // Trashed assets are skipped rather than recorded as tombstones: a record for a
            // photo in Recently Deleted is a photo every reader has to learn to ignore.
            deleted: false,
            deleted_at: None,
            title: asset.title.clone(),
            caption: None,
            rating: 0,
            keywords: Vec::new(),
            people: people
                .get(&asset.pk)
                .map(|names| names.iter().cloned().collect())
                .unwrap_or_default(),
            latitude: asset.location.map(|(latitude, _)| latitude),
            longitude: asset.location.map(|(_, longitude)| longitude),
            phash,
            sources,
            // The original is in iCloud, not here. `size` is its size; `thumb` is what this
            // Mac can actually hand over.
            content: None,
            thumb: thumb.map(|thumb| thumb.value),
            live: None,
        });
        batch.records.push(Record {
            book: options.book.to_string(),
            id: asset.uuid.clone(),
            schema: ASSET_SCHEMA.to_string(),
            body,
        });
        batch.content.extend(content);
    }

    let (albums, items) = albums(&conn, &assets, options.book)?;
    batch.records.extend(albums);
    batch.records.extend(items);
    Ok(batch)
}

struct Asset {
    pk: i64,
    uuid: String,
    /// `0` image, `1` video.
    kind: i64,
    filename: Option<String>,
    uti: Option<String>,
    taken_at: i64,
    added_at: Option<i64>,
    width: i64,
    height: i64,
    duration: Option<f64>,
    size: Option<i64>,
    favorite: bool,
    hidden: bool,
    /// `(latitude, longitude)`, or nothing when Photos has no fix.
    location: Option<(f64, f64)>,
    title: Option<String>,
    cloud: Option<String>,
}

fn assets(conn: &Connection, asset_columns: &[String]) -> Result<Vec<Asset>> {
    let extra = columns(conn, "ZADDITIONALASSETATTRIBUTES")?;
    let joined = !extra.is_empty();
    let pick = |set: &[String], name: &str| -> String {
        if set.iter().any(|column| column == name) {
            name.to_string()
        } else {
            "NULL".to_string()
        }
    };
    let mut sql = String::from("SELECT a.Z_PK, ");
    for column in [
        "ZUUID",
        "ZFILENAME",
        "ZUNIFORMTYPEIDENTIFIER",
        "ZDATECREATED",
        "ZADDEDDATE",
        "ZWIDTH",
        "ZHEIGHT",
        "ZDURATION",
        "ZFAVORITE",
        "ZHIDDEN",
        "ZKIND",
        "ZLATITUDE",
        "ZLONGITUDE",
        "ZCLOUDASSETGUID",
    ] {
        sql.push_str(&match pick(asset_columns, column).as_str() {
            "NULL" => "NULL, ".to_string(),
            name => format!("a.{name}, "),
        });
    }
    for column in ["ZORIGINALFILESIZE", "ZTITLE", "ZORIGINALFILENAME"] {
        sql.push_str(&match (joined, pick(&extra, column).as_str()) {
            (true, name) if name != "NULL" => format!("aa.{name}, "),
            _ => "NULL, ".to_string(),
        });
    }
    sql.truncate(sql.len() - 2);
    sql.push_str(" FROM ZASSET a");
    if joined {
        sql.push_str(" LEFT JOIN ZADDITIONALASSETATTRIBUTES aa ON aa.ZASSET = a.Z_PK");
    }
    // A library from before Recently Deleted has no such column, and then every row is live.
    sql.push_str(match pick(asset_columns, "ZTRASHEDSTATE").as_str() {
        "NULL" => " WHERE 1",
        _ => " WHERE a.ZTRASHEDSTATE = 0",
    });

    let mut statement = conn.prepare(&sql).context("read ZASSET")?;
    let rows = statement
        .query_map([], |row| {
            Ok(Asset {
                pk: row.get(0)?,
                uuid: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                filename: row.get::<_, Option<String>>(17)?.or(row.get(2)?),
                uti: row.get(3)?,
                taken_at: row
                    .get::<_, Option<f64>>(4)?
                    .map(core_data_seconds)
                    .unwrap_or(0),
                added_at: row.get::<_, Option<f64>>(5)?.map(core_data_seconds),
                width: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                height: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                duration: row.get(8)?,
                favorite: row.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
                hidden: row.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0,
                kind: row.get::<_, Option<i64>>(11)?.unwrap_or(0),
                location: location(row.get(12)?, row.get(13)?),
                cloud: row
                    .get::<_, Option<String>>(14)?
                    .filter(|id| !id.is_empty()),
                size: row.get(15)?,
                title: row.get(16)?,
            })
        })
        .context("walk ZASSET")?;
    let mut out = Vec::new();
    for asset in rows {
        let asset = asset?;
        // An asset with no uuid cannot be addressed, and one with no date cannot be placed.
        if asset.uuid.is_empty() {
            continue;
        }
        out.push(asset);
    }
    out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    Ok(out)
}

/// Named people per asset, from the face clustering Photos already did.
///
/// This is the one part of Photos' analysis that is both readable and worth having: Apple
/// stores the name a person was given, on the asset the face was detected in. Scene labels
/// arrive as opaque numeric codes with no name table, and the OCR text is a compressed
/// blob in Apple's private document format — see `docs/photos-store.md` for both.
///
/// Absent tables are an empty map, not a failure: a macOS that stopped writing faces has
/// lost an extra, not the library.
fn people(conn: &Connection) -> Result<BTreeMap<i64, BTreeSet<String>>> {
    let mut out = BTreeMap::new();
    if columns(conn, "ZDETECTEDFACE")?.is_empty() || columns(conn, "ZPERSON")?.is_empty() {
        return Ok(out);
    }
    let mut statement = conn
        .prepare(
            "SELECT f.ZASSETFORFACE, p.ZDISPLAYNAME FROM ZDETECTEDFACE f \
             JOIN ZPERSON p ON p.Z_PK = f.ZPERSONFORFACE \
             WHERE p.ZDISPLAYNAME IS NOT NULL AND p.ZDISPLAYNAME != ''",
        )
        .context("read ZDETECTEDFACE")?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, String>(1)?))
        })
        .context("walk ZDETECTEDFACE")?;
    for row in rows {
        let (owner, name) = row?;
        if let Some(owner) = owner {
            out.entry(owner).or_insert_with(BTreeSet::new).insert(name);
        }
    }
    Ok(out)
}

/// Albums a person made, and what is in them.
///
/// A row in `ZGENERICALBUM` is not necessarily an album: system collections (Screenshots,
/// Selfies, People) and folders are rows too, and they are the ones without a title. So the
/// rule is "titled, live, and holding at least one asset" — a folder holds none.
fn albums(conn: &Connection, assets: &[Asset], book: &str) -> Result<(Vec<Record>, Vec<Record>)> {
    let mut records = Vec::new();
    let mut items = Vec::new();
    if columns(conn, "ZGENERICALBUM")?.is_empty() {
        return Ok((records, items));
    }
    if columns(conn, "Z_30ASSETS")?.is_empty() {
        anyhow::bail!("Photos.sqlite has ZGENERICALBUM but no Z_30ASSETS: album membership moved");
    }

    let by_pk: BTreeMap<i64, &Asset> = assets.iter().map(|asset| (asset.pk, asset)).collect();
    let mut membership: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    let mut statement = conn
        .prepare("SELECT Z_30ALBUMS, Z_3ASSETS FROM Z_30ASSETS")
        .context("read Z_30ASSETS")?;
    let rows = statement
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
        .context("walk Z_30ASSETS")?;
    for row in rows {
        let (album, asset) = row?;
        if by_pk.contains_key(&asset) {
            membership.entry(album).or_default().push(asset);
        }
    }

    let mut statement = conn
        .prepare(
            "SELECT Z_PK, ZUUID, ZTITLE FROM ZGENERICALBUM \
             WHERE ZTRASHEDSTATE = 0 AND ZTITLE IS NOT NULL AND ZTITLE != ''",
        )
        .context("read ZGENERICALBUM")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, String>(2)?,
            ))
        })
        .context("walk ZGENERICALBUM")?;
    let mut found: Vec<(String, String, Vec<i64>)> = Vec::new();
    for row in rows {
        let (pk, uuid, title) = row?;
        if uuid.is_empty() {
            continue;
        }
        let mut members = membership.remove(&pk).unwrap_or_default();
        if members.is_empty() {
            continue;
        }
        // Newest first, the order the library shows an album in.
        members.sort_by(|a, b| match (by_pk.get(a), by_pk.get(b)) {
            (Some(left), Some(right)) => right
                .taken_at
                .cmp(&left.taken_at)
                .then_with(|| left.uuid.cmp(&right.uuid)),
            _ => std::cmp::Ordering::Equal,
        });
        found.push((uuid, title, members));
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));

    // A folder holding albums has no assets of its own, so nothing here can be a parent yet:
    // `folder` is false and `parent` is null until folders are records too.
    for (position, (uuid, title, members)) in found.iter().enumerate() {
        records.push(Record {
            book: book.to_string(),
            id: uuid.clone(),
            schema: ALBUM_SCHEMA.to_string(),
            body: json!({
                "name": title,
                "position": position as i64,
                "folder": false,
                "parent": Value::Null,
                "count": members.len(),
            }),
        });
        for (position, member) in members.iter().enumerate() {
            let Some(asset) = by_pk.get(member) else {
                continue;
            };
            items.push(Record {
                book: book.to_string(),
                id: format!("{uuid}/{}", asset.uuid),
                schema: ALBUM_ITEM_SCHEMA.to_string(),
                body: json!({
                    "album": uuid,
                    "asset": asset.uuid,
                    "position": position,
                }),
            });
        }
    }
    Ok((records, items))
}

/// Every preview on disk, keyed by the asset uuid its filename starts with.
///
/// A library holds more than one preview per asset — a screen-sized `…_1_105_c.jpeg` and a
/// small `…_4_5005_c.jpeg`, the second under `masters/` — so the larger one wins. Reading the
/// folder once is also what keeps this cheap: the alternative is a stat per asset, and a
/// library has tens of thousands.
fn previews(root: &Path) -> BTreeMap<String, PathBuf> {
    let mut out: BTreeMap<String, (u8, PathBuf)> = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jpeg") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Some((uuid, _rest)) = stem.split_once('_') else {
                continue;
            };
            if uuid.len() != 36 {
                continue;
            }
            let rank = if stem.contains("_1_105_c") { 2 } else { 1 };
            match out.get(uuid) {
                Some((best, _)) if *best >= rank => {}
                _ => {
                    out.insert(uuid.to_string(), (rank, path));
                }
            }
        }
    }
    out.into_iter()
        .map(|(uuid, (_, path))| (uuid, path))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Calendars.

/// Calendar.app's EventKit store, inside its group container.
const CALENDAR_DATABASE: &str = "Calendar.sqlitedb";

/// Read every event in a Calendar store, as `fungos.calendar/event` records.
///
/// `dir` is the container `Calendar.sqlitedb` lives in, or the database itself.
/// The store is the same kind of undocumented Core Data schema as the address
/// book and Photos: every column is looked up before it is used, a missing one
/// reads as null, and a missing `CalendarItem` table is the loud failure.
///
/// Two conversions matter. Core Data counts seconds from 2001-01-01, and
/// `start_tz` is the zone the event was entered in, which is what makes an
/// all-day event land on the right day rather than the day before in UTC.
pub fn events_from_calendar(dir: &Path, book: &str) -> Result<Vec<Record>> {
    let path = if dir.is_dir() {
        dir.join(CALENDAR_DATABASE)
    } else {
        dir.to_path_buf()
    };
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| format!("open {} (read-only)", path.display()))?;

    let item_columns = columns(&conn, "CalendarItem")?;
    if item_columns.is_empty() {
        anyhow::bail!(
            "{} has no CalendarItem table: not a Calendar database",
            path.display()
        );
    }
    let pick = |name: &str| -> String {
        if item_columns.iter().any(|column| column == name) {
            name.to_string()
        } else {
            "NULL".to_string()
        }
    };

    // The calendar an event belongs to is its category, and a title is the only
    // thing a reader can show without its own list of colours.
    let mut titles: BTreeMap<i64, String> = BTreeMap::new();
    if !columns(&conn, "Calendar")?.is_empty() {
        let mut statement = conn
            .prepare("SELECT ROWID, title FROM Calendar")
            .context("read Calendar")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        for (id, title) in rows {
            titles.insert(id, title.unwrap_or_default());
        }
    }

    let sql = format!(
        "SELECT ROWID, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {} FROM CalendarItem",
        pick("summary"),
        pick("description"),
        pick("start_date"),
        pick("end_date"),
        pick("all_day"),
        pick("start_tz"),
        pick("url"),
        pick("sequence_num"),
        pick("creation_date"),
        pick("last_modified"),
        pick("external_id"),
        pick("calendar_id"),
    );
    let mut statement = conn.prepare(&sql).context("read CalendarItem")?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<f64>>(3)?,
                row.get::<_, Option<f64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, Option<i64>>(8)?,
                row.get::<_, Option<f64>>(9)?,
                row.get::<_, Option<f64>>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<i64>>(12)?,
            ))
        })
        .context("walk CalendarItem")?;

    let mut out = Vec::new();
    for row in rows {
        let (
            rowid,
            summary,
            description,
            start,
            end,
            all_day,
            timezone,
            url,
            sequence,
            created,
            modified,
            external,
            calendar,
        ) = row?;
        // An event with no start cannot be placed on a calendar or in a window.
        let Some(start) = start else {
            continue;
        };
        // A CalDAV path ends in the event's UUID, which is the identity a
        // second reader of the same account would use too. Without one, a row
        // number is only unique inside this database, so it says so.
        let uid = external
            .as_deref()
            .and_then(|path| path.rsplit('/').next())
            .map(|name| name.trim_end_matches(".ics"))
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("macos-{rowid}"));
        let category = calendar
            .and_then(|id| titles.get(&id).cloned())
            .unwrap_or_default();
        out.push(Record {
            book: book.to_string(),
            id: uid.clone(),
            schema: EVENT_SCHEMA.to_string(),
            body: json!({
                "summary": summary.unwrap_or_default(),
                "description": description.unwrap_or_default(),
                "location": "",
                "status": "",
                "uid": uid,
                "start": instant(start),
                "end": end.map(instant).unwrap_or_default(),
                "duration": "",
                "all_day": all_day.unwrap_or(0) != 0,
                "timezone": timezone.unwrap_or_default(),
                "rrule": "",
                "categories": if category.is_empty() { Vec::<String>::new() } else { vec![category] },
                "organizer": { "name": "", "value": "" },
                "attendees": Vec::<Value>::new(),
                "url": url.unwrap_or_default(),
                "created": created.map(instant).unwrap_or_default(),
                "last_modified": modified.map(instant).unwrap_or_default(),
                "sequence": sequence.map(|value| value.to_string()).unwrap_or_default(),
                "raw_ics": "",
                "raw_timezones": Vec::<String>::new(),
            }),
        });
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Core Data seconds as the RFC 3339 UTC string an iCalendar body carries.
fn instant(seconds: f64) -> String {
    jiff::Timestamp::from_second(core_data_seconds(seconds))
        .map(|timestamp| timestamp.to_string())
        .unwrap_or_default()
}

/// Core Data counts seconds from 2001-01-01.
fn core_data_seconds(seconds: f64) -> i64 {
    (seconds + 978_307_200.0) as i64
}

/// Photos writes "no location" as a point below the map, not as null.
///
/// Without this every asset with no GPS fix pins itself to the South Pacific and a map shows
/// the whole library taken on one empty ocean. A location is a point, so it is read as one:
/// one implausible half means there is no point.
fn location(latitude: Option<f64>, longitude: Option<f64>) -> Option<(f64, f64)> {
    match (latitude, longitude) {
        (Some(latitude), Some(longitude)) if latitude.abs() <= 90.0 && longitude.abs() <= 180.0 => {
            Some((latitude, longitude))
        }
        _ => None,
    }
}

fn body(person: &Person, emails: Vec<Value>, phones: Vec<Value>) -> Value {
    let display = [
        person.first.as_deref(),
        person.middle.as_deref(),
        person.last.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let display = if display.trim().is_empty() {
        person
            .organization
            .clone()
            .unwrap_or_else(|| "No Name".to_string())
    } else {
        display
    };
    // Every field the contacts book validates, present even when empty: a body that loses a
    // key is a record the store refuses.
    json!({
        "display_name": display,
        "given_name": person.first.clone().unwrap_or_default(),
        "family_name": person.last.clone().unwrap_or_default(),
        "organization": person.organization.clone().unwrap_or_default(),
        "title": person.title.clone().unwrap_or_default(),
        "emails": emails,
        "phones": phones,
        "addresses": Vec::<Value>::new(),
        "urls": Vec::<Value>::new(),
        "note": person.note.clone().unwrap_or_default(),
        "birthday": person.birthday.clone().unwrap_or_default(),
        "uid": person.uid,
        "created": "",
        "last_modified": "",
        "raw_vcard": "",
    })
}

/// Core Data counts seconds from 2001-01-01, and a birthday is a date, not an instant.
fn core_data_date(seconds: f64) -> String {
    let unix = seconds + 978_307_200.0;
    match jiff::Timestamp::from_second(unix as i64) {
        Ok(timestamp) => timestamp
            .to_zoned(jiff::tz::TimeZone::UTC)
            .date()
            .to_string(),
        Err(_) => String::new(),
    }
}

fn columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .with_context(|| format!("inspect {table}"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(names)
}

/// `(owner, value)` for one multi-value table, or nothing when macOS renamed it.
fn values(conn: &Connection, table: &str, column: &str) -> Result<Vec<(i64, String)>> {
    if columns(conn, table)?.is_empty() {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare(&format!(
        "SELECT ZOWNER, {column} FROM {table} WHERE ZOWNER IS NOT NULL AND {column} IS NOT NULL AND {column} != ''"
    ))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_table_is_a_loud_failure_not_an_empty_book() {
        let dir = tempfile::tempdir().unwrap();
        Connection::open(dir.path().join(ADDRESS_BOOK)).unwrap();
        let error = contacts_from_address_book(dir.path(), "contacts").unwrap_err();
        assert!(format!("{error}").contains("no ZABCDRECORD"), "{error}");
    }

    #[test]
    fn a_column_this_macos_does_not_have_reads_as_null() {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join(ADDRESS_BOOK)).unwrap();
        conn.execute_batch(
            "CREATE TABLE ZABCDRECORD (Z_PK INTEGER PRIMARY KEY, ZFIRSTNAME TEXT, ZLASTNAME TEXT);
             INSERT INTO ZABCDRECORD (Z_PK, ZFIRSTNAME, ZLASTNAME) VALUES (1, 'Ada', 'Lovelace');",
        )
        .unwrap();
        drop(conn);
        let records = contacts_from_address_book(dir.path(), "contacts").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].book, "contacts");
        assert_eq!(records[0].body["display_name"], "Ada Lovelace");
        assert_eq!(records[0].body["emails"], json!([]));
        assert_eq!(records[0].id, "macos-1");
    }

    #[test]
    fn a_core_data_birthday_is_a_date() {
        // 1985-01-05T00:00:00Z, counted from Core Data's 2001 epoch.
        assert_eq!(core_data_date(473_731_200.0 - 978_307_200.0), "1985-01-05");
    }

    // -----------------------------------------------------------------------------------------
    // Photos.

    /// A preview small enough to check in, real enough to decode.
    const PREVIEW: &[u8] = include_bytes!("../tests/fixtures/preview.jpeg");

    const SCHEMA: &str = "
        CREATE TABLE ZASSET (
            Z_PK INTEGER PRIMARY KEY, ZUUID TEXT, ZFILENAME TEXT, ZUNIFORMTYPEIDENTIFIER TEXT,
            ZDATECREATED REAL, ZADDEDDATE REAL, ZWIDTH INTEGER, ZHEIGHT INTEGER, ZDURATION REAL,
            ZFAVORITE INTEGER, ZHIDDEN INTEGER, ZKIND INTEGER, ZTRASHEDSTATE INTEGER,
            ZLATITUDE REAL, ZLONGITUDE REAL, ZCLOUDASSETGUID TEXT
        );
        CREATE TABLE ZADDITIONALASSETATTRIBUTES (
            Z_PK INTEGER PRIMARY KEY, ZASSET INTEGER, ZORIGINALFILESIZE INTEGER, ZTITLE TEXT,
            ZORIGINALFILENAME TEXT
        );
        CREATE TABLE ZDETECTEDFACE (ZASSETFORFACE INTEGER, ZPERSONFORFACE INTEGER);
        CREATE TABLE ZPERSON (Z_PK INTEGER PRIMARY KEY, ZDISPLAYNAME TEXT);
        CREATE TABLE ZGENERICALBUM (
            Z_PK INTEGER PRIMARY KEY, ZUUID TEXT, ZTITLE TEXT, ZTRASHEDSTATE INTEGER
        );
        CREATE TABLE Z_30ASSETS (Z_30ALBUMS INTEGER, Z_3ASSETS INTEGER);
    ";

    const FIRST: &str = "CC62DEDB-B772-4AA1-88AE-347D6FF9B6FD";
    const VIDEO: &str = "742D2900-8123-478C-B075-51E403CF43EB";
    const GONE: &str = "99F5C541-74FA-4CD5-95CE-77E76F72C624";

    /// A `.photoslibrary` shaped like the one on a Mac, with two live assets in one album.
    fn a_library() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("database")).unwrap();
        let conn = Connection::open(dir.path().join(PHOTOS_DATABASE)).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let insert = |pk: i64, uuid: &str, kind: i64, trashed: i64, lat: f64, lon: f64| {
            format!(
                "INSERT INTO ZASSET (Z_PK, ZUUID, ZFILENAME, ZUNIFORMTYPEIDENTIFIER, ZDATECREATED,
                     ZADDEDDATE, ZWIDTH, ZHEIGHT, ZDURATION, ZFAVORITE, ZHIDDEN, ZKIND,
                     ZTRASHEDSTATE, ZLATITUDE, ZLONGITUDE, ZCLOUDASSETGUID)
                 VALUES ({pk}, '{uuid}', '{uuid}.heic', 'public.heic', {pk}, {pk}, 4032, 3024,
                     5.5, 1, 0, {kind}, {trashed}, {lat}, {lon}, '{uuid}');
                 INSERT INTO ZADDITIONALASSETATTRIBUTES
                     (ZASSET, ZORIGINALFILESIZE, ZTITLE, ZORIGINALFILENAME)
                 VALUES ({pk}, 1752491, 'Summer', 'IMG_4193.HEIC');"
            )
        };
        // Photo, a trashed photo, and a video — the video taken later, so it sorts first.
        conn.execute_batch(&insert(1, FIRST, 0, 0, -180.0, -180.0))
            .unwrap();
        conn.execute_batch(&insert(2, GONE, 0, 1, -180.0, -180.0))
            .unwrap();
        conn.execute_batch(&insert(3, VIDEO, 1, 0, 48.8138, -122.1319))
            .unwrap();
        conn.execute_batch(
            "INSERT INTO ZPERSON VALUES (1, 'Ada');
             INSERT INTO ZPERSON VALUES (2, 'Grace');
             INSERT INTO ZDETECTEDFACE VALUES (1, 1);
             INSERT INTO ZDETECTEDFACE VALUES (3, 1);
             INSERT INTO ZDETECTEDFACE VALUES (3, 2);
             INSERT INTO ZGENERICALBUM VALUES (7, 'ALBUM-BOAT', 'Boat', 0);
             INSERT INTO ZGENERICALBUM VALUES (8, 'ALBUM-SYSTEM', '', 0);
             INSERT INTO ZGENERICALBUM VALUES (9, 'ALBUM-EMPTY', 'San Juan Islands', 0);
             INSERT INTO Z_30ASSETS VALUES (7, 1);
             INSERT INTO Z_30ASSETS VALUES (7, 3);
             INSERT INTO Z_30ASSETS VALUES (8, 1);",
        )
        .unwrap();

        // Two previews for the first photo, one for the video; the screen-sized one wins.
        let previews = dir.path().join(PREVIEWS).join("C");
        std::fs::create_dir_all(&previews).unwrap();
        std::fs::write(previews.join(format!("{FIRST}_1_105_c.jpeg")), PREVIEW).unwrap();
        std::fs::write(previews.join(format!("{FIRST}_4_5005_c.jpeg")), b"smaller").unwrap();
        std::fs::create_dir_all(dir.path().join(PREVIEWS).join("masters/7")).unwrap();
        std::fs::write(
            dir.path()
                .join(PREVIEWS)
                .join("masters/7")
                .join(format!("{VIDEO}_4_5005_c.jpeg")),
            PREVIEW,
        )
        .unwrap();
        dir
    }

    fn read(dir: &Path) -> Batch {
        photos_from_library(dir, &PhotoOptions::default()).unwrap()
    }

    #[test]
    fn an_asset_is_a_record_pointing_at_its_preview() {
        let dir = a_library();
        let batch = read(dir.path());
        let photo = batch
            .records
            .iter()
            .find(|record| record.id == FIRST)
            .expect("the photo is a record");
        assert_eq!(photo.schema, ASSET_SCHEMA);
        assert_eq!(photo.book, "photos");
        assert_eq!(photo.body["filename"], "IMG_4193.HEIC");
        assert_eq!(photo.body["kind"], "image");
        assert_eq!(photo.body["taken_at"], 978_307_201i64);
        assert_eq!(photo.body["size"], 1_752_491);
        assert_eq!(photo.body["favorite"], true);
        // A still has no duration, not a duration of zero.
        assert_eq!(photo.body["duration"], Value::Null);
        assert_eq!(photo.body["title"], "Summer");
        assert_eq!(photo.body["people"], json!(["Ada"]));
        // The original is in iCloud: what this Mac hands over is the preview.
        assert_eq!(photo.body["content"], Value::Null);
        assert_eq!(photo.body["thumb"]["media_type"], "image/jpeg");
        // The screen-sized preview, not the small one beside it.
        let expected = format!("blake3:{}", blake3::hash(PREVIEW).to_hex());
        assert_eq!(photo.body["thumb"]["content_id"], expected);
        let hash = photo.body["phash"].as_str().unwrap();
        assert!(hash.starts_with("dhash64:"), "{hash}");
        assert_eq!(photo.body["sources"][0]["provider"], "photos-library");
        assert_eq!(photo.body["sources"][1]["provider"], "icloud");
        // Photos writes "no location" as a point below the map, and it stays empty.
        assert_eq!(photo.body["latitude"], Value::Null);
        assert_eq!(photo.body["longitude"], Value::Null);
    }

    #[test]
    fn the_trashed_asset_is_not_a_record() {
        let batch = read(a_library().path());
        assert!(!batch.records.iter().any(|record| record.id == GONE));
    }

    #[test]
    fn a_titled_album_is_a_record_and_an_untitled_one_is_not() {
        let batch = read(a_library().path());
        let album = batch
            .records
            .iter()
            .find(|record| record.schema == ALBUM_SCHEMA)
            .expect("one album");
        assert_eq!(album.id, "ALBUM-BOAT");
        assert_eq!(album.body["name"], "Boat");
        // Titled but holding no assets: a folder, or a smart album — not an album yet.
        assert!(!batch
            .records
            .iter()
            .any(|record| record.id == "ALBUM-EMPTY"));
        assert!(!batch
            .records
            .iter()
            .any(|record| record.id == "ALBUM-SYSTEM"));
        // Membership is its own record, newest first.
        let items: Vec<_> = batch
            .records
            .iter()
            .filter(|record| record.schema == ALBUM_ITEM_SCHEMA)
            .collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].body["asset"], VIDEO);
        assert_eq!(items[1].body["asset"], FIRST);
        assert_eq!(album.body["count"], 2);
    }

    #[test]
    fn a_video_is_a_video() {
        let batch = read(a_library().path());
        let video = batch
            .records
            .iter()
            .find(|record| record.id == VIDEO)
            .unwrap();
        assert_eq!(video.body["kind"], "video");
        assert_eq!(video.body["duration"], 5.5);
        assert_eq!(video.body["people"], json!(["Ada", "Grace"]));
        assert_eq!(video.body["latitude"], 48.8138);
        // The preview is a picture, so it is hashed even for a video.
        assert!(video.body["phash"]
            .as_str()
            .unwrap()
            .starts_with("dhash64:"));
    }

    #[test]
    fn a_metadata_pass_reads_no_pictures() {
        let dir = a_library();
        let batch = photos_from_library(
            dir.path(),
            &PhotoOptions {
                book: "photos",
                phash: false,
            },
        )
        .unwrap();
        assert!(batch
            .records
            .iter()
            .all(|record| record.body["phash"] == Value::Null));
        // The content ids are still there: hashing bytes is not the expensive part.
        assert_eq!(batch.content.len(), 2);
    }

    #[test]
    fn a_database_without_assets_is_a_loud_failure() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("database")).unwrap();
        Connection::open(dir.path().join(PHOTOS_DATABASE)).unwrap();
        let error = photos_from_library(dir.path(), &PhotoOptions::default()).unwrap_err();
        assert!(format!("{error}").contains("no ZASSET"), "{error}");
    }

    #[test]
    fn a_column_this_photos_does_not_have_reads_as_null() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("database")).unwrap();
        let conn = Connection::open(dir.path().join(PHOTOS_DATABASE)).unwrap();
        conn.execute_batch(
            "CREATE TABLE ZASSET (Z_PK INTEGER PRIMARY KEY, ZUUID TEXT, ZDATECREATED REAL);
             INSERT INTO ZASSET VALUES (1, 'ONLY-A-UUID', 0);",
        )
        .unwrap();
        drop(conn);
        let batch = photos_from_library(dir.path(), &PhotoOptions::default()).unwrap();
        assert_eq!(batch.records.len(), 1);
        assert_eq!(batch.records[0].body["width"], 0);
        assert_eq!(batch.records[0].body["filename"], Value::Null);
        assert!(batch.content.is_empty());
    }
}
