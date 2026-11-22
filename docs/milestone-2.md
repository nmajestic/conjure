# Milestone 2 — persist and export

**Status:** specified, not started
**Depends on:** 1a (core types). Does not depend on a live model — 1c is useful for real art, not for this milestone.
**Crates:** `conjure-store`, `conjure-export`, plus CLI commands in `conjure-cli`.

## Goal

Make Conjure *manage* assets, not just print JSON. A solo dev (or an agent) can save a style card and a verified asset set into the project, list what is there, and export an engine-ready indexed PNG plus a JSON manifest. Same bytes in, same bytes out.

## Locked decisions (2026-11-16)

| Decision | Choice |
|---|---|
| Where the library lives | Project-local `.conjure/` (like `.git`). Not a user-global XDG library. |
| PNG encoding | Indexed-color PNG. Palette is the `PLTE` chunk, not expanded RGBA. |
| Transparency | Palette index `0` is transparent. Written to the PNG `tRNS` entry. No alpha channel in the core model. |
| Versioning | Immutable and content-addressed. Identity is a content hash. Saving the same bytes twice is a no-op. |
| Export formats | PNG (single sprite, strip, or grid sheet) + JSON manifest. No Godot / Unity / Aseprite exporters. |

## What "ship" means

The core loop ends at `store/export`. This milestone is that last arrow:

```
verified Asset[] + StyleCard
        │
        ├─ save  → .conjure/  (hash + metadata)
        └─ export → PNG + manifest.json
```

Nothing is written unless `verify_request` passes and `StyleCard::validate` passes. A failed asset is never persisted and never exported. The store is not a junk drawer.

## Layout on disk

```
.conjure/
  conjure.sqlite          # metadata only
  blobs/
    <hex-hash>            # raw indexed pixel bytes, one file per unique payload
```

- `.conjure/` is created on first `conjure init` (or implicitly on first `save`).
- Blob files are the `pixels` vec and nothing else. Width, height, and palette live in SQLite, not in the blob. Two 16×16 sprites with identical indices share one blob even if their style cards differ.
- The hash is over the canonical payload defined below, not over the file path or the clock.
- `.conjure/` is project data. It is **not** gitignored by default — a solo dev should be able to commit the library. Document that `blobs/` can get large; do not add it to `.gitignore` in this milestone.

### Canonical hash payload

BLAKE3 over this exact byte layout (recommended; confirm at build time — see open items):

```
magic:        b"conjure-asset-v1"   # 16 bytes, nul-padded
style_card_id: utf-8, length-prefixed u32 le
width:        u32 le
height:       u32 le
pixels:       raw bytes, length == width * height
```

Style cards hash separately:

```
magic:        b"conjure-card-v1\0"  # 16 bytes
canonical JSON of the StyleCard (serde field order as defined in code, no whitespace)
```

Same payload → same hash → same blob. Renaming a record does not change the hash. The hash is the version.

## SQLite schema

`rusqlite` with the `bundled` feature so the CLI does not depend on a system libsqlite. WAL mode. One connection per command; no long-lived server.

```sql
CREATE TABLE style_cards (
  hash        TEXT PRIMARY KEY,          -- blake3 hex of the card payload
  id          TEXT NOT NULL,             -- StyleCard.id (human id, not unique across versions)
  name        TEXT NOT NULL,
  json        TEXT NOT NULL,             -- full StyleCard JSON
  saved_at    INTEGER NOT NULL           -- unix seconds; metadata only, not part of the hash
);

CREATE TABLE assets (
  hash           TEXT PRIMARY KEY,       -- blake3 hex of the asset payload
  style_card_hash TEXT NOT NULL REFERENCES style_cards(hash),
  name           TEXT NOT NULL,          -- request name, or "<name>#<index>" for tiles
  kind           TEXT NOT NULL,          -- "sprite" | "tile"
  width          INTEGER NOT NULL,
  height         INTEGER NOT NULL,
  blob_hash      TEXT NOT NULL,          -- filename under blobs/
  saved_at       INTEGER NOT NULL
);

CREATE TABLE tags (
  asset_hash  TEXT NOT NULL REFERENCES assets(hash),
  tag         TEXT NOT NULL,
  PRIMARY KEY (asset_hash, tag)
);
```

Rules:

- Inserts are `INSERT OR IGNORE`. Re-saving identical content updates nothing, including `saved_at`.
- There is no `UPDATE` of pixel bytes. A "new version" is a new row with a new hash.
- Human names are not unique. Listing shows `name` + short hash so collisions are obvious.
- Deleting a row does not delete a blob that another row still points at. Orphan blobs are left until an explicit `conjure gc` (in scope — see commands).
- Foreign keys on. A save that cannot store the style card first is an error; never an asset without its card.

## `conjure-store` API

Pure-ish library: filesystem + SQLite, no HTTP, no tokio requirement (rusqlite is sync; the CLI can call it from the main thread). Does not depend on `conjure-agent` or `conjure-inference`.

