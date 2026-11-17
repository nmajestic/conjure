use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaletteColor {
    pub name: String,
    pub hex: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimensions {
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    pub tile_size: Dimensions,
    pub unit_scale: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outline {
    None,
    One,
    Two,
    Heavy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shading {
    Flat,
    TwoTone,
    Dither,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    pub outline: Outline,
    pub outline_color: String,
    pub shading: Shading,
    pub proportions: String,
    pub era: String,
    pub theme: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StyleCard {
    pub id: String,
    pub name: String,
    pub palette: Vec<PaletteColor>,
    pub grid: Grid,
    pub style: Style,
}

impl PaletteColor {
    pub fn rgb(&self) -> Result<[u8; 3], String> {
        let bytes = self.hex.strip_prefix('#').unwrap_or(&self.hex).as_bytes();
        if bytes.len() != 6 && bytes.len() != 8 {
            return Err(format!("invalid hex {}", self.hex));
        }
        let mut out = [0u8; 3];
        for (i, slot) in out.iter_mut().enumerate() {
            let start = i * 2;
            let pair = std::str::from_utf8(&bytes[start..start + 2])
                .map_err(|_| format!("invalid hex {}", self.hex))?;
            *slot =
                u8::from_str_radix(pair, 16).map_err(|_| format!("invalid hex {}", self.hex))?;
        }
        Ok(out)
    }
}

impl Dimensions {
    pub fn pixel_count(self) -> usize {
        self.w as usize * self.h as usize
    }
}

impl StyleCard {
    pub const MIN_PALETTE: usize = 2;
    pub const MAX_PALETTE: usize = 32;

    pub fn validate(&self) -> Result<(), String> {
        let len = self.palette.len();
        if len < Self::MIN_PALETTE {
            return Err(format!(
                "palette must contain at least {} colors, found {}",
                Self::MIN_PALETTE,
                len
            ));
        }
        if len > Self::MAX_PALETTE {
            return Err(format!(
                "palette exceeds max {} colors, found {}",
                Self::MAX_PALETTE,
                len
            ));
        }
        let mut seen = std::collections::HashSet::new();
        for color in &self.palette {
            color.rgb()?;
            let key = color.hex.to_lowercase();
            if !seen.insert(key) {
                return Err(format!("duplicate palette color {}", color.hex));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_card() -> StyleCard {
        StyleCard {
            id: "card-1".to_string(),
            name: "card".to_string(),
            palette: vec![
                PaletteColor {
                    name: "a".to_string(),
                    hex: "#4a6a2a".to_string(),
                },
                PaletteColor {
                    name: "b".to_string(),
                    hex: "#ffffff".to_string(),
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

    #[test]
    fn valid_card_passes_validate() {
        assert!(valid_card().validate().is_ok());
    }

    #[test]
    fn duplicate_color_fails() {
        let mut card = valid_card();
        card.palette.push(PaletteColor {
            name: "dup".to_string(),
            hex: "#4A6A2A".to_string(),
        });
        let err = card.validate().unwrap_err();
        assert!(err.contains("duplicate"));
    }

    #[test]
    fn too_few_colors_fails() {
        let mut card = valid_card();
        card.palette.truncate(1);
        let err = card.validate().unwrap_err();
        assert!(err.contains("at least"));
    }

    #[test]
    fn bad_hex_fails() {
        let mut card = valid_card();
        card.palette[0].hex = "#zzzzzz".to_string();
        assert!(card.validate().is_err());
    }

    #[test]
    fn rgb_parses_rrggbb() {
        let color = PaletteColor {
            name: "c".to_string(),
            hex: "#4a6a2a".to_string(),
        };
        assert_eq!(color.rgb().unwrap(), [0x4a, 0x6a, 0x2a]);
    }
}
