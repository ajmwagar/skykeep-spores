//! What the records a Spore reads actually look like.
//!
//! The shapes here are contracts, not snapshots of quince's structs: a body that loses a key
//! the contacts book validates, or a start time spelled the way `Event` stores it instead of
//! the way iCalendar writes it, is a quiet break between the sync half and the view half.

use std::path::Path;

use quince_core::calendar::{Calendar, Event};
use quince_core::contacts::{Contact, ContactMethod};
use quince_core::photos::{Album, Photo};
use quince_core::Library;
use quince_soil::{
    Options, ALBUM_ITEM_SCHEMA, ALBUM_SCHEMA, ASSET_SCHEMA, CONTACT_SCHEMA, EVENT_SCHEMA,
};
use serde_json::{json, Value};

fn open(dir: &Path) -> Library {
    Library::open(&dir.join("library")).expect("a new library")
}

fn body(batch: &quince_soil::Batch, schema: &str) -> Value {
    batch
        .records
        .iter()
        .find(|record| record.schema == schema)
        .unwrap_or_else(|| panic!("no {schema} record"))
        .body
        .clone()
}

fn contact() -> Contact {
    Contact {
        guid: "contact-1".into(),
        first_name: Some("Ada".into()),
        middle_name: None,
        photo_url: None,
        photo_signature: None,
        photo_crop: None,
        last_name: Some("Lovelace".into()),
        company: Some("Acme".into()),
        is_company: false,
        birthday: Some("--12-10".into()),
        notes: Some("First programmer.".into()),
        etag: None,
        methods: vec![
            ContactMethod {
                kind: "email".into(),
                label: Some("work".into()),
                value: "ada@acme.example".into(),
            },
            ContactMethod {
                kind: "phone".into(),
                label: Some("mobile".into()),
                value: "+1 555 0100".into(),
            },
        ],
    }
}

/// The contacts book validates every body against `fungos_records::Contact`, whose fields are
/// all required. A key missing here is a record `soil:records` refuses.
const CONTACT_FIELDS: [&str; 15] = [
    "display_name",
    "given_name",
    "family_name",
    "organization",
    "title",
    "emails",
    "phones",
    "addresses",
    "urls",
    "note",
    "birthday",
    "uid",
    "created",
    "last_modified",
    "raw_vcard",
];

#[test]
fn a_contact_carries_every_field_the_contacts_book_validates() {
    let dir = tempfile::tempdir().unwrap();
    let mut library = open(dir.path());
    library.put_contacts(&[contact()]).unwrap();

    let batch = quince_soil::project(&library, &Options::default()).unwrap();
    let body = body(&batch, CONTACT_SCHEMA);
    let object = body.as_object().expect("a contact body is an object");
    for field in CONTACT_FIELDS {
        assert!(
            object.contains_key(field),
            "contact body is missing {field}"
        );
    }
    assert_eq!(body["uid"], "contact-1", "the token is the record uid");
    assert_eq!(body["display_name"], "Ada Lovelace");
    assert_eq!(body["emails"][0]["value"], "ada@acme.example");
    assert_eq!(body["phones"][0]["label"], "mobile");
    assert_eq!(batch.records[0].id, "contact-1");
    assert_eq!(batch.records[0].book, "icloud");
}

#[test]
fn an_event_starts_and_ends_as_icalendar_writes_them() {
    let dir = tempfile::tempdir().unwrap();
    let mut library = open(dir.path());
    library
        .put_calendars(&[Calendar {
            guid: "cal-1".into(),
            title: "Home".into(),
            ..Calendar::default()
        }])
        .unwrap();
    library
        .put_events(&[Event {
            guid: "event-1".into(),
            calendar_guid: "cal-1".into(),
            title: "Dentist".into(),
            starts_at: 1_774_000_000,
            ends_at: 1_774_003_600,
            // A timed event with a zone is an instant, so the body is the clock time in UTC
            // however the computer reading it is set up. A floating event would be the wall
            // clock, which the library reads in the local zone.
            timezone: Some("UTC".into()),
            ..Event::default()
        }])
        .unwrap();

    let batch = quince_soil::project(&library, &Options::default()).unwrap();
    let body = body(&batch, EVENT_SCHEMA);
    assert_eq!(body["summary"], "Dentist");
    assert_eq!(body["start"], "2026-03-20T09:46:40Z");
    assert_eq!(body["end"], "2026-03-20T10:46:40Z");
    assert_eq!(body["timezone"], "UTC");
    assert_eq!(body["categories"][0], "Home", "an event names its calendar");
}

