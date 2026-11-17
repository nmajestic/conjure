use serde::{Deserialize, Serialize};

use crate::asset::Asset;
use crate::spec::{AssetKind, AssetRequest};
use crate::style::Dimensions;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompactSpec {
    Sprite { rows: Vec<String> },
    Tileset { tiles: Vec<Vec<String>> },
}

pub fn char_to_index(c: char) -> Option<u8> {
    match c {
        '0'..='9' => Some(c as u8 - b'0'),
        'a'..='v' => Some(c as u8 - b'a' + 10),
        _ => None,
    }
}

pub fn index_to_char(i: u8) -> char {
    if i < 10 {
        (b'0' + i) as char
    } else {
        b'a'.saturating_add(i - 10) as char
    }
}

fn expand_rows(
    rows: &[String],
    size: Dimensions,
    palette_len: usize,
    subject: &str,
    prefix: &str,
) -> Result<Vec<u8>, String> {
    if rows.len() != size.h as usize {
        return Err(format!(
            "{} has {} rows, expected {}",
            subject,
            rows.len(),
            size.h
        ));
    }
    let mut pixels = Vec::with_capacity(size.w as usize * size.h as usize);
    for (r, row) in rows.iter().enumerate() {
        let n = row.chars().count();
        if n != size.w as usize {
            return Err(format!(
                "{}row {} has {} chars, expected {}",
                prefix, r, n, size.w
            ));
        }
        for c in row.chars() {
            let i = match char_to_index(c) {
                Some(i) => i,
                None => {
                    return Err(format!(
                        "{prefix}row {r} contains invalid char '{c}' — chars must be '0'-'9' or 'a'-'v'"
                    ));
                }
            };
            if i as usize >= palette_len {
                return Err(format!(
                    "{prefix}row {r} uses index {i} but palette has only {palette_len} colors — chars must be '0'-'{}'",
                    index_to_char(palette_len.saturating_sub(1) as u8)
                ));
            }
            pixels.push(i);
        }
    }
    Ok(pixels)
}

