use crate::asset::Asset;
use crate::spec::{AssetKind, AssetRequest};

pub fn generate(request: &AssetRequest, seed: u64) -> Vec<Asset> {
    let (size, count) = match request.kind {
        AssetKind::Sprite { size } => (size, 1),
        AssetKind::Tileset { tile_size, count } => (tile_size, count),
    };

    // Guard against unvalidated cards with an empty palette so the modulo below
    // can never divide by zero; StyleCard::validate enforces >= 2 for real input.
    let palette_len = request.style_card.palette.len().max(1);
    let pixel_count = size.w as usize * size.h as usize;

    let mut rng = SplitMix64::new(seed);
    let mut assets = Vec::with_capacity(count);

    for _ in 0..count {
        let mut pixels = Vec::with_capacity(pixel_count);
        for _ in 0..pixel_count {
            pixels.push((rng.next() % palette_len as u64) as u8);
        }
        assets.push(Asset {
            style_card_id: request.style_card.id.clone(),
            width: size.w,
            height: size.h,
            pixels,
        });
    }

    assets
}

struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Dimensions, Grid, Outline, PaletteColor, Shading, Style, StyleCard};

    fn card() -> StyleCard {
        StyleCard {
            id: "card-1".to_string(),
            name: "card".to_string(),
            palette: vec![
                PaletteColor {
                    name: "a".to_string(),
                    hex: "#000000".to_string(),
                },
                PaletteColor {
                    name: "b".to_string(),
                    hex: "#ffffff".to_string(),
                },
                PaletteColor {
                    name: "c".to_string(),
                    hex: "#ff0000".to_string(),
                },
            ],
            grid: Grid {
                tile_size: Dimensions { w: 16, h: 16 },
                unit_scale: 1,
            },
            style: Style {
                outline: Outline::None,
                outline_color: "#000000".to_string(),
                shading: Shading::Flat,
                proportions: "1:1".to_string(),
                era: "modern".to_string(),
                theme: "pixel".to_string(),
            },
        }
    }

    fn tileset_request() -> AssetRequest {
        AssetRequest {
            name: "tiles".to_string(),
            kind: AssetKind::Tileset {
                tile_size: Dimensions { w: 8, h: 8 },
                count: 4,
            },
            style_card: card(),
        }
    }

    fn sprite_request() -> AssetRequest {
        AssetRequest {
            name: "hero".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w: 16, h: 16 },
            },
            style_card: card(),
        }
    }

    #[test]
    fn deterministic_for_same_seed() {
        let req = tileset_request();
        assert_eq!(generate(&req, 42), generate(&req, 42));
    }

    #[test]
    fn tileset_produces_valid_assets() {
        let req = tileset_request();
        let out = generate(&req, 7);
        assert_eq!(out.len(), 4);
        let palette_len = req.style_card.palette.len();
        for asset in &out {
            assert_eq!(asset.width, 8);
            assert_eq!(asset.height, 8);
            assert_eq!(asset.pixels.len(), 64);
            assert_eq!(asset.style_card_id, req.style_card.id);
            assert!(asset.pixels.iter().all(|&i| (i as usize) < palette_len));
        }
    }

    #[test]
    fn sprite_produces_one_asset() {
        let req = sprite_request();
        let out = generate(&req, 1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].width, 16);
        assert_eq!(out[0].height, 16);
        assert_eq!(out[0].pixels.len(), 256);
    }

    #[test]
    fn extreme_seeds_do_not_panic() {
        let req = sprite_request();
        assert_eq!(generate(&req, 0).len(), 1);
        assert_eq!(generate(&req, u64::MAX).len(), 1);
    }

    #[test]
    fn empty_palette_does_not_panic() {
        let mut req = sprite_request();
        req.style_card.palette.clear();
        let out = generate(&req, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].pixels.len(), 256);
    }
}
