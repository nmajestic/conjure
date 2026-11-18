# Milestone 3 — tool-callable surface (`conjure-mcp`)

**Status:** implemented
**Depends on:** 1a–1c (generate/validate/repair/agent) and milestone 2 (save/list/export). MCP is a shell over those crates, not a second implementation.
**Crate:** `conjure-mcp`. CLI gains `conjure mcp` only as a launcher.

## Goal

Any MCP client — OpenCode first, because that is the agent the owner actually runs — can drive Conjure without scraping CLI stdout. The server exposes the same contracts the CLI already has: style cards, compact specs, verification diagnostics, and (after milestone 2) the project store.

This is the "tool-callable" half of agentic-first. The built-in `conjure-agent` loop stays. MCP does not replace it; it lets *another* agent call the same verbs.

## Why this is a shell

`conjure-mcp` depends on `conjure-core`, `conjure-agent`, `conjure-inference`, `conjure-store`, and `conjure-export`. It contains:

- tool input/output structs
- transport (stdio, and Streamable HTTP if the SDK makes that a flag rather than a rewrite)
- mapping of `Result` / `ValidationReport` into MCP tool results

It does not contain a second verifier, a second PNG writer, or a second prompt. If a tool needs behavior the library lacks, the behavior is added to the library and the tool calls it.

## Locked constraints (already true in the repo)

- No API keys, no cloud endpoints, no credential storage. The MCP server does not grow an auth layer. If it binds HTTP, it binds `127.0.0.1` only.
- OpenCode owns model auth. A tool that generates art takes a `provider` + `model` (and `base_url` when provider is `http`) and calls `conjure-agent`, which calls `conjure-inference`. The MCP process never sees a key.
- Diagnostics stay machine-actionable. A failed `verify` is a successful tool call whose structured payload has `passed: false` and the `errors` array — not an MCP protocol error. Protocol errors are for bad input, missing store, or a dead subprocess.
- Stdio is the primary transport. That is how OpenCode launches a local MCP server. Streamable HTTP is in scope only as the SDK's second transport behind a flag, same process, same tools.

## Dependency

`rmcp` (official Rust MCP SDK), on tokio, which the workspace already uses. No other MCP crate. Exact version pinned in `Cargo.lock` when the crate is added; do not vendor the SDK.

Justification: MCP framing, JSON-RPC, and capability negotiation are not worth hand-rolling. The *tools* stay hand-rolled.

## Process model

```
OpenCode  --stdio-->  conjure mcp
                         │
                         ├─ conjure-core      verify / expand / repair / generate
                         ├─ conjure-agent     model loop (only if a generate tool is called)
                         ├─ conjure-inference HTTP or `opencode run` subprocess
                         ├─ conjure-store     .conjure/ from the server's cwd
                         └─ conjure-export    PNG bytes + manifest
```

- One server process. No background daemon in this milestone (`opencode mcp` starts it and owns its lifetime).
- Working directory is the project root, same limitation as `--agent conjure`: the server must be launched from the Conjure project (or be given `--root <path>`, recommended) so `.conjure/` and `.opencode/opencode.json` resolve.
- Nested OpenCode: `conjure mcp` → agent tool → `opencode run --agent conjure` is a second process. It must not recurse into the MCP server. The `conjure` agent already denies tools; the MCP server must not pass `--agent` that has MCP enabled. Document this. Test it by asserting the argv the backend builds (already covered in 1c) still includes `--agent conjure`.

## Tools

Names are stable. Inputs and outputs are JSON, `snake_case`, internally tagged where the core types already are. Reuse `Asset`, `AssetRequest`, `StyleCard`, `CompactSpec`, `ValidationReport` — do not invent parallel schemas.

### Read / verify (no model, no disk required)

| Tool | Input | Output |
|---|---|---|
| `conjure_validate_card` | `StyleCard` | `{ "ok": true }` or `{ "ok": false, "error": "..." }` |
| `conjure_validate` | `request`, `assets` | `ValidationReport` |
| `conjure_expand` | `request`, `spec: CompactSpec` | `{ "assets": [...] }` or `{ "error": "..." }` from `expand` |
| `conjure_repair` | `request`, `assets` | `{ "assets": [...] }` — always the normalized set; also include `verify` so the caller sees it pass |

