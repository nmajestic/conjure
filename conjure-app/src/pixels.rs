use conjure_core::{Asset, StyleCard};

const MISSING: [u8; 4] = [255, 0, 255, 255];

/// Hard cap on composed preview pixels (1 << 24 = 16,777,216 px = 64 MiB RGBA).
/// The export cap guards sheets; this guards the viewer. Like every other cap
/// in Conjure we refuse rather than clamp: a legal stored asset at high zoom
/// must never allocate gigabytes before a failed u32 cast ever surfaces.
pub const MAX_PREVIEW_PIXELS: u64 = 1 << 24;

pub struct Preview {
    pub pixels: rune::Pixels,
    pub w: u32,
    pub h: u32,
    pub zoom: u32,
    pub palette: Vec<[u8; 4]>,
}

pub fn compose(asset: &Asset, card: &StyleCard, zoom: u32) -> Option<Preview> {
    // All arithmetic in u64 before any u32 cast; passing the cap also bounds
    // each dimension below 2^32, so the casts back down cannot truncate.
    let w64 = (asset.width as u64) * zoom as u64;
    let h64 = (asset.height as u64) * zoom as u64;
    if w64 * h64 > MAX_PREVIEW_PIXELS {
        return None;
    }
    let (w, h) = (w64 as u32, h64 as u32);
    let palette = palette_rgba(card);
    let rgba = compose_buffer(asset, &palette, zoom);
    let pixels = rune::Pixels::new();
    pixels.set(w, h, rgba);
    Some(Preview {
        pixels,
        w,
        h,
        zoom,
        palette,
    })
}

fn palette_rgba(card: &StyleCard) -> Vec<[u8; 4]> {
    card.palette
        .iter()
        .enumerate()
        .map(|(i, color)| match i {
            0 => [0, 0, 0, 0],
            _ => color.rgb().map_or(MISSING, |[r, g, b]| [r, g, b, 255]),
        })
        .collect()
}

