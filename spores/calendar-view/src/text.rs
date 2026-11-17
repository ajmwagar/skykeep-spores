//! Words, rasterized in the guest.
//!
//! `fontdue` turns Inter's outlines into coverage bitmaps at any size, so the
//! frame keeps one typeface instead of a hand-drawn font that would only match
//! itself. Inter ships under the OFL (see `assets/Inter-OFL.txt`).

use fontdue::{Font, FontSettings};

pub const REGULAR: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");
pub const MEDIUM: &[u8] = include_bytes!("../assets/Inter-Medium.ttf");

pub struct Face {
    font: Font,
}

impl Face {
    pub fn regular() -> Self {
        Self::from_bytes(REGULAR)
    }

    pub fn medium() -> Self {
        Self::from_bytes(MEDIUM)
    }

    fn from_bytes(bytes: &[u8]) -> Self {
        Self {
            font: Font::from_bytes(bytes, FontSettings::default()).expect("an embedded face"),
        }
    }

    /// Distance from the top of a line to its baseline.
    fn ascent(&self, size: f32) -> i32 {
        self.font
            .horizontal_line_metrics(size)
            .map(|metrics| metrics.ascent.round() as i32)
            .unwrap_or((size * 0.8) as i32)
    }

    pub fn width(&self, text: &str, size: f32) -> f32 {
        text.chars()
            .map(|character| self.font.metrics(character, size).advance_width)
            .sum()
    }

    /// Draw `text` with its top-left at (`x`, `y`), and answer the x it ended at.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        size: f32,
        colour: [u8; 4],
        text: &str,
    ) -> i32 {
        let baseline = y + self.ascent(size);
        let mut pen = x as f32;
        for character in text.chars() {
            let (metrics, coverage) = self.font.rasterize(character, size);
            let left = pen as i32 + metrics.xmin;
            // `ymin` is the glyph's bottom measured up from the baseline, so the
            // bitmap's first row sits that far below the baseline minus its own
            // height. Anchoring on `ymin` alone put descenders a line too high,
            // which reads as a stray mark above the letter before them.
            let top = baseline - metrics.ymin - metrics.height as i32;
            for row in 0..metrics.height {
                for column in 0..metrics.width {
                    let alpha = coverage[row * metrics.width + column];
                    if alpha == 0 {
                        continue;
                    }
                    blend(
                        canvas,
                        width,
                        height,
                        left + column as i32,
                        top + row as i32,
                        colour,
                        alpha,
                    );
                }
            }
            pen += metrics.advance_width;
        }
        pen as i32
    }

    /// The same line, truncated with an ellipsis when it would not fit.
    pub fn draw_clipped(
        &self,
        canvas: &mut [u8],
        width: u32,
        height: u32,
        x: i32,
        y: i32,
        size: f32,
        room: f32,
        colour: [u8; 4],
        text: &str,
    ) {
        if self.width(text, size) <= room {
            self.draw(canvas, width, height, x, y, size, colour, text);
            return;
        }
        let mut clipped = String::new();
        for character in text.chars() {
            let mut next = clipped.clone();
            next.push(character);
            next.push('…');
            if self.width(&next, size) > room {
                break;
            }
            clipped.push(character);
        }
        clipped.push('…');
        self.draw(canvas, width, height, x, y, size, colour, &clipped);
    }
}

fn blend(canvas: &mut [u8], width: u32, height: u32, x: i32, y: i32, colour: [u8; 4], alpha: u8) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let at = ((y as u32 * width + x as u32) as usize) * 4;
    let coverage = u16::from(alpha);
    for channel in 0..3 {
        let under = u16::from(canvas[at + channel]);
        let over = u16::from(colour[channel]);
        canvas[at + channel] = ((under * (255 - coverage) + over * coverage) / 255) as u8;
    }
    canvas[at + 3] = 0xff;
}
