# Skykeep as spores — the map

Three Spore packages and one bridge, so an iCloud library can be read by a host that is
not Skykeep. Everything here is GPL-3.0-only (see [`../NOTICE`](../NOTICE)); the reason
this work lives in this fork rather than upstream is the licence, not the technology.

```text
iCloud ── quince-auth / quince-icloud / quince-sync ── quince-core
                                                       │  library.db
                                                       ▼
                                              crates/quince-soil
                                         (records + content manifest)
                                              │              │
                              soil:records ◄──┘              └──► content store
                                                              (blake3:<hex>)
                 │                                                    │
                 ▼                                                    ▼
   ┌───────────────────────────┐                    ┌─────────────────────────────┐
   │ icloud-contacts           │  reads records     │ icloud-photos               │
   │ icloud-calendar           │  and, for photos,  │ names a content id per tile │
   │                           │  not bytes         │ → vfs-cid finds the place   │
   └───────────────────────────┘                    └─────────────────────────────┘
```

## Who owns what

| thing | owner | why |
|---|---|---|
| Apple's sign-in, SRP, two-factor, session | `quince-auth` | already built and tested; a WASM guest should never hold a password |
| Apple's web services and CloudKit | `quince-icloud` | 90k lines of Apple's behaviour, in one place |
| durability, revisions, tombstones, peer sync | `soil:records` | one store, one conflict rule, no second database |
| the schema and the meaning of a body | the Spore | a collection is a contract, not a file format |
| the view | Lua, on Canvas | why an update to a screen needs no rebuild |
| the bytes of a photo | the host's content store | large, immutable, addressed by identity |

## The bridge

`quince-soil export --data <Skykeep data dir> --cache <Skykeep photos dir> --book icloud`
writes one JSON document:

```json
{
  "records": [
    { "book": "icloud", "id": "contact-1", "schema": "fungos.contacts/contact", "body": { … } },
    { "book": "icloud", "id": "event-1",   "schema": "fungos.calendar/event",   "body": { … } },
    { "book": "icloud", "id": "P1",        "schema": "fungos.photos/asset@1",   "body": { … } },
    { "book": "icloud", "id": "AL1",       "schema": "fungos.photos/album@1",   "body": { … } },
    { "book": "icloud", "id": "AL1/P1",    "schema": "fungos.photos/album-item@1", "body": { … } }
  ],
  "content": [
    { "content_id": "blake3:…", "path": "/…/photos/original/P1", "size": 2011234, "media_type": "image/heic" }
  ]
}
```

`records` is exactly `soil-records-ingest`'s `POST /api/records/batch` body, so the bridge
is not a second store — it is a producer for the one that exists. `content` is the ingress
half of the content seam: a path is read once, hashed, and never travels with a record.

### Declaring records

A records-shaped Spore names its collections in its manifest, so a host can grant
them without an operator retyping a schema the package defines:

```toml
[records]
collections = ["fungos.contacts/contact"]
book = "contacts"
```

The book is a **system** book, not one per source: iCloud is a source, so a contact
the bridge writes lands in `contacts` and opens in the system contacts Spore — the
same record as one typed there. Calendars land in `calendar`, photos in `photos`.

The grant stays `soil:records` — the table says *which* collections, and the host
still decides whether the capability is granted. A command-line `--collection`
still wins, which is how a write is granted; declared collections are read-only.
Without this table the launcher had nothing to grant, and `soil spore run`
demanded a `--collection` nobody at a launcher could type.

### Collections

| collection | shape | existing? |
|---|---|---|
| `fungos.contacts/contact` | vCard-shaped: `display_name`, `given_name`, `family_name`, `organization`, `title`, `emails`, `phones`, `addresses`, `urls`, `note`, `birthday`, `uid`, `created`, `last_modified`, `raw_vcard` | yes — `fungos-records` defines it, the contacts Spore already writes it |
| `fungos.calendar/event` | iCalendar-shaped: `summary`, `description`, `location`, `start`, `end`, `all_day`, `timezone`, `rrule`, `url`, … | yes — `fungos-records` defines it, the feeds importer writes it |
| `fungos.photos/asset@1` | what a photo *is*, plus `content`/`thumb`/`live` as `{content_id, size, media_type}` | new, owned here |
| `fungos.photos/album@1` | `name`, `position`, `folder`, `parent`, `count` | new, owned here |
| `fungos.photos/album-item@1` | `album`, `asset`, `position`; record id `album/asset` | new, owned here |

Contacts and calendar reuse the established shapes on purpose: a contact written by the
bridge opens in the contacts Spore's editor and the other way round, which is the whole
argument for the records layer. A second contacts schema would have been easier this week
and unmergeable next month.

## Why photos are shaped differently

A photo is not a value. A contact is a few hundred bytes you can copy, compare and
conflict-resolve whole; a photo is a 2 MB immutable blob you would copy on every sync pass
and could never merge. So:

- the **record** carries what the photo *is* — when it was taken, its kind, size in pixels,
  its title, caption, rating, keywords, location — and the **content id** of its bytes;
- the **bytes** are addressed by identity (`blake3:<hex>`), which is exactly what
  `soil:vfs` already publishes as `content-id`;
- **membership** is its own record, so two peers adding different photos to one album are
  two records, not one lost update.

The missing piece is resolving a content id to bytes without a spore naming a path:
[`VFS-CID.md`](VFS-CID.md).

## What a Spore is allowed to be

