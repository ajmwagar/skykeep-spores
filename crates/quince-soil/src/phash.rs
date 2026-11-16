//! A perceptual hash: the picture, not the file.
//!
//! `blake3` answers "are these the same bytes", which is the right question for a content
//! store and the wrong one for a library. The same photo arrives as a HEIC from an iPhone, a
//! JPEG re-encoded by Google Photos, and a 2048-pixel preview the Mac already cached: three
//! files, one picture. A perceptual hash is what makes them one asset with three sources.
//!
//! This is dHash (difference hash) at 9×8: resample to nine columns by eight rows of
//! brightness, then record, for each row, whether each pixel is brighter than the one to its
//! right. Sixty-four bits, invariant to size, re-encoding and small shifts in colour — which
//! is exactly the kind of difference a phone upgrade introduces.
//!
//! It is not a fingerprint of the subject: two photos of the same beach an hour apart are
//! close, two photos of the same person against different walls are not. Near-duplicate
//! grouping compares the bits by Hamming distance and never claims identity — the content id
//! still decides whether two records are the same bytes.

use std::io::Cursor;

/// Columns of a dHash: one more than the bits across, because each bit compares a pair.
const DHASH_COLUMNS: usize = 9;
/// Rows of a dHash, and so the bits across it.
const DHASH_ROWS: usize = 8;

/// The hash, spelled so a record says which algorithm produced it.
pub fn tag(bits: u64) -> String {
    format!("dhash64:{bits:016x}")
}

/// dHash of a JPEG, or `None` when the bytes are not a picture we can read.
///
/// A thumbnail we cannot decode is not an error: the record is still worth storing, and a
/// hash that is `null` says "not computed" rather than inventing a value that would make
/// every undecodable photo look like every other one.
pub fn of_jpeg(bytes: &[u8]) -> Option<u64> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    let pixels = decoder.decode().ok()?;
    let info = decoder.info()?;
    let (width, height) = (info.width as usize, info.height as usize);
    if width == 0 || height == 0 {
        return None;
    }
    let gray = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => pixels,
        // L16 and CMYK are not previews we meet; anything else is RGB24.
        jpeg_decoder::PixelFormat::RGB24 => luma(&pixels),
        _ => return None,
    };
    if gray.len() < width * height {
        return None;
    }
    Some(of_gray(&gray, width, height))
}

/// dHash of an 8-bit grayscale raster, row-major.
///
/// Split out from [`of_jpeg`] so the comparison itself is testable without an encoder: the
/// test builds the pixels, and the decoder stays out of the assertion.
pub fn of_gray(gray: &[u8], width: usize, height: usize) -> u64 {
    let sample = resample(gray, width, height, DHASH_COLUMNS, DHASH_ROWS);
    let mut bits = 0u64;
    for row in 0..DHASH_ROWS {
        for column in 0..DHASH_COLUMNS - 1 {
            let left = sample[row * DHASH_COLUMNS + column];
            let right = sample[row * DHASH_COLUMNS + column + 1];
            bits = (bits << 1) | u64::from(left > right);
        }
    }
    bits
}

/// How many bits two hashes disagree on.
///
/// Eight of sixty-four is the usual near-duplicate threshold: re-encodes and resizes stay
/// under it, different pictures land well above.
pub fn distance(left: u64, right: u64) -> u32 {
    (left ^ right).count_ones()
}

/// Box-average a raster down to `columns` × `rows`.
///
/// Averaging (rather than point sampling) is what makes the hash survive a resize: a preview
/// and its original have the same average brightness in the same cells even at different
/// scales.
fn resample(gray: &[u8], width: usize, height: usize, columns: usize, rows: usize) -> Vec<u8> {
    let mut out = vec![0u8; columns * rows];
    for row in 0..rows {
        let y0 = row * height / rows;
        let y1 = ((row + 1) * height / rows).max(y0 + 1).min(height);
        for column in 0..columns {
            let x0 = column * width / columns;
            let x1 = ((column + 1) * width / columns).max(x0 + 1).min(width);
            let mut sum = 0u64;
            let mut count = 0u64;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += u64::from(gray[y * width + x]);
                    count += 1;
                }
            }
            out[row * columns + column] = (sum / count.max(1)) as u8;
        }
    }
    out
}

fn luma(rgb: &[u8]) -> Vec<u8> {
    rgb.as_chunks::<3>()
        .0
        .iter()
        .map(|[red, green, blue]| {
            // Rec. 601: what a JPEG decoder's own grayscale conversion uses.
            ((299 * u32::from(*red) + 587 * u32::from(*green) + 114 * u32::from(*blue)) / 1000)
                as u8
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A raster that is bright on the left and dark on the right, whatever its size.
    fn split(width: usize, height: usize) -> Vec<u8> {
        let mut gray = vec![0u8; width * height];
        for y in 0..height {
            for x in 0..width {
                gray[y * width + x] = if x < width / 2 { 240 } else { 16 };
            }
        }
        gray
    }

    #[test]
    fn the_same_picture_at_two_sizes_has_the_same_hash() {
        let small = of_gray(&split(90, 80), 90, 80);
        let large = of_gray(&split(1800, 1600), 1800, 1600);
        assert_eq!(distance(small, large), 0);
    }

    #[test]
    fn different_pictures_are_far_apart() {
        // Two ramps, one rising left to right and one falling: every comparison in one is
        // the opposite of the other, which is as far apart as two pictures get.
        let rising: Vec<u8> = (0..80)
            .flat_map(|_| (0..90).map(|x| x as u8).collect::<Vec<_>>())
            .collect();
        let falling: Vec<u8> = (0..80)
            .flat_map(|_| (0..90).map(|x| (89 - x) as u8).collect::<Vec<_>>())
            .collect();
        assert_eq!(
            distance(of_gray(&rising, 90, 80), of_gray(&falling, 90, 80)),
            64
        );
    }

    #[test]
    fn bytes_that_are_not_a_jpeg_are_null_not_an_error() {
        assert_eq!(of_jpeg(b"not a picture"), None);
    }

    #[test]
    fn a_hash_says_which_algorithm_made_it() {
        assert_eq!(tag(0x1234), "dhash64:0000000000001234");
        assert_eq!(tag(u64::MAX), "dhash64:ffffffffffffffff");
    }

    #[test]
    fn a_flat_image_is_all_zeroes() {
        assert_eq!(of_gray(&vec![7u8; 90 * 80], 90, 80), 0);
    }
}
