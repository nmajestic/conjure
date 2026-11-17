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
| **Inference client** | Hand-rolled `conjure-inference` (`reqwest` + `tokio`) | Thin OpenAI-compatible client + local-server discovery. No API key. |
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
  conjure-inference/  # thin OpenAI-compatible client + local server discovery (no API key)
  conjure-agent/      # hand-rolled agent loop, prompts, repair (no LLM framework)
  conjure-mcp/        # rmcp server wrapping core (tool-callable surface)
  conjure-store/      # SQLite + file store, versioning
  conjure-export/     # PNG/manifest/engine exporters
  conjure-cli/        # headless prove-the-loop
  conjure-app/        # Rune desktop app (last to build)
```

Dependencies point *inward*: `app`/`mcp`/`cli` → `agent` → `inference` + `core`;
everything → `store`/`export`. `core` depends only on `serde`, `image`, `png`.

**Current state:** planning only — no code has been written yet. All crates
above are planned.

## Inference layer — local, no API key (inspired by T3 Code)

Conjure does not run models and does not require an API key. It speaks the
OpenAI-compatible API to whatever local inference server the user already runs
— Ollama, LM Studio, llama.cpp, vLLM, or SGLang — the way
[T3 Code](https://github.com/MiaAI-Lab/t3code) does: a thin client that drives
the user's server and discovers it automatically.

- **`conjure-inference`**
  - OpenAI-compatible `chat/completions`.
  - **Local server discovery:** probe the usual ports (Ollama `11434`, LM
    Studio `1234`, llama.cpp `8080`, vLLM `8000`) and read `GET /v1/models`.
  - **Config** (`base_url`, `model`, optional `api_key`, `context_window`,
    `max_tokens`); local endpoints need no key.
- **`conjure-agent`**
  - Build prompt (system + `AssetRequest`/`StyleCard`) → ask for a **compact
    spec as JSON** → expand deterministically in core → verify → feed
    diagnostics back as a repair turn (bounded retries) → deterministic
    safety-net repair.

**What we give up vs. Rig, and how we replace it:**

| Rig feature | Our replacement |
|---|---|
| Multi-provider abstraction | Not needed — local OpenAI-compatible only |
| Structured extraction | "Return JSON" + lenient parse + re-prompt on failure |
| `mock`/`cassette` test harness | In-process mock OpenAI-compatible server |

## Asset representation (decided)

**Indexed pixels + palette map.** Each sprite is a grid of indices into the
Style Card palette. Palette fidelity is structural; sprites are tiny and
diffable; PNG export is a deterministic index → RGBA expansion.

**The model does not emit raw grids.** It emits a *compact declarative spec*
(run-length lines, symmetry, or a tiny draw program) that `conjure-core`
expands deterministically into the indexed grid, then the verifier checks the
result. Fallback: the model emits the grid as JSON indices and repair fixes
off-palette/off-grid pixels.

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
3. **2 — `conjure-store` + `conjure-export`:** persistence, versioning,
   PNG/manifest export.
4. **3 — `conjure-mcp`:** tool-callable surface.
5. **4 — `conjure-app` (Rune):** desktop UI, headless-tested via Rune.

## Risks

- **Rune is pre-release** — API may shift; pin a rev and isolate it behind
  `conjure-app`.
- **Model grid reliability is unproven** — Milestones 1a/1b answer this
  cheaply.
- **Local-model capability varies** — mitigation: deterministic generator
  first, bounded repair retries, deterministic safety-net repair.
- **Rune needs `wgpu`/GPU** — CLI/MCP milestones stay headless.
