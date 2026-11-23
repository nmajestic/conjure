# Reference: MCP server

`conjure-mcp` exposes Conjure's operations as MCP tools, so any agent can use
Conjure's contract: generate pixels however it likes, then validate, repair,
save and export through Conjure. It is a shell over the same crates the CLI
uses and adds no logic of its own.

## Running it

```
conjure mcp                           # stdio, project root = current directory
conjure mcp --root <path>             # stdio, explicit project root
conjure mcp --http 127.0.0.1:9711     # Streamable HTTP on loopback
```

The `conjure-mcp` binary takes the same flags.

- stdio is the default. No port is opened unless `--http` is given.
- `--http` accepts only `127.0.0.1` or `::1`. Any other address is refused.
- There is no token or other auth. The server is local only.
- The store is found from the project root the same way as the CLI.

## Tools

Inputs and outputs are JSON using Conjure's own types (`StyleCard`,
`AssetRequest`, `Asset`, `CompactSpec`, `ValidationReport`).

### Check and transform (no model, no store)

| Tool | Input | Output |
|---|---|---|
| `conjure_validate_card` | `style_card` | `{"ok": true}` or `{"ok": false, "error"}` |
| `conjure_validate` | `request`, `assets` | `ValidationReport`, or `{"ok": false, "error"}` if the request's style card is invalid |
| `conjure_expand` | `request`, `spec` | `{"assets"}`, or `{"error"}` if the spec is rejected |
| `conjure_repair` | `request`, `assets` | `{"assets", "verify"}`: the repaired set and its report |
| `conjure_generate_seeded` | `request`, `seed` | `{"assets"}` from the deterministic generator |

### Generate with a model

| Tool | Input | Output |
|---|---|---|
| `conjure_models` | none | `{"local_servers": [{"base_url", "models"}], "opencode_models", "opencode_error"}` |
| `conjure_generate` | `request`, `provider` (`http` or `opencode`), `model`, `base_url` (http only, required), `max_repairs` (optional) | `{"assets", "repairs_used", "used_fallback"}` |

`conjure_models` succeeds with empty lists when nothing is running. A missing
OpenCode binary shows up in `opencode_error`, not as a failed call.

### Store and export

| Tool | Input | Output |
|---|---|---|
| `conjure_init` | `root` (optional; relative paths are under the server's root) | `{"path"}` of the created `.conjure/` |
| `conjure_save` | `request`, `assets` | `{"card_hash", "asset_hashes"}`, or the `ValidationReport` if verification fails |
| `conjure_list` | `tag` (optional) | `{"cards", "assets"}` summaries, never pixel data |
| `conjure_show` | `hash` (full or a unique prefix of 8+ hex characters) | the card or asset JSON |
| `conjure_export` | `hashes`, `layout` (`strip_horizontal` or `grid`), `columns` (grid only), `scale` | `{"png_base64", "manifest"}` |

Unlike `conjure save`, `conjure_save` never creates a store. Call
`conjure_init` first.

`conjure_export` returns the PNG inline as base64 and never writes a file.
A PNG over 8 MiB is refused. Export rules are in [export.md](export.md).

## Errors

- **Invalid input or a failed operation** (bad JSON, unknown provider,
  `base_url` given for `opencode`, ambiguous prefix, missing store, oversize
  export, a model call that fails or times out): a tool result with
  `isError: true` and the same message the CLI would print.
- **A contract check that fails** (invalid card, rejected spec, verification
  failure): a normal result containing the report or error. These are answers,
  not errors.
- The model fallback happens inside the agent loop, never in the MCP layer. It
  shows up as `used_fallback: true`.

## Boundaries

- No tool runs arbitrary commands.
- No tool writes outside `.conjure/`.
- No tool returns the agent's prompt; callers use `conjure_generate` or
  `conjure_expand`.
- The only network access is `conjure_models` and `conjure_generate`, both
  local.

## Using it from OpenCode

The committed `.opencode/opencode.json` does not enable this server, because
starting `cargo run` on every OpenCode launch would be surprising. To enable
it yourself, add:

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "conjure": {
        "type": "local",
        "command": ["cargo", "run", "-q", "-p", "conjure-mcp", "--", "--root", "/path/to/project"]
      }
    }
  }
}
```

When `conjure_generate` uses the OpenCode provider, it starts a second
OpenCode process with `--agent conjure`, which has every tool denied, so it
can't call back into the MCP server.
