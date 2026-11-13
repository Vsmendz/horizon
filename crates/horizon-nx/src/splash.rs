//! Startup artwork and the single-owner handoff from libnx to Vulkan.

const WIDTH: usize = 1280;
const HEIGHT: usize = 720;
const BG: u32 = 0xff241a10;
const MINT: u32 = 0xff9ae479;
const TEXT: u32 = 0xffeaf2e8;

#[cfg(target_os = "horizon")]
mod native;
#[cfg(target_os = "horizon")]
pub(crate) use native::mark_boot;
#[cfg(target_os = "horizon")]
pub use native::{active, close, draw, open, stamp};

fn paint(pixels: &mut [u32], stride: usize, stage: &str, ms: u64) {
    for row in pixels.chunks_exact_mut(stride).take(HEIGHT) {
        row[..WIDTH].fill(BG);
    }
    let mut rect = |x: usize, y: usize, w: usize, h: usize, color| {
        for row in y..y + h {
            pixels[row * stride + x..row * stride + x + w].fill(color);
        }
    };
    // Original stepped horizon mark.
    rect(432, 227, 14, 39, MINT);
    rect(446, 214, 14, 52, MINT);
    rect(460, 201, 14, 65, MINT);
    text(&mut rect, 508, 215, 7, "HORIZON NX", TEXT);
    text(&mut rect, 514, 284, 3, "ENHANCED EDITION", MINT);
    rect(384, 440, 512, 8, 0xff514037);
    let phase = (ms / 12 % 800) as usize;
    rect(384 + phase.min(800 - phase), 440, 112, 8, MINT);
    let length = stage.chars().count().min(40);
    text(&mut rect, 640 - length * 9, 476, 3, stage, TEXT);
}

fn text(
    rect: &mut impl FnMut(usize, usize, usize, usize, u32),
    x: usize,
    y: usize,
    scale: usize,
    line: &str,
    color: u32,
) {
    for (i, letter) in line.bytes().take(40).enumerate() {
        let glyph = match letter.to_ascii_uppercase() {
            b'A' => [14, 17, 17, 31, 17, 17, 17],
            b'B' => [30, 17, 17, 30, 17, 17, 30],
            b'C' => [14, 17, 16, 16, 16, 17, 14],
            b'D' => [30, 17, 17, 17, 17, 17, 30],
            b'E' => [31, 16, 16, 30, 16, 16, 31],
            b'F' => [31, 16, 16, 30, 16, 16, 16],
            b'G' => [14, 17, 16, 23, 17, 17, 15],
            b'H' => [17, 17, 17, 31, 17, 17, 17],
            b'I' => [31, 4, 4, 4, 4, 4, 31],
            b'J' => [7, 2, 2, 2, 18, 18, 12],
            b'K' => [17, 18, 20, 24, 20, 18, 17],
            b'L' => [16, 16, 16, 16, 16, 16, 31],
            b'M' => [17, 27, 21, 21, 17, 17, 17],
            b'N' => [17, 25, 21, 19, 17, 17, 17],
            b'O' => [14, 17, 17, 17, 17, 17, 14],
            b'P' => [30, 17, 17, 30, 16, 16, 16],
            b'Q' => [14, 17, 17, 17, 21, 18, 13],
            b'R' => [30, 17, 17, 30, 20, 18, 17],
            b'S' => [15, 16, 16, 14, 1, 1, 30],
            b'T' => [31, 4, 4, 4, 4, 4, 4],
            b'U' => [17, 17, 17, 17, 17, 17, 14],
            b'V' => [17, 17, 17, 17, 17, 10, 4],
            b'W' => [17, 17, 17, 21, 21, 21, 10],
            b'X' => [17, 17, 10, 4, 10, 17, 17],
            b'Y' => [17, 17, 10, 4, 4, 4, 4],
            b'Z' => [31, 1, 2, 4, 8, 16, 31],
            _ => [0; 7],
        };
        for (row, bits) in glyph.into_iter().enumerate() {
            for col in 0..5 {
                if bits & (16 >> col) != 0 {
                    rect(
                        x + (i * 6 + col) * scale,
                        y + row * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paints_only_visible_pixels_and_moves_the_progress_segment() {
        let stride = WIDTH + 32;
        let mut pixels = vec![0; stride * HEIGHT];
        paint(&mut pixels, stride, "LOADING MENU", 0);
        assert_eq!(pixels[0], BG);
        assert_eq!(pixels[227 * stride + 432], MINT);
        assert_eq!(pixels[440 * stride + 384], MINT);
        assert!(
            pixels
                .chunks_exact(stride)
                .all(|row| row[WIDTH..].iter().all(|p| *p == 0))
        );
        paint(&mut pixels, stride, "LOADING MENU", 4800);
        assert_ne!(pixels[440 * stride + 384], MINT);
        assert_eq!(pixels[440 * stride + 784], MINT);
    }
}
