# contacts-gpui

Skykeep Contacts drawn by the Spore itself, on the native path from
[`docs/gpui-spores.md`](https://github.com/fungOS-linux/spores/blob/main/docs/gpui-spores.md).

Status: **renders**. `1200×800`, one frame, through `soil-frame` — the first
screenshot in this repository that a GPUI Spore drew itself.

![Skykeep Contacts as a GPUI Spore](../docs/media/spore-contacts-gpui.png)

The picture above is the empty state: that run granted no records, so the rail says so
instead of drawing a blank window. Point `--records` at a `fungos-records` root with an
iCloud book and the same view fills in.

## What it is

A GPUI view — rail of names, white card with the person and the fields they actually have —
reading `fungos.contacts/contact` from `soil:records` and presenting through
`soil:present/surface`. The look is the one macOS Contacts and Skykeep's contacts window
share; the fonts travel in the package (Lilex), because a component has no system fonts.

```
cargo build --release --target wasm32-wasip2
# → target/wasm32-wasip2/release/contacts_gpui.wasm

soil-frame target/wasm32-wasip2/release/contacts_gpui.wasm \
  --frame /tmp/contacts.png --size 1200x800
# → /tmp/contacts.png 1200x800 status 0
```

`soil-frame` is the `--surface frame` the GPUI docs describe: a one-shot capture host
that provides `wasi:webgpu`, `soil:present` and the grants the caller names, runs the
guest until it presents, and writes the frame as PNG. It landed in soil as
`crates/soil-webgpu/src/bin/soil-frame.rs`.

## What it still needs

1. **A manifest.** No GPUI Spore has a `spore.toml` yet (`terminal-gpui` and `probe-gpui`
   are crates, not packages). A package has to declare its `runner` module and the `native`
   surface, and `soil-spore` has to accept that world.
2. **The window surface.** `soil-frame` is capture-only: one frame, no input loop. A live
   window is `canvas-winit`'s job and it does not launch GPUI packages yet.
3. **Records on the live path.** `soil-frame --records DIR --book NAME` grants a book
   read-only; a real host has to do the same from its own records root.

Until (1) lands, the Canvas spores in `spores/icloud-contacts` are the ones you can install
and open.

## Attribution

The glue — resolving the wasi:webgpu instance, the headless GPUI context, the frame readback
and `surface::present` — follows `spores/probe-gpui` and `spores/terminal-gpui` in
`fungOS-linux/spores` (MIT). The view and the records reading are this package's, and it is
GPL-3.0-only with the rest of this fork.
