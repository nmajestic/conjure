use std::path::{Path, PathBuf};

use conjure_core::{Asset, AssetRequest};

use crate::r#gen::GenCommand;
use crate::pixels::Preview;

pub enum Screen {
    NoLibrary,
    Browse,
}

pub struct CardRow {
    pub hash: String,
    pub id: String,
    pub name: String,
}

pub struct AssetRow {
    pub hash: String,
    pub name: String,
    pub kind: String,
    pub w: u32,
    pub h: u32,
    pub tags: Vec<String>,
    pub card_hash: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunState {
    Idle,
    Running,
    Passed,
    Failed,
}

pub struct GenPanel {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub models: Vec<(String, String)>,
    pub opencode_models: Vec<String>,
    pub card_hash: Option<String>,
    pub name: String,
    pub name_edit: rune::TextEdit,
    pub is_sprite: bool,
    pub sprite_w: u32,
    pub sprite_h: u32,
    pub tile_w: u32,
    pub tile_h: u32,
    pub tile_count: u32,
    pub state: RunState,
    pub events: Vec<String>,
    pub note: Option<String>,
    pub used_fallback: bool,
    pub pending: Option<Generated>,
}

pub struct Generated {
    pub request: AssetRequest,
    pub assets: Vec<Asset>,
    pub used_fallback: bool,
}

pub struct ExportPanel {
    pub grid: bool,
    pub columns: u32,
    pub scale: u32,
    pub message: Option<String>,
}

pub struct AppState {
    pub root: PathBuf,
    pub screen: Screen,
    pub notice: Option<String>,
    pub cards: Vec<CardRow>,
    pub assets: Vec<AssetRow>,
    pub selected_card: Option<String>,
    pub selection: Vec<(String, String)>,
    pub preview: Option<Preview>,
    pub zoom: u32,
    pub export: ExportPanel,
    pub r#gen: GenPanel,
    pub gen_tx: Option<std::sync::mpsc::Sender<GenCommand>>,
}

impl AppState {
    pub fn init() -> Self {
        Self::init_from(&std::env::current_dir().unwrap_or_default())
    }

    pub fn init_from(root: &Path) -> Self {
        let mut state = Self {
            root: root.to_path_buf(),
            screen: Screen::NoLibrary,
            notice: None,
            cards: Vec::new(),
            assets: Vec::new(),
            selected_card: None,
            selection: Vec::new(),
            preview: None,
            zoom: 1,
            export: ExportPanel {
                grid: false,
                columns: 4,
                scale: 1,
                message: None,
            },
            r#gen: GenPanel {
                provider: "http".to_string(),
                base_url: "http://localhost:11434/v1".to_string(),
                model: String::new(),
                models: Vec::new(),
                opencode_models: Vec::new(),
                card_hash: None,
                name: String::new(),
                name_edit: rune::TextEdit::new(""),
                is_sprite: false,
                sprite_w: 16,
                sprite_h: 16,
                tile_w: 8,
                tile_h: 8,
                tile_count: 3,
                state: RunState::Idle,
                events: Vec::new(),
                note: None,
                used_fallback: false,
                pending: None,
            },
            gen_tx: None,
        };
        if conjure_store::locate(root).is_ok() {
            state.screen = Screen::Browse;
            crate::store_io::refresh(&mut state);
        }
        state
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use conjure_store::Store;

    use super::*;

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
            "conjure-app-state-test-{}-{nanos}-{label}",
            std::process::id()
        )))
    }

    #[test]
    fn init_from_without_a_store_lands_on_no_library() {
        let bare = temp_root("bare");
        std::fs::create_dir_all(&bare.0).unwrap();

        let state = AppState::init_from(&bare.0);

        assert!(matches!(state.screen, Screen::NoLibrary));
        assert_eq!(state.root, bare.0);
        assert!(state.cards.is_empty());
        assert!(state.assets.is_empty());
        assert!(state.gen_tx.is_none());
    }

    #[test]
    fn init_from_with_a_store_lands_on_browse() {
        let root = temp_root("store");
        Store::create(&root.0).unwrap();

        let state = AppState::init_from(&root.0);

        assert!(matches!(state.screen, Screen::Browse));
        assert!(state.cards.is_empty());
        assert!(state.assets.is_empty());
    }

    #[test]
    fn defaults_match_the_panel_contract() {
        let bare = temp_root("defaults");
        std::fs::create_dir_all(&bare.0).unwrap();
        let state = AppState::init_from(&bare.0);

        assert_eq!(state.zoom, 1);
        assert_eq!(state.r#gen.provider, "http");
        assert_eq!(state.r#gen.base_url, "http://localhost:11434/v1");
        assert_eq!(state.r#gen.model, "");
        assert!(!state.r#gen.is_sprite);
        assert_eq!((state.r#gen.sprite_w, state.r#gen.sprite_h), (16, 16));
        assert_eq!((state.r#gen.tile_w, state.r#gen.tile_h), (8, 8));
        assert_eq!(state.r#gen.tile_count, 3);
        assert_eq!(state.r#gen.state, RunState::Idle);
        assert!(state.r#gen.events.is_empty());
        assert_eq!(state.r#gen.note, None);
        assert!(!state.r#gen.used_fallback);
        assert!(state.r#gen.pending.is_none());
        assert!(!state.export.grid);
        assert_eq!(state.export.scale, 1);
        assert_eq!(state.export.message, None);
    }
}
