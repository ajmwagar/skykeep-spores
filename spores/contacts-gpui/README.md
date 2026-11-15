# contacts-gpui

Skykeep Contacts drawn by the Spore itself, on the native path from
[`docs/gpui-spores.md`](https://github.com/fungOS-linux/spores/blob/main/docs/gpui-spores.md).

Status: **builds for `wasm32-wasip2`; not yet rendered, because the runner is missing.**

## What it is

A GPUI view — rail of names, white card with the person and the fields they actually have —
reading `fungos.contacts/contact` from `soil:records` and presenting through
`soil:present/surface`. The look is the one macOS Contacts and Skykeep's contacts window
share; the fonts travel in the package (Lilex), because a component has no system fonts.

```
cargo build --release --target wasm32-wasip2
# → target/wasm32-wasip2/release/contacts_gpui.wasm
```

## What it needs to run

`soil spore run <dir> --surface frame --frame /tmp/contacts.png --size 1200x800` is the
invocation the GPUI docs describe. Nothing in the soil or spores checkouts implements that
subcommand yet — the pieces exist (`soil-webgpu` presents, `canvas-winit` hosts a window) but
the runner that connects a package to a surface does not. So this crate is verified to
*compile* and not verified to *draw*.

Three things are open, in order:

1. **The runner.** `soil spore run <package> --surface frame|window` that instantiates a
   `runner`-world component, provides `wasi:webgpu` + `soil:present`, and writes the frame.
2. **The manifest.** No GPUI spore has a `spore.toml` yet (`terminal-gpui` and `probe-gpui`
   are crates, not packages). A package needs to declare its `runner` module and the
   `native` surface, and `soil-spore` needs to accept that world.
3. **Records for a runner world.** The host that runs this must link `soil:records` into the
   guest beside `soil:present`; without it the component instantiates nothing.

Until (1) lands, the Canvas spores in `spores/icloud-contacts` are the ones you can install
and open.

## Attribution

The glue — resolving the wasi:webgpu instance, the headless GPUI context, the frame readback
and `surface::present` — follows `spores/probe-gpui` and `spores/terminal-gpui` in
`fungOS-linux/spores` (MIT). The view and the records reading are this package's, and it is
GPL-3.0-only with the rest of this fork.
