# Conjure Terrain Map

Last Synced Commit: 87487c6ed361da24d11f2748cced14870b9a4403

## System Architecture & Control Flow

Rust workspace (resolver "3", edition 2024): `conjure-core` ← `conjure-inference` ← `conjure-agent` ← `conjure-cli`; leaves `conjure-store`/`conjure-export` depend only on core. Entry: `conjure-cli/src/main.rs` hand-rolled `env::args()` dispatch (`generate|validate|repair|models|agent|init|save|ls|show|export|tag|gc`). Agent loop `conjure-agent/src/gen.rs`: chat → parse compact spec → expand → verify → repair turns → seeded fallback; `Backend` enum (`Http(Client)`/`OpenCode(OpenCodeCli)`) is the only inference abstraction. Store: project-local `.conjure/` (SQLite via rusqlite, WAL; content-addressed BLAKE3 blobs), located by walking cwd upward. Export: indexed PNG (`PLTE`+`tRNS`, png crate) + JSON manifest v1; refuses invalid input, never clamps; 2^26-pixel cap. Exit codes: 0 success; 1 validation-fail with JSON report on stdout (validate, save); 2 every other error (usage/errors on stderr).

## Canonical Data Models & Contracts

conjure-core re-exports: `Asset{style_card_id,width,height,pixels}` (palette indices), `AssetRequest{name,kind,style_card}`, `StyleCard` + `validate`, `ValidationReport{passed,errors}`. `AssetKind`/`CompactSpec` internally tagged `#[serde(tag="type",rename_all="snake_case")]`. Core depends only on `serde`; errors are `Result<T,String>`. Store: `Store` (open/create, save_card/save_assets, load/list/asset_summary, tag/gc, resolve_card/resolve_asset), `StoreError` (NotFound/Invalid/Corrupt/Io/Sqlite/Verify via `Verification`), hashes `asset_hash`/`card_hash`/`blob_hash` (BLAKE3 over 16-byte magics). Export: `SheetLayout`, `ExportOpts`, `export_png`, `export_manifest(card,assets,names,opts)`, `Manifest` v1 structs, `ExportError`, `encode_indexed_png`/`decode_indexed_png`.

## Reusable Utilities & Repo Conventions

`PaletteColor::rgb()`; `StyleCard::validate`; CLI helpers `read_json`/`print_json`/`err`/`safe_output_stem`. Test discipline: inline `#[cfg(test)]` in-crate plus `conjure-cli/tests/cli.rs` binary integration tests (spawn `CARGO_BIN_EXE_conjure-cli` in nonce temp dirs with Drop cleanup; no tempfile dep). httpmock 0.8; stub-binary harness in `conjure-inference/src/opencode.rs`. Commits (§15): lowercase `<label>(<scope>): <description>`, imperative, no trailing period; never push. No comments unless a non-obvious why. Docs update in the same change.

## Known Landmines & Coupling Points

- Workspace `members` list is manual; forgetting a new crate silently drops it from `cargo test --workspace`.
- conjure-agent `gen`/`prompt` private. Core stays untouched: store/export wrap types in their own envelopes.
- No clap: positional argv checks; existing commands must stay byte-identical.
- thiserror in store/export only, never core. `rusqlite` needs `bundled`; `png` (not `image`) is the indexed-PNG encoder — image cannot encode indexed color. Every new dep needs written justification.
- Hash lookup requires unique prefix ≥ 8 hex chars. `.conjure/` found by walking cwd upward; leave `.gitignore` alone.
- `cargo check` warns `unused dependency serde_json` for conjure-export (test-only dep; clippy gate passes) — do not "clean it up" by removing it.
- `CONJURE_OPENCODE_BIN` env knob; `conjure agent --provider opencode` requires repo cwd.

## Discovered Commands

- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all -- --check`
- Build: `cargo build --workspace`
- No CI workflows; the three checks above are the acceptance bar.
