# AGENTS.md

Read [docs/codebase-rules.md](docs/codebase-rules.md) before making changes.
Product vision: [docs/product-brief.md](docs/product-brief.md). Architecture: [docs/architecture.md](docs/architecture.md).

## Ground rules

- No API keys, ever. Conjure requires no credentials and never stores or
  transmits them; it talks to local OpenAI-compatible inference servers or
  drives a local agent CLI (OpenCode), which owns all auth. No cloud endpoints,
  secret handling, or key-assuming SDKs in Conjure.
- The codebase rules (crate boundaries, a pure `conjure-core`, the verifier as
  the trust layer, determinism, no comments unless needed) apply to every
  change, and reviewers enforce them.

## Build workflow: Overkill

Every change goes through the [Overkill](https://github.com/nmajestic/overkill_skills)
pipeline: features, bug fixes, refactors, scripts, config and docs alike. A
one-line fix runs every stage too.

| # | Skill | What it does here |
|---|---|---|
| 1 | `spec` | Turns the request into `.record/NNNN-<slug>/spec.md` with Given/When/Then acceptance criteria. The user approves it. |
| 2 | `plan` | Explores the workspace, asks the user at every design fork, and writes tasks in parallel waves. The user approves it. |
| 3 | `write-tests` | Writes a few failing tests from the acceptance criteria and commits them first. |
| 4 | `implement` | Builds the tasks with parallel workers, one commit per task. |
| 5 | `verify` | Runs every check below. Never changes code. |
| 6 | `review` | 8 reviewer personas plus a verifier. Every finding is fixed. |
| 7 | `open-pr` | Pushes the run's branch, opens the PR and fixes CI failures. Never merges. |

- **Start a change** with `run-pipeline <request>`. It runs the stages in order,
  stops only where the user decides, and resumes with `run-pipeline resume`.
- **One skill on its own:** when the user names a single skill, run only that
  skill and stop.
- **Read-only requests** (explaining code, answering questions, inspecting
  logs) skip the pipeline.
- **The skills are not stored in this repo.** If they aren't available, stop
  and ask the user to install them. Never imitate the pipeline from memory.
  - Claude Code: `/plugin marketplace add nmajestic/overkill_skills`, then
    `/plugin install overkill@nmajestic`.
  - OpenCode: clone `overkill_skills` and run `scripts/install.sh --tool opencode`.

## Conjure specifics for Overkill

- **Commands** `plan` records and `verify` runs:

  | Check | Command |
  |---|---|
  | test | `cargo test --workspace` |
  | lint | `cargo clippy --workspace --all-targets -- -D warnings` |
  | format | `cargo fmt --all -- --check` |
  | type-check | Covered by clippy and build (N/A as a separate check) |
  | build | `cargo build --workspace` |
  | audit | `cargo audit` (install with `cargo install cargo-audit`) |

  CI runs format, lint and test in `.github/workflows/ci.yml`.
- **Linux test dependencies:** `conjure-app` links `libxkbcommon` and its
  headless UI tests need a Vulkan adapter. Without a GPU, install Mesa's
  software driver: `sudo apt-get install libxkbcommon-dev mesa-vulkan-drivers libvulkan1`.
- **Run records** live in `.record/NNNN-<slug>/` and are committed with the
  change.
- **Existing product docs** (`docs/specs/`, `docs/milestone-*.md`) are inputs
  to `spec`, not substitutes for it.
- **Git and PRs** follow [docs/codebase-rules.md §15](docs/codebase-rules.md):
  lowercase `<label>(<scope>): <description>` with an optional Overkill ID
  suffix like `[T3]`; Overkill's PR template; under ~400 lines of code diff,
  not counting `.record/`. A `run-pipeline` run may push its own branch and
  open its own PR. Nothing else is pushed without asking.
- **Docs stay truthful:** when behavior or structure changes, update
  `AGENTS.md` and `docs/` in the same change.
