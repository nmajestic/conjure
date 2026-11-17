# Conjure

> **Conjure is the agent-first asset tool for solo and indie game devs. Describe your game's look once — palette, grid, style — and Conjure's agent generates, validates, and manages a whole consistent pixel-art set.**

Conjure makes pixel art a *deterministic, spec-driven* problem instead of a prompt-and-pray one. The core loop is **spec → generate → validate → repair → ship**, and it runs entirely local-first with **no API keys**.

## Status

Early development — planning only. No code has been written yet; the stack and milestones are described in [docs/architecture.md](docs/architecture.md).

## Why Conjure

Pixel art is uniquely hard for image models: they blur grids, drift palettes, and can't keep a whole set consistent. Conjure reframes generation as constraint-satisfaction — a verifier enforces grid, palette, and structure, and the agent repairs until it passes.

## Planned layout

| Path | What it is |
|---|---|
| `conjure-core/` | Pure engine: spec, StyleCard, asset model, verifier. No I/O. |
| `conjure-inference/` | Thin OpenAI-compatible client + local-server discovery. |
| `conjure-agent/` | Hand-rolled generate → validate → repair loop. |
| `conjure-mcp/` | Tool-callable MCP server. |
| `conjure-store/` | Persistence and versioning. |
| `conjure-export/` | PNG/manifest/engine export. |
| `conjure-cli/` | Headless CLI surface over the core. |
| `conjure-app/` | Rune desktop UI. |
| `docs/` | Product brief, architecture, codebase rules. |

## Documentation

- [Product brief](docs/product-brief.md) — what Conjure is and why.
- [Architecture](docs/architecture.md) — the stack, crates, and decisions.
- [Codebase rules](docs/codebase-rules.md) — conventions and hard rules.
- [AGENTS.md](AGENTS.md) — for coding agents working in this repo.

## Principles

1. Core is UI-agnostic.
2. The verifier is hand-rolled and exhaustively tested.
3. Deterministic where it matters.
4. Local-first, no API keys.
