# Conjure — Architecture

This is the authoritative technical document for Conjure. It describes the
stack, the crate layout, and the decisions behind them.

## Overview

Conjure is a local-first, agent-first asset tool. The core loop is:

```
spec → generate → validate → repair → ship
```

The verifier is the trust layer: every output must pass, or report *why* it
failed in a way the agent can act on.

## Guiding principles

1. **Core is UI-agnostic.** The engine is a pure library driven identically by
   the CLI, MCP server, and Rune app.
2. **The verifier is hand-rolled and exhaustively tested.**
3. **Deterministic where it matters.** Same spec + seed = same asset.
4. **Local-first, no API keys.** Conjure never runs a model; it drives the
   user's local inference server.

## The stack

| Layer | Choice | Why |
|---|---|---|
| Language | Rust, edition 2024 | One language across UI, core, MCP, CLI. |
| **UI** | **Rune** (git dep, pinned rev) | GPU-rendered (wgpu), Taffy layout, Parley text, spring animation; macOS/Win/Linux + WebGPU. Headless rendering lets an agent inspect and test the UI. |
| **Inference client** | Hand-rolled `conjure-inference` (`reqwest` + `tokio`) | Two local backends: OpenAI-compatible client + local-server discovery, and an OpenCode CLI driver. No API key. |
| **Agent loop** | Hand-rolled `conjure-agent` | Prompt → compact spec → expand → verify → repair. No LLM framework. |
| **Tool-callable surface** | **rmcp** (official MCP Rust SDK) | Exposes Conjure as an MCP server (stdio + Streamable HTTP). |
| Async runtime | tokio | Needed for `reqwest` and `rmcp`. |
| Serialization | serde + serde_json | Style Card, specs, manifests, tool schemas. |
| **Pixel data** | `image` + `png` | PNG in/out; indexed-color PNG for palette-exact sprites. |
| Storage | SQLite via `rusqlite` | Metadata; sprite bytes on disk (content-addressed). |
| Validation | Hand-rolled pure functions | The trust layer. |
| Export | PNG + JSON manifest first | Then Godot/Unity/Aseprite. |
| Testing | In-process mock OpenAI server + Rune headless render | Test the loop offline; verify the UI headlessly. |

### HTTP client decision (2026-11-16)

`reqwest` + `tokio`. We need tokio for `rmcp`/MCP anyway, and streaming matters
for the desktop UI. Swapping clients later would be churn.

## Crate responsibilities

```
conjure/
  conjure-core/       # Spec, StyleCard, asset model, verifier, generator, repair (pure, no I/O)
  conjure-inference/  # backends: OpenAI-compatible HTTP client + local server discovery + OpenCode CLI (no API key)
  conjure-agent/      # hand-rolled agent loop, prompts, repair (no LLM framework)
  conjure-mcp/        # rmcp server wrapping core (tool-callable surface)
  conjure-store/      # SQLite + file store, versioning
  conjure-export/     # PNG/manifest/engine exporters
  conjure-cli/        # headless prove-the-loop
  conjure-app/        # Rune desktop app (last to build)
```

Dependencies point *inward*: `app`/`mcp`/`cli` → `agent` → `inference` + `core`;
everything → `store`/`export`. `core` depends only on `serde`.

**Current state:** milestones 1a, 1b, and 1c are implemented — `conjure-core`
(including the `compact` module), `conjure-inference` (HTTP and OpenCode
backends), `conjure-agent`, and `conjure-cli` (including the `models` and
`agent` commands) all exist and are tested. `conjure-store`, `conjure-export`,
`conjure-mcp`, and `conjure-app` are still planned.

## Inference layer — local, no API key (inspired by T3 Code)

