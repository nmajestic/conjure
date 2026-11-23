# Conjure: architecture

How Conjure is built and why. The rules every change must follow are in
[rules.md](rules.md). Formats and behavior of each part are in
[reference/](reference/).

## Overview

```
style card ─┐
            ├─▶ conjure-agent ──model call──▶ conjure-inference ──▶ local server or OpenCode
request ────┘        │
                     ▼
              compact spec ──▶ conjure-core: expand ──▶ indexed assets ──▶ verify
                                                                            │
                                     fail: diagnostics back to the agent ◀──┤
                                     pass ──▶ conjure-store / conjure-export
```

Three shells drive the same libraries the same way: the CLI, the MCP server
and the desktop app.

## Crates

| Crate | What it does | Depends on |
|---|---|---|
| `conjure-core` | Style card, request, compact spec, assets, verifier, deterministic generator, repair. Pure. | `serde` |
| `conjure-inference` | Model backends: a keyless OpenAI-compatible HTTP client with local server discovery, and an OpenCode CLI driver. | `reqwest`, `tokio` |
| `conjure-agent` | The generate → verify → repair loop. | core, inference |
| `conjure-store` | The project's `.conjure/` library: SQLite metadata and content-addressed pixel blobs. `asset_names(asset_hash, name)` keeps every name an asset was saved under; `list_assets` returns one row per (asset, name), and identical pixels under different names share one blob and one asset row. | core, `rusqlite`, `blake3` |
| `conjure-export` | Indexed PNG sheets and JSON manifests. | core, `png` |
| `conjure-cli` | The `conjure` command. | all libraries, mcp |
| `conjure-mcp` | MCP server exposing the same operations as tools. | all libraries, `rmcp`, `axum` |
| `conjure-app` | Rune desktop app: browse, preview, export, generate. | all libraries, `rune` |

## Stack

| Layer | Choice | Why |
|---|---|---|
| Language | Rust, edition 2024 | One language for core, CLI, MCP and UI. |
| UI | [Rune](https://github.com/anomalyco/rune), pinned git revision | GPU-rendered with wgpu, and it can render headlessly so the UI can be tested without a screen. |
| HTTP | `reqwest` + `tokio` | `rmcp` needs tokio anyway, and the app streams progress. |
| MCP | `rmcp` (official Rust SDK) | stdio and Streamable HTTP transports. |
| Storage | SQLite (`rusqlite`, bundled) + files | Metadata in SQLite, pixels as content-addressed files. |
| Hashing | BLAKE3 | Content addressing for assets, cards and blobs. |
| Images | `png` | Writes indexed color (`PLTE` + `tRNS`) directly, which the `image` wrapper can't. |
| Agent loop | Hand-rolled | No LLM framework; JSON reply, lenient parse, re-prompt on failure. |

## Assets and the compact spec

An asset is a grid of indices into its style card's palette (2 to 32 colors).
Palette fidelity is structural: an asset can't contain a color the palette
doesn't have. Color is applied only at export.

Models never write pixel arrays. They write a `CompactSpec`: rows of text, one
character per pixel, where `0`–`9` are palette indices 0–9 and `a`–`v` are
10–31.

```json
{"type": "sprite", "rows": ["0110", "1221", "1221", "0110"]}
{"type": "tileset", "tiles": [["01", "10"], ["22", "22"]]}
```

`conjure_core::expand` turns it into assets strictly. A wrong row length,
unknown character or index past the palette is an error message the agent
sends back to the model.

The verifier then checks the hard constraints: dimensions, pixel count,
palette range and asset count. Each failure is a typed `Diagnostic` with a
`DiagnosticCode`.

## The agent loop

`conjure_agent::generate` runs up to `max_repairs + 1` model calls (3 repairs
by default):

1. Send the system prompt and the request, asking for a compact spec as JSON.
2. Parse the reply leniently (code fences and surrounding text are tolerated).
   If it isn't valid JSON, say so and ask again.
3. Expand the spec. If expansion fails, send the error back and ask again.
4. Verify. If it passes, return. If not, send every diagnostic back and ask
   again.

If no attempt passes, the deterministic fallback takes over: `repair` on the
last expanded assets, or `generate` with seed 0 if nothing ever expanded. The
result says how many repairs were used and whether the fallback ran.

Progress is reported as events: model call, spec parsed, verify failed,
repair turn, fallback engaged, passed. The desktop app shows them live.

## Model backends

Conjure never runs a model and never holds a credential. It has two
backends:

- **HTTP:** a keyless OpenAI-compatible client for local servers. It never
  sends an `Authorization` header and times out after 180 seconds. Discovery
  probes Ollama (`11434`), LM Studio (`1234`), llama.cpp (`8080`) and vLLM
  (`8000`) on localhost and reads `/v1/models`. Any other OpenAI-compatible
  server works through `--base-url`.
- **OpenCode:** runs `opencode run --format json --archived --model <id>
  --agent conjure` as a subprocess, killed after 180 seconds, and reads the
  reply from its JSON output. OpenCode owns all auth. Each repair turn resends
  the whole conversation, since runs are stateless. The binary can be
  overridden with `CONJURE_OPENCODE_BIN`.

The `conjure` OpenCode agent is defined in `.opencode/opencode.json` with every
tool denied. OpenCode finds that file by searching upward from the working
directory, so the OpenCode backend only works when run from inside this
repository.

## The desktop app at runtime

Rune owns the UI thread. A background thread runs a tokio runtime for
generation. The UI sends commands over a channel, and generation events come
back as state updates. Cancelling aborts the running task. Details:
[reference/desktop-app.md](reference/desktop-app.md).

## Decisions

| Date | Decision |
|---|---|
| 2026-11-16 | Indexed pixels plus a palette as the canonical asset form. |
| 2026-11-16 | Models emit a compact text spec, not raw index arrays. |
| 2026-11-16 | `reqwest` + `tokio` for HTTP. |
| 2026-11-16 | Hand-rolled agent loop instead of an LLM framework (Rig). |
| 2026-11-16 | OpenCode CLI as a second backend, so hosted models work without Conjure touching a key. |
| 2026-11-16 | Project-local `.conjure/` library, content-addressed and immutable. |
| 2026-11-16 | Indexed PNG export with palette index 0 transparent. |
| 2026-11-18 | Desktop app uses the crates in-process (not as an MCP client); Linux first. |

## Risks

- **Rune is pre-release.** Its API may change. It is pinned to one revision and
  used only by `conjure-app`.
- **Local models vary in quality.** Mitigated by bounded repair turns and the
  deterministic fallback.
- **The app needs a GPU.** The CLI and MCP server don't, and the app's tests
  run headlessly (on a software Vulkan driver when there's no GPU).
