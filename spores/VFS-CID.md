# `vfs/cid` — resolving a content id inside the namespace

Status: **landed in soil as `soil:vfs-cid@0.1.0`** (interface `cid`, one function
`find`), implemented on `soil_host::vfs_host::VfsHost` with four tests, and **registered in
the runner** (`crates/soil-webgpu` links it beside `soil:vfs`, so a Spore that declares the
grant can call it). The first Spore that paints a content id is
[`spores/photos-view`](photos-view): it rasterizes its own frame, so the "surface that
paints" below is the Spore itself rather than a host node.

```
record  thumb.content_id = "blake3:…"
   │  soil:vfs-cid   find(cid)        → paths the caller could already list
   │  soil:vfs       read(path)       → bytes, in the host's range budget
   │  (guest)        decode, lay out  → one RGBA frame
   └  soil:present   present_rgba(…)  → the window, or one captured PNG
```

![Skykeep Photos, drawn from content ids](media/photos-view.png)

## The gap

`soil:vfs` publishes, on every entry, a canonical whole-content identity:

```wit
/// Canonical whole-content identity (`blake3:<hex>`), not a provider CID.
content-id: string,
```

and the rest of the interface is addressed by **path**. That is right for a namespace: a
Spore names a workspace-relative path, the host canonicalises it, and a provider locator
never becomes authority.

It is not enough for a photo library. A photo record cannot name a path:

- the path is host state — it moves when a workspace moves, when a cache is trimmed, when
  the same library is read on another machine;
- two paths can hold the same bytes, and one path can be replaced with different bytes;
- a record is synced to peers that have neither path.

So the record names `blake3:…` — identity — and there was no way to turn that identity back
into a place in the workspace. The photos Spore could list a library and not draw a single
picture.

## What landed

```wit
package soil:vfs-cid@0.1.0;

interface cid {
  use soil:vfs/vfs@1.0.0.{entry};

  enum error { denied, over-budget, invalid, io, unsupported }

  record page { entries: list<entry>, next: option<u64> }

  /// Entries whose whole-content identity is `content-id`, in path order.
  find: func(content-id: string, offset: u64, limit: u32) -> result<page, error>;
}
```

Four decisions worth keeping:

- **`find`, not `read`.** The first draft of this doc proposed reading bytes by content id.
  Implementing it showed that is a second byte path with a second budget to keep in step.
  `find` answers *where* and the existing `read(path)` answers *what*: one byte path, one set
  of budgets, and the caller already holds `soil:vfs` if it is allowed to read the bytes it
  found.
- **A separate package, not `soil:vfs@1.1.0`.** A version bump re-links every Spore that
  vendors the read-only file and makes "which vfs does this host speak" a new installer
  question. A separate package is additive, exactly like `soil:vfs-mut`: a host opts in, a
  Spore that never resolves an id never sees it.
- **A content id is not authority.** The answer is entries the caller could already see by
  walking the namespace — no host path, no provider locator, no URL, no lease. A Spore learns
  nothing about the machine by asking that it could not learn by listing.
- **Empty is an answer; a malformed id is an error.** `blake3:` + 64 hex, checked on the host
  side, because the index is keyed by that string: a malformed id would be a query that can
  only miss, and a miss reads as "the bytes are gone". Absent bytes are an empty page, never
  a tombstone — deletion is a records concern and conflating the two makes a cache miss look
  like lost data.

## In the host

`VfsHost` answers `find` from its derived index, which already carries each entry's
`content-id`; the lookup is `by_content` on the index backend. Same refresh rules as `ls`
and `stat`, same `MAX_PAGE` budget, results in path order so pagination is stable. Two paths
holding the same bytes both answer, which is the point: the identity is the bytes, not the
name.

Tests (`crates/soil/tests/vfs_cid.rs`): every path holding the bytes, paging in path order,
an unknown id as an empty page, a malformed id as `invalid`, a zero limit as `over-budget`,
and the one path a viewer walks — write, `find`, `read`, same bytes back.

It is registered in `crates/soil-webgpu` next to `soil:vfs`, from the same host object: a
lookup answered from a second index could disagree with the listing a Spore already saw.

## What landed on top of it

`spores/photos-view` is the first Spore that draws a picture from a content id, and it did
not need a new host surface: the guest rasterizes its own frame and hands it over through
`soil:present/surface.present-rgba`, which the runner already accepts. So the "surface that
paints" turned out to be the Spore, not a host node — the composition stays the Spore's and
the host keeps the window.

The other half of that flow is bytes: a namespace holds what was *written* into it, and a
`Photographs` folder on disk is not a workspace. `soil content import --manifest FILE` puts
the bytes an exporter listed (a `quince-soil macos-photos --chunks` run writes one) into a
workspace by identity, checking that each file hashes to the id its record names.

## What remains

1. **A canvas text node, or a font.** The view draws pictures and nothing else: a tile
   cannot say "2 March" or "Boat" because the guest has no text rasterizer and the
   `soil:canvas` scene has an `Image` node but no text one on this path. The Lua canvas
   Spores (`spores/icloud-photos`) still name a content id they cannot paint, because their
   host is the core-Wasm one and it does not register `soil:vfs-cid` yet.
2. **Decode where the bytes are.** The guest carries a JPEG decoder so it can draw; a host
   that decodes once and caches a texture would keep a library's worth of decoders out of
   every Spore. `soil:media/image` exists for exactly that and is not linked in the runner.
3. **Hydration.** A record whose bytes are in iCloud draws a placeholder tile, which is
   honest and not yet useful: fetching the original by its `icloud` source is the next
   milestone.
