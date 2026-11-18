//! The Conjure MCP server: twelve tools over stdio or loopback HTTP.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};

use crate::{params, results};

pub struct ConjureMcp {
    root: PathBuf,
    tool_router: ToolRouter<Self>,
}

#[derive(serde::Serialize)]
struct AssetsBody {
    assets: Vec<conjure_core::Asset>,
}

#[derive(serde::Serialize)]
struct ErrorBody {
    error: String,
}

#[derive(serde::Serialize)]
struct RepairBody {
    assets: Vec<conjure_core::Asset>,
    verify: conjure_core::ValidationReport,
}

#[derive(serde::Serialize)]
struct InitBody {
    path: String,
}

fn success<T: serde::Serialize>(payload: &T) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(
        results::tool_text(payload),
    )]))
}

fn tool_error(message: &str) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::error(vec![ContentBlock::text(
        results::err_text(message),
    )]))
}

async fn models_output(urls: &[String]) -> results::ModelsOutput {
    let local_servers = conjure_inference::discover(urls)
        .await
        .into_iter()
        .map(|server| results::LocalServerSummary {
            base_url: server.base_url,
            models: server.models,
        })
        .collect();
    let (opencode_models, opencode_error) =
        match conjure_inference::list_opencode_models(&conjure_inference::opencode_bin()).await {
            Ok(models) => (models, None),
            Err(e) => (Vec::new(), Some(e)),
        };
    results::ModelsOutput {
        local_servers,
        opencode_models,
        opencode_error,
    }
}

impl ConjureMcp {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            tool_router: Self::tool_router(),
        }
    }

    fn store(&self) -> Result<conjure_store::Store, String> {
        let root = conjure_store::locate(&self.root).map_err(|e| e.to_string())?;
        conjure_store::Store::open(&root).map_err(|e| e.to_string())
    }
}

#[tool_router]
impl ConjureMcp {
    #[tool(description = "Validate a style card against the Conjure contract")]
    async fn conjure_validate_card(
        &self,
        Parameters(p): Parameters<params::ValidateCardParams>,
    ) -> Result<CallToolResult, McpError> {
        let card = match params::parse_style_card(&p.style_card) {
            Ok(card) => card,
            Err(e) => return tool_error(&e),
        };
        let payload = match card.validate() {
            Ok(()) => results::OkCard {
                ok: true,
                error: None,
            },
            Err(e) => results::OkCard {
                ok: false,
                error: Some(e),
            },
        };
        success(&payload)
    }

    #[tool(description = "Validate assets against a request; returns the verification report")]
    async fn conjure_validate(
        &self,
        Parameters(p): Parameters<params::ValidateParams>,
    ) -> Result<CallToolResult, McpError> {
        let request = match params::parse_request(&p.request) {
            Ok(request) => request,
            Err(e) => return tool_error(&e),
        };
        let assets = match params::parse_assets(&p.assets) {
            Ok(assets) => assets,
            Err(e) => return tool_error(&e),
        };
        if let Err(e) = request.style_card.validate() {
            return success(&results::OkCard {
                ok: false,
                error: Some(format!("invalid style card: {e}")),
            });
        }
        let report = conjure_core::verify_request(&request, &assets);
        success(&report)
    }

    #[tool(description = "Expand a compact spec (sprite rows or tileset tiles) into assets")]
    async fn conjure_expand(
        &self,
        Parameters(p): Parameters<params::ExpandParams>,
    ) -> Result<CallToolResult, McpError> {
        let request = match params::parse_request(&p.request) {
            Ok(request) => request,
            Err(e) => return tool_error(&e),
        };
        let spec = match params::parse_spec(&p.spec) {
            Ok(spec) => spec,
            Err(e) => return tool_error(&e),
        };
        match conjure_core::expand(&request, &spec) {
            Ok(assets) => success(&AssetsBody { assets }),
            Err(e) => success(&ErrorBody { error: e }),
        }
    }

    #[tool(description = "Normalize assets to the request contract and verify the result")]
    async fn conjure_repair(
        &self,
        Parameters(p): Parameters<params::RepairParams>,
    ) -> Result<CallToolResult, McpError> {
        let request = match params::parse_request(&p.request) {
            Ok(request) => request,
            Err(e) => return tool_error(&e),
        };
        let assets = match params::parse_assets(&p.assets) {
            Ok(assets) => assets,
            Err(e) => return tool_error(&e),
        };
        let repaired = conjure_core::repair(&request, &assets);
        let verify = conjure_core::verify_request(&request, &repaired);
        success(&RepairBody {
            assets: repaired,
            verify,
        })
    }

    #[tool(description = "Generate deterministic placeholder assets from a seed")]
    async fn conjure_generate_seeded(
        &self,
        Parameters(p): Parameters<params::GenerateSeededParams>,
    ) -> Result<CallToolResult, McpError> {
        let request = match params::parse_request(&p.request) {
            Ok(request) => request,
            Err(e) => return tool_error(&e),
        };
        let assets = conjure_core::generate(&request, p.seed);
        success(&AssetsBody { assets })
    }