```rust
pub struct Store { /* path to .conjure */ }

impl Store {
    pub fn open(project_root: &Path) -> Result<Self, StoreError>;
    pub fn create(project_root: &Path) -> Result<Self, StoreError>;

    pub fn save_card(&self, card: &StyleCard) -> Result<RecordId, StoreError>;
    pub fn save_assets(
        &self,
        request: &AssetRequest,
        assets: &[Asset],
    ) -> Result<Vec<RecordId>, StoreError>;

    pub fn load_card(&self, hash: &str) -> Result<StyleCard, StoreError>;
    pub fn load_asset(&self, hash: &str) -> Result<Asset, StoreError>;

    pub fn list_cards(&self) -> Result<Vec<CardSummary>, StoreError>;
    pub fn list_assets(&self, filter: &ListFilter) -> Result<Vec<AssetSummary>, StoreError>;

    pub fn tag(&self, hash: &str, tag: &str) -> Result<(), StoreError>;
    pub fn gc(&self) -> Result<GcReport, StoreError>;
}
```

`save_assets` must:

1. `request.style_card.validate()?`
2. `verify_request(request, assets)` — if not passed, return the `ValidationReport` inside the error. Do not write.
3. Refuse if any `asset.style_card_id != request.style_card.id`.
4. Write the card, then each asset, in one SQLite transaction. Blob files are written before the transaction commits; a crash may leave an orphan blob, which `gc` collects. A crash must not leave a row pointing at a missing blob.

`load_asset` recomputes the hash and returns an error if the blob does not match. That is the corruption diagnostic from the product brief.

Errors are a small `thiserror` enum (`NotFound`, `Invalid`, `Corrupt { expected, actual }`, `Io`, `Sqlite`) — not `String`. This is the first crate that earns typed IO errors (codebase rule 8).

## `conjure-export` API

Depends on `conjure-core` and `image` (with the `png` feature). No store dependency — export takes values, not hashes. The CLI wires store → export.

```rust
pub enum SheetLayout { StripHorizontal, Grid { columns: u32 } }

pub struct ExportOpts {
    pub layout: SheetLayout,   // default: strip, one tile after another
    pub scale: u32,            // integer nearest-neighbor only; default 1; max 16
}

pub fn export_png(card: &StyleCard, assets: &[Asset], opts: &ExportOpts) -> Result<Vec<u8>, ExportError>;
pub fn export_manifest(card: &StyleCard, assets: &[Asset], opts: &ExportOpts) -> Result<Manifest, ExportError>;
```

`export_png` refuses (does not silently fix) if:

- palette is empty or longer than 256 (indexed PNG limit; StyleCard max is 32, so this is defensive)
- any asset fails `verify_asset` against the card and its own dimensions
- assets are not all the same height for a horizontal strip (tilesets from core always are)
- `scale` is 0 or > 16

### Indexed PNG rules

- Color type: indexed (`ColorType::Indexed`).
- `PLTE`: one RGB triple per palette entry, in palette order. Hex parsed with `PaletteColor::rgb`.
- `tRNS`: a single transparent entry for index `0`. Indices `1..` are fully opaque. There is no partial alpha.
- Bit depth: 8. Do not pack to 4-bit even when the palette is small — simpler, and 32 colors still fits.
- No filtering tricks that change pixels. A round-trip decode must reproduce the index grid exactly, not merely the RGB.
- `scale > 1` repeats each index into an `scale × scale` block. No interpolation. The manifest records both the logical size and the scaled PNG size.

### Manifest

Written next to the PNG. Stable field names (rule 9). Versioned so a later Godot exporter can consume it without guessing.

```json
{
  "manifest_version": 1,
  "style_card": {
    "id": "demo-palette",
    "hash": "<card hash, omitted if the caller has no store>",
    "palette": [
      { "index": 0, "name": "background", "hex": "#101010", "transparent": true },
      { "index": 1, "name": "outline", "hex": "#000000", "transparent": false }
    ]
  },
  "image": {
    "file": "demo.png",
    "layout": "strip_horizontal",
    "scale": 1,
    "png_width": 24,
    "png_height": 8
  },
  "frames": [
    {
      "name": "demo#0",
      "index": 0,
      "x": 0,
      "y": 0,
      "w": 8,
      "h": 8,
      "origin": { "x": 0, "y": 0 }
    }
  ]
}
```

- `origin` is always `{0, 0}` in v1. The field exists so milestone 4 / a later exporter can set a pivot without a breaking manifest change.
- Frame `name` is `request.name` for a sprite and `{name}#{i}` for tiles, matching the store.
- `hash` on the style card is optional at the export API. The CLI fills it when the card was loaded from the store.

## CLI

New commands. Existing commands stay byte-identical.