All three packages declare `soil:records` and deny `soil:http` and `wasi:filesystem`, and
the calendar denies `wasi:clocks` as well: a view that read the clock would draw a
different week on every render. Photos additionally declares `soil:vfs-cid` — a grant no
host can satisfy yet, written down so the gap is a manifest line rather than a later
surprise.

A Spore here never writes. The direction of a future write (star a photo, move a contact's
phone number) is `soil:records` → an outbox → `quince-sync`'s existing replay, and none of
it is in this cut.

## The next questions, in the order they bite

1. **Who wins?** Apple is authoritative for iCloud data; records are last-writer-wins
   between peers. An edit made in a Spore needs an outbox and Apple's own change tag, or
   the next sync pass silently reverts it. `quince-core` already has the outbox table.
2. **Two accounts.** A book per account (`icloud/<account-id>`) rather than one, so a
   household sharing a desktop cannot read the wrong library. This cut assumes one.
3. **Deletions.** `delete` on the bridge side must publish tombstones, and a photo Apple
   expunged (30 days in Recently Deleted) must not stay forever in a peer's records.
4. **Thumbnails at scale.** A library of 100k photos is 100k thumbnails; the content store
   needs eviction or the host's disk fills. `quince-vfs` already has a bounded cache with
   a trim policy worth copying.
5. **Shared desktop.** The `soil:records` peer sync is already there; what a shared
   desktop needs is consent that the *other* person's iCloud library may be read on this
   machine at all, which is a `soil:principal` question, not a Spore one.

## Packaging

Each Spore is a package: `spore.toml`, `module.wasm`, the Lua under `ui/`, `docs/man.md`,
`assets/`, and `spore.lock`. `src/`, `wit/`, `Cargo.*` and `tests/` are build inputs and are
not in the tar — `spore.lock` names the five files that ship, which is how a reviewer can
see that the source did not travel with the binary.

```sh
soil-spore check <pkg> --publish   # listing gate; a human still reads it
soil-spore pack  <pkg> -o x.spore  # deterministic tar: same tree, same bytes, same CID
soil-spore verify x.spore          # from the bytes alone, no source needed
soil-install install x.spore --root ~/.local/share/fungos/spores   # the host's side
soil-install status --root ~/.local/share/fungos/spores --json
soil-install rollback --id org.skykeep.contacts --root ~/.local/share/fungos/spores
```

`soil-install` is the host authority, not a Spore: it verifies the tar's lock, writes the
tree, and remembers the digest it replaced so a change can be undone. It also installs from
a store index by CID (`--cid ipfs://… --index … --packages …`), which is the path a
storefront will use once a Spore is allowed to ask for it.

All three pass `--publish`. The artwork a listing cites is content-addressed and derived
from files in this repository by `tools/spore-cid.py`, which also has a `--check` mode:
run it after changing a screenshot, or the listing cites bytes nobody can reproduce.

### One package or three

Three, because a package is a consent scope. Photos declares `soil:vfs-cid`, which is a new
capability on a host; bundling contacts and calendar into the same package would make
someone who wants an address book approve a content-store grant. Three packages also means
three digests and three updates, and the store's update classes (`live` for Lua, `restart`
for a module) apply per package rather than to everything at once.

The bridge is deliberately *not* a Spore. `quince-soil` reads Skykeep's SQLite library and
hashes files on disk; a WASM guest has neither a filesystem nor a reason to hold one, and
the sync half runs with the user's own account session. A Spore that could read an
arbitrary path and hold a token would be exactly the shape `soil:vfs`'s rules exist to
prevent. The split is the point: Apple and the library on one side, records and views on the
other, and a manifest line the only thing connecting them.

## What was checked

- `cargo test -p quince-soil` — 6 tests: the contacts body against every field the
  contacts book validates, the iCalendar start/end spelling, a cached original becoming a
  content id and an album membership record, a library with no cached bytes, the cache
  name escaping, and the read-only open the CLI uses.
- `cargo test` in each Spore — 4 + 4 + 2 tests: civil-date arithmetic, week boundaries,
  birthdays, initials, accent folding, month and day labels.
- `soil-spore check` on all three packages: manifest, module, view, manual, assets, lock.
- `soil-spore check --publish` on all three: listing requirements met, artwork included as
  two screenshots and a `presentation.icon`, all three reproducible from
  `docs/media/*.png` and `spores/*/assets/icon.svg` by `tools/spore-cid.py --check`.
- `soil-spore pack` + `soil-spore verify` on all three: 260 KB, 245 KB and 291 KB tars,
  each verified back from its bytes.
- `soil-install install` on all three into a fresh root, then `soil-install status --json`:
  three installed trees with the digests above, `previous_digest: null`. The host side works
  today; the storefront side is what is missing.
- `soil:vfs-cid` in soil (`crates/soil-host/tests/vfs_cid.rs`, 3 tests): every path holding
  the bytes, paging in path order, an unknown id as an empty page, a malformed id as
  `invalid`, a zero limit as `over-budget`.
- Each view rendered through the spores repository's `tools/shot.lua` into the screenshots
  in [`../docs/media`](../docs/media).

Not checked: a real host running a Spore against a real records store, and any of this
against a live iCloud account. Those need a host with `soil:records` wired to a Spore, and
the bridge pointed at a signed-in Skykeep — the two things this spike deliberately did not
change. The photo tiles are still marks: `soil:vfs-cid` gives a host the *place*, and the
missing piece is a host surface that paints it (see [`VFS-CID.md`](VFS-CID.md)).
