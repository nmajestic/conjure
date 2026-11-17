# Conjure

> **Conjure is the agent-first asset tool for solo and indie game devs. Describe your game's look once — palette, grid, style — and Conjure's agent generates, validates, and manages a whole consistent pixel-art set.**

Pixel art is easy to generate once and hard to generate consistently. Conjure treats that as a contract, not a prompt: **spec → generate → validate → repair → ship**. It runs local-first. Conjure never stores or transmits credentials. A local OpenAI-compatible server or the OpenCode CLI owns the model.

## Status

Early development. Milestones 1a–1c are implemented and tested.

| Milestone | What landed |
|---|---|
| 1a | Pure engine and headless verifier: spec, style card, indexed assets, deterministic generator, repair. |
| 1b | Model loop. The model emits an ASCII-row compact spec; core expands and verifies it; up to 3 repair turns, then a deterministic fallback. |
| 1c | OpenCode as a provider. `conjure agent --provider opencode` drives `opencode run`. No API key in Conjure. |

Still planned: [store and export](docs/milestone-2.md) (SQLite + content-addressed blobs, indexed PNG), [MCP](docs/milestone-3.md), and the Rune desktop app.

## Quick start

```sh
cargo test --workspace
cargo run -p conjure-cli -- --help
```

Generate a placeholder tileset (no model) from the demo request:

```sh
cargo run -p conjure-cli -- generate examples/demo/request.json 42 > /tmp/assets.json
cargo run -p conjure-cli -- validate examples/demo/request.json /tmp/assets.json
```

Generate with OpenCode (must be run from this repo so the `conjure` agent resolves):

```sh
cargo run -p conjure-cli -- models
cargo run -p conjure-cli -- agent examples/demo/request.json \
  --provider opencode --model opencode-go/glm-5.3
```

A local server uses the other shape. Conjure does not send an `Authorization` header:

```sh
cargo run -p conjure-cli -- agent examples/demo/request.json \
  --base-url http://localhost:11434/v1 --model gemma4
```

## Commands

```
conjure generate <request.json> <seed>
conjure validate <request.json> <assets.json>
conjure repair <request.json> <assets.json>
conjure models
conjure agent <request.json> --base-url <url> --model <id>
conjure agent <request.json> --provider opencode --model <id>
```

`validate` prints a JSON report and exits 0 on pass, 1 on failure. Every other error exits 2. The model does not emit raw pixels. It emits a `CompactSpec`: one character per pixel, `'0'`–`'9'` and `'a'`–`'v'`, expanded by core into palette indices.

## Layout

| Path | What it is |
|---|---|
| `conjure-core/` | Pure engine. Spec, style card, compact spec, assets, verifier, generator, repair. No I/O. |
| `conjure-inference/` | Local backends: OpenAI-compatible HTTP client and server discovery, plus the OpenCode CLI driver. |
| `conjure-agent/` | Generate → validate → repair loop. No LLM framework. |
| `conjure-cli/` | Headless CLI over the crates above. |
| `conjure-store/` | Persistence and versioning (planned). |
| `conjure-export/` | Indexed PNG and JSON manifest (planned). |
| `conjure-mcp/` | Tool-callable MCP server (planned). |
| `conjure-app/` | Rune desktop UI (planned). |
| `docs/` | Product brief, architecture, rules, milestone specs. |
| `examples/demo/` | A dungeon tileset request (8×8, 3 tiles, 6-color palette). |

## Documentation

- [Product brief](docs/product-brief.md) — what Conjure is and why.
- [Architecture](docs/architecture.md) — stack, crates, and decisions.
- [Codebase rules](docs/codebase-rules.md) — conventions and hard rules.
- [Milestone 2](docs/milestone-2.md) — project-local `.conjure/` store and indexed PNG export.
- [Milestone 3](docs/milestone-3.md) — MCP server over the same contracts.
- [AGENTS.md](AGENTS.md) — for coding agents working in this repo.

## Principles

1. Core is UI-agnostic.
2. The verifier is hand-rolled and exhaustively tested.
3. Deterministic where it matters. Same spec and seed, same asset.
4. Local-first. No API keys in Conjure.