    #[tool(description = "Generate assets with a local model through the conjure agent loop")]
    async fn conjure_generate(
        &self,
        Parameters(p): Parameters<params::GenerateParams>,
    ) -> Result<CallToolResult, McpError> {
        let request = match params::parse_request(&p.request) {
            Ok(request) => request,
            Err(e) => return tool_error(&e),
        };
        if let Err(e) = request.style_card.validate() {
            return tool_error(&format!("invalid style card: {e}"));
        }
        let backend = match p.provider.as_str() {
            "http" => {
                let Some(base_url) = p.base_url.as_deref() else {
                    return tool_error("provider http requires base_url");
                };
                conjure_inference::Backend::Http(conjure_inference::Client::new(
                    base_url,
                    p.model.as_str(),
                ))
            }
            "opencode" => {
                if p.base_url.is_some() {
                    return tool_error("provider opencode does not take base_url");
                }
                conjure_inference::Backend::OpenCode(conjure_inference::OpenCodeCli::new(
                    conjure_inference::opencode_bin(),
                    p.model.as_str(),
                ))
            }
            other => {
                return tool_error(&format!("unknown provider {other} (use http or opencode)"));
            }
        };
        let opts = conjure_agent::GenOpts {
            max_repairs: p.max_repairs.unwrap_or(3),
            on_event: None,
        };
        match conjure_agent::generate(&backend, &request, &opts).await {
            Ok(result) => success(&results::GenOutput {
                assets: result.assets,
                repairs_used: result.repairs_used,
                used_fallback: result.used_fallback,
            }),
            Err(e) => tool_error(&e),
        }
    }

    #[tool(description = "List local model servers and opencode models")]
    async fn conjure_models(&self) -> Result<CallToolResult, McpError> {
        let output = models_output(&conjure_inference::local_urls()).await;
        success(&output)
    }

    #[tool(description = "Create the .conjure store for a project root")]
    async fn conjure_init(
        &self,
        Parameters(p): Parameters<params::InitParams>,
    ) -> Result<CallToolResult, McpError> {
        let effective = match &p.root {
            None => self.root.clone(),
            Some(given) => {
                let path = PathBuf::from(given);
                if path.is_absolute() {
                    path
                } else {
                    self.root.join(path)
                }
            }
        };
        match conjure_store::Store::create(&effective) {
            Ok(_) => success(&InitBody {
                path: effective.join(".conjure").display().to_string(),
            }),
            Err(e) => tool_error(&e.to_string()),
        }
    }

    #[tool(description = "Save a style card and verified assets to the project store")]
    async fn conjure_save(
        &self,
        Parameters(p): Parameters<params::SaveParams>,
    ) -> Result<CallToolResult, McpError> {
        let request = match params::parse_request(&p.request) {
            Ok(request) => request,
            Err(e) => return tool_error(&e),
        };
        let assets = match params::parse_assets(&p.assets) {
            Ok(assets) => assets,
            Err(e) => return tool_error(&e),
        };
        let store = match self.store() {
            Ok(store) => store,
            Err(e) => return tool_error(&e),
        };
        match store.save_assets(&request, &assets) {
            Ok(hashes) => {
                let card_hash = match conjure_store::card_hash(&request.style_card) {
                    Ok(hash) => hash,
                    Err(e) => return tool_error(&e.to_string()),
                };
                success(&results::SaveOutput {
                    card_hash,
                    asset_hashes: hashes,
                })
            }
            Err(conjure_store::StoreError::Verify(verification)) => success(&verification.0),
            Err(e) => tool_error(&e.to_string()),
        }
    }

    #[tool(description = "List saved card and asset summaries, never pixel data")]
    async fn conjure_list(
        &self,
        Parameters(p): Parameters<params::ListParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = match self.store() {
            Ok(store) => store,
            Err(e) => return tool_error(&e),
        };
        let cards = match store.list_cards() {
            Ok(cards) => cards,
            Err(e) => return tool_error(&e.to_string()),
        };
        let assets = match store.list_assets(&conjure_store::ListFilter { tag: p.tag }) {
            Ok(assets) => assets,
            Err(e) => return tool_error(&e.to_string()),
        };
        success(&results::ListOutput {
            cards: cards.iter().map(results::card_summary_json).collect(),
            assets: assets.iter().map(results::asset_summary_json).collect(),
        })
    }

    #[tool(description = "Show one saved card or asset by hash or unique prefix")]
    async fn conjure_show(
        &self,
        Parameters(p): Parameters<params::ShowParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = match self.store() {
            Ok(store) => store,
            Err(e) => return tool_error(&e),
        };
        match store.resolve_card(&p.hash) {
            Ok(hash) => match store.load_card(&hash) {
                Ok(card) => success(&card),
                Err(e) => tool_error(&e.to_string()),
            },
            Err(conjure_store::StoreError::NotFound(_)) => match store.resolve_asset(&p.hash) {
                Ok(hash) => match store.load_asset(&hash) {
                    Ok(asset) => success(&asset),
                    Err(e) => tool_error(&e.to_string()),
                },
                Err(conjure_store::StoreError::NotFound(_)) => {
                    tool_error(&format!("no card or asset matches {}", p.hash))
                }
                Err(e) => tool_error(&e.to_string()),
            },
            Err(e) => tool_error(&e.to_string()),
        }
    }

