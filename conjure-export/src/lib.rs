mod error;
mod manifest;
mod png;

pub use error::ExportError;
pub use manifest::{
    MANIFEST_VERSION, Manifest, ManifestCard, ManifestFrame, ManifestImage, ManifestOrigin,
    ManifestPaletteEntry,
};
pub use png::{DecodedPng, decode_indexed_png, encode_indexed_png};

use conjure_core::{Asset, StyleCard};

#[derive(Debug, Clone)]
pub enum SheetLayout {
    StripHorizontal,
    Grid { columns: u32 },
}

#[derive(Debug, Clone)]
pub struct ExportOpts {
    pub layout: SheetLayout,
    pub scale: u32,
}

impl Default for ExportOpts {
    fn default() -> Self {
        Self {
            layout: SheetLayout::StripHorizontal,
            scale: 1,
        }
    }
}

/// Hard cap on total exported sheet pixels (1 << 26). The composed index
/// buffer is allocated only after this cap passes.
const MAX_SHEET_PIXELS: u64 = 1 << 26;

fn sheet_too_large() -> ExportError {
    ExportError::Invalid("sheet too large: dimensions exceed the export cap".to_string())
}

struct Sheet {
    png_width: u32,
    png_height: u32,
    frames: Vec<(u32, u32, u32, u32)>,
    indices: Vec<u8>,
}

fn validate(card: &StyleCard, assets: &[Asset], opts: &ExportOpts) -> Result<Sheet, ExportError> {
    if assets.is_empty() {
        return Err(ExportError::Invalid("no assets to export".to_string()));
    }
    let n = card.palette.len();
    if n == 0 || n > 256 {
        return Err(ExportError::Invalid(format!(
            "palette must contain 1..=256 colors, found {n}"
        )));
    }
    for color in &card.palette {
        color.rgb().map_err(ExportError::Invalid)?;
    }
    for (i, asset) in assets.iter().enumerate() {
        let report = conjure_core::verify_asset(card, asset.dimensions(), asset);
        if !report.passed {
            return Err(ExportError::Invalid(format!(
                "asset {i} fails verification: {}",
                report.errors[0].message
            )));
        }
    }
    match opts.layout {
        SheetLayout::StripHorizontal => {
            let first_h = assets[0].height;
            if assets.iter().any(|a| a.height != first_h) {
                return Err(ExportError::Invalid(
                    "horizontal strip requires equal heights".to_string(),
                ));
            }
        }
        SheetLayout::Grid { columns } => {
            if columns == 0 {
                return Err(ExportError::Invalid(
                    "grid requires at least 1 column".to_string(),
                ));
            }
            let (tw, th) = (assets[0].width, assets[0].height);
            if assets.iter().any(|a| a.width != tw || a.height != th) {
                return Err(ExportError::Invalid(
                    "grid layout requires uniform tile size".to_string(),
                ));
            }
        }
    }
    if opts.scale == 0 || opts.scale > 16 {
        return Err(ExportError::Invalid(
            "scale must be between 1 and 16".to_string(),
        ));
    }

    let scale = u64::from(opts.scale);
    let mut frame_rects: Vec<(u64, u64, u64, u64)> = Vec::with_capacity(assets.len());
    let (unscaled_width, unscaled_height) = match opts.layout {
        SheetLayout::StripHorizontal => {
            let mut x = 0u64;
            for asset in assets {
                frame_rects.push((x, 0, u64::from(asset.width), u64::from(asset.height)));
                x = x
                    .checked_add(u64::from(asset.width))
                    .ok_or_else(sheet_too_large)?;
            }
            (x, u64::from(assets[0].height))
        }
        SheetLayout::Grid { columns } => {
            let (tw, th) = (u64::from(assets[0].width), u64::from(assets[0].height));
            let columns = u64::from(columns);
            let rows = assets.len().div_ceil(columns as usize) as u64;
            for (i, _) in assets.iter().enumerate() {
                let (col, row) = (i as u64 % columns, i as u64 / columns);
                frame_rects.push((
                    col.checked_mul(tw).ok_or_else(sheet_too_large)?,
                    row.checked_mul(th).ok_or_else(sheet_too_large)?,
                    tw,
                    th,
                ));
            }
            (
                tw.checked_mul(columns).ok_or_else(sheet_too_large)?,
                th.checked_mul(rows).ok_or_else(sheet_too_large)?,
            )
        }
    };

    // Enforce the pixel cap before allocating or composing any buffer.
    let png_width = unscaled_width
        .checked_mul(scale)
        .ok_or_else(sheet_too_large)?;
    let png_height = unscaled_height
        .checked_mul(scale)
        .ok_or_else(sheet_too_large)?;
    let pixels = png_width
        .checked_mul(png_height)
        .filter(|p| *p <= MAX_SHEET_PIXELS)
        .ok_or_else(|| {
            ExportError::Invalid(format!(
                "sheet too large: {png_width}x{png_height} exceeds the export cap"
            ))
        })?;

    // The cap bounds every dimension below; the checked casts keep that
    // guarantee explicit instead of relying on wrapping.
    let to_usize = |v: u64| usize::try_from(v).map_err(|_| sheet_too_large());
    let png_width = u32::try_from(png_width).map_err(|_| sheet_too_large())?;
    let png_height = u32::try_from(png_height).map_err(|_| sheet_too_large())?;

    let unscaled_len = unscaled_width
        .checked_mul(unscaled_height)
        .ok_or_else(sheet_too_large)?;
    let mut unscaled = vec![0u8; to_usize(unscaled_len)?];
    for (asset, &(fx, fy, fw, fh)) in assets.iter().zip(&frame_rects) {
        for r in 0..fh {
            let dst = fy
                .checked_add(r)
                .and_then(|v| v.checked_mul(unscaled_width))
                .and_then(|v| v.checked_add(fx))
                .ok_or_else(sheet_too_large)?;
            let src = r.checked_mul(fw).ok_or_else(sheet_too_large)?;
            let (dst, src, fw) = (to_usize(dst)?, to_usize(src)?, to_usize(fw)?);
            unscaled[dst..dst + fw].copy_from_slice(&asset.pixels[src..src + fw]);
        }
    }

    let mut indices = Vec::with_capacity(to_usize(pixels)?);
    for y in 0..unscaled_height {
        let row_start = y.checked_mul(unscaled_width).ok_or_else(sheet_too_large)?;
        let row_end = row_start
            .checked_add(unscaled_width)
            .ok_or_else(sheet_too_large)?;
        let row = &unscaled[to_usize(row_start)?..to_usize(row_end)?];
        for _ in 0..scale {
            for &b in row {
                indices.extend(std::iter::repeat_n(b, to_usize(scale)?));
            }
        }
    }

    let mut frames: Vec<(u32, u32, u32, u32)> = Vec::with_capacity(frame_rects.len());
    for (x, y, w, h) in frame_rects {
        frames.push((
            u32::try_from(x).map_err(|_| sheet_too_large())?,
            u32::try_from(y).map_err(|_| sheet_too_large())?,
            u32::try_from(w).map_err(|_| sheet_too_large())?,
            u32::try_from(h).map_err(|_| sheet_too_large())?,
        ));
    }

    Ok(Sheet {
        png_width,
        png_height,
        frames,
        indices,
    })
}

