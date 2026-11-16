//! macOS as a source, not a login.
//!
//! The Mac is signed into iCloud in System Settings, so Contacts are already mirrored to
//! disk. Reading them is a local read of an undocumented schema — no Apple ID, no SRP, no
//! session — and it is the difference between "ingest your stuff" being a login problem and
//! being a file problem.
//!
//! The schema moves with every macOS release, so nothing here assumes a column exists: the
//! table is inspected once and a missing column reads as null. A missing *table* is the loud
//! failure — an address book we cannot read is not an empty address book.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};

use crate::{CONTACT_SCHEMA, Record};

/// The address book's SQLite file, by the name macOS has used for years.
const ADDRESS_BOOK: &str = "AddressBook-v22.abcddb";

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
        let empty = [&person.first, &person.middle, &person.last, &person.organization]
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
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
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
}
