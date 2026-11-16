//! Skykeep Photos, drawn by the Spore, fetched by content id.
//!
//! A photo record names its picture the way a peer can check it: `blake3:<hex>`, the
//! whole-content identity. It does not name a path, because a path is host state — it moves
//! when a workspace moves, and the record is synced to machines that never had it. So the
//! guest asks where those bytes are (`soil:vfs-cid`), reads them (`soil:vfs`), decodes them
//! itself, and presents one frame (`soil:present`).
//!
//! Nothing here holds authority it was not granted: the cid lookup answers only paths the
//! namespace already shows, the read obeys the host's range budget, and the frame is the
//! extent the host granted. The guest never sees a filesystem path from outside that
//! namespace and never chooses a display.
//!
//! Layout is deliberately a rasterizer, not a UI toolkit: a grid of tiles, newest first, is
//! the whole of a library view and it needs no font, no GPU and no element tree. Text comes
//! when the canvas surface has a text node to draw — see `spores/VFS-CID.md`.

wit_bindgen::generate!({
    path: "wit",
    world: "runner",
    generate_all,
});

use serde_json::Value;
use soil::present::surface;
use soil::records::records;
use soil::vfs::vfs;
use soil::vfs_cid::cid;

const ASSET_SCHEMA: &str = "fungos.photos/asset@1";

/// Records per page. The host bounds it too; this is what the guest asks for.
const PAGE: u32 = 256;

/// The most records one pass will read. A library is tens of thousands and a frame shows
/// twelve; the cap is what keeps a runaway collection from being an unbounded read, and
/// reaching it is reported rather than silently truncating the choice.
const SCAN: usize = 100_000;

/// Tiles on the frame: four across, three down.
const COLS: u32 = 4;
const ROWS: u32 = 3;

const MARGIN: u32 = 28;
const GAP: u32 = 16;

const BACKGROUND: [u8; 4] = [244, 241, 236, 0xff];
const PLACEHOLDER: [u8; 4] = [222, 217, 209, 0xff];

/// One tile's worth of a record.
struct Photo {
    content_id: String,
    taken_at: i64,
}

fn main() -> Result<(), String> {
    let mut drawn: Option<(u32, u32)> = None;
    loop {
        let (width, height) = surface::size();
        if drawn != Some((width, height)) {
            let (photos, note) = load();
            if !note.is_empty() {
                eprintln!("photos-view: {note}");
            }
            let pixels = draw(width, height, &photos);
            if let Err(error) = surface::present_rgba(&surface::RgbaFrame {
                width,
                height,
                pixels,
            }) {
                return Err(format!("present: {error}"));
            }
            eprintln!(
                "photos-view: {} pictures on a {width}x{height} frame",
                photos.len()
            );
            drawn = Some((width, height));
        }
        if !soil::present::input::interactive() {
            return Ok(());
        }
        // A window keeps the last frame until it is resized; a capture host returns above
        // after one.
        let _ = soil::present::input::poll(250);
    }
}

/// The newest photos this frame could resolve, and what went wrong when little did.
fn load() -> (Vec<Photo>, String) {
    let mut photos = Vec::new();
    let mut note = String::new();
    let mut cursor: Option<String> = None;
    let mut scanned = 0usize;
    loop {
        let page = match records::scan(ASSET_SCHEMA, cursor.as_deref(), PAGE) {
            Ok(page) => page,
            Err(error) => {
                note = format!("records: {error:?}");
                break;
            }
        };
        for entry in page.entries {
            scanned += 1;
            if entry.deleted {
                continue;
            }
            let Ok(body) = serde_json::from_slice::<Value>(&entry.body) else {
                continue;
            };
            // A photo the sync half has no preview for cannot be drawn, and a record that
            // says so is not an error: the original is in iCloud and the bytes are not here.
            let Some(content_id) = body
                .get("thumb")
                .and_then(|thumb| thumb.get("content_id"))
                .and_then(Value::as_str)
            else {
                continue;
            };
            if !content_id.starts_with("blake3:") {
                continue;
            }
            photos.push(Photo {
                content_id: content_id.to_string(),
                taken_at: body.get("taken_at").and_then(Value::as_i64).unwrap_or(0),
            });
        }
        match page.next {
            Some(next) if Some(next.as_str()) != cursor.as_deref() => cursor = Some(next),
            _ => break,
        }
        if scanned >= SCAN {
            note = format!("stopped at {scanned} records: the pass is bounded");
            break;
        }
    }
    photos.sort_by(|a, b| {
        b.taken_at
            .cmp(&a.taken_at)
            .then_with(|| a.content_id.cmp(&b.content_id))
    });
    // The same bytes in two records is one picture; a grid should show it once.
    photos.dedup_by(|a, b| a.content_id == b.content_id);
    photos.truncate((COLS * ROWS) as usize);
    (photos, note)
}

