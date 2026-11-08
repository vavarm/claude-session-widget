use crate::sessions::{Kind, Session};
use tauri::image::Image;

const SIZE: u32 = 32;
const BADGE_CENTER: f32 = 24.0;
const BADGE_RADIUS: f32 = 7.0;
const RING_RADIUS: f32 = 9.0;
const RING_COLOR: [u8; 3] = [24, 24, 27];

pub fn color(sessions: &[Session]) -> [u8; 3] {
    let any = |kind| sessions.iter().any(|s| s.kind == kind);
    if any(Kind::Attention) {
        [248, 81, 73]
    } else if any(Kind::Busy) {
        [245, 165, 36]
    } else if sessions.is_empty() {
        [130, 130, 140]
    } else {
        [63, 185, 80]
    }
}

pub fn base_icon() -> Image<'static> {
    Image::from_bytes(include_bytes!("../icons/icon.png")).expect("icons/icon.png is a valid PNG")
}

// Averages the source pixels covered by each target pixel, weighted by alpha so transparent edges do not darken.
fn downscale(base: &Image<'_>) -> Vec<[f32; 4]> {
    let (sw, sh) = (base.width() as usize, base.height() as usize);
    let rgba = base.rgba();
    let size = SIZE as usize;
    let mut out = vec![[0.0; 4]; size * size];
    for ty in 0..size {
        for tx in 0..size {
            let (x0, x1) = (
                tx * sw / size,
                ((tx + 1) * sw / size).max(tx * sw / size + 1),
            );
            let (y0, y1) = (
                ty * sh / size,
                ((ty + 1) * sh / size).max(ty * sh / size + 1),
            );
            let (mut r, mut g, mut b, mut a, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for y in y0..y1.min(sh) {
                for x in x0..x1.min(sw) {
                    let p = &rgba[(y * sw + x) * 4..][..4];
                    let alpha = f32::from(p[3]) / 255.0;
                    r += f32::from(p[0]) * alpha;
                    g += f32::from(p[1]) * alpha;
                    b += f32::from(p[2]) * alpha;
                    a += alpha;
                    n += 1.0;
                }
            }
            if a > 0.0 {
                out[ty * size + tx] = [r / a, g / a, b / a, a / n];
            }
        }
    }
    out
}

fn blend(pixel: &mut [f32; 4], color: [u8; 3], coverage: f32) {
    let out_alpha = coverage + pixel[3] * (1.0 - coverage);
    if out_alpha <= 0.0 {
        return;
    }
    for channel in 0..3 {
        pixel[channel] = (f32::from(color[channel]) * coverage
            + pixel[channel] * pixel[3] * (1.0 - coverage))
            / out_alpha;
    }
    pixel[3] = out_alpha;
}

// The app icon with a dot in the corner showing the most urgent state.
pub fn icon(base: &Image<'_>, state: [u8; 3]) -> Image<'static> {
    let mut pixels = downscale(base);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - BADGE_CENTER;
            let dy = y as f32 + 0.5 - BADGE_CENTER;
            let distance = (dx * dx + dy * dy).sqrt();
            let pixel = &mut pixels[(y * SIZE + x) as usize];
            blend(
                pixel,
                RING_COLOR,
                (RING_RADIUS - distance + 0.5).clamp(0.0, 1.0),
            );
            blend(
                pixel,
                state,
                (BADGE_RADIUS - distance + 0.5).clamp(0.0, 1.0),
            );
        }
    }
    let rgba = pixels
        .iter()
        .flat_map(|p| {
            [
                p[0].round() as u8,
                p[1].round() as u8,
                p[2].round() as u8,
                (p[3] * 255.0).round() as u8,
            ]
        })
        .collect();
    Image::new_owned(rgba, SIZE, SIZE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(image: &Image<'_>, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * image.width() + x) * 4) as usize;
        image.rgba()[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn badge_takes_the_state_colour_and_the_rest_keeps_the_icon() {
        let base = Image::new_owned([40u8, 80, 120, 255].repeat(64 * 64), 64, 64);
        let result = icon(&base, [248, 81, 73]);
        assert_eq!((result.width(), result.height()), (32, 32));
        assert_eq!(pixel(&result, 24, 24), [248, 81, 73, 255]);
        assert_eq!(pixel(&result, 2, 2), [40, 80, 120, 255]);
    }

    #[test]
    fn badge_shows_even_on_a_transparent_icon() {
        let base = Image::new_owned([0u8; 4].repeat(64 * 64), 64, 64);
        let result = icon(&base, [63, 185, 80]);
        assert_eq!(pixel(&result, 24, 24), [63, 185, 80, 255]);
        assert_eq!(pixel(&result, 2, 2)[3], 0);
    }

    #[test]
    fn the_bundled_icon_decodes() {
        let result = icon(&base_icon(), [130, 130, 140]);
        assert_eq!((result.width(), result.height()), (32, 32));
    }

    #[test]
    fn colour_follows_the_most_urgent_session() {
        assert_eq!(color(&[]), [130, 130, 140]);
    }
}