```
conjure init                          # create .conjure/ in the cwd
conjure save <request.json> <assets.json>
                                      # verify, then save card + assets; print hashes
conjure ls                            # cards, then assets: name, hash-prefix, size
conjure ls --tag dungeon
conjure show <hash-or-prefix>        # pretty JSON of one card or asset
conjure export <hash-or-prefix>... --out dir/
                                      # PNG + manifest; multiple assets → one sheet
conjure tag <hash-or-prefix> <tag>
conjure gc                            # delete unreferenced blobs; print counts
```

Conventions carried from 1a–1c:

- Hand-rolled args. No clap.
- Errors on stderr, exit 2. `save` of an invalid set exits 1 and prints the `ValidationReport` JSON on stdout (same split as `validate`: machine output on stdout, failure is exit 1).
- `export` and `ls` success: human listing on stdout is acceptable for `ls`; `export` prints the paths written, one per line. Do not mix JSON and prose on the same stream.
- Hash prefixes must be unique or the command errors with the candidates. No silent shortest-prefix magic that can change meaning when a new asset arrives — require a unique prefix of at least 8 hex chars, or the full hash.
- Store root is the cwd, walking upward for `.conjure/` the way git finds `.git`. If none, error: `no .conjure/ found (run conjure init)`. Do not create a store in `$HOME`.

`save` does not call the model. Generation stays `conjure agent` / `conjure generate`. The pipeline a person runs is:

```
conjure agent examples/demo/request.json --provider opencode --model <id> > assets.json
conjure save examples/demo/request.json assets.json
conjure export <prefix> --out build/
```

## Dependencies (justification required in the commit)

| Crate | Where | Why std is not enough |
|---|---|---|
| `rusqlite` (`bundled`) | store | SQL metadata, transactions, queries. Std has no embedded DB. |
| `blake3` | store | Fast, stable, no length-extension issues. Std has no hash. |
| `image` (`png` feature only) | export | Indexed PNG encode/decode with `PLTE` + `tRNS`. Hand-rolling a PNG writer is a bug farm. |
| `thiserror` | store, export | Typed IO errors. First use in the repo; do not add it to `conjure-core`. |

No tokio in these two crates. No `serde_json` in `conjure-core` (manifest JSON is built in `conjure-export`, which may depend on `serde_json`).

## Tests

Store (temp dirs, no network):

- Round-trip: save → load → `Asset` / `StyleCard` equal.
- Idempotent save: second save returns the same hash and does not change `saved_at` or add a blob.
- Different pixels → different hash; identical pixels + same card id → same hash.
- `verify_request` failure → nothing on disk (no db row, no blob).
- Corrupt blob (flip one byte) → `load_asset` returns `Corrupt`.
- `gc` removes an orphan and keeps a referenced blob.
- Two projects (two temp roots) do not see each other's rows.

Export:

- Decode the PNG with `image` and assert every pixel index matches the source grid.
- Index 0 is the `tRNS` index; index 1 is opaque.
- Palette order matches the style card, not sorted by hex.
- `scale: 2` doubles width and height and repeats indices in blocks.
- Manifest frame rectangles tile the sheet with no gaps and no overlap.
- A sprite with an out-of-range index is refused, not clamped (export is not repair).

CLI smoke (temp project):

- `init` then `save` of `examples/demo` assets produced by `conjure generate <req> 42`.
- `export` writes `demo.png` + `demo.json`; `validate` is unchanged.
- `save` of a bad assets file exits 1 and leaves `.conjure/` without that asset.

## Out of scope

- Animation frame timing, origins other than `{0,0}`, nine-slice, tileset adjacency.
- RGBA PNG, Aseprite `.aseprite`, Godot `.import`, Unity sprites.
- User-global library, sync, accounts, cloud.
- Editing pixels in the store (that is repair/generate, then a new save).
- Watching the filesystem. The Rune app (milestone 4) reads this API; it does not get a daemon here.
- Migrating old stores. There is no v0. Ship `manifest_version: 1` and the hash magics above; a later change bumps the magic and refuses unknown magics.

## Acceptance

- `cargo test --workspace` green, including the new crate tests.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` clean.
- A scripted demo: generate (seed 42) → save → export → decode PNG → indices equal the generated asset.
- `conjure-core` still depends only on `serde`. No new I/O in core.
- Docs (`architecture.md` current-state, README layout table) updated in the same change.

## Open items to close before building

1. Hash algorithm: BLAKE3 (recommended) vs SHA-256 (more ubiquitous, slower, fine at this size).
2. Whether `.conjure/` is committed by convention only, or we add a `conjure.sqlite` note to the README and leave gitignore alone (recommended: leave gitignore alone).
3. Sheet default for a tileset export: horizontal strip (recommended) vs square-ish grid.

## Suggested build waves

1. Store schema + `open`/`save`/`load` + hash tests.
2. `list` / `tag` / `gc` + export PNG/manifest (disjoint files).
3. CLI commands + README/architecture notes.
