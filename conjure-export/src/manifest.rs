pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub style_card: ManifestCard,
    pub image: ManifestImage,
    pub frames: Vec<ManifestFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestCard {
    pub id: String,
    pub hash: Option<String>,
    pub palette: Vec<ManifestPaletteEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestPaletteEntry {
    pub index: usize,
    pub name: String,
    pub hex: String,
    pub transparent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestImage {
    pub file: String,
    pub layout: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<u32>,
    pub scale: u32,
    pub png_width: u32,
    pub png_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestFrame {
    pub name: String,
    pub index: usize,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub origin: ManifestOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ManifestOrigin {
    pub x: u32,
    pub y: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_manifest() -> Manifest {
        Manifest {
            manifest_version: MANIFEST_VERSION,
            style_card: ManifestCard {
                id: "card-1".to_string(),
                hash: None,
                palette: vec![
                    ManifestPaletteEntry {
                        index: 0,
                        name: "bg".to_string(),
                        hex: "#ffffff".to_string(),
                        transparent: true,
                    },
                    ManifestPaletteEntry {
                        index: 1,
                        name: "fg".to_string(),
                        hex: "#101010".to_string(),
                        transparent: false,
                    },
                ],
            },
            image: ManifestImage {
                file: "demo.png".to_string(),
                layout: "strip_horizontal".to_string(),
                columns: None,
                scale: 1,
                png_width: 16,
                png_height: 8,
            },
            frames: vec![
                ManifestFrame {
                    name: "demo#0".to_string(),
                    index: 0,
                    x: 0,
                    y: 0,
                    w: 8,
                    h: 8,
                    origin: ManifestOrigin { x: 0, y: 0 },
                },
                ManifestFrame {
                    name: "demo#1".to_string(),
                    index: 1,
                    x: 8,
                    y: 0,
                    w: 8,
                    h: 8,
                    origin: ManifestOrigin { x: 0, y: 0 },
                },
            ],
        }
    }

    // U10.1: strip manifest JSON carries version, layout, no columns, zero origins.
    #[test]
    fn strip_manifest_json_shape() {
        let json = serde_json::to_string(&strip_manifest()).unwrap();
        assert!(json.contains("\"manifest_version\":1"));
        assert!(json.contains("\"layout\":\"strip_horizontal\""));
        assert!(!json.contains("columns"));
        assert!(json.contains("\"origin\":{\"x\":0,\"y\":0}"));
    }

    // U10.2: every frame carries the zero origin and serde roundtrips exactly.
    #[test]
    fn every_frame_has_zero_origin_and_roundtrips() {
        let json = serde_json::to_string(&strip_manifest()).unwrap();
        assert_eq!(json.matches("\"origin\":{\"x\":0,\"y\":0}").count(), 2);
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, strip_manifest());
    }
}
