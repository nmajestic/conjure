# Reference: export

`conjure-export` turns verified assets into one indexed-color PNG sheet plus a
JSON manifest describing it. The same inputs always produce the same bytes.

## Rules

- **All assets come from one style card.** Each must pass verification against
  it, and the palette must have 1 to 256 colors with valid hex values.
- **Layouts:**
  - `strip_horizontal` (default): assets side by side. All must be the same
    height.
  - `grid` with `columns ≥ 1`: filled row by row. All assets must be the same
    size.
- **Scale** 1 to 16 (default 1). Each pixel becomes a scale × scale block, with
  no smoothing.
- **Size cap:** a sheet over 2²⁶ pixels (67,108,864) after scaling is refused.
  Nothing is written when any rule fails.

## The PNG

- Indexed color, 8-bit, even for small palettes.
- `PLTE` holds the palette in order. `tRNS` makes index 0 fully transparent;
  every other index is fully opaque.
- Decoding the PNG gives back the exact index grid, not just the same colors.

## The manifest

Written next to the PNG. `manifest_version` is 1.

```json
{
  "manifest_version": 1,
  "style_card": {
    "id": "demo-palette",
    "hash": "<card hash>",
    "palette": [
      { "index": 0, "name": "background", "hex": "#101010", "transparent": true },
      { "index": 1, "name": "outline", "hex": "#000000", "transparent": false }
    ]
  },
  "image": {
    "file": "demo.png",
    "layout": "grid",
    "columns": 2,
    "scale": 4,
    "png_width": 64,
    "png_height": 64
  },
  "frames": [
    { "name": "demo#0", "index": 0, "x": 0, "y": 0, "w": 8, "h": 8, "origin": { "x": 0, "y": 0 } }
  ]
}
```

- `style_card.hash` is the card's store hash when the card came from the store,
  and `null` otherwise.
- `image.columns` appears only for `grid`.
- `image.file` is the first asset's name up to any `#`, plus `.png`.
- **Frame positions and sizes are in unscaled pixels.** Multiply by `scale` to
  get PNG coordinates.
- Frame `name` matches the store: the request name for a sprite, `<name>#<i>`
  for a tile.
- `origin` is always `{0, 0}`. It exists so a pivot can be added later without
  changing the format.

## Commands

```
conjure export <hash-or-prefix>... --out <dir>
```

Exports saved assets with the defaults (horizontal strip, scale 1), creates
`<dir>` if needed, writes `<file>.png` and `<file>.json`, and prints both
paths. The CLI refuses a file name that could escape `<dir>`: it must be ASCII
letters, digits, `-`, `_` and `.`, must not start with `.`, and must not
contain `..`. The desktop app and the MCP server can also choose `grid` and a scale.
