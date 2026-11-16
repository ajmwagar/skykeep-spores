//! Skykeep's iCloud library, as soil records.
//!
//! Skykeep already syncs iCloud: `quince-auth` signs in, `quince-icloud` speaks each Apple
//! service, `quince-sync` walks it, and `quince-core` stores the result in one SQLite library.
//! What it has never had is a second reader. This crate is that seam.
//!
//! It reads the library and writes two things a fungOS host already understands:
//!
//! - **records**, in the shape `soil-records-ingest` accepts (`POST /api/records/batch`), so
//!   `soil:records` owns durability, revisions and peer sync;
//! - **content**, a manifest of the bytes a record points at — a photo's original, its
//!   thumbnail, a Live Photo's moving part — keyed by the same whole-content identity
//!   `soil:vfs` uses (`blake3:<hex>`).
//!
//! The split matters. A record is small, versioned, and syncs by value; a photo is large,
//! immutable, and syncs by reference. Putting bytes in a record body would make every sync
//! pass carry a library.
//!
//! `docs/photos-store.md` is the contract [`asset_body`] implements, and the reasons a photo
//! record looks the way it does.

pub mod macos;
pub mod phash;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use quince_core::photos::{Kind, Photo, Source, View};
use quince_core::Library;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Collections this bridge writes, and who owns each schema.
///
/// The first two already exist in `fungos-records` and are shared with every other writer:
/// a contact this bridge writes opens in the contacts Spore, and one the contacts Spore
/// writes is readable here. `fungos.photos/*` is ours: no records crate has a photo schema,
/// and a photo cannot be a value in one anyway (see [`Content`]).
pub const CONTACT_SCHEMA: &str = "fungos.contacts/contact";
pub const EVENT_SCHEMA: &str = "fungos.calendar/event";
pub const ASSET_SCHEMA: &str = "fungos.photos/asset@1";
pub const ALBUM_SCHEMA: &str = "fungos.photos/album@1";
pub const ALBUM_ITEM_SCHEMA: &str = "fungos.photos/album-item@1";

/// One record document for `soil-records-ingest`.
///
/// `book` is repeated per record because that is the ingest contract; a batch is a list of
/// them, so a caller may send one or all.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Record {
    pub book: String,
    pub id: String,
    pub schema: String,
    pub body: Value,
}

/// One blob a record body points at, and where its bytes are on this machine.
///
/// `content_id` is the whole-content identity, not a locator: the host's content store
/// decides where to keep it and `soil:vfs` decides how to serve it. `path` is only the
/// ingress side of the seam — it is read once, hashed, and never travels with the record.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Content {
    pub content_id: String,
    pub path: String,
    pub size: u64,
    pub media_type: String,
}

/// What one export found.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Batch {
    pub records: Vec<Record>,
    #[serde(default)]
    pub content: Vec<Content>,
}

/// Every field a [`ASSET_SCHEMA`] body carries, before it is one JSON document.
///
/// Two sources project into this and nothing else does, so the macOS library and Skykeep's
/// library produce the same record shape: a photos Spore does not learn which one wrote a
/// record, and a field added here is added for both at once. Fields a source does not know
/// are empty — never absent, because a record that loses a key is a record every reader has
/// to special-case.
pub struct AssetDraft {
    pub filename: Option<String>,
    /// `image`, `video` or `live`.
    pub kind: &'static str,
    /// Unix seconds; `0` when the source does not know.
    pub taken_at: i64,
    pub added_at: Option<i64>,
    pub width: i64,
    pub height: i64,
    /// Seconds, for a video.
    pub duration: Option<f64>,
    pub uti: Option<String>,
    /// The original's size in bytes.
    pub size: Option<i64>,
    pub favorite: bool,
    pub hidden: bool,
    pub deleted: bool,
    pub deleted_at: Option<i64>,
    pub title: Option<String>,
    pub caption: Option<String>,
    pub rating: i64,
    pub keywords: Vec<String>,
    /// People the source already named, for free: Apple clusters faces on device.
    pub people: Vec<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    /// `dhash64:<hex>`, when a picture was read.
    pub phash: Option<String>,
    /// Where this asset can be hydrated from, one entry per backend.
    pub sources: Vec<Value>,
    pub content: Option<Value>,
    pub thumb: Option<Value>,
    pub live: Option<Value>,
}