    #[tool(description = "Export saved assets as a base64 PNG sheet plus manifest")]
    async fn conjure_export(
        &self,
        Parameters(p): Parameters<params::ExportParams>,
    ) -> Result<CallToolResult, McpError> {
        let store = match self.store() {
            Ok(store) => store,
            Err(e) => return tool_error(&e),
        };
        let mut summaries = Vec::with_capacity(p.hashes.len());
        for prefix in &p.hashes {
            let hash = match store.resolve_asset(prefix) {
                Ok(hash) => hash,
                Err(e) => return tool_error(&e.to_string()),
            };
            let summary = match store.asset_summary(&hash) {
                Ok(summary) => summary,
                Err(e) => return tool_error(&e.to_string()),
            };
            summaries.push(summary);
        }
        let Some(card_hash) = summaries.first().map(|s| s.style_card_hash.clone()) else {
            return tool_error("no assets to export");
        };
        if summaries.iter().any(|s| s.style_card_hash != card_hash) {
            return tool_error("export requires assets from one style card");
        }
        let card = match store.load_card(&card_hash) {
            Ok(card) => card,
            Err(e) => return tool_error(&e.to_string()),
        };
        let mut assets = Vec::with_capacity(summaries.len());
        for summary in &summaries {
            let asset = match store.load_asset(&summary.hash) {
                Ok(asset) => asset,
                Err(e) => return tool_error(&e.to_string()),
            };
            assets.push(asset);
        }
        let names: Vec<String> = summaries.iter().map(|s| s.name.clone()).collect();
        let layout = match p.layout.as_deref() {
            None | Some("strip_horizontal") => conjure_export::SheetLayout::StripHorizontal,
            Some("grid") => match p.columns {
                Some(columns) => conjure_export::SheetLayout::Grid { columns },
                None => return tool_error("grid layout requires columns"),
            },
            Some(other) => {
                return tool_error(&format!(
                    "unknown layout {other} (use strip_horizontal or grid)"
                ));
            }
        };
        let opts = conjure_export::ExportOpts {
            layout,
            scale: p.scale.unwrap_or(1),
        };
        let png = match conjure_export::export_png(&card, &assets, &opts) {
            Ok(png) => png,
            Err(e) => return tool_error(&e.to_string()),
        };
        if let Err(e) = results::png_size_check(png.len()) {
            return tool_error(&e);
        }
        let mut manifest = match conjure_export::export_manifest(&card, &assets, &names, &opts) {
            Ok(manifest) => manifest,
            Err(e) => return tool_error(&e.to_string()),
        };
        manifest.style_card.hash = Some(card_hash);
        success(&results::ExportOutput {
            png_base64: results::png_base64(&png),
            manifest,
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ConjureMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "conjure-mcp",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(
                "Conjure pixel-asset tools over a .conjure/ store: conjure_validate_card, \
                 conjure_validate, conjure_expand, conjure_repair, conjure_generate_seeded, \
                 conjure_generate, conjure_models, conjure_init, conjure_save, conjure_list, \
                 conjure_show, conjure_export. Validate or expand first, save verified assets \
                 with conjure_save (run conjure_init once per project), then export sheets as \
                 base64 PNGs with conjure_export.",
            )
    }
}

pub async fn serve_stdio(root: PathBuf) -> Result<(), String> {
    let running = ConjureMcp::new(root)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| e.to_string())?;
    running.waiting().await.map_err(|e| e.to_string())?;
    Ok(())
}

pub fn check_loopback(addr: SocketAddr) -> Result<(), String> {
    let loopback = match addr.ip() {
        IpAddr::V4(ip) => ip == std::net::Ipv4Addr::LOCALHOST,
        IpAddr::V6(ip) => ip == std::net::Ipv6Addr::LOCALHOST,
    };
    if loopback {
        Ok(())
    } else {
        Err(format!("refusing to bind non-loopback address {addr}"))
    }
}

pub async fn serve_http(root: PathBuf, addr: SocketAddr) -> Result<(), String> {
    check_loopback(addr)?;
    let service = StreamableHttpService::new(
        move || Ok(ConjureMcp::new(root.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let app = axum::Router::new().route_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| e.to_string())?;
    axum::serve(listener, app)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    use base64::Engine;
    use conjure_core::{
        Asset, AssetKind, AssetRequest, CompactSpec, Dimensions, Grid, Outline, PaletteColor,
        Shading, Style, StyleCard, ValidationReport, verify_request,
    };
    use httpmock::{Method, MockServer};
    use rmcp::model::ContentBlock;
    use serde_json::json;

    use super::*;

    const SPRITE_SPEC: &str = r#"{"type":"sprite","rows":["0000","0110","0121","0000"]}"#;
    const GARBAGE_REPLY: &str = "sorry, i cannot draw that.";

    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_root(label: &str) -> TempRoot {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        TempRoot(std::env::temp_dir().join(format!(
            "conjure-mcp-test-{}-{nanos}-{label}",
            std::process::id()
        )))
    }

    #[cfg(unix)]
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(unix)]
    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The guard must span the awaited body: the code under test reads
    /// CONJURE_OPENCODE_BIN while running, and sibling tests that touch the
    /// same variable have to stay blocked for the whole window. The
    /// single-threaded test runtime makes the cross-await hold safe.
    #[cfg(unix)]
    #[allow(clippy::await_holding_lock)]
    async fn with_opencode_bin<T>(bin: &str, body: impl Future<Output = T>) -> T {
        let _env = lock_env();
        let original = std::env::var("CONJURE_OPENCODE_BIN").ok();
        unsafe { std::env::set_var("CONJURE_OPENCODE_BIN", bin) };
        let result = body.await;
        match original {
            Some(value) => unsafe { std::env::set_var("CONJURE_OPENCODE_BIN", value) },
            None => unsafe { std::env::remove_var("CONJURE_OPENCODE_BIN") },
        }
        result
    }

    #[cfg(unix)]
    struct Stub {
        path: PathBuf,
        cleanup_dir: Option<PathBuf>,
    }

    #[cfg(unix)]
    impl Drop for Stub {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
            if let Some(dir) = self.cleanup_dir.take() {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    /// A private directory per stub so two tests can never share an inode
    /// (writing a script another test is exec'ing is ETXTBSY).
    #[cfg(unix)]
    fn write_stub(label: &str, body: &str) -> Stub {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "conjure-mcp-test-{}-{nanos}-{label}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("stub.sh");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o755)
            .open(&path)
            .unwrap();
        file.write_all(body.as_bytes()).unwrap();
        drop(file);
        // umask can clear the execute bit that mode() requested.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        Stub {
            path,
            cleanup_dir: Some(dir),
        }
    }

    fn result_text(result: &CallToolResult) -> String {
        match result.content.first() {
            Some(ContentBlock::Text(text)) => text.text.clone(),
            other => panic!("expected a text content block, got {other:?}"),
        }
    }

    fn demo_request_json() -> serde_json::Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/demo/request.json");
        let text = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn sprite_request() -> AssetRequest {
        let palette: Vec<PaletteColor> = (0..4)
            .map(|i| PaletteColor {
                name: format!("c{i}"),
                hex: format!("#{:06x}", i * 0x101010),
            })
            .collect();
        AssetRequest {
            name: "hero".to_string(),
            kind: AssetKind::Sprite {
                size: Dimensions { w: 4, h: 4 },
            },
            style_card: StyleCard {
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
            },
        }
    }

    fn big_palette_card() -> StyleCard {
        StyleCard {
            id: "big".to_string(),
            name: "big".to_string(),
            palette: (0..32)
                .map(|i| PaletteColor {
                    name: format!("c{i}"),
                    hex: format!("#{:02x}0000", i),
                })
                .collect(),
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

    /// 32-symbol LCG noise: near-incompressible under deflate, so an export
    /// large enough always crosses the 8 MiB payload cap.
    fn lcg_pixels(len: usize) -> Vec<u8> {
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut pixels = Vec::with_capacity(len);
        for _ in 0..len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            pixels.push((state % 32) as u8);
        }
        pixels
    }

    fn blob_count(root: &Path) -> usize {
        std::fs::read_dir(root.join(".conjure").join("blobs"))
            .map(|entries| entries.count())
            .unwrap_or(0)
    }

    async fn init(server: &ConjureMcp, root: Option<String>) -> CallToolResult {
        let result = server
            .conjure_init(Parameters(params::InitParams { root }))
            .await;
        result.unwrap()
    }

    async fn save_demo_assets(server: &ConjureMcp, seed: u64) -> (AssetRequest, Vec<Asset>) {
        let request_json = demo_request_json();
        let request = params::parse_request(&request_json).unwrap();
        let assets = conjure_core::generate(&request, seed);
        let result = server
            .conjure_save(Parameters(params::SaveParams {
                request: request_json,
                assets: serde_json::to_value(&assets).unwrap(),
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        (request, assets)
    }

    #[test]
    fn server_identity_and_tool_names() {
        let server = ConjureMcp::new(PathBuf::from("."));
        let info = server.get_info();
        assert_eq!(info.server_info.name, "conjure-mcp");
        assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
        assert!(info.capabilities.tools.is_some());
        assert!(
            info.instructions
                .as_deref()
                .is_some_and(|instructions| !instructions.is_empty())
        );

        let names: Vec<String> = server
            .tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect();
        let expected = [
            "conjure_validate_card",
            "conjure_validate",
            "conjure_expand",
            "conjure_repair",
            "conjure_generate_seeded",
            "conjure_generate",
            "conjure_models",
            "conjure_init",
            "conjure_save",
            "conjure_list",
            "conjure_show",
            "conjure_export",
        ];
        assert_eq!(names.len(), expected.len());
        for name in expected {
            assert!(names.contains(&name.to_string()), "missing tool {name}");
        }
    }

    #[tokio::test]
    async fn validate_card_ok_and_invalid() {
        let server = ConjureMcp::new(PathBuf::from("."));
        let mut card_json = demo_request_json()["style_card"].clone();

        let result = server
            .conjure_validate_card(Parameters(params::ValidateCardParams {
                style_card: card_json.clone(),
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(value["ok"], json!(true));
        assert!(value.get("error").is_none());

        card_json["palette"][0]["hex"] = json!("#zzzzzz");
        let result = server
            .conjure_validate_card(Parameters(params::ValidateCardParams {
                style_card: card_json,
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(value["ok"], json!(false));
        assert_eq!(value["error"], json!("invalid hex #zzzzzz"));
    }

    #[tokio::test]
    async fn validate_reports_without_protocol_error() {
        let server = ConjureMcp::new(PathBuf::from("."));
        let request_json = demo_request_json();
        let request = params::parse_request(&request_json).unwrap();
        let mut assets = conjure_core::generate(&request, 42);
        assets[1].pixels[0] = 9;

        // A failed verification is a successful tool call carrying the
        // report, never a protocol error, so assert the Ok variant.
        let Ok(result) = server
            .conjure_validate(Parameters(params::ValidateParams {
                request: request_json,
                assets: serde_json::to_value(&assets).unwrap(),
            }))
            .await
        else {
            panic!("expected Ok(CallToolResult), got Err");
        };
        assert!(result.is_error == Some(false));
        let report: ValidationReport = serde_json::from_str(&result_text(&result)).unwrap();
        assert!(!report.passed);
        assert!(
            report
                .errors
                .iter()
                .any(|diagnostic| diagnostic.message.contains("palette index 9"))
        );

        let mut bad_card_json = demo_request_json();
        bad_card_json["style_card"]["palette"] = json!([
            { "name": "only", "hex": "#101010" }
        ]);
        let result = server
            .conjure_validate(Parameters(params::ValidateParams {
                request: bad_card_json,
                assets: serde_json::to_value(&assets).unwrap(),
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(value["ok"], json!(false));
        assert_eq!(
            value["error"],
            json!("invalid style card: palette must contain at least 2 colors, found 1")
        );
    }

    #[tokio::test]
    async fn expand_ok_and_rejected() {
        let server = ConjureMcp::new(PathBuf::from("."));
        let request_json = demo_request_json();
        let tile: Vec<String> = (0..8).map(|_| "01234501".to_string()).collect();
        let spec = json!({"type": "tileset", "tiles": [tile.clone(), tile.clone(), tile]});

        let result = server
            .conjure_expand(Parameters(params::ExpandParams {
                request: request_json.clone(),
                spec: spec.clone(),
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        let assets: Vec<Asset> = serde_json::from_value(value["assets"].clone()).unwrap();
        assert_eq!(assets.len(), 3);
        assert_eq!(assets[0].pixels[..8], [0, 1, 2, 3, 4, 5, 0, 1]);

        let mut rejected = spec.clone();
        rejected["tiles"][0][0] = json!("0123450");
        let result = server
            .conjure_expand(Parameters(params::ExpandParams {
                request: request_json.clone(),
                spec: rejected,
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert!(
            value["error"]
                .as_str()
                .unwrap()
                .contains("row 0 has 7 chars, expected 8")
        );

        let mut broken_request = request_json;
        broken_request.as_object_mut().unwrap().remove("style_card");
        let result = server
            .conjure_expand(Parameters(params::ExpandParams {
                request: broken_request,
                spec,
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(true));
        assert!(result_text(&result).starts_with("error: cannot parse request"));
    }

    #[tokio::test]
    async fn repair_normalizes() {
        let server = ConjureMcp::new(PathBuf::from("."));
        let request_json = demo_request_json();
        let request = params::parse_request(&request_json).unwrap();
        let mut assets = conjure_core::generate(&request, 7);
        assets[0].pixels[3] = 200;
        assets[2].style_card_id = "wrong-card".to_string();

        let result = server
            .conjure_repair(Parameters(params::RepairParams {
                request: request_json,
                assets: serde_json::to_value(&assets).unwrap(),
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(value["verify"]["passed"], json!(true));
        let repaired: Vec<Asset> = serde_json::from_value(value["assets"].clone()).unwrap();
        assert_eq!(repaired.len(), 3);
        let palette_len = request.style_card.palette.len();
        for asset in &repaired {
            assert_eq!(asset.style_card_id, "demo-palette");
            assert_eq!((asset.width, asset.height), (8, 8));
            assert!(asset.pixels.iter().all(|&p| (p as usize) < palette_len));
        }
    }

    #[tokio::test]
    async fn generate_seeded_matches_core() {
        let server = ConjureMcp::new(PathBuf::from("."));
        let request_json = demo_request_json();
        let request = params::parse_request(&request_json).unwrap();
        let expected = conjure_core::generate(&request, 42);

        let result = server
            .conjure_generate_seeded(Parameters(params::GenerateSeededParams {
                request: request_json,
                seed: 42,
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        let assets: Vec<Asset> = serde_json::from_value(value["assets"].clone()).unwrap();
        assert_eq!(assets, expected);
    }

    #[tokio::test]
    async fn generate_provider_rules() {
        let server = ConjureMcp::new(PathBuf::from("."));
        let request_json = demo_request_json();
        let cases = [
            (
                params::GenerateParams {
                    request: request_json.clone(),
                    provider: "http".to_string(),
                    model: "x".to_string(),
                    base_url: None,
                    max_repairs: None,
                },
                "provider http requires base_url",
            ),
            (
                params::GenerateParams {
                    request: request_json.clone(),
                    provider: "opencode".to_string(),
                    model: "x".to_string(),
                    base_url: Some("http://127.0.0.1:9/v1".to_string()),
                    max_repairs: None,
                },
                "provider opencode does not take base_url",
            ),
            (
                params::GenerateParams {
                    request: request_json,
                    provider: "weird".to_string(),
                    model: "x".to_string(),
                    base_url: None,
                    max_repairs: None,
                },
                "unknown provider weird (use http or opencode)",
            ),
        ];
        for (case, message) in cases {
            let result = server.conjure_generate(Parameters(case)).await.unwrap();
            assert!(result.is_error == Some(true));
            assert_eq!(result_text(&result), results::err_text(message));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn models_with_mock_and_error_path() {
        let server = ConjureMcp::new(PathBuf::from("."));

        let mock = MockServer::start();
        let models_mock = mock.mock(|when, then| {
            when.method(Method::GET).path("/v1/models");
            then.status(200).json_body(json!({
                "data": [{"id": "m1"}, {"id": "m2"}]
            }));
        });
        let urls = vec![format!("{}/v1", mock.base_url())];
        let failing = write_stub(
            "models-fail",
            "#!/bin/sh\nif [ \"$1\" = \"models\" ]; then\n  echo boom >&2\n  exit 7\nfi\n",
        );
        let bin = failing.path.to_string_lossy().into_owned();

        let output = with_opencode_bin(&bin, models_output(&urls)).await;
        models_mock.assert();
        assert_eq!(output.local_servers.len(), 1);
        assert_eq!(output.local_servers[0].base_url, urls[0]);
        assert_eq!(
            output.local_servers[0].models,
            vec!["m1".to_string(), "m2".to_string()]
        );
        assert!(
            output
                .opencode_error
                .as_deref()
                .is_some_and(|e| e.contains("exited with 7"))
        );

        let result = with_opencode_bin(&bin, server.conjure_models())
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert!(
            value["opencode_error"]
                .as_str()
                .is_some_and(|e| e.contains("exited with 7"))
        );
        assert_eq!(value["opencode_models"], json!([]));
        assert!(value["local_servers"].as_array().is_some());
    }

    #[tokio::test]
    async fn init_creates_store() {
        let root = temp_root("init");
        let server = ConjureMcp::new(root.0.clone());

        let result = init(&server, None).await;
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(
            value["path"],
            json!(root.0.join(".conjure").display().to_string())
        );
        assert!(root.0.join(".conjure").exists());

        let result = init(&server, Some("sub".to_string())).await;
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(
            value["path"],
            json!(root.0.join("sub").join(".conjure").display().to_string())
        );
        assert!(root.0.join("sub").join(".conjure").exists());
    }

    #[tokio::test]
    async fn init_with_absolute_root_lands_there() {
        let home = temp_root("init-abs-home");
        let target = temp_root("init-abs-target");
        let server = ConjureMcp::new(home.0.clone());

        let result = init(&server, Some(target.0.to_string_lossy().into_owned())).await;
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(
            value["path"],
            json!(target.0.join(".conjure").display().to_string())
        );
        assert!(target.0.join(".conjure").exists());
    }

    #[tokio::test]
    async fn list_reports_uninitialized_store() {
        let root = temp_root("uninitialized");
        std::fs::create_dir_all(root.0.join(".conjure")).unwrap();
        let server = ConjureMcp::new(root.0.clone());

        let result = server
            .conjure_list(Parameters(params::ListParams { tag: None }))
            .await
            .unwrap();
        assert!(result.is_error == Some(true));
        let text = result_text(&result);
        assert!(
            text.contains("store not initialized (run conjure init)"),
            "unexpected error: {text}"
        );
    }

    #[tokio::test]
    async fn save_requires_init_then_round_trips() {
        let root = temp_root("save");
        let server = ConjureMcp::new(root.0.clone());
        let request_json = demo_request_json();
        let request = params::parse_request(&request_json).unwrap();
        let assets = conjure_core::generate(&request, 42);

        let refused = server
            .conjure_save(Parameters(params::SaveParams {
                request: request_json.clone(),
                assets: serde_json::to_value(&assets).unwrap(),
            }))
            .await
            .unwrap();
        assert!(refused.is_error == Some(true));
        let text = result_text(&refused);
        assert!(
            text.contains("no .conjure/ found (run conjure init)"),
            "unexpected error: {text}"
        );

        let result = init(&server, None).await;
        assert!(result.is_error == Some(false));

        let saved = server
            .conjure_save(Parameters(params::SaveParams {
                request: request_json,
                assets: serde_json::to_value(&assets).unwrap(),
            }))
            .await
            .unwrap();
        assert!(saved.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&saved)).unwrap();
        let card_hash: String = serde_json::from_value(value["card_hash"].clone()).unwrap();
        let asset_hashes: Vec<String> =
            serde_json::from_value(value["asset_hashes"].clone()).unwrap();
        assert_eq!(asset_hashes.len(), 3);

        let store = conjure_store::Store::open(&root.0).unwrap();
        assert_eq!(store.save_assets(&request, &assets).unwrap(), asset_hashes);
        assert_eq!(
            conjure_store::card_hash(&request.style_card).unwrap(),
            card_hash
        );

        let mut broken = assets.clone();
        broken[0].pixels[0] = 9;
        let refused = server
            .conjure_save(Parameters(params::SaveParams {
                request: serde_json::to_value(&request).unwrap(),
                assets: serde_json::to_value(&broken).unwrap(),
            }))
            .await
            .unwrap();
        assert!(refused.is_error == Some(false));
        let report: ValidationReport = serde_json::from_str(&result_text(&refused)).unwrap();
        assert!(!report.passed);
        assert_eq!(
            store
                .list_assets(&conjure_store::ListFilter { tag: None })
                .unwrap()
                .len(),
            3
        );
        assert_eq!(blob_count(&root.0), 3);
    }

    #[tokio::test]
    async fn list_and_show() {
        let root = temp_root("list-show");
        let server = ConjureMcp::new(root.0.clone());
        let result = init(&server, None).await;
        assert!(result.is_error == Some(false));
        let (request, assets) = save_demo_assets(&server, 42).await;
        let card_hash = conjure_store::card_hash(&request.style_card).unwrap();

        let store = conjure_store::Store::open(&root.0).unwrap();
        let asset_hashes = store.save_assets(&request, &assets).unwrap().clone();

        let list = server
            .conjure_list(Parameters(params::ListParams { tag: None }))
            .await
            .unwrap();
        assert!(list.is_error == Some(false));
        let text = result_text(&list);
        assert!(!text.contains("pixels"), "list leaked pixel data: {text}");
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["cards"].as_array().unwrap().len(), 1);
        assert_eq!(value["assets"].as_array().unwrap().len(), 3);

        let card_show = server
            .conjure_show(Parameters(params::ShowParams {
                hash: card_hash[..8].to_string(),
            }))
            .await
            .unwrap();
        assert!(card_show.is_error == Some(false));
        let text = result_text(&card_show);
        assert!(text.contains("\"palette\""));
        let card: StyleCard = serde_json::from_str(&text).unwrap();
        assert_eq!(card.id, "demo-palette");

        let asset_show = server
            .conjure_show(Parameters(params::ShowParams {
                hash: asset_hashes[0][..8].to_string(),
            }))
            .await
            .unwrap();
        assert!(asset_show.is_error == Some(false));
        let text = result_text(&asset_show);
        assert!(text.contains("\"pixels\""));
        let asset: Asset = serde_json::from_str(&text).unwrap();
        assert_eq!(asset.pixels, assets[0].pixels);

        let missing = server
            .conjure_show(Parameters(params::ShowParams {
                hash: "ffffffff".to_string(),
            }))
            .await
            .unwrap();
        assert!(missing.is_error == Some(true));
        assert_eq!(
            result_text(&missing),
            results::err_text("no card or asset matches ffffffff")
        );
    }

    #[tokio::test]
    async fn export_round_trips() {
        let root = temp_root("export");
        let server = ConjureMcp::new(root.0.clone());
        let result = init(&server, None).await;
        assert!(result.is_error == Some(false));
        let (request, assets) = save_demo_assets(&server, 42).await;
        let card_hash = conjure_store::card_hash(&request.style_card).unwrap();

        let store = conjure_store::Store::open(&root.0).unwrap();
        let asset_hashes = store.save_assets(&request, &assets).unwrap().clone();

        let export = server
            .conjure_export(Parameters(params::ExportParams {
                hashes: asset_hashes.clone(),
                layout: None,
                columns: None,
                scale: None,
            }))
            .await
            .unwrap();
        assert!(export.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&export)).unwrap();
        assert_eq!(value["manifest"]["style_card"]["hash"], json!(card_hash));

        let png = base64::prelude::BASE64_STANDARD
            .decode(value["png_base64"].as_str().unwrap())
            .unwrap();
        let decoded = conjure_export::decode_indexed_png(&png).unwrap();
        assert_eq!((decoded.width, decoded.height), (24, 8));

        let stored: Vec<Asset> = asset_hashes
            .iter()
            .map(|hash| store.load_asset(hash).unwrap())
            .collect();
        let mut expected = Vec::new();
        for y in 0..8usize {
            for asset in &stored {
                expected.extend_from_slice(&asset.pixels[y * 8..(y + 1) * 8]);
            }
        }
        assert_eq!(decoded.indices, expected);

        let grid = server
            .conjure_export(Parameters(params::ExportParams {
                hashes: asset_hashes.clone(),
                layout: Some("grid".to_string()),
                columns: None,
                scale: None,
            }))
            .await
            .unwrap();
        assert!(grid.is_error == Some(true));
        assert_eq!(
            result_text(&grid),
            results::err_text("grid layout requires columns")
        );

        let diag = server
            .conjure_export(Parameters(params::ExportParams {
                hashes: asset_hashes,
                layout: Some("diag".to_string()),
                columns: Some(2),
                scale: None,
            }))
            .await
            .unwrap();
        assert!(diag.is_error == Some(true));
        assert_eq!(
            result_text(&diag),
            results::err_text("unknown layout diag (use strip_horizontal or grid)")
        );
    }

    #[tokio::test]
    async fn export_over_cap() {
        let root = temp_root("over-cap");
        let server = ConjureMcp::new(root.0.clone());
        let result = init(&server, None).await;
        assert!(result.is_error == Some(false));
        let store = conjure_store::Store::open(&root.0).unwrap();
        let card = big_palette_card();

        // 3072 stays under the cap with LCG noise; the blueprint sanctions
        // escalating to 4096 if deflate compresses the smaller sheet below it.
        let mut capped = false;
        for size in [3072u32, 4096] {
            let request = AssetRequest {
                name: format!("huge-{size}"),
                kind: AssetKind::Sprite {
                    size: Dimensions { w: size, h: size },
                },
                style_card: card.clone(),
            };
            let asset = Asset {
                style_card_id: card.id.clone(),
                width: size,
                height: size,
                pixels: lcg_pixels(size as usize * size as usize),
            };
            let hashes = store.save_assets(&request, &[asset]).unwrap();

            let export = server
                .conjure_export(Parameters(params::ExportParams {
                    hashes,
                    layout: None,
                    columns: None,
                    scale: None,
                }))
                .await
                .unwrap();
            let text = result_text(&export);
            if export.is_error == Some(true) && text.contains("over the 8 MiB mcp payload cap") {
                capped = true;
                break;
            }
        }
        assert!(capped, "no export hit the 8 MiB mcp payload cap");
    }

    #[test]
    fn loopback_rules() {
        let refused = check_loopback("0.0.0.0:9711".parse().unwrap()).unwrap_err();
        assert_eq!(
            refused,
            "refusing to bind non-loopback address 0.0.0.0:9711"
        );
        assert!(check_loopback("127.0.0.1:9711".parse().unwrap()).is_ok());
        assert!(check_loopback("[::1]:9711".parse().unwrap()).is_ok());
    }

    #[tokio::test]
    async fn generate_httpmock_repair_turn() {
        let server = ConjureMcp::new(PathBuf::from("."));
        let request = sprite_request();
        let mock = MockServer::start();
        let garbage = mock.mock(|when, then| {
            when.method(Method::POST)
                .path("/v1/chat/completions")
                .body_excludes("not valid JSON");
            then.status(200).json_body(json!({
                "choices": [{ "message": { "role": "assistant", "content": GARBAGE_REPLY } }]
            }));
        });
        let repaired = mock.mock(|when, then| {
            when.method(Method::POST)
                .path("/v1/chat/completions")
                .body_includes("not valid JSON");
            then.status(200).json_body(json!({
                "choices": [{ "message": { "role": "assistant", "content": SPRITE_SPEC } }]
            }));
        });

        let result = server
            .conjure_generate(Parameters(params::GenerateParams {
                request: serde_json::to_value(&request).unwrap(),
                provider: "http".to_string(),
                model: "x".to_string(),
                base_url: Some(format!("{}/v1", mock.base_url())),
                max_repairs: Some(3),
            }))
            .await
            .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(value["repairs_used"], json!(1));
        assert_eq!(value["used_fallback"], json!(false));
        let generated: Vec<Asset> = serde_json::from_value(value["assets"].clone()).unwrap();
        assert!(verify_request(&request, &generated).passed);
        garbage.assert();
        repaired.assert();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn generate_opencode_stub() {
        let event = json!({"type": "text", "part": {"text": SPRITE_SPEC}});
        let line = serde_json::to_string(&event).unwrap();
        let script =
            format!("#!/bin/sh\nif [ \"$1\" = \"run\" ]; then\n  printf '%s\\n' '{line}'\nfi\n");
        let stub = write_stub("opencode-gen", &script);
        let bin = stub.path.to_string_lossy().into_owned();

        let server = ConjureMcp::new(PathBuf::from("."));
        let request = sprite_request();
        let result = with_opencode_bin(
            &bin,
            server.conjure_generate(Parameters(params::GenerateParams {
                request: serde_json::to_value(&request).unwrap(),
                provider: "opencode".to_string(),
                model: "test-model".to_string(),
                base_url: None,
                max_repairs: Some(0),
            })),
        )
        .await
        .unwrap();
        assert!(result.is_error == Some(false));
        let value: serde_json::Value = serde_json::from_str(&result_text(&result)).unwrap();
        assert_eq!(value["used_fallback"], json!(false));
        assert_eq!(value["repairs_used"], json!(0));
        let generated: Vec<Asset> = serde_json::from_value(value["assets"].clone()).unwrap();
        assert!(verify_request(&request, &generated).passed);
    }

    #[test]
    fn expand_spec_shape_matches_core_compact() {
        let request = sprite_request();
        let spec: CompactSpec = serde_json::from_str(SPRITE_SPEC).unwrap();
        let expanded = conjure_core::expand(&request, &spec).unwrap();
        assert_eq!(
            expanded[0].pixels,
            vec![0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 2, 1, 0, 0, 0, 0]
        );
    }
}
