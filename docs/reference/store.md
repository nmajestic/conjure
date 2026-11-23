# Reference: the store

`conjure-store` keeps a project's style cards and verified assets in a
`.conjure/` folder, like `.git`. Records are immutable and content-addressed:
the hash is the version.

## Finding the store

Commands look for `.conjure/` in the current directory and then each parent,
and use the nearest one. `conjure init` creates it in the current directory.
`conjure save` also creates it there if none is found. Every other command
fails with `no .conjure/ found (run conjure init)`.

`.conjure/` is project data and is not git-ignored by default, so a project
can commit its library. `blobs/` can grow large.

## Layout

```
.conjure/
  conjure.sqlite   # metadata
  blobs/
    <blake3-hex>   # raw palette indices of one asset, nothing else
```

A blob holds only the pixel indices. Width, height and palette live in
SQLite, so assets with identical indices share one blob even if their sizes
or style cards differ.

## Hashes

All hashes are BLAKE3, written as lowercase hex.

| Hash | Over |
|---|---|
| Style card | `conjure-card-v1\0` (16 bytes), then the card's compact JSON |
| Asset | `conjure-asset-v1` (16 bytes), the style card's `id` length as u32 little-endian, the `id` bytes, width as u32 LE, height as u32 LE, then the pixel indices |
| Blob | The pixel indices only; this is the blob's file name |

The asset hash uses the style card's `id` field, not the card's hash. Names,
tags and save times are not part of any hash, so renaming or re-saving never
changes one.

## Schema

SQLite in WAL mode with foreign keys on and a 5-second busy timeout.

```sql
CREATE TABLE style_cards(
  hash TEXT PRIMARY KEY,   -- card hash
  id TEXT NOT NULL,        -- StyleCard.id
  name TEXT NOT NULL,
  json TEXT NOT NULL,      -- full StyleCard JSON
  saved_at INTEGER NOT NULL
);

CREATE TABLE assets(
  hash TEXT PRIMARY KEY,   -- asset hash
  style_card_hash TEXT NOT NULL REFERENCES style_cards(hash),
  name TEXT NOT NULL,      -- request name, or "<name>#<i>" for tiles
  kind TEXT NOT NULL,      -- "sprite" or "tile"
  width INTEGER NOT NULL,
  height INTEGER NOT NULL,
  blob_hash TEXT NOT NULL, -- file name under blobs/
  saved_at INTEGER NOT NULL
);

CREATE TABLE tags(
  asset_hash TEXT NOT NULL REFERENCES assets(hash),
  tag TEXT NOT NULL,
  PRIMARY KEY(asset_hash, tag)
);
```

`saved_at` is Unix seconds and is metadata only.

## Saving

1. The style card must be valid and every asset must pass `verify_request`.
   Every asset must reference the request's style card `id`. If anything
   fails, nothing is written.
2. Each blob is written to a temporary file and renamed into place. An
   existing blob whose contents don't match its hash is rewritten.
3. The card and asset rows are inserted in one transaction with
   `INSERT OR IGNORE`. Saving identical content again changes nothing,
   including `saved_at`.

Pixels are never updated in place. A changed asset is a new row with a new
hash. Names aren't unique; listings show the name and a short hash.

## Loading

Loading an asset reads its blob and checks it against the stored blob hash. A
missing or corrupt blob is an error, never silently wrong pixels.

## Hash prefixes

Commands accept a full hash or a prefix of at least 8 hex characters (case
doesn't matter). A prefix matching nothing or more than one record is an
error.

## Garbage collection

`conjure gc` deletes blobs no asset references, but only ones older than one
hour, so a save in progress is never cut short. Files in `blobs/` that aren't
named like a hash are left alone. It prints how many blobs it removed and
kept.

## Commands

| Command | Does | Output |
|---|---|---|
| `conjure init` | Creates `.conjure/` in the current directory | nothing |
| `conjure save <request.json> <assets.json>` | Verifies, then saves the card and assets | `{"card_hash", "asset_hashes"}` JSON; on failed verification, the `ValidationReport` JSON and exit code 1 |
| `conjure ls [--tag <tag>]` | Lists cards and assets, optionally only those with a tag | text listing with short hashes |
| `conjure show <hash-or-prefix>` | Prints one card or asset | its JSON |
| `conjure tag <hash-or-prefix> <tag>` | Adds a tag to an asset | nothing |
| `conjure gc` | Removes old unreferenced blobs | counts |

Any other error prints `error: <message>` and exits with code 2.
