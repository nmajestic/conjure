use serde::{Deserialize, Serialize};

use crate::style::{Dimensions, StyleCard};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssetKind {
    Sprite { size: Dimensions },
    Tileset { tile_size: Dimensions, count: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRequest {
    pub name: String,
    pub kind: AssetKind,
    pub style_card: StyleCard,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Grid, Outline, PaletteColor, Shading, Style};

    fn test_style_card() -> StyleCard {
        StyleCard {
            id: "test".into(),
            name: "Test".into(),
            palette: vec![
                PaletteColor {
                    name: "a".into(),
                    hex: "#000000".into(),
                },
                PaletteColor {
                    name: "b".into(),
                    hex: "#ffffff".into(),
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
                proportions: "chibi".into(),
                era: "16-bit".into(),
                theme: "test".into(),
            },
        }
    }

    #[test]
    fn sprite_kind_and_request_hold_fields() {
        let kind = AssetKind::Sprite {
            size: Dimensions { w: 4, h: 4 },
        };

        let request = AssetRequest {
            name: "demo".into(),
            kind,
            style_card: test_style_card(),
        };

        assert!(matches!(request.kind, AssetKind::Sprite { size } if size.w == 4 && size.h == 4));
        assert_eq!(request.name, "demo");
        assert_eq!(request.style_card.id, "test");
        assert_eq!(request.style_card.name, "Test");
        assert_eq!(request.style_card.palette.len(), 2);
        assert_eq!(request.style_card.grid.tile_size, Dimensions { w: 4, h: 4 });
        assert_eq!(request.style_card.grid.unit_scale, 1);
        assert_eq!(request.style_card.style.outline, Outline::None);
        assert_eq!(request.style_card.style.shading, Shading::Flat);
    }
}
