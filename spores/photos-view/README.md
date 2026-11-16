# photos-view

The photo library as pictures, drawn by the Spore.

It reads `fungos.photos/asset@1` from `soil:records`, and every tile is fetched the way a
record can name it — by identity, not by path:

```
record  thumb.content_id = "blake3:…"
   │  soil:vfs-cid   find(cid)        → paths the Spore could already list
   │  soil:vfs       read(path)       → bytes, in the host's range budget
   │  (this Spore)   decode, lay out  → one RGBA frame
   └  soil:present   present_rgba(…)  → the window, or one captured PNG
```

No GPU, no element tree, no font: the frame is rasterized in software and handed over
through `present-rgba`, which is the arm `soil:present` exists for. A tile that stays grey is
a record whose bytes are not in this workspace — the preview was never imported, or the
original is still in iCloud.

## Running it

```sh
# The records and the bytes: one projects the library, the other puts the previews in a
# workspace under the content ids the records name.
quince-soil macos-photos --library "$HOME/Pictures/Photos Library.photoslibrary" \
                         --chunks /tmp/photos
soil --root ~/.local/share/fungos/vfs content import --manifest /tmp/photos/content.json
for part in /tmp/photos/part-*.json; do
  curl -sS -X POST http://127.0.0.1:8791/api/records/batch \
       -H "Authorization: Bearer $SOIL_RECORDS_TOKEN" --data-binary @"$part"
done

# One frame, captured.
soil spore run . --surface frame --frame /tmp/photos.png --size 1200x800 \
     --records ~/.local/share/fungos/records --vfs-root ~/.local/share/fungos/vfs
```

`[records]` declares the collection and the book, so a host grants the read without an
operator retyping a schema; `soil:vfs` and `soil:vfs-cid` are named in `[grants]` because a
view that cannot find its bytes draws nothing.

## What it does not do yet

Text (a tile has no date or album name), hydration (a placeholder is not a download), and
thumbnails sized for the tile rather than the screen — the guest decodes a ~2048px preview
per tile, which is why a frame takes about ten seconds on a library of 22,000 records.