pub fn export_png(
    card: &StyleCard,
    assets: &[Asset],
    opts: &ExportOpts,
) -> Result<Vec<u8>, ExportError> {
    let sheet = validate(card, assets, opts)?;
    let palette_triples: Vec<[u8; 3]> = card
        .palette
        .iter()
        .map(|c| c.rgb().map_err(ExportError::Invalid))
        .collect::<Result<Vec<[u8; 3]>, ExportError>>()?;
    crate::png::encode_indexed_png(
        sheet.png_width,
        sheet.png_height,
        &palette_triples,
        &sheet.indices,
    )
}

pub fn export_manifest(
    card: &StyleCard,
    assets: &[Asset],
    names: &[String],
    opts: &ExportOpts,
) -> Result<Manifest, ExportError> {
    if names.len() != assets.len() {
        return Err(ExportError::Invalid(
            "names and assets must have the same length".to_string(),
        ));
    }
    let sheet = validate(card, assets, opts)?;
    let (layout, columns) = match opts.layout {
        SheetLayout::StripHorizontal => ("strip_horizontal", None),
        SheetLayout::Grid { columns } => ("grid", Some(columns)),
    };
    let base = names[0].split('#').next().unwrap_or(&names[0]);
    let image = ManifestImage {
        file: format!("{}.png", base),
        layout: layout.to_string(),
        columns,
        scale: opts.scale,
        png_width: sheet.png_width,
        png_height: sheet.png_height,
    };
    let palette = card
        .palette
        .iter()
        .enumerate()
        .map(|(i, color)| ManifestPaletteEntry {
            index: i,
            name: color.name.clone(),
            hex: color.hex.clone(),
            transparent: i == 0,
        })
        .collect();
    let frames = sheet
        .frames
        .iter()
        .enumerate()
        .map(|(i, &(x, y, w, h))| ManifestFrame {
            name: names[i].clone(),
            index: i,
            x,
            y,
            w,
            h,
            origin: ManifestOrigin { x: 0, y: 0 },
        })
        .collect();
    Ok(Manifest {
        manifest_version: MANIFEST_VERSION,
        style_card: ManifestCard {
            id: card.id.clone(),
            hash: None,
            palette,
        },
        image,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use conjure_core::style::{Dimensions, Grid, Outline, PaletteColor, Shading, Style};

    use super::*;

    fn card(palette: Vec<PaletteColor>) -> StyleCard {
        StyleCard {
            id: "card-1".to_string(),
            name: "card".to_string(),
            palette,
            grid: Grid {
                tile_size: Dimensions { w: 8, h: 8 },
                unit_scale: 1,
            },
            style: Style {
                outline: Outline::None,
                outline_color: "#000000".to_string(),
                shading: Shading::Flat,
                proportions: "1:1".to_string(),
                era: "modern".to_string(),
                theme: "pixel".to_string(),
            },
        }
    }

    fn three_color_card() -> StyleCard {
        card(vec![
            PaletteColor {
                name: "bg".to_string(),
                hex: "#ffffff".to_string(),
            },
            PaletteColor {
                name: "mid".to_string(),
                hex: "#101010".to_string(),
            },
            PaletteColor {
                name: "fg".to_string(),
                hex: "#4a6a2a".to_string(),
            },
        ])
    }

    fn tile(pixels: &[u8]) -> Asset {
        Asset {
            style_card_id: "card-1".to_string(),
            width: 8,
            height: 8,
            pixels: pixels.to_vec(),
        }
    }

    fn tile_wh(w: u32, h: u32, pixels: &[u8]) -> Asset {
        Asset {
            style_card_id: "card-1".to_string(),
            width: w,
            height: h,
            pixels: pixels.to_vec(),
        }
    }

    fn solid_tile(index: u8) -> Asset {
        tile(&[index; 64])
    }

    fn assert_invalid<T>(result: Result<T, ExportError>, msg: &str) {
        match result {
            Ok(_) => panic!("expected Invalid({msg:?}), got Ok"),
            Err(ExportError::Invalid(m)) => assert_eq!(m, msg),
            Err(e) => panic!("expected Invalid({msg:?}), got {e:?}"),
        }
    }

    // U11.1: default opts export a valid strip PNG.
    #[test]
    fn default_opts_export_strip_png() {
        let assets = vec![solid_tile(0), solid_tile(1), solid_tile(2)];
        let bytes = export_png(&three_color_card(), &assets, &ExportOpts::default()).unwrap();
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let decoded = decode_indexed_png(&bytes).unwrap();
        assert_eq!(decoded.width, 24);
        assert_eq!(decoded.height, 8);
    }

    // U11.2: each refusal returns Invalid with the exact message.
    #[test]
    fn refuses_empty_assets() {
        let opts = ExportOpts::default();
        assert_invalid(
            export_png(&three_color_card(), &[], &opts),
            "no assets to export",
        );
    }

    #[test]
    fn refuses_empty_palette() {
        let c = card(vec![]);
        let assets = vec![solid_tile(0)];
        assert_invalid(
            export_png(&c, &assets, &ExportOpts::default()),
            "palette must contain 1..=256 colors, found 0",
        );
    }

    #[test]
    fn refuses_invalid_palette_hex() {
        let c = card(vec![
            PaletteColor {
                name: "bg".to_string(),
                hex: "#ffffff".to_string(),
            },
            PaletteColor {
                name: "bad".to_string(),
                hex: "#zzzzzz".to_string(),
            },
        ]);
        let assets = vec![solid_tile(0)];
        assert_invalid(
            export_png(&c, &assets, &ExportOpts::default()),
            "invalid hex #zzzzzz",
        );
    }

    #[test]
    fn refuses_oversized_palette() {
        let palette: Vec<PaletteColor> = (0..257)
            .map(|i| PaletteColor {
                name: format!("c{i}"),
                hex: format!("#{:06x}", i),
            })
            .collect();
        let c = card(palette);
        let assets = vec![solid_tile(0)];
        assert_invalid(
            export_png(&c, &assets, &ExportOpts::default()),
            "palette must contain 1..=256 colors, found 257",
        );
    }

    #[test]
    fn refuses_out_of_range_pixel_index() {
        let mut pixels = vec![0u8; 64];
        pixels[0] = 5;
        let assets = vec![tile(&pixels)];
        assert_invalid(
            export_png(&three_color_card(), &assets, &ExportOpts::default()),
            "asset 0 fails verification: pixel (0,0) uses palette index 5, but palette has 3 colors",
        );
    }

    #[test]
    fn refuses_unequal_strip_heights() {
        let assets = vec![solid_tile(0), tile_wh(8, 6, &[0; 48])];
        assert_invalid(
            export_png(&three_color_card(), &assets, &ExportOpts::default()),
            "horizontal strip requires equal heights",
        );
    }

    #[test]
    fn refuses_grid_zero_columns() {
        let assets = vec![solid_tile(0)];
        let opts = ExportOpts {
            layout: SheetLayout::Grid { columns: 0 },
            scale: 1,
        };
        assert_invalid(
            export_png(&three_color_card(), &assets, &opts),
            "grid requires at least 1 column",
        );
    }

    #[test]
    fn refuses_non_uniform_grid_tiles() {
        let assets = vec![solid_tile(0), tile_wh(6, 8, &[0; 48])];
        let opts = ExportOpts {
            layout: SheetLayout::Grid { columns: 2 },
            scale: 1,
        };
        assert_invalid(
            export_png(&three_color_card(), &assets, &opts),
            "grid layout requires uniform tile size",
        );
    }

    #[test]
    fn refuses_scale_zero() {
        let assets = vec![solid_tile(0)];
        let opts = ExportOpts {
            layout: SheetLayout::StripHorizontal,
            scale: 0,
        };
        assert_invalid(
            export_png(&three_color_card(), &assets, &opts),
            "scale must be between 1 and 16",
        );
    }

    #[test]
    fn refuses_scale_seventeen() {
        let assets = vec![solid_tile(0)];
        let opts = ExportOpts {
            layout: SheetLayout::StripHorizontal,
            scale: 17,
        };
        assert_invalid(
            export_png(&three_color_card(), &assets, &opts),
            "scale must be between 1 and 16",
        );
    }

    #[test]
    fn refuses_sheet_over_pixel_cap() {
        let pixels = vec![0u8; 2048 * 2048];
        let assets = vec![tile_wh(2048, 2048, &pixels)];
        let opts = ExportOpts {
            layout: SheetLayout::StripHorizontal,
            scale: 16,
        };
        assert_invalid(
            export_png(&three_color_card(), &assets, &opts),
            "sheet too large: 32768x32768 exceeds the export cap",
        );
    }

    #[test]
    fn refuses_names_length_mismatch() {
        let assets = vec![solid_tile(0), solid_tile(1), solid_tile(2)];
        let names = vec!["demo#0".to_string(), "demo#1".to_string()];
        assert_invalid(
            export_manifest(&three_color_card(), &assets, &names, &ExportOpts::default()),
            "names and assets must have the same length",
        );
    }

    // U11.3: strip scale 1 concatenates left to right; scale 2 makes 2x2 blocks.
    #[test]
    fn strip_scale_one_concatenates_left_to_right() {
        let assets = vec![solid_tile(0), solid_tile(1), solid_tile(2)];
        let bytes = export_png(&three_color_card(), &assets, &ExportOpts::default()).unwrap();
        let decoded = decode_indexed_png(&bytes).unwrap();
        assert_eq!(decoded.width, 24);
        assert_eq!(decoded.height, 8);
        let expected: Vec<u8> = [[0u8; 8], [1u8; 8], [2u8; 8]].concat();
        let expected: Vec<u8> = expected.into_iter().cycle().take(24 * 8).collect();
        assert_eq!(decoded.indices, expected);
    }

    #[test]
    fn strip_scale_two_makes_2x2_blocks() {
        let assets = vec![solid_tile(0), solid_tile(1), solid_tile(2)];
        let opts = ExportOpts {
            layout: SheetLayout::StripHorizontal,
            scale: 2,
        };
        let bytes = export_png(&three_color_card(), &assets, &opts).unwrap();
        let decoded = decode_indexed_png(&bytes).unwrap();
        assert_eq!(decoded.width, 48);
        assert_eq!(decoded.height, 16);
        for y in 0..16u32 {
            for x in 0..48u32 {
                let expected = ((x / 2) / 8) as u8;
                assert_eq!(decoded.indices[(y * 48 + x) as usize], expected);
            }
        }
    }

    // U11.4: grid with 2 columns lays frames out (0,0), (8,0), (0,8).
    #[test]
    fn grid_two_columns_places_frames_correctly() {
        let assets = vec![solid_tile(0), solid_tile(1), solid_tile(2)];
        let names: Vec<String> = (0..3).map(|i| format!("demo#{i}")).collect();
        let opts = ExportOpts {
            layout: SheetLayout::Grid { columns: 2 },
            scale: 1,
        };
        let manifest = export_manifest(&three_color_card(), &assets, &names, &opts).unwrap();
        assert_eq!(manifest.image.png_width, 16);
        assert_eq!(manifest.image.png_height, 16);
        assert_eq!(manifest.image.layout, "grid");
        assert_eq!(manifest.image.columns, Some(2));
        let rects: Vec<(u32, u32, u32, u32)> = manifest
            .frames
            .iter()
            .map(|f| (f.x, f.y, f.w, f.h))
            .collect();
        assert_eq!(rects, vec![(0, 0, 8, 8), (8, 0, 8, 8), (0, 8, 8, 8)]);
    }

    // U11.5: frames tile the sheet exactly and manifest metadata is correct.
    #[test]
    fn frames_tile_sheet_and_manifest_metadata() {
        let assets = vec![solid_tile(0), solid_tile(1), solid_tile(2)];
        let names = vec![
            "demo#0".to_string(),
            "demo#1".to_string(),
            "demo#2".to_string(),
        ];
        let manifest =
            export_manifest(&three_color_card(), &assets, &names, &ExportOpts::default()).unwrap();
        let total_frame_area: u32 = manifest.frames.iter().map(|f| f.w * f.h).sum();
        assert_eq!(
            total_frame_area,
            manifest.image.png_width * manifest.image.png_height
        );
        assert_eq!(manifest.frames[0].x, 0);
        assert_eq!(
            manifest.frames[1].x,
            manifest.frames[0].x + manifest.frames[0].w
        );
        assert_eq!(
            manifest.frames[2].x,
            manifest.frames[1].x + manifest.frames[1].w
        );
        assert_eq!(
            manifest.frames[2].x + manifest.frames[2].w,
            manifest.image.png_width
        );
        assert!(manifest.frames.iter().all(|f| f.y == 0));
        assert_eq!(manifest.image.file, "demo.png");
        assert!(manifest.style_card.palette[0].transparent);
        assert!(
            manifest
                .style_card
                .palette
                .iter()
                .skip(1)
                .all(|e| !e.transparent)
        );
        assert_eq!(manifest.style_card.hash, None);
    }

    // U11.6: PLTE order matches card order exactly, even when unsorted by hex.
    #[test]
    fn palette_order_preserved_verbatim() {
        let c = card(vec![
            PaletteColor {
                name: "bg".to_string(),
                hex: "#ffffff".to_string(),
            },
            PaletteColor {
                name: "dark".to_string(),
                hex: "#101010".to_string(),
            },
            PaletteColor {
                name: "green".to_string(),
                hex: "#4a6a2a".to_string(),
            },
        ]);
        let assets = vec![solid_tile(0), solid_tile(1), solid_tile(2)];
        let bytes = export_png(&c, &assets, &ExportOpts::default()).unwrap();
        let decoded = decode_indexed_png(&bytes).unwrap();
        assert_eq!(
            decoded.palette,
            vec![[255, 255, 255], [16, 16, 16], [74, 106, 42]]
        );
    }

    // U11.7: grid columns >= assets yields a single row; manifest serde roundtrips.
    #[test]
    fn grid_single_row_and_serde_roundtrip() {
        let assets = vec![solid_tile(0), solid_tile(1), solid_tile(2)];
        let names: Vec<String> = (0..3).map(|i| format!("demo#{i}")).collect();
        let opts = ExportOpts {
            layout: SheetLayout::Grid { columns: 3 },
            scale: 1,
        };
        let manifest = export_manifest(&three_color_card(), &assets, &names, &opts).unwrap();
        assert_eq!(manifest.image.png_width, 24);
        assert_eq!(manifest.image.png_height, 8);
        assert!(manifest.frames.iter().all(|f| f.y == 0));
        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("\"columns\":3"));
        let back: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, manifest);
    }
}
