# Conjure — Codebase Rules

These are the rules every change to this repository must follow. `AGENTS.md`
summarizes them for coding agents; this file is the authoritative version.

## 1. Crate boundaries and dependency direction

The workspace is layered. Dependencies point *inward* only:

```
app / mcp / cli  →  agent  →  inference + core  →  store / export
```

- `conjure-core` depends only on `serde` — nothing else.
- `conjure-inference` depends on `reqwest`, `tokio`, and `serde`/`serde_json`.
- `conjure-agent` depends on `inference` and `core`.
- `conjure-mcp`, `conjure-cli`, `conjure-app` are shells: no business logic.
- A lower crate must never depend on a higher one.

## 2. `conjure-core` is pure

- No I/O: no filesystem, no network, no `println!` in library code.
- No async runtime (no tokio), no `reqwest`.
- No UI types.
- No `panic!`/`unwrap`/`expect` in library code; return `Result` or a typed
  value instead.

## 3. The verifier is the trust layer

- Hand-rolled, pure, exhaustive. No framework.
- Every failure is a typed `Diagnostic` with a `DiagnosticCode` and a
  *machine-actionable* message (e.g. `pixel (2,1) uses palette index 9, but
  palette has 4 colors`). Never a bare boolean.
- A new validation rule requires: a new `DiagnosticCode` variant, an
  implementation, and a unit test that triggers it.
- The verifier checks hard constraints only (dimensions, pixel count, palette
  membership, count). Soft style checks are the agent's job, not the verifier's.

## 4. No API keys, local-first

- Conjure never requires an API key and never runs a model itself; it never
  stores or transmits credentials of any kind.
- It speaks the OpenAI-compatible API to a *local* inference server (Ollama, LM
  Studio, llama.cpp, vLLM, SGLang), or drives a *local* agent CLI (OpenCode),
  which owns all auth. No cloud endpoints, no secret handling, no key-assuming
  SDKs in Conjure.
- HTTP is done with `reqwest` + `tokio` (decided 2026-11-16).

## 5. Determinism

- Generation must be reproducible: same spec + seed → same asset.
- No hidden randomness in `conjure-core`. Any randomness is an explicit,
  seeded parameter passed in from the caller.

## 6. Asset representation

- Canonical form is **indexed pixels + palette map**: each pixel is an index
  into the Style Card palette, never raw RGBA.
- The model emits a *compact declarative spec*: a `CompactSpec` of ASCII
  rows, one char per pixel (`'0'`–`'9'` are palette indices 0–9, `'a'`–`'v'`
  are 10–31) — never raw index arrays, and models must not be asked for raw
  pixel grids.
- `conjure-core` expands the spec deterministically into the indexed grid.
  Expansion is strict: dimension, char, and palette-range errors become
  actionable repair messages for the agent.
- Preserve this representation; converting to RGBA happens only at export time.

## 7. Dependencies

- Minimal. Prefer the standard library.
- Any new dependency requires a written justification in the change
  description: what it does and why std / an existing crate can't do it.
- No LLM framework (we dropped Rig deliberately). The inference layer is
  hand-rolled.

## 8. Error handling

- Domain errors in `conjure-core` use `Result<T, String>` with actionable,
  human-readable messages.
- Protocol/IO errors may use typed errors where it adds value, but keep the
  surface small.
- Never swallow errors silently in library code.

## 9. Serialization conventions

- Use `serde` with `#[derive(Serialize, Deserialize)]`.
- JSON uses `snake_case` (`#[serde(rename_all = "snake_case")]`).
- Polymorphic payloads use internally-tagged enums
  (`#[serde(tag = "type", rename_all = "snake_case")]`).
- Field names are stable and documented; they are part of the spec format.

## 10. Testing

- `cargo test --workspace` must pass before any change is done.
- Every verifier rule and every `DiagnosticCode` has a unit test.
- The agent loop is tested against an in-process mock OpenAI-compatible server
  (canned responses); never against a real model in tests.
- Keep tests fast and deterministic.

## 11. Style and tooling

- Rust edition 2024, workspace `resolver = "3"`.
- `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D
  warnings` must pass.

## 12. Comments