/// One asset record body.
///
/// `content`, `thumb` and `live` are `{content_id, size, media_type}` or null; `sources` says
/// where the bytes may be fetched when they are not here. Both are needed: content ids are
/// identity and travel, source locators are host state and do not.
pub fn asset_body(asset: AssetDraft) -> Value {
    json!({
        "filename": asset.filename,
        "kind": asset.kind,
        "taken_at": asset.taken_at,
        "added_at": asset.added_at,
        "width": asset.width,
        "height": asset.height,
        "duration": asset.duration,
        "uti": asset.uti,
        "size": asset.size,
        "favorite": asset.favorite,
        "hidden": asset.hidden,
        "deleted": asset.deleted,
        "deleted_at": asset.deleted_at,
        "title": asset.title,
        "caption": asset.caption,
        "rating": asset.rating,
        "keywords": asset.keywords,
        "people": asset.people,
        "latitude": asset.latitude,
        "longitude": asset.longitude,
        "phash": asset.phash,
        "sources": asset.sources,
        "content": asset.content,
        "thumb": asset.thumb,
        "live": asset.live,
    })
}

/// The books each domain's records land in.
///
/// iCloud is one source into the system's own books, not a book of its own: a contact the
/// bridge writes opens in the system contacts Spore and one typed there is readable here,
/// because both are the `contacts` book. The same holds for calendars and photos.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Books {
    pub contacts: String,
    pub calendar: String,
    pub photos: String,
}

