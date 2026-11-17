use crate::asset::Asset;
use crate::spec::{AssetKind, AssetRequest};

pub fn repair(request: &AssetRequest, assets: &[Asset]) -> Vec<Asset> {
    let (size, count) = match request.kind {
        AssetKind::Sprite { size } => (size, 1),
        AssetKind::Tileset { tile_size, count } => (tile_size, count),
    };
    let expected = (size.w as usize) * (size.h as usize);
    let palette_len = request.style_card.palette.len();
    let id = &request.style_card.id;

    (0..count)
        .map(|i| {
            let mut asset = assets.get(i).cloned().unwrap_or_else(|| Asset {
                style_card_id: id.clone(),
                width: size.w,
                height: size.h,
                pixels: Vec::new(),
            });
            asset.width = size.w;
            asset.height = size.h;
            asset.style_card_id = id.clone();
            asset.pixels.resize(expected, 0);
            if palette_len > 0 {
                for p in &mut asset.pixels {
                    *p = ((*p as usize) % palette_len) as u8;
                }
            }
            asset
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::repair;
    use crate::asset::Asset;
    use crate::spec::{AssetKind, AssetRequest};
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
                    hex: "#111111".to_string(),
                },
                PaletteColor {
                    name: "c".to_string(),
                    hex: "#222222".to_string(),
                },
                PaletteColor {
                    name: "d".to_string(),
                    hex: "#333333".to_string(),
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

    fn card_with_palette(len: usize) -> StyleCard {
        let mut c = card();
        c.palette = (0..len)
            .map(|i| PaletteColor {
                name: format!("c{}", i),
                hex: format!("#{:06x}", i * 0x101010),
            })
            .collect();
        c
    }

    #[test]
    fn idempotent() {
        let request = AssetRequest {
            name: "tiles".to_string(),
            kind: AssetKind::Tileset {
                tile_size: Dimensions { w: 2, h: 2 },
                count: 2,
            },
            style_card: card(),
        };
        let input = vec![Asset {
            style_card_id: "wrong".to_string(),
            width: 9,
            height: 9,
            pixels: vec![0, 255, 7, 3, 100],
        }];
        let once = repair(&request, &input);
        let twice = repair(&request, &once);
        assert_eq!(once, twice);
    }

    #[test]
    fn corrupted_input_repairs_to_valid() {
        let request = AssetRequest {
            name: "tiles".to_string(),
            kind: AssetKind::Tileset {
                tile_size: Dimensions { w: 2, h: 2 },
                count: 2,
            },
            style_card: card(),
        };
        let input = vec![Asset {
            style_card_id: "wrong".to_string(),
            width: 3,
            height: 1,
            pixels: vec![0, 255, 9],
        }];
        let out = repair(&request, &input);
        let palette_len = request.style_card.palette.len();
        assert_eq!(out.len(), 2);
        for asset in &out {
            assert_eq!(asset.width, 2);
            assert_eq!(asset.height, 2);
            assert_eq!(asset.pixels.len(), 4);
            assert_eq!(asset.style_card_id, request.style_card.id);
            assert!(asset.pixels.iter().all(|&p| (p as usize) < palette_len));
        }
    }

    #[test]
    fn valid_input_unchanged() {
        let request = AssetRequest {
            name: "sprite".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w: 2, h: 2 },
            },
            style_card: card(),
        };
        let input = vec![Asset {
            style_card_id: "card-1".to_string(),
            width: 2,
            height: 2,
            pixels: vec![0, 1, 2, 3],
        }];
        let out = repair(&request, &input);
        assert_eq!(out, input);
    }

    #[test]
    fn empty_assets_are_padded() {
        let request = AssetRequest {
            name: "tiles".to_string(),
            kind: AssetKind::Tileset {
                tile_size: Dimensions { w: 2, h: 2 },
                count: 3,
            },
            style_card: card(),
        };
        let out = repair(&request, &[]);
        assert_eq!(out.len(), 3);
        for asset in &out {
            assert_eq!(asset.width, 2);
            assert_eq!(asset.height, 2);
            assert_eq!(asset.pixels, vec![0; 4]);
            assert_eq!(asset.style_card_id, request.style_card.id);
        }
    }

    #[test]
    fn empty_palette_leaves_indices_unchanged() {
        let request = AssetRequest {
            name: "sprite".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w: 2, h: 1 },
            },
            style_card: card_with_palette(0),
        };
        let input = vec![Asset {
            style_card_id: "other".to_string(),
            width: 2,
            height: 1,
            pixels: vec![7, 250],
        }];
        let out = repair(&request, &input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].pixels, vec![7, 250]);
        assert_eq!(out[0].style_card_id, request.style_card.id);
    }

    #[test]
    fn palette_256_does_not_divide_by_zero() {
        let request = AssetRequest {
            name: "sprite".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w: 2, h: 2 },
            },
            style_card: card_with_palette(256),
        };
        let input = vec![Asset {
            style_card_id: "other".to_string(),
            width: 2,
            height: 2,
            pixels: vec![0, 255, 128, 1],
        }];
        let out = repair(&request, &input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].pixels.len(), 4);
        assert!(out[0].pixels.iter().all(|&p| (p as usize) < 256));
    }
}