Conjure does not run models and does not require an API key. It either speaks
the OpenAI-compatible API to whatever local inference server the user already
runs — Ollama, LM Studio, llama.cpp, vLLM, or SGLang — the way
[T3 Code](https://github.com/MiaAI-Lab/t3code) does: a thin client that drives
the user's server and discovers it automatically; or it drives the local
OpenCode agent CLI (see Backends below).

- **`conjure-inference`**
  - Keyless client (`Client::new(base_url, model)`): OpenAI-compatible
    `chat/completions`, 180-second timeout, and **no `Authorization` header
    ever** — there is no code path that could send an API key.
  - **Local server discovery:** `discover(local_urls())` probes the usual
    ports (Ollama `11434`, LM Studio `1234`, llama.cpp `8080`, vLLM `8000`)
    and reads `GET /v1/models`.
- **`conjure-agent`**
  - The loop: build prompt (system + `AssetRequest`/`StyleCard`) → ask for a
    **compact spec as JSON** → parse the reply leniently (code fences and
    surrounding prose are tolerated) → expand strictly in core → verify →
    feed failures back as repair turns (up to 3 by default) → deterministic
    safety-net fallback (core `repair`/`generate`) if the model never
    succeeds.
  - Tested against an in-process `httpmock` mock OpenAI-compatible server —
    never a real model.

### Backends (milestone 1c, decided 2026-11-16)

`conjure-inference` exposes a `Backend` enum with two variants:

- **`Http(Client)`** — unchanged from 1b: the keyless OpenAI-compatible client
  for localhost servers, over `reqwest` + `tokio`.
- **`OpenCode(OpenCodeCli)`** — drives the local OpenCode agent CLI as a
  subprocess: `opencode run --format json --archived --model <provider/model>
  --agent conjure`. The process is killed after 180 seconds. Output is NDJSON;
  the reply text is read from `part.text` on `type:"text"` lines. The binary is
  injectable via `CONJURE_OPENCODE_BIN`. Conjure never handles credentials —
  OpenCode owns all auth. Multi-turn repair conversations are stateless: the
  full history is folded into each run's prompt. A minimal no-tools `conjure`
  agent is defined in the project `.opencode/opencode.json` — OpenCode
  discovers that config from the working directory upward, so the CLI must
  run from inside the Conjure project (known limitation: `--agent conjure`
  does not resolve elsewhere).

**CLI surface (1b–1c):** `conjure models` lists both discovered local servers
and OpenCode models; `conjure agent <request.json> --provider opencode --model
<id>` runs the agent loop through the OpenCode backend. The HTTP path is
unchanged from 1b: `conjure agent <request.json> --base-url <url> --model <id>`.

**What we give up vs. Rig, and how we replace it:**

| Rig feature | Our replacement |
|---|---|
| Multi-provider abstraction | Not needed — local backends only (OpenAI-compatible servers, OpenCode CLI) |
| Structured extraction | "Return JSON" + lenient parse + re-prompt on failure |
| `mock`/`cassette` test harness | In-process mock OpenAI-compatible server |

## Asset representation (decided)

**Indexed pixels + palette map.** Each sprite is a grid of indices into the
Style Card palette. Palette fidelity is structural; sprites are tiny and
diffable; PNG export is a deterministic index → RGBA expansion.

**The model does not emit raw grids.** It emits a `CompactSpec` (decided
2026-11-16, implemented in `conjure-core::compact`): ASCII rows, one char per
pixel. Chars `'0'..='9'` map to palette indices 0–9 and `'a'..='v'` to
indices 10–31 (32 colors max). The variants mirror `AssetKind`: a sprite is
`{"type":"sprite","rows":[...]}` and a tileset is
`{"type":"tileset","tiles":[[...],...]}` (one row-list per tile).

`conjure-core::compact::expand` expands the spec **strictly** into the
indexed grid, then the verifier checks the result. Dimension, char, and
palette-range errors are actionable messages that the agent feeds back as
repair turns. The original fallback in this decision — the model emitting
the grid as raw JSON index arrays — was considered and not chosen.

## Runtime / threading model

Rune runs the UI on its own thread; the agent and MCP server are async on a
tokio runtime.

- Rune app owns a background tokio runtime.
- UI actions → commands over an mpsc channel.
- Agent streams progress events back → Rune state updates.
- MCP server runs headless (stdio) or alongside the app.

## Data flow (end to end)

```
Style Card ─┐
            ├─→ agent (conjure-agent) ─→ compact spec ─→ core expands → indexed Asset
AssetRequest┘        │                                            │
                     │  model call (conjure-inference → local server)
                     ▼
            verifier (conjure-core)
            ┌── pass ──→ store/export
            └── fail ──→ diagnostic ─→ agent repairs (bounded loop)
```

## Milestones

1. **1a — `conjure-core` + deterministic generator + `repair` + `conjure-cli`:**
   prove generate → validate → repair with no model at all (pure Rust).
2. **1b — `conjure-inference` + `conjure-agent`:** swap the deterministic
   generator for a real local model; still no API key.
3. **1c — OpenCode provider:** second `conjure-inference` backend that drives
   the local OpenCode agent CLI; still no API key.
4. **2 — `conjure-store` + `conjure-export`:** persistence, versioning,
   PNG/manifest export.
5. **3 — `conjure-mcp`:** tool-callable surface.
6. **4 — `conjure-app` (Rune):** desktop UI, headless-tested via Rune.

## Risks

- **Rune is pre-release** — API may shift; pin a rev and isolate it behind
  `conjure-app`.
- **Model grid reliability is unproven** — Milestones 1a/1b answer this
  cheaply.
- **Local-model capability varies** — mitigation: deterministic generator
  first, bounded repair retries, deterministic safety-net repair.
- **Rune needs `wgpu`/GPU** — CLI/MCP milestones stay headless.