fn photo(id: &str, master: &str) -> Photo {
    serde_json::from_value(json!({
        "id": id,
        "master": master,
        "takenAt": 1_774_000_000,
        "kind": "image",
        "width": 4032,
        "height": 3024,
        "size": 2_000_000,
        "uti": "public.jpeg",
        "favorite": true,
        "hidden": false,
        "deleted": false,
        "subtype": 0,
        "changeTag": null,
    }))
    .expect("a photo")
}

#[test]
fn a_cached_original_becomes_a_content_id_and_the_album_a_membership_record() {
    let dir = tempfile::tempdir().unwrap();
    let mut library = open(dir.path());
    library.put_photos(&[photo("P1", "M1")]).unwrap();
    library
        .replace_albums(&[Album {
            id: "AL1".into(),
            name: "Trip".into(),
            position: 0,
            change_tag: None,
            folder: false,
            parent: None,
        }])
        .unwrap();
    library
        .put_album_items(&[("AL1".into(), "P1".into(), 0)])
        .unwrap();

    let cache = dir.path().join("photos");
    std::fs::create_dir_all(cache.join("original")).unwrap();
    std::fs::write(cache.join("original/P1"), b"not really a jpeg").unwrap();

    let options = Options {
        book: "icloud".into(),
        cache: Some(cache),
    };
    let batch = quince_soil::project(&library, &options).unwrap();

    let asset = body(&batch, ASSET_SCHEMA);
    let content = &asset["content"];
    let expected = format!("blake3:{}", blake3::hash(b"not really a jpeg").to_hex());
    assert_eq!(content["content_id"], expected);
    assert_eq!(content["media_type"], "image/jpeg");
    assert!(
        asset["thumb"].is_null(),
        "nothing was cached at a thumbnail size"
    );

    assert_eq!(batch.content.len(), 1);
    assert_eq!(batch.content[0].content_id, expected);
    assert_eq!(batch.content[0].size, 17);

    let album = body(&batch, ALBUM_SCHEMA);
    assert_eq!(album["name"], "Trip");
    assert_eq!(album["count"], 1);
    let item = batch
        .records
        .iter()
        .find(|record| record.schema == ALBUM_ITEM_SCHEMA)
        .expect("an album membership record");
    assert_eq!(item.id, "AL1/P1");
    assert_eq!(item.body["position"], 0);
}

#[test]
fn a_library_with_no_cached_bytes_still_lists_the_photos() {
    let dir = tempfile::tempdir().unwrap();
    let mut library = open(dir.path());
    library.put_photos(&[photo("P1", "M1")]).unwrap();

    let batch = quince_soil::project(&library, &Options::default()).unwrap();
    let asset = body(&batch, ASSET_SCHEMA);
    assert_eq!(asset["filename"], Value::Null);
    assert!(asset["content"].is_null());
    assert!(batch.content.is_empty());
}

#[test]
fn export_reads_a_library_the_app_wrote_and_left_closed() {
    // The CLI opens the library read-only (`Library::open_reader`), which only
    // works on a library the app has already opened and migrated. This is the one
    // test that walks that path rather than `project`.
    let dir = tempfile::tempdir().unwrap();
    {
        let mut library = open(dir.path());
        library.put_contacts(&[contact()]).unwrap();
    }
    let batch = quince_soil::export(&dir.path().join("library"), &Options::default()).unwrap();
    assert_eq!(batch.records.len(), 1);
    assert_eq!(batch.records[0].schema, CONTACT_SCHEMA);
}

#[test]
fn cache_names_are_the_ones_skykeep_writes() {
    assert_eq!(quince_soil::cache_name("ABC-123_x"), "ABC-123_x");
    assert_eq!(quince_soil::cache_name("a/b"), "a~2fb");
    assert_eq!(quince_soil::cache_name(""), "");
}