fn compose_buffer(asset: &Asset, palette: &[[u8; 4]], zoom: u32) -> Vec<u8> {
    let mut rgba = Vec::new();
    for y in 0..asset.height {
        for _ in 0..zoom {
            for x in 0..asset.width {
                let index = asset.pixels[(y as usize) * asset.width as usize + x as usize] as usize;
                let color = palette.get(index).copied().unwrap_or(MISSING);
                for _ in 0..zoom {
                    rgba.extend_from_slice(&color);
                }
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use conjure_core::style::{Dimensions, Grid, Outline, PaletteColor, Shading, Style};
    use conjure_core::{Asset, StyleCard};

    use super::*;

    fn card() -> StyleCard {
        StyleCard {
            id: "card".into(),
            name: "card".into(),
            palette: vec![
                PaletteColor {
                    name: "bg".into(),
                    hex: "#000000".into(),
                },
                PaletteColor {
                    name: "red".into(),
                    hex: "#ff0000".into(),
                },
                PaletteColor {
                    name: "green".into(),
                    hex: "#00ff00".into(),
                },
            ],
            grid: Grid {
                tile_size: Dimensions { w: 4, h: 4 },
                unit_scale: 1,
            },
            style: Style {
                outline: Outline::None,
                outline_color: "#000000".into(),
                shading: Shading::Flat,
                proportions: "1:1".into(),
                era: "modern".into(),
                theme: "test".into(),
            },
        }
    }

    fn asset(pixels: &[u8]) -> Asset {
        Asset {
            style_card_id: "card".into(),
            width: 3,
            height: 2,
            pixels: pixels.to_vec(),
        }
    }

    #[test]
    fn exact_rgba_for_a_3x2_grid() {
        let palette = palette_rgba(&card());
        let rgba = compose_buffer(&asset(&[0, 1, 2, 2, 1, 0]), &palette, 1);
        let expected: Vec<u8> = [
            [0, 0, 0, 0],
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 255, 0, 255],
            [255, 0, 0, 255],
            [0, 0, 0, 0],
        ]
        .concat();
        assert_eq!(rgba, expected);
    }

    #[test]
    fn index_zero_is_transparent() {
        let palette = palette_rgba(&card());
        assert_eq!(palette[0], [0, 0, 0, 0]);
        let rgba = compose_buffer(&asset(&[0, 0, 0, 0, 0, 0]), &palette, 1);
        assert_eq!(rgba, vec![0; 24]);
    }

    #[test]
    fn zoom_two_repeats_each_pixel_as_a_block() {
        let palette = palette_rgba(&card());
        let rgba = compose_buffer(&asset(&[1, 0, 0, 0, 0, 0]), &palette, 2);
        assert_eq!(rgba.len(), 96);
        let t = [0u8, 0, 0, 0];
        let red = [255u8, 0, 0, 255];
        let expected: Vec<u8> = [
            red, red, t, t, t, t, red, red, t, t, t, t, t, t, t, t, t, t, t, t, t, t, t, t,
        ]
        .concat();
        assert_eq!(rgba, expected);
    }

    #[test]
    fn palette_order_is_preserved_and_dims_report_the_zoomed_size() {
        let preview = compose(&asset(&[0, 1, 2, 2, 1, 0]), &card(), 1).unwrap();
        assert_eq!(
            preview.palette,
            vec![[0, 0, 0, 0], [255, 0, 0, 255], [0, 255, 0, 255]]
        );
        assert_eq!(preview.w, 3);
        assert_eq!(preview.h, 2);
        assert_eq!(preview.zoom, 1);
        assert_eq!(preview.pixels.size(), (3, 2));
    }

    #[test]
    fn invalid_palette_hex_falls_back_to_magenta() {
        let mut bad = card();
        bad.palette[1].hex = "#zzzzzz".into();
        let palette = palette_rgba(&bad);
        assert_eq!(palette[1], [255, 0, 255, 255]);
        let rgba = compose_buffer(&asset(&[1, 1, 1, 1, 1, 1]), &palette, 1);
        let expected: Vec<u8> = [[255u8, 0, 255, 255]; 6].concat();
        assert_eq!(rgba, expected);
    }

    /// Deterministic xorshift32 noise over the card's three palette slots, so
    /// the big-buffer tests exercise real index lookups without randomness.
    fn xorshift_fill(len: usize) -> Vec<u8> {
        let mut state = 0x9e37_79b9u32;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state % 3) as u8
            })
            .collect()
    }

    #[test]
    fn a_2048_square_sprite_composes_at_zoom_one_but_refuses_zoom_eight() {
        let big = Asset {
            style_card_id: "card".into(),
            width: 2048,
            height: 2048,
            pixels: xorshift_fill(2048 * 2048),
        };
        // 4,194,304 composed px is under the cap; only check Some + dims, not
        // the full 16 MiB buffer.
        let preview = compose(&big, &card(), 1).unwrap();
        assert_eq!((preview.w, preview.h, preview.zoom), (2048, 2048, 1));
        // 16,384x16,384 = 268,435,456 px: over the cap, refused.
        assert!(compose(&big, &card(), 8).is_none());
    }

    #[test]
    fn the_cap_boundary_composes_exactly_and_refuses_one_step_over() {
        let at_cap = Asset {
            style_card_id: "card".into(),
            width: 4096,
            height: 4096,
            pixels: vec![0u8; 4096 * 4096],
        };
        assert_eq!(
            (at_cap.width as u64) * at_cap.height as u64,
            MAX_PREVIEW_PIXELS
        );
        let preview = compose(&at_cap, &card(), 1).unwrap();
        assert_eq!((preview.w, preview.h, preview.zoom), (4096, 4096, 1));

        let one_over = Asset {
            style_card_id: "card".into(),
            width: 4097,
            height: 4096,
            pixels: vec![0u8; 4097 * 4096],
        };
        assert!(compose(&one_over, &card(), 1).is_none());
    }
}
