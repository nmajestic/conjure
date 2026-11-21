# Conjure

> **Conjure is the agent-first asset tool for solo and indie game devs. Describe your game's look once — palette, grid, style — and Conjure's agent generates, validates, and manages a whole consistent pixel-art set.**

Pixel art is easy to generate once and hard to generate consistently. Conjure treats that as a contract, not a prompt: **spec → generate → validate → repair → ship**. It runs local-first. Conjure never stores or transmits credentials. A local OpenAI-compatible server or the OpenCode CLI owns the model.

## Status

Early development. Milestones 1a–1c, 2, 3, and 4 are implemented and tested.

| Milestone | What landed |
|---|---|
| 1a | Pure engine and headless verifier: spec, style card, indexed assets, deterministic generator, repair. |
| 1b | Model loop. The model emits an ASCII-row compact spec; core expands and verifies it; up to 3 repair turns, then a deterministic fallback. |
| 1c | OpenCode as a provider. `conjure agent --provider opencode` drives `opencode run`. No API key in Conjure. |
| 2 | Persistence and export: project-local `.conjure/` store (SQLite + content-addressed blobs), `save`/`ls`/`show`/`tag`/`gc`, indexed PNG + JSON manifest export. |
| 3 | Tool-callable MCP server (`conjure-mcp`, official `rmcp` SDK): 12 tools over the same contracts; stdio default, loopback-only Streamable HTTP behind a flag; `conjure mcp` CLI launcher. |
| 4 | Desktop app (`conjure-app`, Rune): library browser with pixel-exact viewer, export, and a generation cockpit streaming repair turns live; Linux-first, headless-tested. |

Still planned: engine-specific exporters (Godot/Unity) and packaging.

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

```sh
cargo run -p conjure-cli -- init
cargo run -p conjure-cli -- save examples/demo/request.json /tmp/assets.json
cargo run -p conjure-cli -- export <hash-prefix> --out build/ # from save output
cargo run -p conjure-app
```

The desktop app opens the project's library (run it from the project directory; it locates `.conjure/` like the CLI).

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
conjure init
conjure save <request.json> <assets.json>
conjure ls [--tag <tag>]
conjure show <hash-or-prefix>
conjure export <hash-or-prefix>... [--layout strip_horizontal|grid] [--columns <n>] [--scale <1-16>] --out <dir>
conjure tag <hash-or-prefix> <tag>
conjure gc
conjure mcp [--root <path>] [--http 127.0.0.1:9711]
```

Defaults are a horizontal strip at scale 1, and mixed-size sets are refused because strips need equal heights and grids need uniform tiles.

`validate` prints a JSON report and exits 0 on pass, 1 on failure. Every other error exits 2. The model does not emit raw pixels. It emits a `CompactSpec`: one character per pixel, `'0'`–`'9'` and `'a'`–`'v'`, expanded by core into palette indices.

`save` verifies before writing anything; a failed save exits 1 and prints the `ValidationReport` JSON on stdout. Hashes are content-addressed (BLAKE3); a hash prefix must be at least 8 hex characters and unambiguous. `export` writes an indexed-color PNG (`PLTE` palette, index 0 transparent via `tRNS`, via the `png` crate) plus a JSON manifest. Identical pixels saved under different names keep every name, so `ls` shows one row per name.

`conjure mcp` starts the MCP server (stdio by default, `--http` binds loopback only — never a non-loopback address, never a token). It exposes the same verbs as tools: validate/expand/repair/generate/models and init/save/list/show/export. See [Milestone 3](docs/milestone-3.md) for the OpenCode `mcp.servers` snippet — it is documented there, not enabled in the committed config.

## Layout

| Path | What it is |
|---|---|
| `conjure-core/` | Pure engine. Spec, style card, compact spec, assets, verifier, generator, repair. No I/O. |
| `conjure-inference/` | Local backends: OpenAI-compatible HTTP client and server discovery, plus the OpenCode CLI driver. |
| `conjure-agent/` | Generate → validate → repair loop. No LLM framework. |
| `conjure-cli/` | Headless CLI over the crates above. |
| `conjure-store/` | Project-local `.conjure/` store: SQLite metadata, content-addressed blobs, tags, gc. |
| `conjure-export/` | Indexed-color PNG (`PLTE`/`tRNS`) and JSON manifest export. |
| `conjure-mcp/` | Tool-callable MCP server over the same contracts (rmcp). |
| `conjure-app/` | Rune desktop UI: browse, view, export, generate (Linux-first). |
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
