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

- Format: `<label>(<optional-scope>): <description>`, optionally followed by
  one bracketed Overkill ID: a task (`[T3]`), a fix (`[F2]`) or a run
  (`[0007]`).
- Strictly all lowercase except the bracketed ID, imperative present tense, no
  trailing period.
- Allowed labels: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `chore`,
  `style`, `revert`. `revert` is used when an approved plan revision undoes a
  task.

### Pull requests

- Title follows the commit format, without an ID suffix.
- Body follows Overkill's `open-pr` template: Summary, Acceptance criteria,
  Checks, Review, Follow-ups and Record.
- Keep PRs atomic to a single primary label and under ~400 lines of code diff.
  `.record/` does not count toward the limit. `spec` splits an idea that would
  exceed it into separate runs.

### Push

- Never `git push` unless explicitly instructed.
- Starting an Overkill run (`run-pipeline`) is the instruction to push that
  run's own branch and open its pull request. Nothing else is pushed without
  asking.

## 16. Mandatory build workflow

Every change goes through the Overkill pipeline, in order:
`spec` → `plan` → `write-tests` → `implement` → `verify` → `review` →
`open-pr`, run by `run-pipeline`. Do not create, modify, or delete
implementation files until the spec and the plan are approved.

- Applies to new features, bug fixes, architectural changes, refactors,
  scripts, config and docs. Every change runs all seven stages, however small.
- Skip only for read-only requests (explaining code, answering conceptual
  questions, inspecting logs).
- Direct standalone override: if the user explicitly invokes a single skill by
  name, run only that skill and stop.
- Each run's spec, plan, tests, checks and review history is committed in
  `.record/NNNN-<slug>/`.
- Existing product specs in `docs/specs/` and the milestone docs are inputs to
  `spec`. They do not replace it.
- The skills come from <https://github.com/nmajestic/overkill_skills> and are
  installed per machine, not stored in this repo. If they are not installed,
  stop and ask; never imitate the pipeline from memory.
- `AGENTS.md` is the agent-facing summary of this workflow. When this file or
  the Overkill skills change, update `AGENTS.md` to match. Do not leave the
  two in conflict.

## 17. Commands and CI

- The checks this repo requires are in §10 and §11. Overkill's `plan` records
  them and `verify` runs them on every change: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, `cargo build --workspace` and `cargo audit`.
- CI (`.github/workflows/ci.yml`) runs format, lint and test on every push to
  `main` and every pull request.
- On Linux, `conjure-app` needs `libxkbcommon-dev` to link, and its headless
  UI tests need a Vulkan adapter (Mesa's `mesa-vulkan-drivers` without a GPU).
- The commit and PR standard in §15 is this repository's convention. Overkill
  follows it here and must not invent a different format.
