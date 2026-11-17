//! Shapes with edges you cannot see.
//!
//! A frame is pixels, so every surface here is drawn twice: a solid fill and a
//! one-pixel band whose coverage comes from the distance to the edge. That is
//! all "anti-aliased" needs to mean for rectangles and rounded corners.

pub type Rgba = [u8; 4];

pub fn mix(under: Rgba, over: Rgba, amount: f32) -> Rgba {
    let a = amount.clamp(0.0, 1.0);
    [
        (under[0] as f32 * (1.0 - a) + over[0] as f32 * a) as u8,
        (under[1] as f32 * (1.0 - a) + over[1] as f32 * a) as u8,
        (under[2] as f32 * (1.0 - a) + over[2] as f32 * a) as u8,
        0xff,
    ]
}

pub fn fill(canvas: &mut [u8], width: u32, height: u32, x: i32, y: i32, w: i32, h: i32, colour: Rgba) {
    for row in y..(y + h).max(y) {
        if row < 0 || row >= height as i32 {
            continue;
        }
        for column in x..(x + w).max(x) {
            if column < 0 || column >= width as i32 {
                continue;
            }
            let at = ((row as u32 * width + column as u32) as usize) * 4;
            canvas[at..at + 4].copy_from_slice(&colour);
        }
    }
}

pub fn rounded(
    canvas: &mut [u8],
    width: u32,
    height: u32,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    colour: Rgba,
) {
    let radius = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let left = x.floor() as i32 - 1;
    let top = y.floor() as i32 - 1;
    let right = (x + w).ceil() as i32 + 1;
    let bottom = (y + h).ceil() as i32 + 1;
    for row in top..bottom {
        if row < 0 || row >= height as i32 {
            continue;
        }
        for column in left..right {
            if column < 0 || column >= width as i32 {
                continue;
            }
            // Signed distance to a rounded rectangle: negative inside.
            let px = column as f32 + 0.5;
            let py = row as f32 + 0.5;
            let dx = (x + radius - px).max(px - (x + w - radius)).max(0.0);
            let dy = (y + radius - py).max(py - (y + h - radius)).max(0.0);
            let distance = (dx * dx + dy * dy).sqrt() - radius;
            let coverage = (0.5 - distance).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }
            let at = ((row as u32 * width + column as u32) as usize) * 4;
            for channel in 0..3 {
                let under = canvas[at + channel] as f32;
                let over = colour[channel] as f32;
                canvas[at + channel] = (under * (1.0 - coverage) + over * coverage) as u8;
            }
            canvas[at + 3] = 0xff;
        }
    }
}