pub fn expand(request: &AssetRequest, spec: &CompactSpec) -> Result<Vec<Asset>, String> {
    let card = &request.style_card;
    let palette_len = card.palette.len();
    match (&request.kind, spec) {
        (AssetKind::Sprite { size }, CompactSpec::Sprite { rows }) => {
            let pixels = expand_rows(rows, *size, palette_len, "sprite", "")?;
            Ok(vec![Asset {
                style_card_id: card.id.clone(),
                width: size.w,
                height: size.h,
                pixels,
            }])
        }
        (AssetKind::Tileset { tile_size, count }, CompactSpec::Tileset { tiles }) => {
            if tiles.len() != *count {
                return Err(format!(
                    "tileset has {} tiles, expected {}",
                    tiles.len(),
                    count
                ));
            }
            let mut assets = Vec::with_capacity(*count);
            for (t, tile) in tiles.iter().enumerate() {
                let subject = format!("tile {t}");
                let prefix = format!("{subject}: ");
                let pixels = expand_rows(tile, *tile_size, palette_len, &subject, &prefix)?;
                assets.push(Asset {
                    style_card_id: card.id.clone(),
                    width: tile_size.w,
                    height: tile_size.h,
                    pixels,
                });
            }
            Ok(assets)
        }
        (AssetKind::Sprite { .. }, CompactSpec::Tileset { .. }) => {
            Err("spec is a tileset but request expects a sprite".to_string())
        }
        (AssetKind::Tileset { .. }, CompactSpec::Sprite { .. }) => {
            Err("spec is a sprite but request expects a tileset".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::AssetKind;
    use crate::style::{Grid, Outline, PaletteColor, Shading, Style, StyleCard};
    use crate::verify::verify_request;

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

    fn sprite_request(palette_len: usize, w: u32, h: u32) -> AssetRequest {
        AssetRequest {
            name: "hero".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w, h },
            },
            style_card: style_card(palette_len),
        }
    }

    fn tileset_request(palette_len: usize, w: u32, h: u32, count: usize) -> AssetRequest {
        AssetRequest {
            name: "floor".to_string(),
            kind: AssetKind::Tileset {
                tile_size: Dimensions { w, h },
                count,
            },
            style_card: style_card(palette_len),
        }
    }

    #[test]
    fn char_index_round_trip() {
        for i in 0..=31u8 {
            assert_eq!(char_to_index(index_to_char(i)), Some(i));
        }
        assert_eq!(char_to_index('0'), Some(0));
        assert_eq!(char_to_index('9'), Some(9));
        assert_eq!(char_to_index('a'), Some(10));
        assert_eq!(char_to_index('v'), Some(31));
        assert_eq!(char_to_index('w'), None);
        assert_eq!(char_to_index('x'), None);
        assert_eq!(char_to_index('z'), None);
        assert_eq!(char_to_index('A'), None);
        assert_eq!(char_to_index(' '), None);
        assert_eq!(char_to_index('#'), None);
        assert_eq!(char_to_index('é'), None);
        assert_eq!(index_to_char(0), '0');
        assert_eq!(index_to_char(10), 'a');
        assert_eq!(index_to_char(31), 'v');
    }

    #[test]
    fn index_to_char_out_of_range_does_not_panic() {
        for i in 32..=255u8 {
            let _ = index_to_char(i);
        }
    }

    #[test]
    fn multibyte_row_is_counted_by_chars() {
        let request = sprite_request(4, 4, 2);
        let spec = CompactSpec::Sprite {
            rows: vec!["0123".to_string(), "12é0".to_string()],
        };
        let err = expand(&request, &spec).unwrap_err();
        assert!(
            err.contains("row 1 contains invalid char 'é' — chars must be '0'-'9' or 'a'-'v'"),
            "{err}"
        );
    }

    #[test]
    fn valid_sprite_expands_and_verifies() {
        let request = sprite_request(4, 4, 2);
        let spec = CompactSpec::Sprite {
            rows: vec!["0123".to_string(), "1230".to_string()],
        };
        let assets = expand(&request, &spec).unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].style_card_id, "card");
        assert_eq!(assets[0].width, 4);
        assert_eq!(assets[0].height, 2);
        assert_eq!(assets[0].pixels, vec![0, 1, 2, 3, 1, 2, 3, 0]);
        assert!(verify_request(&request, &assets).passed);
    }

    #[test]
    fn wrong_row_count_flagged() {
        let request = sprite_request(4, 4, 2);
        let spec = CompactSpec::Sprite {
            rows: vec!["0123".to_string()],
        };
        let err = expand(&request, &spec).unwrap_err();
        assert!(err.contains("sprite has 1 rows, expected 2"), "{err}");
    }

    #[test]
    fn wrong_row_width_flagged() {
        let request = sprite_request(4, 4, 2);
        let spec = CompactSpec::Sprite {
            rows: vec!["0123".to_string(), "123".to_string()],
        };
        let err = expand(&request, &spec).unwrap_err();
        assert!(err.contains("row 1 has 3 chars, expected 4"), "{err}");
    }

    #[test]
    fn invalid_char_flagged() {
        let request = sprite_request(4, 4, 2);
        let spec = CompactSpec::Sprite {
            rows: vec!["0123".to_string(), "12w0".to_string()],
        };
        let err = expand(&request, &spec).unwrap_err();
        assert!(
            err.contains("row 1 contains invalid char 'w' — chars must be '0'-'9' or 'a'-'v'"),
            "{err}"
        );
    }

    #[test]
    fn out_of_range_index_flagged() {
        let request = sprite_request(4, 4, 2);
        let spec = CompactSpec::Sprite {
            rows: vec!["0123".to_string(), "1234".to_string()],
        };
        let err = expand(&request, &spec).unwrap_err();
        assert!(
            err.contains(
                "row 1 uses index 4 but palette has only 4 colors — chars must be '0'-'3'"
            ),
            "{err}"
        );
    }

    #[test]
    fn tileset_wrong_count_flagged() {
        let request = tileset_request(4, 2, 2, 2);
        let spec = CompactSpec::Tileset {
            tiles: vec![vec!["01".to_string(), "10".to_string()]],
        };
        let err = expand(&request, &spec).unwrap_err();
        assert!(err.contains("tileset has 1 tiles, expected 2"), "{err}");
    }

    #[test]
    fn tileset_row_errors_are_prefixed() {
        let request = tileset_request(4, 2, 2, 2);
        let spec = CompactSpec::Tileset {
            tiles: vec![
                vec!["01".to_string(), "10".to_string()],
                vec!["0w".to_string(), "10".to_string()],
            ],
        };
        let err = expand(&request, &spec).unwrap_err();
        assert!(
            err.contains("tile 1: row 0 contains invalid char 'w'"),
            "{err}"
        );
    }

    #[test]
    fn valid_tileset_expands_and_verifies() {
        let request = tileset_request(4, 2, 2, 2);
        let spec = CompactSpec::Tileset {
            tiles: vec![
                vec!["01".to_string(), "10".to_string()],
                vec!["23".to_string(), "32".to_string()],
            ],
        };
        let assets = expand(&request, &spec).unwrap();
        assert_eq!(assets.len(), 2);
        assert_eq!(assets[1].pixels, vec![2, 3, 3, 2]);
        assert!(verify_request(&request, &assets).passed);
    }

    #[test]
    fn kind_mismatch_flagged() {
        let sprite_req = sprite_request(4, 2, 2);
        let tileset_req = tileset_request(4, 2, 2, 1);
        let tileset_spec = CompactSpec::Tileset {
            tiles: vec![vec!["01".to_string(), "10".to_string()]],
        };
        let sprite_spec = CompactSpec::Sprite {
            rows: vec!["01".to_string(), "10".to_string()],
        };
        let err = expand(&sprite_req, &tileset_spec).unwrap_err();
        assert!(
            err.contains("spec is a tileset but request expects a sprite"),
            "{err}"
        );
        let err = expand(&tileset_req, &sprite_spec).unwrap_err();
        assert!(
            err.contains("spec is a sprite but request expects a tileset"),
            "{err}"
        );
    }
}
