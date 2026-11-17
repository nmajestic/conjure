use serde::{Deserialize, Serialize};

use crate::asset::Asset;
use crate::spec::{AssetKind, AssetRequest};
use crate::style::{Dimensions, StyleCard};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticCode {
    DimensionsMismatch,
    PixelCountMismatch,
    PaletteIndexOutOfBounds,
    CountMismatch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: DiagnosticCode,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub passed: bool,
    pub errors: Vec<Diagnostic>,
}

impl ValidationReport {
    pub fn ok() -> Self {
        Self {
            passed: true,
            errors: Vec::new(),
        }
    }
}

pub fn verify_asset(card: &StyleCard, expected: Dimensions, asset: &Asset) -> ValidationReport {
    let mut errors = Vec::new();

    if asset.width != expected.w || asset.height != expected.h {
        errors.push(Diagnostic {
            code: DiagnosticCode::DimensionsMismatch,
            message: format!(
                "asset is {}x{}, expected {}x{}",
                asset.width, asset.height, expected.w, expected.h
            ),
        });
    }

    let pixel_count = asset.width as usize * asset.height as usize;
    if asset.pixels.len() != pixel_count {
        errors.push(Diagnostic {
            code: DiagnosticCode::PixelCountMismatch,
            message: format!(
                "asset declares {}x{} ({} pixels) but has {} indices",
                asset.width,
                asset.height,
                pixel_count,
                asset.pixels.len()
            ),
        });
    }

    let width = asset.width.max(1) as usize;
    for (i, &idx) in asset.pixels.iter().enumerate() {
        if idx as usize >= card.palette.len() {
            errors.push(Diagnostic {
                code: DiagnosticCode::PaletteIndexOutOfBounds,
                message: format!(
                    "pixel ({},{}) uses palette index {}, but palette has {} colors",
                    i % width,
                    i / width,
                    idx,
                    card.palette.len()
                ),
            });
        }
    }

    ValidationReport {
        passed: errors.is_empty(),
        errors,
    }
}

pub fn verify_tileset(
    card: &StyleCard,
    tile_size: Dimensions,
    count: usize,
    tiles: &[Asset],
) -> ValidationReport {
    let mut errors = Vec::new();

    if tiles.len() != count {
        errors.push(Diagnostic {
            code: DiagnosticCode::CountMismatch,
            message: format!("tileset has {} tiles, expected {}", tiles.len(), count),
        });
    }

    for (i, tile) in tiles.iter().enumerate() {
        for mut diagnostic in verify_asset(card, tile_size, tile).errors {
            diagnostic.message = format!("tile {}: {}", i, diagnostic.message);
            errors.push(diagnostic);
        }
    }

    ValidationReport {
        passed: errors.is_empty(),
        errors,
    }
}

