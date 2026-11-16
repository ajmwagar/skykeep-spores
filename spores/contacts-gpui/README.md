# contacts-gpui

Skykeep Contacts drawn by the Spore itself, on the native path from
[`docs/gpui-spores.md`](https://github.com/fungOS-linux/spores/blob/main/docs/gpui-spores.md).

Status: **renders**. `1200×800`, one frame, through soil's runner — the first
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

soil spore run . --surface frame --frame /tmp/contacts.png --size 1200x800
# → /tmp/contacts.png 1200x800
```

`--surface frame` is the capture surface of soil's one runner (`crates/soil-shell`):
it provides `wasi:webgpu`, `soil:present` and the grants the package asks for, runs the
guest until it presents, and writes the frame. The earlier per-surface binaries
(`soil-webgpu --frame`, `canvas-winit`, `soil-web-bridge`) were folded into it by
`692ef00 runner: one binary, one surface set`.

## What it still needs

1. **A manifest.** No GPUI Spore has a `spore.toml` yet (`terminal-gpui` and `probe-gpui`
   are crates, not packages). A package has to declare its `runner` module and the `native`
   surface, and `soil-spore` has to accept that world.
2. **The window surface.** `--surface frame` is capture-only: one frame, no input loop. A live
   window is `canvas-winit`'s job and it does not launch GPUI packages yet.
3. **Records on the live path.** The package declares its collection in `[records]`,
   so a host grants it from its own records root — no `--collection` for anyone to type.
   A launcher that passes a records root gets the address book; without one, it gets the
   empty state.

Until (1) lands, the Canvas spores in `spores/icloud-contacts` are the ones you can install
and open.

## Attribution

The glue — resolving the wasi:webgpu instance, the headless GPUI context, the frame readback
and `surface::present` — follows `spores/probe-gpui` and `spores/terminal-gpui` in
`fungOS-linux/spores` (MIT). The view and the records reading are this package's, and it is
GPL-3.0-only with the rest of this fork.
