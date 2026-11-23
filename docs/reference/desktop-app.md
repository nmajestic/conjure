# Reference: desktop app

`conjure-app` is a Rune desktop app for everything the CLI does with a
library: browse it, preview assets pixel-exact, export sheets, and run
generation while watching the repair turns live. It uses the Conjure crates
in-process. It does not talk to the MCP server.

```sh
cargo run -p conjure-app
```

Run it from inside a project. It finds `.conjure/` the same way the CLI does.
v1 targets Linux and needs a GPU session to start.

## Screens

**No library.** Shown when no `.conjure/` is found. It explains why and offers
**initialize library**, which does the same as `conjure init`. It never
searches outside the project.

**Browse.** The library on the left: style cards (name and short hash) and
assets (name, kind, size, short hash, tags). Selecting an asset opens the
workbench:

- **Preview:** a zoomable, pixel-exact render of the indices in the card's
  palette, with index 0 transparent, plus the palette strip, dimensions and
  hash.
- **Export:** horizontal strip or grid (with a column count), scale 1–16, and
  a folder picker. It writes the same PNG and manifest as `conjure export` for
  the same inputs, and shows the CLI's exact error text when export is
  refused. See [export.md](export.md).
- **Generate:** described below.

## Generating

1. Pick a provider: a discovered local server (HTTP) or OpenCode. **Refresh
   models** re-runs discovery. With no model available, generation is disabled
   and says why. Everything else still works.
2. Pick an existing style card, a name, and either a sprite size or a tile size
   and count. Style cards can't be created or edited in the app.
3. Run. Events appear in order as they happen: model call, spec parsed, verify
   failed (with the error count), repair turn N, fallback engaged, passed.
4. When it passes, save the assets with one click. Saving works exactly like
   `conjure save`.

The run state is idle, running, passed or failed. One generation runs at a
time; browsing and export stay usable meanwhile. Cancel stops the run and
discards it. A failed run shows the agent's error text as is, and a run where
the deterministic fallback produced the assets says so.

## How it's built

Rune owns the UI thread. A background thread runs a tokio runtime for
generation. The UI sends commands over a channel, and the generation task
sends events back as state updates.

## Testing

Every screen and state is tested headlessly in-process with Rune's offscreen
renderer. Tests tap elements by key and assert on app state; the generation
tests run Rune's headless host so events stream as they would in the real app.
A smoke test also runs the `conjure-app` binary with `--snap`. Generation tests
use a stub model, so nothing touches the network. On machines without a GPU, the tests
need a software Vulkan driver (see [AGENTS.md](../../AGENTS.md#commands)).

## Not in v1

Style card creation or editing, pixel editing, image import, animation timing,
several generations at once, remote models, macOS and Windows builds,
packaging.
