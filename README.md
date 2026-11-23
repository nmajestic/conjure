# Conjure

> **Describe your game's look once (palette, grid, style) and Conjure's agent generates, validates and manages a whole consistent pixel-art set.**

Pixel art is easy to generate once and hard to generate consistently. Conjure
treats it as a contract, not a prompt: **spec → generate → validate → repair →
ship**. Every asset is checked against your style card, and the same request
and seed always give the same result.

Conjure is local-first and never handles API keys. It drives a local
OpenAI-compatible server (Ollama, LM Studio, llama.cpp, vLLM) or the OpenCode
CLI, which owns any credentials.

## Status

Early development. Everything below is built and tested.

| Milestone | What it added |
|---|---|
| 1a | Pure engine and verifier: style cards, indexed assets, deterministic generator, repair. |
| 1b | Model loop: the model writes a compact text spec, Conjure expands and verifies it, with up to 3 repair turns and then a deterministic fallback. |
| 1c | OpenCode as a model provider. |
| 2 | Project library (`.conjure/`: SQLite and content-addressed files) and indexed PNG + JSON manifest export. |
| 3 | MCP server with 12 tools over the same operations. |
| 4 | Linux desktop app: library browser, pixel-exact preview, export, and live generation. |

Planned: engine-specific exporters (Godot, Unity) and packaging.

## Quick start

```sh
cargo test --workspace
cargo run -p conjure-cli -- --help
```

Generate placeholder tiles from the demo request without a model, then
validate them:

```sh
cargo run -p conjure-cli -- generate examples/demo/request.json 42 > /tmp/assets.json
cargo run -p conjure-cli -- validate examples/demo/request.json /tmp/assets.json
```

Save them to a library, export a sheet, and open the app:

```sh
cargo run -p conjure-cli -- init
cargo run -p conjure-cli -- save examples/demo/request.json /tmp/assets.json
cargo run -p conjure-cli -- export <hash-prefix> --out build/
cargo run -p conjure-app
```

Generate with a model, through OpenCode (run from this repo) or a local server:

```sh
cargo run -p conjure-cli -- models
cargo run -p conjure-cli -- agent examples/demo/request.json --provider opencode --model <provider/model>
cargo run -p conjure-cli -- agent examples/demo/request.json --base-url http://localhost:11434/v1 --model <model>
```

On Linux, the desktop app needs `libxkbcommon-dev` to build.

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

`validate` prints a JSON report and exits 0 on pass and 1 on failure. `save`
exits 1 and prints the report if verification fails. Any other error exits 2.
Defaults are a horizontal strip at scale 1, and mixed-size sets are refused
because strips need equal heights and grids need uniform tiles. Identical
pixels saved under different names keep every name, so `ls` shows one row per
name.

## Layout

| Path | What it is |
|---|---|
| `conjure-core/` | Pure engine: style card, request, compact spec, assets, verifier, generator, repair. |
| `conjure-inference/` | Model backends: local OpenAI-compatible servers and the OpenCode CLI. |
| `conjure-agent/` | The generate → verify → repair loop. |
| `conjure-store/` | The `.conjure/` library. |
| `conjure-export/` | Indexed PNG and JSON manifest export. |
| `conjure-cli/` | The `conjure` command. |
| `conjure-mcp/` | MCP server. |
| `conjure-app/` | Rune desktop app. |
| `examples/demo/` | A dungeon tileset request: three 8×8 tiles, six colors. |

## Docs

- [Product](docs/product.md): what Conjure is for and who it's for.
- [Architecture](docs/architecture.md): how it's built and why.
- [Rules](docs/rules.md): the rules every change follows.
- Reference: [store](docs/reference/store.md), [export](docs/reference/export.md),
  [MCP server](docs/reference/mcp.md), [desktop app](docs/reference/desktop-app.md).
- [AGENTS.md](AGENTS.md): how changes are made (the Overkill workflow).
