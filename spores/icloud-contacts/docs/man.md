# icloud-contacts

Your iCloud address book, drawn from soil's records.

```text
icloud-contacts                  the contact shown, and the rail beside it
icloud-contacts view [<id>]      the same, with one person chosen
icloud-contacts list             names only, for a launcher or a search result
icloud-contacts show <id>        one contact, unabridged
icloud-contacts search <text>    names and methods matching every word
```

## What it reads

The ``fungos.contacts/contact`` collection: one record per person, vCard-shaped,
with `display_name`, `given_name`, `family_name`, `organization`, `title`,
`emails`, `phones`, `addresses`, `urls`, `note`, `birthday`, `uid` and
`raw_vcard`.

The Spore writes nothing. It has no network grant and no filesystem grant: the
records are already there, put there by `quince-soil` from Skykeep's library or
by the contacts Spore's editor. Two writers, one schema, so a contact made in
either opens in both.

## Ordering

The list is the address book's own order: family name first where there is one,
folding accents so that Émile files under E. Someone with no name at all still
gets a row — their first method, as Contacts does — because a contact with only
a phone number is a contact.

## Limits

`scan` pages at 256 records and stops at 20,000; a larger address book is a
paging problem the view does not have yet, not a silent truncation.

## Licence

GPL-3.0-only. This Spore is a port of Skykeep's contacts window and travels with
that licence; see `NOTICE` at the root of this repository.
