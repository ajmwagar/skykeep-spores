# icloud-photos

Your iCloud photo library, by month and by album.

```text
icloud-photos                  the library, newest month first
icloud-photos view <album-id>  one album, in the album's own order
icloud-photos albums           every album, with its count
icloud-photos asset <id>       one photo, with the content ids of its bytes
```

## What it reads

Three collections, because a photo is not a value.

``fungos.photos/asset@1`` — one record per photo. It carries what the photo *is*
(when it was taken, its kind, width and height, its title, caption, rating,
keywords and location) and, separately, the **content ids** of its bytes:

```json
{
  "taken_at": 1774000000,
  "kind": "image",
  "width": 4032,
  "height": 3024,
  "favorite": true,
  "content": { "content_id": "blake3:…", "size": 2011234, "media_type": "image/jpeg" },
  "thumb":   { "content_id": "blake3:…", "size": 41233,  "media_type": "image/jpeg" },
  "live":    null
}
```

``fungos.photos/album@1`` — one record per album or folder, with `position`,
`folder` and `parent` so folders nest the way Photos nests them.

``fungos.photos/album-item@1`` — one record per membership, id `album/asset`,
body `{album, asset, position}`. Membership is its own record on purpose: an
album is edited far more often than its photos are, and a body holding ten
thousand ids would be one record rewritten on every add — and would turn two
people adding different photos to the same album into a lost update.

## Why content ids and not paths

`blake3:<hex>` is the whole-content identity `soil:vfs` already publishes. It is
not a locator: no path, no provider, no lease. That is the point — a photo's
bytes may be on this machine, on a peer, or nowhere yet, and the record does not
have to change when that changes.

Resolving an id to a place in the workspace is the host's job, and the interface for it
landed in soil as `soil:vfs-cid@0.1.0` (`find`, in `spores/VFS-CID.md`). A host that runs
this Spore still has to register it and paint what it finds; until then a tile draws its
kind mark, the day and the
filename: the grid is right and the pixels are missing, rather than the other
way round.

## Limits

2400 photos and 5000 albums per document, newest month first. A larger library
is a paging problem this Spore does not have yet — the records are all there,
and the view draws one window of them.

## Licence

GPL-3.0-only. This Spore is a port of Skykeep's photos window and travels with
that licence; see `NOTICE` at the root of this repository.
