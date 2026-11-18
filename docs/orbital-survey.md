# Conjure Terrain Map

Last Synced Commit: b916da19da0d854290bda4fd825fd15e0148dbc0

## System Architecture & Control Flow

Rust workspace (resolver "3", edition 2024): `conjure-core` ← `conjure-inference` ← `conjure-agent` ← `conjure-cli`; leaves `conjure-store`/`conjure-export` depend only on core; `conjure-mcp` (rmcp 3.11) wraps agent/core/store/export as 12 MCP tools — stdio default, loopback-only Streamable HTTP. Entry: `conjure-cli/src/main.rs` hand-rolled dispatch (`generate|validate|repair|models|agent|init|save|ls|show|export|tag|gc|mcp`); `conjure mcp` and the `conjure-mcp` bin share one `serve_stdio`/`serve_http`. Store: project-local `.conjure/` (SQLite WAL + BLAKE3 blobs), located by walking cwd upward. Export: indexed PNG (`PLTE`+`tRNS`, png crate) + manifest v1; refuses, never clamps; 2^26-pixel cap. Exit codes: 0 success; 1 validation-fail JSON on stdout (validate, save); 2 everything else.

## Canonical Data Models & Contracts

conjure-core: `Asset{style_card_id,width,height,pixels}`, `AssetRequest{name,kind,style_card}`, `StyleCard`+`validate`, `ValidationReport{passed,errors}`; internally tagged `#[serde(tag="type",rename_all="snake_case")]`; core is serde-only, `Result<T,String>`. Store: `Store` (open/create, save/load/list/asset_summary, tag/gc, resolve_card/resolve_asset), `StoreError` (NotFound/Invalid/Corrupt/Io/Sqlite/Verify), BLAKE3 hashes with 16-byte magics. Export: `SheetLayout`, `ExportOpts`, `export_png`, `export_manifest(card,assets,names,opts)`, `Manifest` v1, `encode/decode_indexed_png`. MCP: `ConjureMcp{root,tool_router}`, `serve_stdio/serve_http/check_loopback`; tool inputs are `serde_json::Value` parsed strictly into core types (core has no JsonSchema derives); two-bucket results — validation failures are successful tool results with report bodies, everything else is caller-visible `error:` text.

## Reusable Utilities & Repo Conventions

`PaletteColor::rgb()`; CLI helpers `read_json`/`print_json`/`err`/`safe_output_stem`. Test discipline: inline `#[cfg(test)]` plus binary/integration tests spawning real processes — `conjure-cli/tests/cli.rs` (CARGO_BIN_EXE_conjure-cli) and `conjure-mcp/tests/client.rs` (in-process rmcp client over tokio duplex); nonce temp dirs with Drop cleanup; no tempfile dep. httpmock 0.8; stub-binary harness in `conjure-inference/src/opencode.rs`. Commits (§15): lowercase `<label>(<scope>): <description>`, imperative, no trailing period; never push. No comments unless a non-obvious why. Docs update in the same change.

## Known Landmines & Coupling Points

- Workspace `members` list is manual; forgetting a crate drops it from `cargo test --workspace`.
- conjure-agent `gen`/`prompt` private. Core untouched: store/export/mcp wrap types in their own envelopes (store summaries serialize via mcp mapping fns, not serde derives).
- No clap: positional argv checks; existing commands must stay byte-identical.
- rmcp pinned 3.11; schemars via rmcp's re-export (`rmcp::schemars`), never a direct dep; `tool_router` field name is macro-required.
- stdout is the MCP protocol in conjure-mcp — zero println! in lib code; logs only via pre-serve stderr.
- MCP `conjure_init` per-call root resolves against the session root (relative → join); other tools locate from the server root only.
- thiserror store/export only; rusqlite bundled; png (not image) for indexed color; hash prefixes ≥ 8 hex unique.
- `cargo check` warns unused-dep serde_json in conjure-export (test-only; clippy passes) — do not remove it.
- `CONJURE_OPENCODE_BIN` env knob; `conjure agent --provider opencode` requires repo cwd.

## Discovered Commands

- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all -- --check`
- Build: `cargo build --workspace`
- No CI workflows; the three checks above are the acceptance bar.
