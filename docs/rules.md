# Conjure rules

Every change must follow these rules, and reviewers check them. How changes
are made (the Overkill workflow, commits, pull requests) is in
[AGENTS.md](../AGENTS.md).

## 1. Layering

Dependencies point toward `conjure-core`. A lower crate never depends on a
higher one.

| Tier | Crates | May depend on |
|---|---|---|
| Core | `conjure-core` | `serde` only |
| Libraries | `conjure-inference` | no Conjure crates |
| | `conjure-agent` | core, inference |
| | `conjure-store`, `conjure-export` | core |
| Shells | `conjure-cli`, `conjure-mcp`, `conjure-app` | any library |

Shells wire libraries together and hold no business logic. `conjure-cli`
depends on `conjure-mcp` only to launch it.

## 2. Core is pure

- No I/O, no async runtime, no UI types, no printing.
- No `unwrap`, `expect` or `panic!` outside tests. Return a `Result`.
- No hidden randomness. Randomness is a seed passed in by the caller, so the
  same request and seed always produce the same asset.

## 3. The verifier is the trust layer

- Hand-rolled pure functions, no framework.
- Every failure is a `Diagnostic` with a `DiagnosticCode` and a message the
  agent can act on, such as `pixel (2,1) uses palette index 9, but palette has
  4 colors`. Never a bare boolean.
- A new check needs a new `DiagnosticCode`, its implementation, and a unit test
  that triggers it.
- It checks hard constraints only (dimensions, pixel count, palette range,
  asset count). Judging style is the agent's job.

## 4. Local-first, no credentials

- Conjure never asks for, stores or sends an API key or any other credential.
- It talks only to a local OpenAI-compatible server or the local OpenCode CLI,
  which owns all auth. No cloud endpoints and no SDKs that assume a key.

## 5. Indexed pixels

- An asset is a grid of indices into its style card's palette, never RGBA.
  Converting to color happens only at export.
- Models emit a `CompactSpec` (one character per pixel), never raw pixel
  arrays. Core expands it strictly, and every expansion error is a message the
  agent can repair from.

## 6. Dependencies

- Prefer the standard library and crates already in the workspace.
- A new dependency needs a written reason in the pull request: what it does,
  and why existing code can't.
- No LLM framework. The inference client and agent loop are hand-rolled.

## 7. Errors

- `conjure-core` returns `Result<T, String>` with actionable messages.
- Other crates may use a small typed error enum where it helps callers.
- Never swallow an error in library code.

## 8. Serialization

- `serde` derives, `snake_case` JSON, and internally tagged enums
  (`#[serde(tag = "type", rename_all = "snake_case")]`).
- Field names are part of the file formats. Renaming one is a breaking change.

## 9. Tests

- Every verifier check and every `DiagnosticCode` has a unit test.
- Code that talks to a model is tested against in-process fakes (a mock
  OpenAI-compatible server, a stub `opencode` binary), never a real model or
  the network.
- Tests are fast and deterministic.

## 10. Code style

- `cargo fmt` and `cargo clippy -D warnings` are clean.
- Comments only for a non-obvious why. No commented-out code.
- Follow the existing patterns. Don't add a second way to do something the
  codebase already does; if a change sets a new pattern, say so in the pull
  request.

## 11. Docs stay true

- A change that alters behavior or structure updates the affected docs in the
  same pull request.
- Docs describe what exists. Planned work is marked as planned.