impl Default for Books {
    fn default() -> Self {
        Self {
            contacts: "contacts".to_string(),
            calendar: "calendar".to_string(),
            photos: "photos".to_string(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Options {
    pub books: Books,
    /// Skykeep's `photos/` cache directory (under its state dir). Absent: records only, with
    /// every `content` field `null`, which is what a host holding no photo bytes should show.
    pub cache: Option<PathBuf>,
    /// Hash every thumbnail that is read, so near-duplicates can be found later. Off makes a
    /// metadata-only pass, which is a different question and a much faster one.
    pub phash: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            books: Books::default(),
            cache: None,
            phash: true,
        }
    }
}

/// Read the library under `data` and project every collection.
///
/// Read-only: `open_reader` takes a `query_only` connection, so an export can run while
/// Skykeep is syncing and never writes a row of its own.
pub fn export(data: &Path, options: &Options) -> Result<Batch> {
    let library = Library::open_reader(data)
        .with_context(|| format!("open the Skykeep library in {}", data.display()))?;
    project(&library, options)
}

/// Project an already-open library, for callers (and tests) that own one.
pub fn project(library: &Library, options: &Options) -> Result<Batch> {
    let mut batch = Batch::default();
    batch
        .records
        .extend(contacts(library, &options.books.contacts)?);
    batch
        .records
        .extend(events(library, &options.books.calendar)?);
    let (records, content) = photos(library, options)?;
    batch.records.extend(records);
    batch.content.extend(content);
    Ok(batch)
}

// ---------------------------------------------------------------------------------------------
// Contacts.

fn contacts(library: &Library, book: &str) -> Result<Vec<Record>> {
    let cards = library.cards(None).context("read contacts")?;
    let mut out = Vec::with_capacity(cards.len());
    for card in cards {
        let contact = &card.contact;
        let mut emails = Vec::new();
        let mut phones = Vec::new();
        let mut urls = Vec::new();
        let mut addresses = Vec::new();
        for method in &contact.methods {
            let value =
                json!({ "label": method.label.clone().unwrap_or_default(), "value": method.value });
            match method.kind.as_str() {
                "email" => emails.push(value),
                "phone" => phones.push(value),
                "url" => urls.push(value),
                "address" => addresses.push(value),
                _ => {}
            }
        }
        // The body is `fungos_records::Contact`, whose fields are all required: a key left
        // out is a record the contacts book refuses to store, so every one is written even
        // when it is empty.
        let body = json!({
            "display_name": contact.display_name(),
            "given_name": contact.first_name.clone().unwrap_or_default(),
            "family_name": contact.last_name.clone().unwrap_or_default(),
            "organization": contact.company.clone().unwrap_or_default(),
            "title": card.details.job_title.clone().unwrap_or_default(),
            "emails": emails,
            "phones": phones,
            "addresses": addresses,
            "urls": urls,
            "note": contact.notes.clone().unwrap_or_default(),
            "birthday": contact.birthday.clone().unwrap_or_default(),
            "uid": contact.guid,
            "created": "",
            "last_modified": "",
            "raw_vcard": "",
        });
        out.push(Record {
            book: book.to_string(),
            id: contact.guid.clone(),
            schema: CONTACT_SCHEMA.to_string(),
            body,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Calendars and events.

/// Every event the library holds.
///
/// `events_between` is a window query, and this asks for the whole of time: an event that
/// starts before 1970 or after 2200 is still an event somebody created.
fn events(library: &Library, book: &str) -> Result<Vec<Record>> {
    let from = i64::MIN / 4;
    let to = i64::MAX / 4;
    let events = library.events_between(from, to).context("read events")?;
    let calendars = library.calendars().context("read calendars")?;
    let mut out = Vec::with_capacity(events.len());
    for event in events {
        let body = json!({
            "summary": event.title,
            "description": event.notes.clone().unwrap_or_default(),
            "location": event.location.clone().unwrap_or_default(),
            "status": "",
            "uid": event.guid,
            "start": rfc3339(event.starts_at),
            "end": rfc3339(event.ends_at),
            "duration": "",
            "all_day": event.all_day,
            "timezone": event.timezone.clone().unwrap_or_default(),
            "rrule": "",
            "categories": calendars
                .iter()
                .find(|c| c.guid == event.calendar_guid)
                .map(|c| vec![c.title.clone()])
                .unwrap_or_default(),
            "organizer": { "name": "", "value": "" },
            "attendees": Vec::<Value>::new(),
            "url": event.url.clone().unwrap_or_default(),
            "created": "",
            "last_modified": "",
            "sequence": "",
            "raw_ics": "",
            "raw_timezones": Vec::<String>::new(),
        });
        out.push(Record {
            book: book.to_string(),
            id: event.guid.clone(),
            schema: EVENT_SCHEMA.to_string(),
            body,
        });
    }
    Ok(out)
}

/// Unix seconds as the RFC 3339 UTC string an iCalendar body carries.
fn rfc3339(seconds: i64) -> String {
    jiff::Timestamp::from_second(seconds)
        .map(|t| t.to_string())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Photos, albums and the bytes they point at.

fn photos(library: &Library, options: &Options) -> Result<(Vec<Record>, Vec<Content>)> {
    let book = &options.books.photos;
    let photos = library.photos(View::Library).context("read photos")?;
    let mut records = Vec::with_capacity(photos.len());
    let mut content = Vec::new();
    for photo in &photos {
        let (body, mut blobs) = asset(photo, options.cache.as_deref(), options.phash)?;
        records.push(Record {
            book: book.clone(),
            id: photo.id.clone(),
            schema: ASSET_SCHEMA.to_string(),
            body,
        });
        content.append(&mut blobs);
    }

    for album in library.albums().context("read albums")? {
        let items = if album.folder {
            Vec::new()
        } else {
            library
                .photo_index(&Source::Album(album.id.clone()), None)
                .with_context(|| format!("read album {}", album.id))?
                .ids
        };
        records.push(Record {
            book: book.clone(),
            id: album.id.clone(),
            schema: ALBUM_SCHEMA.to_string(),
            body: json!({
                "name": album.name,
                "position": album.position,
                "folder": album.folder,
                "parent": album.parent,
                "count": items.len(),
            }),
        });
        // Membership is a record of its own: an album is edited far more often than the
        // photos in it are, and a body holding ten thousand ids would be one record rewritten
        // on every add.
        for (position, asset) in items.into_iter().enumerate() {
            records.push(Record {
                book: book.clone(),
                id: format!("{}/{}", album.id, asset),
                schema: ALBUM_ITEM_SCHEMA.to_string(),
                body: json!({
                    "album": album.id,
                    "asset": asset,
                    "position": position,
                }),
            });
        }
    }
    Ok((records, content))
}

fn asset(photo: &Photo, cache: Option<&Path>, want_phash: bool) -> Result<(Value, Vec<Content>)> {
    let mut blobs = Vec::new();
    let original = cache
        .map(|cache| cache.join("original").join(cache_name(&photo.id).as_ref()))
        .filter(|path| path.is_file());
    let content = match original {
        Some(path) => Some(
            ingest(
                &path,
                media_type(photo.uti.as_deref()),
                &mut blobs,
                want_phash,
            )?
            .value,
        ),
        None => None,
    };
    let thumb_path = cache
        .map(|cache| cache.join("thumb").join(cache_name(&photo.id).as_ref()))
        .filter(|path| path.is_file());
    let thumb = match &thumb_path {
        Some(path) => Some(ingest(path, "image/jpeg", &mut blobs, want_phash)?),
        None => None,
    };
    let live_path = cache
        .filter(|_| photo.kind == Kind::Live)
        .map(|cache| cache.join("live").join(cache_name(&photo.id).as_ref()))
        .filter(|path| path.is_file());
    let live = match live_path {
        Some(path) => Some(ingest(&path, "video/quicktime", &mut blobs, want_phash)?.value),
        None => None,
    };

    // The thumbnail is the small copy we always read, so it is the one worth hashing: hashing
    // the original as well would record the same picture twice.
    let phash = thumb.as_ref().and_then(|thumb| thumb.phash.clone());
    let body = asset_body(AssetDraft {
        filename: photo.filename.clone(),
        kind: match photo.kind {
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Live => "live",
        },
        taken_at: photo.taken_at,
        added_at: photo.added_at,
        width: photo.width,
        height: photo.height,
        duration: photo.duration,
        uti: photo.uti.clone(),
        size: photo.size,
        favorite: photo.favorite,
        hidden: photo.hidden,
        deleted: photo.deleted,
        deleted_at: photo.deleted_at,
        title: photo.title.clone(),
        caption: photo.caption.clone(),
        rating: photo.rating,
        keywords: photo.keywords.clone(),
        // iCloud's photo records carry no faces: Apple does that clustering on the device and
        // never syncs the clusters. The macOS source reads them where they live.
        people: Vec::new(),
        latitude: photo.latitude,
        longitude: photo.longitude,
        phash,
        sources: vec![json!({
            "provider": "icloud",
            "id": photo.id,
            "master": photo.master,
        })],
        content: content.clone(),
        thumb: thumb.map(|thumb| thumb.value),
        live,
    });
    Ok((body, blobs))
}

/// One blob, read once: its content id, its size, and the picture's perceptual hash.
///
/// The hash comes back with the bytes because they are already in memory — decoding a
/// thumbnail we had to read anyway is the cheap part of a pass that has to read it at all.
pub(crate) struct Ingested {
    pub value: Value,
    pub phash: Option<String>,
}

/// Hash one file into a content manifest entry.
pub(crate) fn ingest(
    path: &Path,
    media_type: &str,
    blobs: &mut Vec<Content>,
    want_phash: bool,
) -> Result<Ingested> {
    let bytes = std::fs::read(path).with_context(|| format!("read {}", path.display()))?;
    let content_id = format!("blake3:{}", blake3::hash(&bytes).to_hex());
    blobs.push(Content {
        content_id: content_id.clone(),
        path: path.display().to_string(),
        size: bytes.len() as u64,
        media_type: media_type.to_string(),
    });
    let phash = (want_phash && media_type.starts_with("image/"))
        .then(|| phash::of_jpeg(&bytes))
        .flatten()
        .map(phash::tag);
    Ok(Ingested {
        value: json!({
            "content_id": content_id,
            "size": bytes.len() as u64,
            "media_type": media_type,
        }),
        phash,
    })
}

/// A record id as one name in Skykeep's photo cache.
///
/// This is `cache_name` from `src-tauri/src/photos.rs`, which lives in the app crate and so
/// cannot be linked from here. It is pinned by a test here and by that crate's own; if the
/// spelling changes, both agree or the export stops finding files it should find. Ids are
/// letters, digits, `-` and `_` in every case Apple has been seen to send; anything else is
/// spelled out in hex behind a `~`.
pub fn cache_name(id: &str) -> std::borrow::Cow<'_, str> {
    if !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return std::borrow::Cow::Borrowed(id);
    }
    let mut out = String::with_capacity(id.len() * 3);
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
            out.push(byte as char);
        } else {
            out.push_str(&format!("~{byte:02x}"));
        }
    }
    std::borrow::Cow::Owned(out)
}

/// A UTI as a media type, for the content manifest. Only the types a photo library holds.
fn media_type(uti: Option<&str>) -> &'static str {
    match uti {
        Some("public.jpeg") => "image/jpeg",
        Some("public.heic") | Some("public.heif") => "image/heic",
        Some("public.png") => "image/png",
        Some("public.tiff") => "image/tiff",
        Some("public.mpeg-4") => "video/mp4",
        Some("com.apple.quicktime-movie") => "video/quicktime",
        Some("com.compuserve.gif") => "image/gif",
        _ => "application/octet-stream",
    }
}
