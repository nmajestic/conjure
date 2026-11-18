//! In-process MCP integration tests: a real rmcp client speaking the
//! protocol over a tokio duplex against the Conjure server, proving the
//! tool surface and the caller-visible error contract end to end.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use conjure_mcp::ConjureMcp;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock};
use rmcp::service::{RoleClient, RunningService};
use serde_json::json;

type Client = RunningService<RoleClient, ()>;

/// A nonce temp root removed on drop (pid + nanos + label keeps
/// concurrent runs from colliding).
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
        "conjure-mcp-client-test-{}-{nanos}-{label}",
        std::process::id()
    )))
}

/// Spawns the server on one end of a duplex and returns a handshaken
/// client on the other.
async fn spawn_server(root: &Path) -> Client {
    let (client_side, server_side) = tokio::io::duplex(4096);
    let server = ConjureMcp::new(root.to_path_buf());
    tokio::spawn(async move {
        // Hold the RunningService: dropping it cancels the server.
        if let Ok(running) = server.serve(server_side).await {
            let _ = running.waiting().await;
        }
    });
    ().serve(client_side).await.expect("client handshake")
}

async fn call(client: &Client, name: &str, arguments: serde_json::Value) -> CallToolResult {
    let arguments = arguments.as_object().cloned().expect("object arguments");
    client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments))
        .await
        .expect("call_tool")
}

fn first_text(result: &CallToolResult) -> String {
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

#[tokio::test]
async fn lists_all_twelve_tools() {
    let root = temp_root("list-tools");
    let client = spawn_server(&root.0).await;

    let tools = client.list_all_tools().await.expect("list_all_tools");
    let mut names: Vec<String> = tools
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    let mut expected: Vec<String> = [
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
    ]
    .iter()
    .map(|name| name.to_string())
    .collect();
    expected.sort();
    assert_eq!(names, expected);
    assert_eq!(names.len(), 12);
}

#[tokio::test]
async fn validate_card_round_trip() {
    let root = temp_root("validate-card");
    let client = spawn_server(&root.0).await;
    let card = demo_request_json()["style_card"].clone();

    let result = call(
        &client,
        "conjure_validate_card",
        json!({ "style_card": card }),
    )
    .await;
    assert_eq!(result.is_error, Some(false));
    let value: serde_json::Value = serde_json::from_str(&first_text(&result)).unwrap();
    assert_eq!(value["ok"], json!(true));
    assert!(value.get("error").is_none());
}

#[tokio::test]
async fn generate_seeded_via_protocol() {
    let root = temp_root("generate-seeded");
    let client = spawn_server(&root.0).await;
    let request_json = demo_request_json();
    let request = conjure_mcp::params::parse_request(&request_json).unwrap();

    let result = call(
        &client,
        "conjure_generate_seeded",
        json!({ "request": request_json, "seed": 42 }),
    )
    .await;
    assert_eq!(result.is_error, Some(false));
    let value: serde_json::Value = serde_json::from_str(&first_text(&result)).unwrap();
    let assets: Vec<conjure_core::Asset> = serde_json::from_value(value["assets"].clone()).unwrap();
    assert_eq!(assets.len(), 3);
    for asset in &assets {
        assert_eq!((asset.width, asset.height), (8, 8));
    }
    assert!(conjure_core::verify_request(&request, &assets).passed);
}

#[tokio::test]
async fn list_without_store_is_a_visible_tool_error() {
    let root = temp_root("no-store");
    std::fs::create_dir_all(&root.0).unwrap();
    let client = spawn_server(&root.0).await;

    // No .conjure anywhere in or above the fresh root: the failure must
    // reach the caller as tool content, not as an opaque protocol error.
    let result = call(&client, "conjure_list", json!({})).await;
    assert_eq!(result.is_error, Some(true));
    let text = first_text(&result);
    assert!(
        text.contains("no .conjure/ found (run conjure init)"),
        "unexpected error: {text}"
    );
}
