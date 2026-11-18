//! MCP tool input parameter structs.
//!
//! Core-typed fields arrive as raw `serde_json::Value` — the identical wire
//! shape the CLI consumes — because core types do not derive `JsonSchema`;
//! handlers strictly parse them with the `parse_*` helpers below.

use rmcp::schemars;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ValidateCardParams {
    #[schemars(description = "style card JSON: id, name, palette, grid, style")]
    pub style_card: serde_json::Value,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ValidateParams {
    #[schemars(description = "asset request JSON: name, kind, style_card")]
    pub request: serde_json::Value,
    #[schemars(description = "asset JSON array: style_card_id, width, height, pixels")]
    pub assets: serde_json::Value,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExpandParams {
    #[schemars(description = "asset request JSON: name, kind, style_card")]
    pub request: serde_json::Value,
    #[schemars(description = "compact spec JSON: sprite rows or tileset tiles")]
    pub spec: serde_json::Value,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RepairParams {
    #[schemars(description = "asset request JSON: name, kind, style_card")]
    pub request: serde_json::Value,
    #[schemars(description = "asset JSON array: style_card_id, width, height, pixels")]
    pub assets: serde_json::Value,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GenerateSeededParams {
    #[schemars(description = "asset request JSON: name, kind, style_card")]
    pub request: serde_json::Value,
    #[schemars(description = "deterministic PRNG seed for reproducible pixels")]
    pub seed: u64,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct GenerateParams {
    #[schemars(description = "asset request JSON: name, kind, style_card")]
    pub request: serde_json::Value,
    #[schemars(description = "inference provider: http or opencode")]
    pub provider: String,
    #[schemars(description = "model id as listed by conjure_models")]
    pub model: String,
    #[schemars(
        description = "required for the http provider: base URL of a local OpenAI-compatible server"
    )]
    pub base_url: Option<String>,
    #[schemars(description = "maximum repair turns before generation gives up")]
    pub max_repairs: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InitParams {
    #[schemars(
        description = "project root directory; defaults to the server's project root; a relative path resolves against it"
    )]
    pub root: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SaveParams {
    #[schemars(description = "asset request JSON: name, kind, style_card")]
    pub request: serde_json::Value,
    #[schemars(description = "asset JSON array: style_card_id, width, height, pixels")]
    pub assets: serde_json::Value,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListParams {
    #[schemars(description = "only list assets carrying this tag")]
    pub tag: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ShowParams {
    #[schemars(
        description = "card or asset hash, or an unambiguous prefix of at least 8 characters"
    )]
    pub hash: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExportParams {
    #[schemars(description = "asset hashes to export, in sheet order")]
    pub hashes: Vec<String>,
    #[schemars(description = "sheet layout: strip_horizontal or grid")]
    pub layout: Option<String>,
    #[schemars(description = "column count for the grid layout")]
    pub columns: Option<u32>,
    #[schemars(description = "integer pixel scale, 1 to 16")]
    pub scale: Option<u32>,
}

pub fn parse_request(value: &serde_json::Value) -> Result<conjure_core::AssetRequest, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("cannot parse request: {e}"))
}

pub fn parse_assets(value: &serde_json::Value) -> Result<Vec<conjure_core::Asset>, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("cannot parse assets: {e}"))
}

pub fn parse_style_card(value: &serde_json::Value) -> Result<conjure_core::StyleCard, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("cannot parse style card: {e}"))
}

pub fn parse_spec(value: &serde_json::Value) -> Result<conjure_core::CompactSpec, String> {
    serde_json::from_value(value.clone()).map_err(|e| format!("cannot parse spec: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_request_json() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/demo/request.json");
        let text = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn parse_request_round_trips_demo_request() {
        let value = demo_request_json();
        let request = parse_request(&value).unwrap();
        assert_eq!(request.name, "demo");
        let conjure_core::AssetKind::Tileset { tile_size, count } = request.kind else {
            panic!("expected demo request kind to be a tileset");
        };
        assert_eq!((tile_size.w, tile_size.h, count), (8, 8, 3));
        assert_eq!(request.style_card.id, "demo-palette");
        assert_eq!(request.style_card.palette.len(), 6);
    }

    #[test]
    fn malformed_request_reports_exact_prefix() {
        let mut value = demo_request_json();
        value.as_object_mut().unwrap().remove("style_card");
        let err = parse_request(&value).unwrap_err();
        assert!(
            err.starts_with("cannot parse request: "),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_assets_accepts_demo_shape_assets() {
        let assets: Vec<serde_json::Value> = (0..3)
            .map(|_| {
                serde_json::json!({
                    "style_card_id": "demo-palette",
                    "width": 8,
                    "height": 8,
                    "pixels": vec![0u8; 64],
                })
            })
            .collect();
        let parsed = parse_assets(&serde_json::Value::Array(assets)).unwrap();
        assert_eq!(parsed.len(), 3);
        for asset in &parsed {
            assert_eq!(asset.style_card_id, "demo-palette");
            assert_eq!((asset.width, asset.height), (8, 8));
            assert_eq!(asset.pixels.len(), 64);
        }
    }
}
