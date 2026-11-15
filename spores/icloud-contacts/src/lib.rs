//! Skykeep Contacts: the address book iCloud filled in, as a fungOS Spore.
//!
//! Actions:
//!   view [<id>]     the document the view draws
//!   list            names only, for a launcher or a search result
//!   show <id>       one contact, unabridged
//!   search <text>   names and methods matching every word
//!   help
//!
//! The records are `fungos.contacts/contact`, written by `quince-soil` from
//! Skykeep's library and by the contacts Spore from its editor. Two writers, one
//! schema: a contact made here opens there and the other way round.

wit_bindgen::generate!({ path: "wit", world: "contacts", generate_all });

use serde_json::{Value, json};
use soil::records::records;

const CONTACT_SCHEMA: &str = "fungos.contacts/contact";
const PAGE: u32 = 256;
const MAX: usize = 20_000;

struct Component;

export!(Component);

impl Guest for Component {
    fn run(args: Vec<String>) -> Result<(), String> {
        match args.first().map(String::as_str).unwrap_or("view") {
            "view" => print_json(&view(args.get(1).map(String::as_str))?),
            "list" => print_json(&json!({
                "contacts": load()?.iter().map(summary).collect::<Vec<_>>(),
            })),
            "show" => {
                let id = args.get(1).ok_or("usage: show <id>")?;
                let contacts = load()?;
                let contact = contacts
                    .iter()
                    .find(|contact| contact.id == *id)
                    .ok_or_else(|| format!("no contact {id}"))?;
                print_json(&json!({ "contact": card(contact) }))
            }
            "search" => {
                let query = args.get(1).cloned().unwrap_or_default();
                print_json(&json!({
                    "query": query,
                    "contacts": search(&query)?,
                }))
            }
            "help" => print_json(&help()),
            other => Err(format!("unknown action {other:?}; try help")),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// The address book, as records.

struct Contact {
    id: String,
    body: Value,
}

fn load() -> Result<Vec<Contact>, String> {
    let mut contacts = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let page = records::scan(CONTACT_SCHEMA, cursor.as_deref(), PAGE)
            .map_err(|error| format!("scan contacts: {error:?}"))?;
        for entry in page.entries {
            if entry.deleted || entry.schema != CONTACT_SCHEMA {
                continue;
            }
            match serde_json::from_slice::<Value>(&entry.body) {
                Ok(body) => contacts.push(Contact {
                    id: entry.key.id,
                    body,
                }),
                Err(_) => continue,
            }
        }
        if contacts.len() >= MAX {
            break;
        }
        match page.next {
            Some(next) if Some(next.as_str()) != cursor.as_deref() => cursor = Some(next),
            _ => break,
        }
    }
    contacts.sort_by_key(|contact| sort_key(&contact.body));
    Ok(contacts)
}

fn text(body: &Value, key: &str) -> String {
    body.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn listed(body: &Value, key: &str) -> Vec<Value> {
    body.get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter(|value| {
                    value
                        .get("value")
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.is_empty())
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// Who this is, for a row and a heading.
fn name(body: &Value) -> String {
    let display = text(body, "display_name");
    if !display.is_empty() {
        return display;
    }
    let joined = format!("{} {}", text(body, "given_name"), text(body, "family_name"));
    let joined = joined.trim();
    if !joined.is_empty() {
        return joined.to_string();
    }
    let organization = text(body, "organization");
    if !organization.is_empty() {
        return organization;
    }
    for values in [listed(body, "emails"), listed(body, "phones")] {
        if let Some(value) = values
            .first()
            .and_then(|value| value.get("value"))
            .and_then(Value::as_str)
        {
            return value.to_string();
        }
    }
    "No name".to_string()
}

/// The monogram a rail row draws: Apple uses the same two letters.
fn initials(body: &Value) -> String {
    let first = text(body, "given_name");
    let last = text(body, "family_name");
    let mut letters = String::new();
    for part in [first, last] {
        if let Some(letter) = part.chars().find(|c| c.is_alphanumeric()) {
            letters.extend(letter.to_uppercase());
        }
    }
    if letters.is_empty() {
        let display = name(body);
        letters = display
            .chars()
            .filter(|c| c.is_alphanumeric())
            .take(1)
            .collect::<String>()
            .to_uppercase();
    }
    letters
}

/// Sorted on the family name where there is one, as Contacts orders the list.
fn sort_key(body: &Value) -> String {
    let last = text(body, "family_name");
    let first = text(body, "given_name");
    let key = if last.trim().is_empty() {
        name(body)
    } else {
        format!("{last} {first}")
    };
    fold(&key)
}

/// Lower case without the accents, so "Émile" files under E.
///
/// Lowercasing comes first and is Unicode-aware, so one table of accents covers
/// both cases: `É` is `é` before the table ever sees it.
fn fold(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            'ñ' => 'n',
            'ý' | 'ÿ' => 'y',
            other => other,
        })
        .collect()
}

fn summary(contact: &Contact) -> Value {
    let body = &contact.body;
    let email = listed(body, "emails")
        .first()
        .and_then(|value| value.get("value").cloned())
        .unwrap_or(Value::Null);
    let phone = listed(body, "phones")
        .first()
        .and_then(|value| value.get("value").cloned())
        .unwrap_or(Value::Null);
    json!({
        "id": contact.id,
        "name": name(body),
        "initials": initials(body),
        "email": email,
        "phone": phone,
        "organization": text(body, "organization"),
    })
}

/// The whole card: every field Apple keeps, in the order Contacts shows them.
fn card(contact: &Contact) -> Value {
    let body = &contact.body;
    json!({
        "id": contact.id,
        "name": name(body),
        "initials": initials(body),
        "given_name": text(body, "given_name"),
        "family_name": text(body, "family_name"),
        "organization": text(body, "organization"),
        "title": text(body, "title"),
        "emails": listed(body, "emails"),
        "phones": listed(body, "phones"),
        "addresses": listed(body, "addresses"),
        "urls": listed(body, "urls"),
        "note": text(body, "note"),
        "birthday": birthday_text(&text(body, "birthday")),
        "uid": text(body, "uid"),
        "raw_vcard": text(body, "raw_vcard"),
    })
}

/// A birthday as a person reads it.
///
/// Apple stores a year-less birthday as `--12-10` (the vCard form for "December
/// 10th, year unknown") and a complete one as `1985-12-10`. Both are spelled out
/// with the year only when there is one: a year nobody gave is not one to invent.
fn birthday_text(value: &str) -> String {
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
    let value = value.trim();
    if value.is_empty() {
        return String::new();
    }
    let (year, month, day) = if let Some(rest) = value.strip_prefix("--") {
        (None, rest.get(..2), rest.get(3..5))
    } else {
        (value.get(..4), value.get(5..7), value.get(8..10))
    };
    let (Some(month), Some(day)) = (month, day) else {
        return value.to_string();
    };
    let (Ok(month), Ok(day)) = (month.parse::<usize>(), day.parse::<u32>()) else {
        return value.to_string();
    };
    let Some(name) = MONTHS.get(month.saturating_sub(1)) else {
        return value.to_string();
    };
    match year {
        Some(year) => format!("{name} {day}, {year}"),
        None => format!("{name} {day}"),
    }
}

fn view(selected: Option<&str>) -> Result<Value, String> {
    let contacts = load()?;
    let chosen = selected
        .and_then(|id| contacts.iter().find(|contact| contact.id == id))
        .or_else(|| contacts.first());
    Ok(json!({
        "source": "iCloud",
        "count": contacts.len(),
        "selected": chosen.map(|contact| contact.id.clone()),
        "contacts": contacts.iter().map(summary).collect::<Vec<_>>(),
        "contact": chosen.map(card),
    }))
}

fn search(query: &str) -> Result<Vec<Value>, String> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|word| fold(word))
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for contact in load()? {
        let haystack = fold(&contact.body.to_string());
        if words.iter().all(|word| haystack.contains(word)) {
            found.push(summary(&contact));
        }
    }
    Ok(found)
}

fn help() -> Value {
    json!({
        "actions": [
            "view [<id>]     the document the view draws",
            "list            names only",
            "show <id>       one contact, unabridged",
            "search <text>   names and methods matching every word",
            "help",
        ],
        "schema": CONTACT_SCHEMA,
        "writes": false,
    })
}

fn print_json(value: &Value) -> Result<(), String> {
    println!("{value}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_year_less_birthday_keeps_no_year() {
        assert_eq!(birthday_text("--12-10"), "December 10");
        assert_eq!(birthday_text("1985-01-05"), "January 5, 1985");
        assert_eq!(birthday_text(""), "");
        assert_eq!(birthday_text("sometime"), "sometime");
        assert_eq!(birthday_text("--13-40"), "--13-40");
    }

    #[test]
    fn initials_are_the_two_letters_contacts_draws() {
        let body = json!({"given_name": "Ada", "family_name": "Lovelace"});
        assert_eq!(initials(&body), "AL");
        let body = json!({"display_name": "Acme"});
        assert_eq!(initials(&body), "A");
        let body = json!({});
        assert_eq!(
            initials(&body),
            "N",
            "a nameless card still gets a monogram"
        );
    }

    #[test]
    fn a_name_falls_back_to_the_first_method() {
        let body = json!({"emails": [{"label": "work", "value": "a@b.example"}]});
        assert_eq!(name(&body), "a@b.example");
        let body = json!({"given_name": "Ada", "family_name": "Lovelace"});
        assert_eq!(name(&body), "Ada Lovelace");
    }

    #[test]
    fn accents_fold_so_the_rail_sorts_like_contacts() {
        assert_eq!(fold("Émile"), "emile");
        assert_eq!(sort_key(&json!({"family_name": "Émile"})), "emile ");
    }
}
