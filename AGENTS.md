# AGENTS.md

How to make changes to Conjure. Every change also follows
[docs/rules.md](docs/rules.md), including the most important one: Conjure
never handles API keys or credentials.

For context: [product](docs/product.md), [architecture](docs/architecture.md),
and the [reference](docs/reference/) pages for the store, export, MCP server
and desktop app.

## Workflow: Overkill

Every change goes through the [Overkill](https://github.com/nmajestic/overkill_skills)
pipeline, however small: features, fixes, refactors, scripts, config and docs.

| # | Skill | What it does |
|---|---|---|
| 1 | `spec` | Writes `.record/NNNN-<slug>/spec.md` with Given/When/Then acceptance criteria. The user approves it. |
| 2 | `plan` | Explores the code, asks the user at every design fork, and splits the work into tasks. The user approves it. |
| 3 | `write-tests` | Writes a few failing tests from the acceptance criteria and commits them first. |
| 4 | `implement` | Builds the tasks with parallel workers, one commit per task. |
| 5 | `verify` | Runs every check in [Commands](#commands). Never changes code. |
| 6 | `review` | 8 reviewer personas, then a verifier. Every finding is fixed. |
| 7 | `open-pr` | Pushes the run's branch, opens the pull request, fixes CI failures. Never merges. |

- **Start** with `run-pipeline <request>`. Resume with `run-pipeline resume`.
- **One skill:** when the user names a single skill, run only that skill.
- **Read-only requests** (explaining code, answering questions, reading logs)
  skip the pipeline.
- **Don't write implementation code** until the spec and plan are approved.
- **Records:** each run's spec, plan, tests, checks and review are committed in
  `.record/NNNN-<slug>/`.
- **The skills are installed per machine**, not stored here. If they aren't
  available, stop and ask the user to install them. Never imitate the pipeline
  from memory.
  - Claude Code: `/plugin marketplace add nmajestic/overkill_skills`, then
    `/plugin install overkill@nmajestic`.
  - OpenCode: clone `overkill_skills` and run `scripts/install.sh --tool opencode`.

## Commands

| Check | Command |
|---|---|
| test | `cargo test --workspace` |
| lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| format | `cargo fmt --all -- --check` |
| type-check | Covered by lint and build |
| build | `cargo build --workspace` |
| audit | `cargo audit` (install with `cargo install cargo-audit`) |

CI (`.github/workflows/ci.yml`) runs format, lint and test on every push to
`main` and every pull request.

**Linux:** `conjure-app` links `libxkbcommon`, and its headless UI tests need a
Vulkan adapter. Without a GPU, use Mesa's software driver:
`sudo apt-get install libxkbcommon-dev mesa-vulkan-drivers libvulkan1`.

## Commits

- `<label>(<optional-scope>): <description>`, optionally ending with one
  Overkill ID: a task `[T3]`, a fix `[F2]` or a run `[0007]`.
- All lowercase except the ID, imperative mood, no trailing period.
- Labels: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `chore`, `style`,
  `revert` (for a task undone by an approved plan revision).

## Pull requests

- Title in the commit format, without an ID.
- Body uses Overkill's `open-pr` template: Summary, Acceptance criteria,
  Checks, Review, Follow-ups, Record.
- One primary label, under about 400 lines of code diff. `.record/` doesn't
  count. `spec` splits bigger ideas into separate runs.
- Existing docs in `docs/` are inputs to `spec`, not a substitute for it.

## Pushing

- Never `git push` unless told to.
- Starting `run-pipeline` counts as being told to push that run's own branch
  and open its pull request. Nothing else is pushed without asking.

## Keeping this file true

This file is the only place the workflow is written down. When the workflow or
the Overkill skills change, update it in the same change.
