//! Result payload helpers. Pure data shapes; no rmcp imports here.

use base64::Engine;

pub fn tool_text<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| "cannot serialize output".to_string())
}

pub fn err_text(msg: &str) -> String {
    format!("error: {msg}")
}

#[derive(serde::Serialize)]
pub struct OkCard {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(serde::Serialize)]
pub struct SaveOutput {
    pub card_hash: String,
    pub asset_hashes: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct ListOutput {
    pub cards: Vec<serde_json::Value>,
    pub assets: Vec<serde_json::Value>,
}

/// Maps the store's CardSummary to its JSON shape; field names mirror the
/// store's public fields exactly (no parallel schema).
pub fn card_summary_json(summary: &conjure_store::CardSummary) -> serde_json::Value {
    serde_json::json!({
        "hash": summary.hash,
        "id": summary.id,
        "name": summary.name,
        "saved_at": summary.saved_at,
    })
}

pub fn asset_summary_json(summary: &conjure_store::AssetSummary) -> serde_json::Value {
    serde_json::json!({
        "hash": summary.hash,
        "style_card_hash": summary.style_card_hash,
        "name": summary.name,
        "kind": summary.kind,
        "width": summary.width,
        "height": summary.height,
        "saved_at": summary.saved_at,
        "tags": summary.tags,
    })
}

#[derive(serde::Serialize)]
pub struct ModelsOutput {
    pub local_servers: Vec<LocalServerSummary>,
    pub opencode_models: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opencode_error: Option<String>,
}

#[derive(serde::Serialize)]
pub struct LocalServerSummary {
    pub base_url: String,
    pub models: Vec<String>,
}

#[derive(serde::Serialize)]
pub struct GenOutput {
    pub assets: Vec<conjure_core::Asset>,
    pub repairs_used: u32,
    pub used_fallback: bool,
}

#[derive(serde::Serialize)]
pub struct ExportOutput {
    pub png_base64: String,
    pub manifest: conjure_export::Manifest,
}

pub const MAX_MCP_PNG_BYTES: usize = 8 * 1024 * 1024;

pub fn png_size_check(png_len: usize) -> Result<(), String> {
    if png_len > MAX_MCP_PNG_BYTES {
        return Err(format!(
            "exported png is {png_len} bytes, over the 8 MiB mcp payload cap"
        ));
    }
    Ok(())
}

pub fn png_base64(png: &[u8]) -> String {
    base64::prelude::BASE64_STANDARD.encode(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ok_card_true_has_no_error_key() {
        let json = tool_text(&OkCard {
            ok: true,
            error: None,
        });
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn ok_card_false_includes_error() {
        let json = tool_text(&OkCard {
            ok: false,
            error: Some("palette must contain at least 2 colors".to_string()),
        });
        assert!(json.contains("\"error\""));
        assert!(json.contains("palette must contain at least 2 colors"));
    }

    #[test]
    fn png_size_check_passes_at_cap_and_refuses_one_over() {
        assert!(png_size_check(MAX_MCP_PNG_BYTES).is_ok());
        assert!(png_size_check(8 * 1024 * 1024).is_ok());
        let err = png_size_check(MAX_MCP_PNG_BYTES + 1).unwrap_err();
        assert_eq!(
            err,
            format!(
                "exported png is {} bytes, over the 8 MiB mcp payload cap",
                MAX_MCP_PNG_BYTES + 1
            )
        );
    }

    #[test]
    fn tool_text_is_pretty_multiline() {
        let text = tool_text(&SaveOutput {
            card_hash: "abc".to_string(),
            asset_hashes: vec!["def".to_string()],
        });
        assert!(text.contains('\n'));
    }

    #[test]
    fn summary_mapping_has_exact_key_sets_and_round_trips() {
        let card = conjure_store::CardSummary {
            hash: "abcdef12".to_string(),
            id: "demo-palette".to_string(),
            name: "Demo palette".to_string(),
            saved_at: 1_700_000_000,
        };
        let card_json = card_summary_json(&card);
        let card_obj = card_json.as_object().unwrap();
        assert_eq!(card_obj.len(), 4);
        assert_eq!(card_obj["hash"], "abcdef12");
        assert_eq!(card_obj["id"], "demo-palette");
        assert_eq!(card_obj["name"], "Demo palette");
        assert_eq!(card_obj["saved_at"], 1_700_000_000);

        let asset = conjure_store::AssetSummary {
            hash: "fedcba98".to_string(),
            style_card_hash: "abcdef12".to_string(),
            name: "demo#0".to_string(),
            kind: "tile".to_string(),
            width: 8,
            height: 8,
            saved_at: 1_700_000_001,
            tags: vec!["demo".to_string(), "extra".to_string()],
        };
        let asset_json = asset_summary_json(&asset);
        let asset_obj = asset_json.as_object().unwrap();
        assert_eq!(asset_obj.len(), 8);
        assert_eq!(asset_obj["hash"], "fedcba98");
        assert_eq!(asset_obj["style_card_hash"], "abcdef12");
        assert_eq!(asset_obj["name"], "demo#0");
        assert_eq!(asset_obj["kind"], "tile");
        assert_eq!(asset_obj["width"], 8);
        assert_eq!(asset_obj["height"], 8);
        assert_eq!(asset_obj["saved_at"], 1_700_000_001);
        assert_eq!(asset_obj["tags"], serde_json::json!(["demo", "extra"]));
    }
}
