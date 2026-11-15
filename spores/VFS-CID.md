# `vfs/cid` — resolving a content id inside the namespace

Status: **landed in soil as `soil:vfs-cid@0.1.0`** (interface `cid`, one function
`find`), implemented on `soil_host::vfs_host::VfsHost` with three tests. A host that runs a
Spore still has to register it, and a surface still has to paint the bytes: see
[what remains](#what-remains).

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

Tests (`crates/soil-host/tests/vfs_cid.rs`): every path holding the bytes, paging in path
order, an unknown id as an empty page, a malformed id as `invalid`, a zero limit as
`over-budget`.

## What remains

1. **Register it where a Spore runs.** Each host that links `soil:vfs` for a guest adds
   `soil:vfs-cid/cid@0.1.0` beside it — one registration line. In this repo that is
   `soil-command` and the per-capability host crates; the spores repo's `canvas-*` hosts are
   the other place it will be needed.
2. **A surface that paints a content id.** The lookup gives a host a path; the pixels are a
   host surface. The photos view is already written for it — a tile wants
   `ui.host_surface("photo", { cid = asset.thumb })` — so the work is host-side: resolve the
   id through `find`, read the thumbnail, hand the renderer a texture, and cache it.
3. **Then the Spore changes by a few lines.** Import `soil:vfs-cid` in
   `spores/icloud-photos/wit/world.wit`, declare it in `[engine] abi`, and let the tile draw
   the picture instead of `IMG`/`VID`/`LIV`.

Until (1) and (2) land, the photo grid renders what it can prove: the kind, the day, the
filename and the content id. The grid is right; the pixels are missing.