/// The bytes behind a content id, or `None` when this workspace does not hold them.
///
/// `find` answers with paths the guest could already have listed, so this is a convenience
/// over `soil:vfs`, not a second kind of authority.
fn bytes_of(content_id: &str) -> Option<Vec<u8>> {
    let page = cid::find(content_id, 0, 8).ok()?;
    let path = page
        .entries
        .iter()
        .find(|entry| entry.kind == vfs::Kind::File)?
        .path
        .clone();
    let mut bytes = Vec::new();
    let mut offset = 0u64;
    loop {
        let chunk = vfs::read(&path, offset, 256 * 1024).ok()?;
        bytes.extend_from_slice(&chunk.data);
        offset += chunk.data.len() as u64;
        if chunk.eof || chunk.data.is_empty() {
            break;
        }
    }
    Some(bytes)
}

/// One decoded picture, in the decoder's own layout.
struct Picture {
    data: Vec<u8>,
    width: u32,
    height: u32,
    rgb: bool,
}

fn picture(content_id: &str) -> Option<Picture> {
    let bytes = bytes_of(content_id)?;
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    let data = decoder.decode().ok()?;
    let info = decoder.info()?;
    let rgb = matches!(info.pixel_format, jpeg_decoder::PixelFormat::RGB24);
    Some(Picture {
        data,
        width: info.width as u32,
        height: info.height as u32,
        rgb,
    })
}

fn draw(width: u32, height: u32, photos: &[Photo]) -> Vec<u8> {
    let mut canvas = vec![0u8; width as usize * height as usize * 4];
    for pixel in canvas.chunks_exact_mut(4) {
        pixel.copy_from_slice(&BACKGROUND);
    }
    let cell_width = (width.saturating_sub(2 * MARGIN + (COLS - 1) * GAP)) / COLS;
    let cell_height = (height.saturating_sub(2 * MARGIN + (ROWS - 1) * GAP)) / ROWS;
    if cell_width == 0 || cell_height == 0 {
        return canvas;
    }
    for (index, photo) in photos.iter().take((COLS * ROWS) as usize).enumerate() {
        let column = index as u32 % COLS;
        let row = index as u32 / COLS;
        let x = MARGIN + column * (cell_width + GAP);
        let y = MARGIN + row * (cell_height + GAP);
        match picture(&photo.content_id) {
            Some(picture) => blit(&mut canvas, width, height, x, y, cell_width, cell_height, &picture),
            // The record names bytes this workspace does not hold: a tile, not a crash.
            None => fill(&mut canvas, width, height, x, y, cell_width, cell_height, PLACEHOLDER),
        }
    }
    canvas
}

/// Draw one picture inside a cell, whole and centred.
///
/// The downsample is a box average: a preview is ~2000 pixels wide and a tile is ~300, so
/// taking one pixel in seven would alias every edge in the library into confetti. Averaging
/// the source rectangle is what makes a thumbnail look like the photo.
fn blit(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    x0: u32,
    y0: u32,
    cell_width: u32,
    cell_height: u32,
    picture: &Picture,
) {
    let scale = f32::min(
        cell_width as f32 / picture.width as f32,
        cell_height as f32 / picture.height as f32,
    );
    let drawn_width = ((picture.width as f32 * scale) as u32).clamp(1, cell_width);
    let drawn_height = ((picture.height as f32 * scale) as u32).clamp(1, cell_height);
    let left = x0 + (cell_width - drawn_width) / 2;
    let top = y0 + (cell_height - drawn_height) / 2;
    let channels = if picture.rgb { 3 } else { 1 };

    for y in 0..drawn_height {
        let source_top = y * picture.height / drawn_height;
        let source_bottom = ((y + 1) * picture.height / drawn_height)
            .max(source_top + 1)
            .min(picture.height);
        for x in 0..drawn_width {
            let source_left = x * picture.width / drawn_width;
            let source_right = ((x + 1) * picture.width / drawn_width)
                .max(source_left + 1)
                .min(picture.width);
            let mut red = 0u32;
            let mut green = 0u32;
            let mut blue = 0u32;
            let mut samples = 0u32;
            for source_y in source_top..source_bottom {
                for source_x in source_left..source_right {
                    let at = ((source_y * picture.width + source_x) as usize) * channels;
                    if picture.rgb {
                        red += picture.data[at] as u32;
                        green += picture.data[at + 1] as u32;
                        blue += picture.data[at + 2] as u32;
                    } else {
                        let luma = picture.data[at] as u32;
                        red += luma;
                        green += luma;
                        blue += luma;
                    }
                    samples += 1;
                }
            }
            let samples = samples.max(1);
            set(
                canvas,
                width,
                height,
                left + x,
                top + y,
                [
                    (red / samples) as u8,
                    (green / samples) as u8,
                    (blue / samples) as u8,
                    0xff,
                ],
            );
        }
    }
}

fn fill(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    x0: u32,
    y0: u32,
    w: u32,
    h: u32,
    color: [u8; 4],
) {
    for y in 0..h {
        for x in 0..w {
            set(canvas, width, height, x0 + x, y0 + y, color);
        }
    }
}

fn set(canvas: &mut [u8], width: u32, height: u32, x: u32, y: u32, color: [u8; 4]) {
    if x >= width || y >= height {
        return;
    }
    let at = ((y * width + x) as usize) * 4;
    canvas[at..at + 4].copy_from_slice(&color);
}

struct Component;

impl Guest for Component {
    async fn run() -> u32 {
        match main() {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("photos-view: {error}");
                1
            }
        }
    }
}

export!(Component);