- **No comments unless absolutely needed.** Prefer self-documenting code:
  clear names and small, single-purpose functions.
- A comment is justified only when it explains a non-obvious *why* — a
  rationale, an invariant, or a deliberate non-obvious choice.
- Never leave noise comments, obvious restatements, or commented-out code.

## 13. Design consistency

- **Use consistent software design patterns across the codebase.**
- Match the existing structure, naming, and patterns. Don't introduce a second
  way to do something that's already done.
- Respect the boundaries and conventions in this document (rules 1–11).
- For anything not yet established, use the most idiomatic Rust and note the
  choice in the change description so it can become the pattern.

## 14. Documentation stays truthful

- When behavior or structure changes, update `AGENTS.md` and `docs/` in the
  same change.
- Docs describe what *is*, not what is planned (mark planned work explicitly).

## 15. Commits and pull requests

### Commit messages

- Format: `<label>(<optional-scope>): <description>`
- Strictly all lowercase (including the description), imperative present
  tense, no trailing period.
- Allowed labels: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `chore`,
  `style`.

### Pull requests

- Title follows the same `<label>(<optional-scope>): <description>` format.
- Keep PRs atomic to a single primary label and under ~400 lines of diff.
- Required body sections: `## summary`, `## verification`, `## risks`.

### Push

- Never `git push` unless explicitly instructed.

## 16. Mandatory build workflow

Before building, implementing, scaffolding, refactoring, or generating any code
or feature, execute the seven-skill pipeline in strict order:
`mission-design` → `orbital-survey` → `static-fire` → `liftoff` → `max-q` →
`catch`, with `abort` on failure recovery in pipeline mode only. Do not create, modify, or delete implementation files — or run build/install
commands — until `static-fire` clears execution at `HIGH`. Before that gate,
`mission-design` may write `docs/specs/<feature-slug>.md` and `orbital-survey`
may write `docs/orbital-survey.md`.

- Applies to new features, bug fixes, architectural changes, scripts, and
  config updates.
- Skip only for read-only queries (explaining code, answering conceptual
  questions, inspecting logs).
- `mission-design` runs only for exploratory or under-specified requests, or
  standalone via `"run mission-design"`. Concrete requests, and requests that
  already have a matching spec in `docs/specs/`, start at `orbital-survey` /
  `static-fire`.
- `orbital-survey` runs before `static-fire` when `docs/orbital-survey.md` is
  missing, stale, or lacks coverage for the target modules. It is the
  reconnaissance writer of that map. `catch` may refresh the map on landing,
  and `abort` may append a confirmed landmine, as specified in `AGENTS.md`.
- Order after planning clears: `liftoff` (implementation) → `max-q` (3-pass
  spec-compliance review) → `catch` (audit, map sync, and landing). `abort`
  runs only in pipeline mode when `liftoff`, `max-q`, or `catch` exhausts its
  retry cap or hits a blocking environment error. A standalone failure asks the
  user and does not start `abort`.
- Direct standalone override: if the user explicitly invokes a single skill by
  name, run only that skill and halt on completion.
- `flight-audit` is a standalone, strictly read-only audit. It is not part of
  the seven-skill pipeline, never chains into it, and never edits source files.
- `AGENTS.md` is the agent-facing summary of this workflow. When the skills
  suite changes, derive `AGENTS.md` from that suite and from the repository
  rules in this file. Do not leave the two in conflict.

## 17. Cross-project portability

- Discover the repository's native test, lint, type-check, and build commands
  from `package.json`, `pyproject.toml`, `Cargo.toml`, `go.mod`, `Makefile`, or
  `.github/workflows`; never hardcode language-specific runners. The checks
  this repo currently requires are in §10 and §11.
- Scale fan-out to task size: 1 worker in `liftoff` for small changes (1–2
  files, < 50 lines); full parallel fan-out for 3 or more files or 50 or more lines. `max-q` always runs 3 passes with 4 reviewers per pass.
  `orbital-survey` uses 4 scouts, or 1–2 when the repo has fewer than 20
  files. `abort` always uses 3 diagnostic sub-agents.
- The lowercase `<label>(<scope>): <description>` commit/PR standard in §15 is
  this repository's convention. Core skills must follow it here; they must not
  invent a different format.
