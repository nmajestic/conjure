# Conjure Desktop App (v1) — Milestone 4 Product Spec

**Adopted decisions (2026-11-18):** v1 job = library browser + viewer + export + live generation cockpit; data path = in-process Conjure crates (not an MCP client); live generation included; test bar = headless render + state assertions (no input simulation); platform = Linux desktop only.

## Objective

A solo dev opens a Conjure project in a desktop window and can do everything the CLI offers: browse the style-card and asset library, zoom into pixel-exact previews, export sheets, and drive agent generation with live repair-turn progress. v1 succeeds when the owner can go generate → save → browse → export entirely inside the app on Linux, every screen provable headlessly without a real model.

## User Flows

1. **Open project.** Launch in a project directory; the app locates `.conjure/` by walking upward (CLI rule), then shows browse.
2. **Browse.** Cards (name, short hash) and assets (name, kind, WxH, short hash, tags). Selecting an asset opens the viewer: zoomable pixel-exact preview rendered from indices and the card palette (index 0 transparent), palette strip, dimensions, hash.
3. **Export.** Select assets of one style card; choose strip or grid (+columns) and scale 1–16; the app writes the same PNG + JSON manifest the CLI `export` produces to a chosen directory.
4. **Generate.** Pick a provider/model from local discovery (`conjure models` parity) or the OpenCode CLI; choose an existing style card plus a name and kind (sprite size, or tileset size + count); run. Live events stream in order: model call → spec parsed → verify result → repair turn N → passed / fallback engaged. Cancel aborts. On success one click saves the assets (store semantics identical to `save`).
5. **Initialize.** From the no-library state, an initialize action creates `.conjure/` (same as `conjure init`).

## States & Edge Cases

- **No `.conjure/`** → no-library screen with the initialize action; never scans `$HOME`.
- **Empty library** → explicit empty state; generate stays available.
- **Generation:** idle → running → verifying → repairing (turn count) → succeeded | failed; failure shows the agent's error text verbatim (transport failure, or a fallback-engaged note when the model never passes and the deterministic fallback ships assets). One generation at a time; browse/export stay usable; cancel returns to idle and discards the run.
- **No model discovered** → generate disabled with a reason and a refresh action; the rest of the app works.
- **Export refusals** (mixed style cards, oversize) surface the CLI's exact error text; nothing is written.
- **Huge assets** preview individually without composing sheets; sheet composition stays under the existing export pixel cap.
- **Missing GPU session** → the app cannot start; tests use Rune's headless renderer.

## Testable Acceptance Criteria

1. Seeded populated store → headless render shows cards and assets with short hashes.
2. No-store and empty-store projects render distinct guidance states.
3. Asset selection renders a preview matching the asset's dimensions and the card's palette order.
4. App export writes byte-identical PNG + manifest to the CLI export for the same inputs.
5. Generate against a stub model: events render in order; the run ends passed; save writes assets that pass `verify_request`; cancel aborts.
6. No-model state renders with generate disabled and a reason.
7. Every state above renders headlessly on Linux; no network beyond the (mocked) model call.

## Explicit Out-of-Scope

Style-card creation or editing (no editor); macOS/Windows, packaging, distribution; input-simulation tests; MCP-client mode; pixel editing, animation timing, image import, concurrent generations, remote/cloud models, credentials of any kind; visual polish beyond a legible minimal layout.
