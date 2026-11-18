use conjure_core::StyleCard;

use crate::error::StoreError;

pub const ASSET_MAGIC: [u8; 16] = *b"conjure-asset-v1";
pub const CARD_MAGIC: [u8; 16] = *b"conjure-card-v1\0";

pub fn asset_hash(card_id: &str, width: u32, height: u32, pixels: &[u8]) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&ASSET_MAGIC);
    hasher.update(&(card_id.len() as u32).to_le_bytes());
    hasher.update(card_id.as_bytes());
    hasher.update(&width.to_le_bytes());
    hasher.update(&height.to_le_bytes());
    hasher.update(pixels);
    hasher.finalize().to_hex().to_string()
}

pub fn card_hash(card: &StyleCard) -> Result<String, StoreError> {
    let json = serde_json::to_string(card)
        .map_err(|e| StoreError::Invalid(format!("cannot serialize style card: {e}")))?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&CARD_MAGIC);
    hasher.update(json.as_bytes());
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn blob_hash(pixels: &[u8]) -> String {
    blake3::hash(pixels).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use conjure_core::{Dimensions, Grid, Outline, PaletteColor, Shading, Style};

    use super::*;

    fn card(id: &str) -> StyleCard {
        StyleCard {
            id: id.into(),
            name: "card".into(),
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
                tile_size: Dimensions { w: 16, h: 16 },
                unit_scale: 1,
            },
            style: Style {
                outline: Outline::None,
                outline_color: "#000000".into(),
                shading: Shading::Flat,
                proportions: "1:1".into(),
                era: "modern".into(),
                theme: "pixel".into(),
            },
        }
    }

    #[test]
    fn magics_are_16_bytes() {
        assert_eq!(ASSET_MAGIC.len(), 16);
        assert_eq!(CARD_MAGIC.len(), 16);
    }

    #[test]
    fn asset_hash_is_64_lowercase_hex_and_deterministic() {
        let hash = asset_hash("card-1", 2, 2, &[0, 1, 1, 0]);
        assert_eq!(hash.len(), 64);
        assert!(
            hash.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert_eq!(hash, asset_hash("card-1", 2, 2, &[0, 1, 1, 0]));
    }

    #[test]
    fn asset_hash_changes_with_each_input() {
        let base = asset_hash("card-1", 2, 2, &[0, 1, 1, 0]);
        assert_ne!(base, asset_hash("card-2", 2, 2, &[0, 1, 1, 0]));
        assert_ne!(base, asset_hash("card-1", 3, 2, &[0, 1, 1, 0]));
        assert_ne!(base, asset_hash("card-1", 2, 3, &[0, 1, 1, 0]));
        assert_ne!(base, asset_hash("card-1", 2, 2, &[0, 1, 1, 1]));
    }

    #[test]
    fn card_hash_matches_for_equal_cards_and_differs_for_ids() {
        assert_eq!(
            card_hash(&card("a")).unwrap(),
            card_hash(&card("a")).unwrap()
        );
        assert_ne!(
            card_hash(&card("a")).unwrap(),
            card_hash(&card("b")).unwrap()
        );
    }

    #[test]
    fn blob_hash_matches_raw_blake3() {
        assert_eq!(
            blob_hash(&[0, 1, 2]),
            blake3::hash(&[0, 1, 2]).to_hex().to_string()
        );
    }
}
