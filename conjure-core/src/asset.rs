use crate::style::Dimensions;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub style_card_id: String,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Asset {
    pub fn dimensions(&self) -> Dimensions {
        Dimensions {
            w: self.width,
            h: self.height,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_returns_width_and_height() {
        let asset = Asset {
            style_card_id: "card".to_string(),
            width: 2,
            height: 3,
            pixels: vec![0; 6],
        };
        assert_eq!(asset.dimensions(), Dimensions { w: 2, h: 3 });
    }
}
