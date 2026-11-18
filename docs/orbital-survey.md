# Conjure Terrain Map

Last Synced Commit: 5c3634dd5c35300f06154dba3f8252e5f74fafb1

## System Architecture & Control Flow

Rust workspace (resolver "3", edition 2024): `conjure-core` ← `conjure-inference` ← `conjure-agent` ← `conjure-cli`; leaves `conjure-store`/`conjure-export` depend only on core; `conjure-mcp` (rmcp 3.11) wraps agent/core/store/export as 12 MCP tools — stdio default, loopback-only Streamable HTTP; `conjure-app` is a Rune desktop UI over the same contracts (browse, pixel-exact viewer, export, live generation cockpit), Linux-first, rune pinned as a git rev with the workspace-level `[patch.crates-io]` table repeating its five vendored crates. CLI dispatch (hand-rolled): `generate|validate|repair|models|agent|init|save|ls|show|export|tag|gc|mcp`; `conjure mcp` and the `conjure-mcp` bin share one `serve_stdio`/`serve_http`; the app's gen thread runs conjure-agent on a background tokio runtime, streaming `GenEvent`s into the UI via `AppHandle::update`. Store: project-local `.conjure/` (SQLite WAL + BLAKE3 blobs), located by walking cwd upward. Exit codes (CLI): 0 success; 1 validation-fail JSON on stdout; 2 everything else.

## Canonical Data Models & Contracts

conjure-core: `Asset{style_card_id,width,height,pixels}`, `AssetRequest{name,kind,style_card}`, `StyleCard`+`validate`, `ValidationReport{passed,errors}`; internally tagged `#[serde(tag="type",rename_all="snake_case")]`; core is serde-only, `Result<T,String>`. Store: `Store` (open/create, save/load/list/asset_summary, tag/gc, resolve_card/resolve_asset), `StoreError` (NotFound/Invalid/Corrupt/Io/Sqlite/Verify), BLAKE3 hashes with 16-byte magics. Export: `SheetLayout`, `ExportOpts`, `export_png`, `export_manifest(card,assets,names,opts)`, `Manifest` v1, `encode/decode_indexed_png`. MCP: `ConjureMcp{root,tool_router}`, `serve_stdio/serve_http/check_loopback`; tool inputs are `serde_json::Value` parsed into core types; validation failures are successful tool results with report bodies. Agent: `GenOpts{max_repairs, on_event}` — `on_event: Option<GenObserver>` streams `GenEvent` (ModelCall/SpecParsed/VerifyFailed/RepairTurn/FallbackEngaged/Passed), None by default (CLI/MCP unchanged). App: `AppState` view models (Screen/Cards/Assets/GenPanel), `pixels::compose → Option<Preview>`, `gen::GenCommand` mpsc runner.

## Reusable Utilities & Repo Conventions

`PaletteColor::rgb()`; CLI helpers `read_json`/`print_json`/`err`/`safe_output_stem` (mirrored by the app's export refusal). Test discipline: inline `#[cfg(test)]` plus real-process integration tests — `conjure-cli/tests/cli.rs` (CARGO_BIN_EXE_conjure-cli), `conjure-mcp/tests/client.rs` (in-process rmcp client over duplex), `conjure-app/tests/app.rs` (rune Runtime + `tooling::headless` keyed taps, GPU-offscreen, `--snap` smoke); nonce temp dirs with Drop cleanup; httpmock 0.8; stub-binary harness in `conjure-inference/src/opencode.rs`. Commits (§15): lowercase `<label>(<scope>): <description>`, imperative, no trailing period; never push. Docs update in the same change; product specs land at `docs/specs/`.

## Known Landmines & Coupling Points

- Workspace `members` list is manual; forgetting a crate drops it from `cargo test --workspace`.
- No clap: positional argv checks; existing commands must stay byte-identical.
- rmcp pinned 3.11 (schemars via rmcp re-export; `tool_router` field macro-required); rune pinned to git rev 1505f921 — updating it means updating the 5-entry `[patch.crates-io]` table and re-verifying the Linux build deps (libxkbcommon/libwayland).
- stdout is the MCP protocol in conjure-mcp — zero println! in lib code.
- App preview cap: `MAX_PREVIEW_PIXELS = 1<<24` composed px, refuse-not-clamp (u64 math; zoom refusal keeps the last composable preview); never remove without an allocation plan.
- `conjure-app` module `gen` is a raw identifier (`r#gen`) in edition 2024.
- `cargo check` warns unused-dep serde_json in conjure-export (test-only; clippy passes) — do not remove it.
- `CONJURE_OPENCODE_BIN` env knob; `conjure agent --provider opencode` requires repo cwd.

## Discovered Commands

- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all -- --check`
- Build: `cargo build --workspace`
- No CI workflows; the three checks above are the acceptance bar.
