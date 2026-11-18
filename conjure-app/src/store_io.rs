use std::path::Path;

use conjure_store::{ListFilter, Store, StoreError, Verification};

use crate::pixels;
use crate::state::{AppState, AssetRow, CardRow, Screen};

const ZOOM_STEPS: [u32; 4] = [1, 2, 4, 8];

pub fn refresh(state: &mut AppState) {
    let store = match open_store(state) {
        Ok(store) => store,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    let cards = match store.list_cards() {
        Ok(cards) => cards,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    let assets = match store.list_assets(&ListFilter { tag: None }) {
        Ok(assets) => assets,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    state.cards = cards
        .into_iter()
        .map(|card| CardRow {
            hash: card.hash,
            id: card.id,
            name: card.name,
        })
        .collect();
    state.assets = assets
        .into_iter()
        .map(|asset| AssetRow {
            hash: asset.hash,
            name: asset.name,
            kind: asset.kind,
            w: asset.width,
            h: asset.height,
            tags: asset.tags,
            card_hash: asset.style_card_hash,
        })
        .collect();
    state.screen = Screen::Browse;
}

pub fn initialize(state: &mut AppState) {
    if let Err(e) = Store::create(&state.root) {
        state.notice = Some(format!("error: {e}"));
        return;
    }
    refresh(state);
}

pub fn select(state: &mut AppState, hash: &str) {
    let Some(row) = state.assets.iter().find(|row| row.hash == hash) else {
        state.notice = Some(format!("no asset {hash}"));
        return;
    };
    let card_hash = row.card_hash.clone();
    let store = match open_store(state) {
        Ok(store) => store,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    let asset = match store.load_asset(hash) {
        Ok(asset) => asset,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    let card = match store.load_card(&card_hash) {
        Ok(card) => card,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    state.selection = vec![hash.to_string()];
    state.selected_card = Some(card_hash);
    match pixels::compose(&asset, &card, state.zoom) {
        Some(preview) => state.preview = Some(preview),
        None => {
            state.preview = None;
            state.notice = Some(format!(
                "preview too large: {}x{} at zoom {} exceeds the viewer cap",
                asset.width, asset.height, state.zoom
            ));
        }
    }
}

pub fn select_card(state: &mut AppState, card_hash: &str) {
    state.selected_card = Some(card_hash.to_string());
    state.selection.clear();
    state.preview = None;
}

pub fn zoom(state: &mut AppState, delta: i32) {
    let position = ZOOM_STEPS
        .iter()
        .position(|&z| z == state.zoom)
        .unwrap_or(0);
    let next = (position as i32 + delta).rem_euclid(ZOOM_STEPS.len() as i32) as usize;
    state.zoom = ZOOM_STEPS[next];
    let Some(hash) = state.selection.last().cloned() else {
        return;
    };
    let Some(row) = state.assets.iter().find(|row| row.hash == hash) else {
        return;
    };
    let card_hash = row.card_hash.clone();
    let store = match open_store(state) {
        Ok(store) => store,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    let asset = match store.load_asset(&hash) {
        Ok(asset) => asset,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    let card = match store.load_card(&card_hash) {
        Ok(card) => card,
        Err(e) => {
            state.notice = Some(e.to_string());
            return;
        }
    };
    match pixels::compose(&asset, &card, state.zoom) {
        Some(preview) => state.preview = Some(preview),
        // Keep the last composable preview on screen instead of blanking it.
        None => {
            state.notice = Some(format!(
                "preview too large: {}x{} at zoom {} exceeds the viewer cap",
                asset.width, asset.height, state.zoom
            ))
        }
    }
}

pub fn save_generated(state: &mut AppState) {
    let Some(pending) = state.r#gen.pending.take() else {
        return;
    };
    let store = match open_store(state) {
        Ok(store) => store,
        Err(e) => {
            state.r#gen.pending = Some(pending);
            state.r#gen.note = Some(format!("error: {e}"));
            return;
        }
    };
    match store.save_assets(&pending.request, &pending.assets) {
        Err(StoreError::Verify(Verification(report))) => {
            let first = report
                .errors
                .first()
                .map(|diagnostic| diagnostic.message.as_str())
                .unwrap_or("no diagnostics");
            state.r#gen.note = Some(format!("verification failed: {first}"));
            state.r#gen.pending = Some(pending);
        }
        Err(e) => {
            state.r#gen.note = Some(format!("error: {e}"));
            state.r#gen.pending = Some(pending);
        }
        Ok(_) => {
            refresh(state);
            state.notice = Some(format!("saved {} assets", pending.assets.len()));
        }
    }
}

/// Mirrors conjure-cli's refusal rule: rejects (never sanitizes) any output
/// name that could escape the export directory.
fn safe_output_stem(file: &str) -> Option<String> {
    let stem = file.strip_suffix(".png")?;
    if stem.is_empty() {
        return None;
    }
    if stem.starts_with('.') {
        return None;
    }
    if !stem
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return None;
    }
    if stem.split('.').any(|part| part.is_empty()) {
        return None;
    }
    Some(stem.to_string())
}

fn export_failed(state: &mut AppState, e: impl std::fmt::Display) {
    state.export.message = Some(format!("error: {e}"));
}

pub fn export(state: &mut AppState, dir: &Path) {
    if state.selection.is_empty() {
        state.export.message = Some("no assets selected".to_string());
        return;
    }
    let mut picks: Vec<(String, String, String)> = Vec::with_capacity(state.selection.len());
    let mut missing: Option<String> = None;
    for hash in &state.selection {
        let Some(row) = state.assets.iter().find(|row| row.hash == *hash) else {
            missing = Some(hash.clone());
            break;
        };
        picks.push((row.hash.clone(), row.name.clone(), row.card_hash.clone()));
    }
    if let Some(hash) = missing {
        state.export.message = Some(format!("error: no asset {hash}"));
        return;
    }
    let card_hash = picks[0].2.clone();
    if picks.iter().any(|(_, _, card)| *card != card_hash) {
        state.export.message = Some("export requires assets from one style card".to_string());
        return;
    }
    let store = match open_store(state) {
        Ok(store) => store,
        Err(e) => return export_failed(state, e),
    };
    let card = match store.load_card(&card_hash) {
        Ok(card) => card,
        Err(e) => return export_failed(state, e),
    };
    let mut assets = Vec::with_capacity(picks.len());
    let mut names = Vec::with_capacity(picks.len());
    for (hash, name, _) in &picks {
        match store.load_asset(hash) {
            Ok(asset) => {
                assets.push(asset);
                names.push(name.clone());
            }
            Err(e) => return export_failed(state, e),
        }
    }
    let opts = conjure_export::ExportOpts {
        layout: if state.export.grid {
            conjure_export::SheetLayout::Grid {
                columns: state.export.columns,
            }
        } else {
            conjure_export::SheetLayout::StripHorizontal
        },
        scale: state.export.scale,
    };
    let png = match conjure_export::export_png(&card, &assets, &opts) {
        Ok(png) => png,
        Err(e) => return export_failed(state, e),
    };
    let mut manifest = match conjure_export::export_manifest(&card, &assets, &names, &opts) {
        Ok(manifest) => manifest,
        Err(e) => return export_failed(state, e),
    };
    manifest.style_card.hash = Some(card_hash);
    let file = manifest.image.file.clone();
    let Some(stem) = safe_output_stem(&file) else {
        state.export.message = Some(format!("export output name {file:?} is unsafe"));
        return;
    };
    let png_path = dir.join(format!("{stem}.png"));
    let json_path = dir.join(format!("{stem}.json"));
    if let Err(e) = std::fs::create_dir_all(dir) {
        return export_failed(state, e);
    }
    if let Err(e) = std::fs::write(&png_path, &png) {
        return export_failed(state, e);
    }
    let json = match serde_json::to_string_pretty(&manifest) {
        Ok(json) => json,
        Err(e) => return export_failed(state, e),
    };
    if let Err(e) = std::fs::write(&json_path, json) {
        return export_failed(state, e);
    }
    state.export.message = Some(format!("{}\n{}", png_path.display(), json_path.display()));
}

fn open_store(state: &AppState) -> Result<Store, StoreError> {
    let root = conjure_store::locate(&state.root)?;
    Store::open(&root)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use conjure_core::style::{Dimensions, Grid, Outline, PaletteColor, Shading, Style};
    use conjure_core::{Asset, AssetKind, AssetRequest, StyleCard};

    use super::*;
    use crate::state::{AppState, Generated};

    struct TempRoot(PathBuf);

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_root(label: &str) -> TempRoot {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        TempRoot(std::env::temp_dir().join(format!(
            "conjure-app-store-test-{}-{nanos}-{label}",
            std::process::id()
        )))
    }

    fn test_card() -> StyleCard {
        StyleCard {
            id: "card-1".into(),
            name: "card one".into(),
            palette: vec![
                PaletteColor {
                    name: "a".into(),
                    hex: "#000000".into(),
                },
                PaletteColor {
                    name: "b".into(),
                    hex: "#111111".into(),
                },
                PaletteColor {
                    name: "c".into(),
                    hex: "#222222".into(),
                },
                PaletteColor {
                    name: "d".into(),
                    hex: "#333333".into(),
                },
            ],
            grid: Grid {
                tile_size: Dimensions { w: 16, h: 16 },
                unit_scale: 1,
            },
            style: Style {
                outline: Outline::None,
                outline_color: "#000000".into(),
                shading: Shading::Flat,
                proportions: "1:1".into(),
                era: "modern".into(),
                theme: "pixel".into(),
            },
        }
    }

    fn sprite_request(name: &str, width: u32, height: u32) -> AssetRequest {
        AssetRequest {
            name: name.into(),
            kind: AssetKind::Sprite {
                size: Dimensions {
                    w: width,
                    h: height,
                },
            },
            style_card: test_card(),
        }
    }

    fn tileset_request(count: usize) -> AssetRequest {
        AssetRequest {
            name: "floor".into(),
            kind: AssetKind::Tileset {
                tile_size: Dimensions { w: 2, h: 2 },
                count,
            },
            style_card: test_card(),
        }
    }

    fn asset(width: u32, height: u32, pixels: Vec<u8>) -> Asset {
        Asset {
            style_card_id: "card-1".into(),
            width,
            height,
            pixels,
        }
    }

    fn tiles(count: usize) -> Vec<Asset> {
        (0..count).map(|i| asset(2, 2, vec![i as u8; 4])).collect()
    }

    fn blob_count(root: &Path) -> usize {
        std::fs::read_dir(root.join(".conjure").join("blobs"))
            .map(|entries| entries.count())
            .unwrap_or(0)
    }

    #[test]
    fn refresh_fills_rows_in_store_order() {
        let root = temp_root("refresh");
        let store = Store::create(&root.0).unwrap();
        store.save_assets(&tileset_request(3), &tiles(3)).unwrap();
        store
            .save_assets(
                &sprite_request("hero", 2, 2),
                &[asset(2, 2, vec![0, 1, 1, 0])],
            )
            .unwrap();

        let mut state = AppState::init_from(&root.0);
        assert!(matches!(state.screen, Screen::Browse));
        assert_eq!(state.cards.len(), 1);
        assert_eq!(state.cards[0].id, "card-1");
        assert_eq!(state.assets.len(), 4);
        let names: Vec<&str> = state.assets.iter().map(|row| row.name.as_str()).collect();
        for name in ["floor#0", "floor#1", "floor#2", "hero"] {
            assert!(names.contains(&name), "missing row {name}");
        }
        for row in &state.assets {
            assert_eq!(
                row.kind,
                if row.name.starts_with("floor") {
                    "tile"
                } else {
                    "sprite"
                }
            );
            assert_eq!(row.w, 2);
            assert_eq!(row.h, 2);
            assert!(row.tags.is_empty());
        }

        let before: Vec<String> = state.assets.iter().map(|row| row.hash.clone()).collect();
        refresh(&mut state);
        let after: Vec<String> = state.assets.iter().map(|row| row.hash.clone()).collect();
        assert_eq!(after, before);
    }

    #[test]
    fn initialize_creates_a_store_and_lands_on_browse() {
        let root = temp_root("initialize");
        std::fs::create_dir_all(&root.0).unwrap();
        let mut state = AppState::init_from(&root.0);
        assert!(matches!(state.screen, Screen::NoLibrary));

        initialize(&mut state);

        assert!(matches!(state.screen, Screen::Browse));
        assert!(state.notice.is_none());
        assert!(root.0.join(".conjure").join("conjure.sqlite").exists());
    }

    #[test]
    fn initialize_failure_sets_notice() {
        let dir = temp_root("initialize-fail-parent");
        std::fs::create_dir_all(&dir.0).unwrap();
        let root = dir.0.join("occupied");
        std::fs::write(&root, b"not a directory").unwrap();
        let mut state = AppState::init_from(&root);
        assert!(matches!(state.screen, Screen::NoLibrary));

        initialize(&mut state);

        assert!(matches!(state.screen, Screen::NoLibrary));
        let notice = state.notice.unwrap();
        assert!(notice.starts_with("error: "), "got: {notice}");
    }

    #[test]
    fn select_loads_preview_and_single_card_selection() {
        let root = temp_root("select");
        let store = Store::create(&root.0).unwrap();
        let hashes = store.save_assets(&tileset_request(3), &tiles(3)).unwrap();
        let mut state = AppState::init_from(&root.0);
        let card_hash = state.assets[0].card_hash.clone();

        select(&mut state, &hashes[1]);

        assert_eq!(state.selection, vec![hashes[1].clone()]);
        assert_eq!(state.selected_card.as_deref(), Some(card_hash.as_str()));
        let preview = state.preview.as_ref().unwrap();
        assert_eq!(preview.w, 2);
        assert_eq!(preview.h, 2);
        assert_eq!(preview.zoom, 1);
        assert_eq!(preview.palette.len(), 4);
    }

    #[test]
    fn select_card_clears_asset_selection_and_preview() {
        let root = temp_root("select-card");
        let store = Store::create(&root.0).unwrap();
        let hashes = store.save_assets(&tileset_request(3), &tiles(3)).unwrap();
        let mut state = AppState::init_from(&root.0);
        select(&mut state, &hashes[0]);
        assert!(state.preview.is_some());

        select_card(&mut state, "abcd1234");

        assert_eq!(state.selected_card.as_deref(), Some("abcd1234"));
        assert!(state.selection.is_empty());
        assert!(state.preview.is_none());
    }

    #[test]
    fn zoom_cycles_the_sequence_and_recomposes_the_preview() {
        let root = temp_root("zoom");
        let store = Store::create(&root.0).unwrap();
        let hashes = store.save_assets(&tileset_request(3), &tiles(3)).unwrap();
        let mut state = AppState::init_from(&root.0);
        select(&mut state, &hashes[0]);

        zoom(&mut state, 1);
        assert_eq!(state.zoom, 2);
        assert_eq!(state.preview.as_ref().unwrap().w, 4);
        zoom(&mut state, 1);
        assert_eq!(state.zoom, 4);
        assert_eq!(state.preview.as_ref().unwrap().w, 8);
        zoom(&mut state, 1);
        assert_eq!(state.zoom, 8);
        assert_eq!(state.preview.as_ref().unwrap().w, 16);
        zoom(&mut state, 1);
        assert_eq!(state.zoom, 1);
        assert_eq!(state.preview.as_ref().unwrap().w, 2);
        zoom(&mut state, -1);
        assert_eq!(state.zoom, 8);
        assert_eq!(state.preview.as_ref().unwrap().w, 16);
    }

    #[test]
    fn select_refuses_a_preview_over_the_viewer_cap_and_clears_it() {
        let root = temp_root("select-too-large");
        let store = Store::create(&root.0).unwrap();
        let hashes = store
            .save_assets(
                &sprite_request("big", 2048, 2048),
                &[asset(2048, 2048, vec![0u8; 2048 * 2048])],
            )
            .unwrap();
        let mut state = AppState::init_from(&root.0);
        state.zoom = 8;
        let card_hash = state.assets[0].card_hash.clone();

        select(&mut state, &hashes[0]);

        // Selection bookkeeping happens either way; only the preview refuses.
        assert_eq!(state.selection, vec![hashes[0].clone()]);
        assert_eq!(state.selected_card.as_deref(), Some(card_hash.as_str()));
        assert!(state.preview.is_none());
        assert_eq!(
            state.notice.as_deref(),
            Some("preview too large: 2048x2048 at zoom 8 exceeds the viewer cap")
        );

        // The same asset at zoom 1 is under the cap and composes fine.
        state.zoom = 1;
        state.notice = None;
        select(&mut state, &hashes[0]);
        assert!(state.notice.is_none());
        let preview = state.preview.as_ref().unwrap();
        assert_eq!((preview.w, preview.h, preview.zoom), (2048, 2048, 1));
    }

    #[test]
    fn zoom_beyond_the_viewer_cap_keeps_the_existing_preview_and_notifies() {
        let root = temp_root("zoom-too-large");
        let store = Store::create(&root.0).unwrap();
        let hashes = store
            .save_assets(
                &sprite_request("big", 2048, 2048),
                &[asset(2048, 2048, vec![0u8; 2048 * 2048])],
            )
            .unwrap();
        let mut state = AppState::init_from(&root.0);
        select(&mut state, &hashes[0]);
        assert_eq!(state.preview.as_ref().unwrap().zoom, 1);

        // 2048x2048 at zoom 2 hits the cap exactly (16,777,216 px) and composes.
        zoom(&mut state, 1);
        assert_eq!(state.zoom, 2);
        assert!(state.notice.is_none());
        assert_eq!(state.preview.as_ref().unwrap().w, 4096);

        // Zoom 4 would compose 67,108,864 px: refused, zoom-2 preview kept.
        zoom(&mut state, 1);
        assert_eq!(state.zoom, 4);
        assert_eq!(
            state.notice.as_deref(),
            Some("preview too large: 2048x2048 at zoom 4 exceeds the viewer cap")
        );
        let preview = state.preview.as_ref().unwrap();
        assert_eq!((preview.w, preview.h, preview.zoom), (4096, 4096, 2));
    }

    #[test]
    fn save_generated_keeps_pending_when_verification_fails() {
        let root = temp_root("save-verify");
        Store::create(&root.0).unwrap();
        let mut state = AppState::init_from(&root.0);
        state.r#gen.pending = Some(Generated {
            request: tileset_request(3),
            assets: tiles(1),
            used_fallback: false,
        });

        save_generated(&mut state);

        let note = state.r#gen.note.as_deref().unwrap();
        assert!(note.starts_with("verification failed: "), "got: {note}");
        assert!(state.r#gen.pending.is_some());
        assert!(state.assets.is_empty());
        assert_eq!(blob_count(&root.0), 0);
        assert!(state.notice.is_none());
    }

    #[test]
    fn save_generated_success_adds_rows() {
        let root = temp_root("save-ok");
        Store::create(&root.0).unwrap();
        let mut state = AppState::init_from(&root.0);
        state.r#gen.pending = Some(Generated {
            request: sprite_request("hero", 2, 2),
            assets: vec![asset(2, 2, vec![0, 1, 1, 0])],
            used_fallback: false,
        });

        save_generated(&mut state);

        assert_eq!(state.notice.as_deref(), Some("saved 1 assets"));
        assert!(state.r#gen.pending.is_none());
        assert_eq!(state.assets.len(), 1);
        assert_eq!(state.assets[0].name, "hero");
        assert_eq!(blob_count(&root.0), 1);
    }

    #[test]
    fn export_writes_files_byte_identical_to_conjure_export() {
        let root = temp_root("export");
        let out_dir = temp_root("export-out");
        let store = Store::create(&root.0).unwrap();
        store.save_assets(&tileset_request(3), &tiles(3)).unwrap();
        let mut state = AppState::init_from(&root.0);
        let card_hash = state.assets[0].card_hash.clone();
        let mut assets = Vec::new();
        let mut names = Vec::new();
        for row in &state.assets {
            assets.push(store.load_asset(&row.hash).unwrap());
            names.push(row.name.clone());
            state.selection.push(row.hash.clone());
        }

        export(&mut state, &out_dir.0);

        let message = state.export.message.as_deref().unwrap();
        let lines: Vec<&str> = message.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with("floor.png"), "got: {message}");
        assert!(lines[1].ends_with("floor.json"), "got: {message}");

        let opts = conjure_export::ExportOpts {
            layout: conjure_export::SheetLayout::StripHorizontal,
            scale: 1,
        };
        let card = store.load_card(&card_hash).unwrap();
        let expected_png = conjure_export::export_png(&card, &assets, &opts).unwrap();
        let mut expected_manifest =
            conjure_export::export_manifest(&card, &assets, &names, &opts).unwrap();
        expected_manifest.style_card.hash = Some(card_hash);
        let expected_json = serde_json::to_string_pretty(&expected_manifest).unwrap();

        let written_png = std::fs::read(out_dir.0.join("floor.png")).unwrap();
        let written_json = std::fs::read_to_string(out_dir.0.join("floor.json")).unwrap();
        assert_eq!(written_png, expected_png);
        assert_eq!(written_json, expected_json);
    }

    #[test]
    fn export_refuses_assets_from_mixed_style_cards() {
        let root = temp_root("export-mixed");
        let store = Store::create(&root.0).unwrap();
        let hashes_a = store
            .save_assets(
                &sprite_request("hero", 2, 2),
                &[asset(2, 2, vec![0, 1, 1, 0])],
            )
            .unwrap();
        let mut other = sprite_request("sidekick", 2, 2);
        other.style_card.id = "card-2".into();
        other.style_card.name = "card two".into();
        let mut other_asset = asset(2, 2, vec![1, 0, 0, 1]);
        other_asset.style_card_id = "card-2".into();
        let hashes_b = store.save_assets(&other, &[other_asset]).unwrap();
        let mut state = AppState::init_from(&root.0);
        state.selection = vec![hashes_a[0].clone(), hashes_b[0].clone()];

        export(&mut state, &root.0.join("out"));

        assert_eq!(
            state.export.message.as_deref(),
            Some("export requires assets from one style card")
        );
        assert!(!root.0.join("out").exists());
    }

    #[test]
    fn export_refuses_an_unsafe_output_name() {
        let root = temp_root("export-unsafe");
        let store = Store::create(&root.0).unwrap();
        let hashes = store
            .save_assets(
                &sprite_request("../evil", 2, 2),
                &[asset(2, 2, vec![0, 1, 1, 0])],
            )
            .unwrap();
        let mut state = AppState::init_from(&root.0);
        state.selection = vec![hashes[0].clone()];

        export(&mut state, &root.0);

        assert_eq!(
            state.export.message.as_deref(),
            Some("export output name \"../evil.png\" is unsafe")
        );
    }

    #[test]
    fn export_refuses_an_empty_selection() {
        let root = temp_root("export-empty");
        Store::create(&root.0).unwrap();
        let mut state = AppState::init_from(&root.0);

        export(&mut state, &root.0);

        assert_eq!(state.export.message.as_deref(), Some("no assets selected"));
    }
}