These four are the trust layer. An external agent can generate pixels however it wants and still be forced through Conjure's contract.

### Generate (model, optional)

| Tool | Input | Output |
|---|---|---|
| `conjure_generate_seeded` | `request`, `seed: u64` | `{ "assets": [...] }` from `conjure_core::generate`. No model. Useful for tests and placeholders. |
| `conjure_generate` | `request`, `provider: "http" \| "opencode"`, `model`, optional `base_url`, optional `max_repairs` | `GenResult` as JSON: `assets`, `repairs_used`, `used_fallback`. Transport failure is an MCP error string, same as the CLI's exit 2. |

`provider: "http"` requires `base_url`. `provider: "opencode"` rejects `base_url` (do not silently ignore it).

### Store / export (needs milestone 2)

| Tool | Input | Output |
|---|---|---|
| `conjure_init` | optional `root` | `{ "path": ".conjure" }` |
| `conjure_save` | `request`, `assets` | `{ "card_hash", "asset_hashes": [...] }` or the `ValidationReport` if refused |
| `conjure_list` | optional `tag` | `{ "cards": [...], "assets": [...] }` summaries, not full pixel arrays |
| `conjure_show` | `hash` (unique prefix ≥ 8, or full) | the card or the asset JSON |
| `conjure_export` | `hashes: [...]`, `layout`, `scale` | `{ "png_base64", "manifest" }` |

`png_base64` is the one binary exception. MCP tool results are JSON; embedding the PNG avoids a side-channel file the client might not be able to read. Cap scale at 16 (same as export). Refuse a sheet whose encoded PNG exceeds 8 MiB with a clear error rather than truncating.

`conjure_list` must not return pixel blobs. A tileset list that inlines every index will blow the client's context. `conjure_show` returns one asset's pixels; that is the explicit "I asked for the bytes" call.

## Resources (optional, only if cheap)

If `rmcp` resources are a few lines, expose:

- `conjure://style-cards` — JSON list of card summaries
- `conjure://assets` — JSON list of asset summaries

No resource for pixel blobs. Tools stay the source of truth. If resources cost a second code path, skip them. Do not block the milestone on resources.

## What is not a tool

- No `conjure_prompt` that returns the system prompt for the caller to freestyle. The prompt lives in `conjure-agent` and changes. Callers use `conjure_generate` or `conjure_expand`.
- No tool that shells out to an arbitrary command.
- No tool that writes outside `.conjure/` and the `export` result payload. `conjure_export` in MCP returns bytes; it does not take an `--out` path. The CLI keeps `--out`. Splitting these avoids an MCP client asking the server to write `~/.ssh`.
- No account, billing, or model-list tool that calls the network except `conjure_models`, which is allowed because it is the existing local discovery (`discover(local_urls())` + `opencode models`). Include it — an agent cannot pick a model id it cannot see.

| Tool | Input | Output |
|---|---|---|
| `conjure_models` | none | `{ "local_servers": [{ "base_url", "models": [] }], "opencode_models": [] }` |

An empty list is success. "Binary missing" for OpenCode is a field `opencode_error`, not a failed tool call, matching the CLI (HTTP section still returns).

## CLI launcher

```
conjure mcp                  # stdio, root = cwd (walk upward for .conjure, like milestone 2)
conjure mcp --root <path>    # stdio, chdir or pass root into the store
conjure mcp --http 127.0.0.1:9711
```

- Default is stdio. No port unless `--http`.
- `--http` refuses non-loopback addresses. `0.0.0.0` is an error.
- No `--token` flag. Adding one later is a product decision, not a hidden default.
- Help text lists `mcp`. No other CLI behavior changes.

## OpenCode project config