pub fn verify_request(request: &AssetRequest, assets: &[Asset]) -> ValidationReport {
    match request.kind {
        AssetKind::Sprite { size } => {
            if assets.len() != 1 {
                return ValidationReport {
                    passed: false,
                    errors: vec![Diagnostic {
                        code: DiagnosticCode::CountMismatch,
                        message: format!(
                            "sprite request produced {} assets, expected 1",
                            assets.len()
                        ),
                    }],
                };
            }
            verify_asset(&request.style_card, size, &assets[0])
        }
        AssetKind::Tileset { tile_size, count } => {
            verify_tileset(&request.style_card, tile_size, count, assets)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Grid, Outline, PaletteColor, Shading, Style};

    fn style_card(palette_len: usize) -> StyleCard {
        let palette = (0..palette_len)
            .map(|i| PaletteColor {
                name: format!("c{}", i),
                hex: format!("#{:06x}", i * 0x101010),
            })
            .collect();
        StyleCard {
            id: "card".to_string(),
            name: "card".to_string(),
            palette,
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

    fn asset(width: u32, height: u32, pixels: Vec<u8>) -> Asset {
        Asset {
            style_card_id: "card".to_string(),
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn valid_asset_passes() {
        let card = style_card(4);
        let a = asset(2, 2, vec![0, 1, 2, 3]);
        let report = verify_asset(&card, Dimensions { w: 2, h: 2 }, &a);
        assert!(report.passed);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn wrong_dimensions_flagged() {
        let card = style_card(4);
        let a = asset(2, 2, vec![0; 4]);
        let report = verify_asset(&card, Dimensions { w: 3, h: 2 }, &a);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].code, DiagnosticCode::DimensionsMismatch);
        assert_eq!(report.errors[0].message, "asset is 2x2, expected 3x2");
    }

    #[test]
    fn out_of_bounds_palette_index_flagged() {
        let card = style_card(2);
        let a = asset(2, 2, vec![0, 1, 2, 3]);
        let report = verify_asset(&card, Dimensions { w: 2, h: 2 }, &a);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 2);
        assert_eq!(
            report.errors[0].code,
            DiagnosticCode::PaletteIndexOutOfBounds
        );
        assert_eq!(
            report.errors[0].message,
            "pixel (0,1) uses palette index 2, but palette has 2 colors"
        );
        assert_eq!(
            report.errors[1].message,
            "pixel (1,1) uses palette index 3, but palette has 2 colors"
        );
    }

    #[test]
    fn tileset_count_mismatch_flagged() {
        let card = style_card(4);
        let tiles = vec![asset(2, 2, vec![0; 4])];
        let report = verify_tileset(&card, Dimensions { w: 2, h: 2 }, 3, &tiles);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].code, DiagnosticCode::CountMismatch);
        assert_eq!(report.errors[0].message, "tileset has 1 tiles, expected 3");
    }

    #[test]
    fn verify_request_dispatches_valid_sprite() {
        let card = style_card(4);
        let request = AssetRequest {
            name: "hero".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w: 2, h: 2 },
            },
            style_card: card,
        };
        let assets = vec![asset(2, 2, vec![0, 1, 2, 3])];
        let report = verify_request(&request, &assets);
        assert!(report.passed);
    }

    #[test]
    fn pixel_count_mismatch_flagged() {
        let card = style_card(4);
        let a = asset(2, 2, vec![0, 1, 2]);
        let report = verify_asset(&card, Dimensions { w: 2, h: 2 }, &a);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].code, DiagnosticCode::PixelCountMismatch);
        assert_eq!(
            report.errors[0].message,
            "asset declares 2x2 (4 pixels) but has 3 indices"
        );
    }

    #[test]
    fn oversized_pixels_flagged_and_iterated_safely() {
        let card = style_card(4);
        let a = asset(2, 2, vec![0, 1, 2, 3, 4]);
        let report = verify_asset(&card, Dimensions { w: 2, h: 2 }, &a);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 2);
        assert_eq!(report.errors[0].code, DiagnosticCode::PixelCountMismatch);
        assert_eq!(
            report.errors[1].code,
            DiagnosticCode::PaletteIndexOutOfBounds
        );
        assert_eq!(
            report.errors[1].message,
            "pixel (0,2) uses palette index 4, but palette has 4 colors"
        );
    }

    #[test]
    fn dimension_and_pixel_count_mismatch_both_reported() {
        let card = style_card(4);
        let a = asset(2, 2, vec![0; 3]);
        let report = verify_asset(&card, Dimensions { w: 3, h: 3 }, &a);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 2);
        assert_eq!(report.errors[0].code, DiagnosticCode::DimensionsMismatch);
        assert_eq!(report.errors[1].code, DiagnosticCode::PixelCountMismatch);
    }

    #[test]
    fn zero_width_does_not_panic() {
        let card = style_card(4);
        let a = asset(0, 0, vec![7]);
        let report = verify_asset(&card, Dimensions { w: 1, h: 1 }, &a);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 3);
        assert_eq!(report.errors[0].code, DiagnosticCode::DimensionsMismatch);
        assert_eq!(report.errors[1].code, DiagnosticCode::PixelCountMismatch);
        assert_eq!(
            report.errors[2].code,
            DiagnosticCode::PaletteIndexOutOfBounds
        );
        assert_eq!(
            report.errors[2].message,
            "pixel (0,0) uses palette index 7, but palette has 4 colors"
        );
    }

    #[test]
    fn empty_palette_flags_all_pixels() {
        let card = style_card(0);
        let a = asset(2, 2, vec![0; 4]);
        let report = verify_asset(&card, Dimensions { w: 2, h: 2 }, &a);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 4);
        assert!(
            report
                .errors
                .iter()
                .all(|d| d.code == DiagnosticCode::PaletteIndexOutOfBounds)
        );
    }

    #[test]
    fn verify_request_dispatches_tileset_with_tile_prefix() {
        let card = style_card(2);
        let request = AssetRequest {
            name: "floor".to_string(),
            kind: AssetKind::Tileset {
                tile_size: Dimensions { w: 2, h: 2 },
                count: 2,
            },
            style_card: card,
        };
        let tiles = vec![asset(2, 2, vec![0, 1, 0, 1]), asset(2, 2, vec![0, 1, 2, 3])];
        let report = verify_request(&request, &tiles);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 2);
        assert_eq!(
            report.errors[0].code,
            DiagnosticCode::PaletteIndexOutOfBounds
        );
        assert!(report.errors[0].message.starts_with("tile 1: "));
    }

    #[test]
    fn verify_request_sprite_count_mismatch() {
        let card = style_card(4);
        let request = AssetRequest {
            name: "hero".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w: 2, h: 2 },
            },
            style_card: card,
        };
        let assets = vec![];
        let report = verify_request(&request, &assets);
        assert!(!report.passed);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].code, DiagnosticCode::CountMismatch);
        assert_eq!(
            report.errors[0].message,
            "sprite request produced 0 assets, expected 1"
        );
    }
}
