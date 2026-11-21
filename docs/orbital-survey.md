# Conjure Terrain Map

Last Synced Commit: 10124f0572185b0946ac3654fc418e4a56f89c19

## System Architecture & Control Flow

Rust workspace (resolver "3", edition 2024): `conjure-core` ← `conjure-inference` ← `conjure-agent` ← `conjure-cli`; leaves `conjure-store`/`conjure-export` depend only on core; `conjure-mcp` (rmcp 3.11) wraps agent/core/store/export as 12 MCP tools (stdio default, loopback-only HTTP); `conjure-app` is a Rune desktop UI over the same contracts (browse/viewer/export/gen cockpit), Linux-first, rune pinned as a git rev via the workspace `[patch.crates-io]` table. CLI dispatch (hand-rolled): `generate|validate|repair|models|agent|init|save|ls|show|export|tag|gc|mcp`; `conjure mcp` and the `conjure-mcp` bin share one serve impl; the app's gen thread runs conjure-agent on a background tokio runtime, streaming `GenEvent`s into the UI. Store: project-local `.conjure/` (SQLite WAL + BLAKE3 blobs; atomic temp+rename writes, 5s busy_timeout, BEGIN IMMEDIATE save/gc), located by walking cwd upward; `asset_names` keeps every name per asset (one list row per name), backfilled once in an Immediate tx only when the table is missing. CLI `export` takes `--layout/--columns/--scale` (defaults strip, scale 1). Exit codes (CLI): 0 success; 1 validation-fail JSON; 2 other errors.

## Canonical Data Models & Contracts

conjure-core: `Asset{style_card_id,width,height,pixels}`, `AssetRequest{name,kind,style_card}`, `StyleCard`+`validate`, `ValidationReport{passed,errors}`; internally tagged `#[serde(tag="type",rename_all="snake_case")]`; core is serde-only. Store: `Store` (open/create, save/load/list/asset_summary, tag/gc(+`gc_with_grace`), resolve_card/resolve_asset), `StoreError` (NotFound/Invalid/Corrupt/Io/Sqlite/Verify), `list_assets` yields one row per (asset,name). Export: `SheetLayout`, `ExportOpts`, `export_png`, `export_manifest(card,assets,names,opts)`, `Manifest` v1, `encode/decode_indexed_png`. MCP: `ConjureMcp{root,tool_router}`, `serve_stdio/serve_http/check_loopback`; tool inputs are `serde_json::Value` parsed into core types; verify failures return report bodies as successful results. Agent: `GenOpts{max_repairs, on_event}` streams `GenEvent` (ModelCall/SpecParsed/VerifyFailed/RepairTurn/FallbackEngaged/Passed) via `Option<GenObserver>`, None by default. App: `AppState` view models (Screen/Cards/Assets/GenPanel), `selection: Vec<(hash,name)>`, `pixels::compose → Option<Preview>`, `gen::GenCommand` mpsc runner.

## Reusable Utilities & Repo Conventions

`PaletteColor::rgb()`; CLI helpers `read_json`/`print_json`/`err`/`safe_output_stem` (mirrored by the app). Test discipline: inline `#[cfg(test)]` plus real-process integration tests — `conjure-cli/tests/cli.rs` (CARGO_BIN_EXE_conjure-cli), `conjure-mcp/tests/client.rs` (in-process rmcp client over duplex), `conjure-app/tests/app.rs` (rune Runtime + `tooling::headless` keyed taps, GPU-offscreen); nonce temp dirs with Drop cleanup; httpmock 0.8; stub-binary harness in `conjure-inference/src/opencode.rs`. Commits (§15): lowercase `<label>(<scope>): <description>`, imperative, no trailing period; never push. Docs update in the same change.

## Known Landmines & Coupling Points

- Workspace `members` list is manual; forgetting a crate drops it from `cargo test --workspace`.
- No clap: positional argv checks; existing commands must stay byte-identical.
- rmcp pinned 3.11 (`tool_router` macro-required); rune pinned to rev 1505f921 — updates require the `[patch.crates-io]` table and Linux build deps (libxkbcommon/libwayland).
- stdout is the MCP protocol in conjure-mcp — zero println! in lib code.
- App preview cap: `MAX_PREVIEW_PIXELS = 1<<24` composed px, refuse-not-clamp (u64 math); never remove without an allocation plan.
- `conjure-app` module `gen` is a raw identifier (`r#gen`) in edition 2024.
- `cargo check` warns unused-dep serde_json in conjure-export (test-only; clippy passes) — do not remove it.
- `CONJURE_OPENCODE_BIN` env knob; `conjure agent --provider opencode` requires repo cwd.
- rusqlite `Transaction` borrows the connection — block-scope statements before `tx.commit()`; `cargo fmt` is a hard gate (§11).

## Discovered Commands

- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all -- --check`
- Build: `cargo build --workspace`
- No CI workflows; the three checks above are the acceptance bar.