Document, and add a commented example (not an enabled server — the owner turns it on), in `.opencode/opencode.json` or in `docs/milestone-3.md` only:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "conjure": {
        "type": "local",
        "command": ["cargo", "run", "-q", "-p", "conjure-mcp"],
      },
    },
  },
}
```

This shape is verified against the OpenCode V2 MCP guide (https://opencode.ai/docs/mcp-servers); the snippet above is the verified V2 `mcp.servers` shape, not a sketch. Run it from the Conjure project root — the server locates `.conjure/` from its working directory, or pass `--root`.

The committed `.opencode/opencode.json` does NOT enable this server. A committed MCP server that launches `cargo run` on every OpenCode start will surprise the owner and can recurse. The example lives in the doc. Enabling it is a manual step after `conjure mcp` is installed.

## Errors

Map into two buckets:

| Situation | MCP result |
|---|---|
| Bad JSON, unknown provider, missing `base_url`, hash prefix ambiguous, store not initialized, PNG too large, non-loopback bind | Tool error (protocol). Message includes the same text the CLI would print after `error:`. |
| Style card invalid, spec rejected, verify failed | Tool success. Body is the report or `{ "ok": false, "error" }`. |
| Model subprocess timeout / non-zero exit | Tool error. Do not fall back inside the MCP layer. Fallback already happened inside `conjure-agent` and comes back as `used_fallback: true` on success. |

Never put a stack trace or a file path outside the project root in a tool error.

## Tests

No live model. No live OpenCode, except the existing stub-binary pattern.

- Unit-test the mapping functions without a socket: each tool handler is a plain `async fn` tested with httpmock (HTTP backend) or a stub `opencode` (OpenCode backend), same as 1b/1c.
- `conjure_validate` on a bad index returns `passed: false` and does not error the protocol.
- `conjure_save` without `init` errors.
- `conjure_export` PNG decodes to the same indices as milestone 2's export tests (call `conjure-export`, do not reimplement).
- `conjure_generate` with the HTTP mock: one repair turn still reports `repairs_used: 1`.
- A test that the HTTP bind helper rejects `0.0.0.0` and accepts `127.0.0.1`.
- Grep test, or a review check: no `Authorization`, no api-key field on any tool input struct.

If `rmcp` supports an in-process client, one integration test lists tools and calls `conjure_validate_card`. If it does not, skip the JSON-RPC integration test rather than standing up a fake stdio harness. Handler tests are the acceptance bar.

## Out of scope

- Remote / authenticated MCP. Loopback or stdio only.
- Resources that stream pixels.
- A tool for milestone 4's UI, or for launching Rune.
- Godot export (still later).
- Prompts-as-MCP-prompts (`prompts/list`). Tools are enough.
- Watching `.conjure/` and pushing notifications. Clients poll `conjure_list`.
- Rate limits. One user, one machine.

## Acceptance

- `conjure mcp` speaks stdio long enough for `rmcp`'s own client (or a documented `npx @modelcontextprotocol/inspector` note) to list every tool in the tables above.
- OpenCode can be pointed at the server using the documented snippet and can call `conjure_validate` and `conjure_list` without a model.
- `conjure_generate` against the stub OpenCode binary returns assets that pass `verify_request`.
- Workspace tests, clippy `-D warnings`, fmt clean.
- `conjure-core` untouched except a re-export if a type was crate-private and must be public (prefer already-public types; do not churn core).
- README and `architecture.md` current-state mention `conjure-mcp`.

## Open items to close in `static-fire` before building

Closed at build time: rmcp 3.11 pinned; V2 config verified; single serve implementation shared by bin and `conjure mcp`; base64 with an 8 MiB encoded-PNG cap.

1. `rmcp` API at the pinned version: stdio helper names, whether Streamable HTTP is a feature flag, tool macro shape. Fetch docs.rs / the SDK README. Do not design the handlers around a guessed macro.
2. OpenCode MCP config schema (V2 guide), so the documented snippet is real.
3. Whether `conjure mcp` is a separate bin (`conjure-mcp`) that the CLI also invokes, or the CLI binary grows a subcommand and the MCP crate is a library the CLI calls. Recommended: `conjure-mcp` is a library + a tiny bin; `conjure mcp` in `conjure-cli` delegates to the same `serve()` function so there is one implementation.
4. Base64 vs a file path for `conjure_export`. Recommended: base64, 8 MiB cap, no path writes.

## Suggested build waves (for later `liftoff`)

1. Crate scaffold + `serve_stdio` + the four verify/expand tools (no store, no model).
2. `conjure_generate` / `conjure_generate_seeded` / `conjure_models` against existing backends.
3. Store/export tools + CLI launcher + docs snippet.

Wave 1 is independently testable if milestone 2 slips. Do not block the trust-layer tools on PNG.
