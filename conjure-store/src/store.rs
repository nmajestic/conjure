use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use conjure_core::{Asset, AssetKind, AssetRequest, StyleCard, ValidationReport, verify_request};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StoreError;
use crate::hash::{asset_hash, blob_hash, card_hash};

pub type RecordId = String;

pub struct Store {
    root: PathBuf,
    conn: Connection,
}

pub struct CardSummary {
    pub hash: String,
    pub id: String,
    pub name: String,
    pub saved_at: i64,
}

pub struct AssetSummary {
    pub hash: String,
    pub style_card_hash: String,
    pub name: String,
    pub kind: String,
    pub width: u32,
    pub height: u32,
    pub saved_at: i64,
    pub tags: Vec<String>,
}

pub struct ListFilter {
    pub tag: Option<String>,
}

pub struct GcReport {
    pub removed: usize,
    pub kept: usize,
}

#[derive(Debug, Clone)]
pub struct Verification(pub ValidationReport);

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

fn card_json(card: &StyleCard) -> Result<String, StoreError> {
    serde_json::to_string(card)
        .map_err(|e| StoreError::Invalid(format!("cannot serialize style card: {e}")))
}

fn kind_label(kind: AssetKind) -> &'static str {
    match kind {
        AssetKind::Sprite { .. } => "sprite",
        AssetKind::Tileset { .. } => "tile",
    }
}

fn is_hex_prefix(prefix: &str) -> bool {
    (8..=64).contains(&prefix.len()) && prefix.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Store {
    pub fn create(project_root: &Path) -> Result<Self, StoreError> {
        std::fs::create_dir_all(project_root.join(".conjure").join("blobs"))?;
        let conn = Connection::open(project_root.join(".conjure").join("conjure.sqlite"))?;
        let mut store = Store {
            root: project_root.to_path_buf(),
            conn,
        };
        store.setup()?;
        Ok(store)
    }

    pub fn open(project_root: &Path) -> Result<Self, StoreError> {
        let conjure_dir = project_root.join(".conjure");
        if !conjure_dir.exists() {
            return Err(StoreError::NotFound(
                "no .conjure/ found (run conjure init)".into(),
            ));
        }
        if !conjure_dir.join("conjure.sqlite").exists() {
            return Err(StoreError::Invalid(
                "store not initialized (run conjure init)".into(),
            ));
        }
        let conn = Connection::open(conjure_dir.join("conjure.sqlite"))?;
        let mut store = Store {
            root: project_root.to_path_buf(),
            conn,
        };
        store.setup()?;
        Ok(store)
    }

    fn setup(&mut self) -> Result<(), StoreError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS style_cards(hash TEXT PRIMARY KEY, id TEXT NOT NULL, name TEXT NOT NULL, json TEXT NOT NULL, saved_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS assets(hash TEXT PRIMARY KEY, style_card_hash TEXT NOT NULL REFERENCES style_cards(hash), name TEXT NOT NULL, kind TEXT NOT NULL, width INTEGER NOT NULL, height INTEGER NOT NULL, blob_hash TEXT NOT NULL, saved_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS tags(asset_hash TEXT NOT NULL REFERENCES assets(hash), tag TEXT NOT NULL, PRIMARY KEY(asset_hash, tag));",
        )?;
        Ok(())
    }

    pub fn save_card(&self, card: &StyleCard) -> Result<RecordId, StoreError> {
        card.validate().map_err(StoreError::Invalid)?;
        let hash = card_hash(card)?;
        let json = card_json(card)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO style_cards(hash, id, name, json, saved_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![hash, card.id, card.name, json, now_secs()],
        )?;
        Ok(hash)
    }

    pub fn save_assets(
        &self,
        request: &AssetRequest,
        assets: &[Asset],
    ) -> Result<Vec<RecordId>, StoreError> {
        request.style_card.validate().map_err(StoreError::Invalid)?;
        let report = verify_request(request, assets);
        if !report.passed {
            return Err(StoreError::Verify(Verification(report)));
        }
        for (i, asset) in assets.iter().enumerate() {
            if asset.style_card_id != request.style_card.id {
                return Err(StoreError::Invalid(format!(
                    "asset {i} references style card {}, expected {}",
                    asset.style_card_id, request.style_card.id
                )));
            }
        }
        let style_hash = card_hash(&request.style_card)?;
        let json = card_json(&request.style_card)?;
        let blobs_dir = self.root.join(".conjure").join("blobs");
        let mut asset_hashes = Vec::with_capacity(assets.len());
        let mut blob_hashes = Vec::with_capacity(assets.len());
        // Blobs are written before the transaction so an interrupted save can
        // only leave orphan files (gc-able), never rows referencing missing
        // blobs.
        for asset in assets {
            let asset_hash_value = asset_hash(
                &request.style_card.id,
                asset.width,
                asset.height,
                &asset.pixels,
            );
            let blob_hash_value = blob_hash(&asset.pixels);
            let blob_path = blobs_dir.join(&blob_hash_value);
            if !blob_path.exists() {
                std::fs::write(blob_path, asset.pixels.as_slice())?;
            }
            asset_hashes.push(asset_hash_value);
            blob_hashes.push(blob_hash_value);
        }
        // Store hands out &self only, so unchecked_transaction is the only way
        // to open a transaction without an exclusive connection borrow.
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO style_cards(hash, id, name, json, saved_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                style_hash,
                request.style_card.id,
                request.style_card.name,
                json,
                now_secs()
            ],
        )?;
        for (i, asset) in assets.iter().enumerate() {
            let name = match request.kind {
                AssetKind::Sprite { .. } => request.name.clone(),
                AssetKind::Tileset { .. } => format!("{}#{i}", request.name),
            };
            tx.execute(
                "INSERT OR IGNORE INTO assets(hash, style_card_hash, name, kind, width, height, blob_hash, saved_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    asset_hashes[i],
                    style_hash,
                    name,
                    kind_label(request.kind),
                    asset.width,
                    asset.height,
                    blob_hashes[i],
                    now_secs()
                ],
            )?;
        }
        tx.commit()?;
        Ok(asset_hashes)
    }

    pub fn load_card(&self, hash: &str) -> Result<StyleCard, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT id, json FROM style_cards WHERE hash = ?1",
                params![hash],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((row_id, json)) = row else {
            return Err(StoreError::NotFound(format!("no style card {hash}")));
        };
        let card: StyleCard = serde_json::from_str(&json)
            .map_err(|e| StoreError::Invalid(format!("cannot deserialize style card: {e}")))?;
        let recomputed = card_hash(&card)?;
        if recomputed != hash {
            return Err(StoreError::Corrupt {
                path: format!(".conjure/conjure.sqlite#style_cards:{hash}"),
                expected: hash.to_string(),
                actual: recomputed,
            });
        }
        if card.id != row_id {
            return Err(StoreError::Corrupt {
                path: format!(".conjure/conjure.sqlite#style_cards:{hash}"),
                expected: row_id,
                actual: card.id,
            });
        }
        Ok(card)
    }

    pub fn load_asset(&self, hash: &str) -> Result<Asset, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT a.style_card_hash, c.id, a.width, a.height, a.blob_hash \
                 FROM assets a JOIN style_cards c ON a.style_card_hash = c.hash \
                 WHERE a.hash = ?1",
                params![hash],
                |row| {
                    Ok((
                        row.get::<_, String>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        let Some((card_id, width, height, stored_blob_hash)) = row else {
            return Err(StoreError::NotFound(format!("no asset {hash}")));
        };
        let blob_path = self.blob_path(&stored_blob_hash);
        let blob_str = blob_path.to_string_lossy().into_owned();
        let pixels = match std::fs::read(blob_path) {
            Ok(pixels) => pixels,
            Err(_) => {
                return Err(StoreError::Corrupt {
                    path: blob_str,
                    expected: stored_blob_hash,
                    actual: "unreadable".to_string(),
                });
            }
        };
        let recomputed = asset_hash(&card_id, width, height, &pixels);
        if recomputed != hash {
            return Err(StoreError::Corrupt {
                path: blob_str.clone(),
                expected: hash.to_string(),
                actual: recomputed,
            });
        }
        let recomputed_blob = blob_hash(&pixels);
        if recomputed_blob != stored_blob_hash {
            return Err(StoreError::Corrupt {
                path: blob_str,
                expected: stored_blob_hash,
                actual: recomputed_blob,
            });
        }
        Ok(Asset {
            style_card_id: card_id,
            width,
            height,
            pixels,
        })
    }

    pub fn list_cards(&self) -> Result<Vec<CardSummary>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash, id, name, saved_at FROM style_cards ORDER BY id, hash")?;
        let cards = stmt
            .query_map([], |row| {
                Ok(CardSummary {
                    hash: row.get(0)?,
                    id: row.get(1)?,
                    name: row.get(2)?,
                    saved_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(cards)
    }

    fn summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssetSummary> {
        Ok(AssetSummary {
            hash: row.get(0)?,
            style_card_hash: row.get(1)?,
            name: row.get(2)?,
            kind: row.get(3)?,
            width: row.get(4)?,
            height: row.get(5)?,
            saved_at: row.get(6)?,
            tags: Vec::new(),
        })
    }

    fn tags_for(&self, hash: &str) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT tag FROM tags WHERE asset_hash = ?1 ORDER BY tag")?;
        let tags = stmt
            .query_map(params![hash], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tags)
    }

    pub fn list_assets(&self, filter: &ListFilter) -> Result<Vec<AssetSummary>, StoreError> {
        let sql = match filter.tag {
            Some(_) => {
                "SELECT hash, style_card_hash, name, kind, width, height, saved_at FROM assets \
                 WHERE hash IN (SELECT asset_hash FROM tags WHERE tag = ?1) ORDER BY name, hash"
            }
            None => {
                "SELECT hash, style_card_hash, name, kind, width, height, saved_at FROM assets \
                 ORDER BY name, hash"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let mut summaries = match &filter.tag {
            Some(tag) => stmt
                .query_map(params![tag], Self::summary_row)?
                .collect::<Result<Vec<_>, _>>()?,
            None => stmt
                .query_map([], Self::summary_row)?
                .collect::<Result<Vec<_>, _>>()?,
        };
        for summary in &mut summaries {
            summary.tags = self.tags_for(&summary.hash)?;
        }
        Ok(summaries)
    }

    pub fn asset_summary(&self, hash: &str) -> Result<AssetSummary, StoreError> {
        let row = self
            .conn
            .query_row(
                "SELECT hash, style_card_hash, name, kind, width, height, saved_at FROM assets \
                 WHERE hash = ?1",
                params![hash],
                Self::summary_row,
            )
            .optional()?;
        let Some(mut summary) = row else {
            return Err(StoreError::NotFound(format!("no asset {hash}")));
        };
        summary.tags = self.tags_for(hash)?;
        Ok(summary)
    }

    pub fn tag(&self, hash: &str, tag: &str) -> Result<(), StoreError> {
        let exists = self
            .conn
            .query_row(
                "SELECT 1 FROM assets WHERE hash = ?1",
                params![hash],
                |_| Ok(()),
            )
            .optional()?;
        if exists.is_none() {
            return Err(StoreError::NotFound(format!("no asset {hash}")));
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO tags(asset_hash, tag) VALUES (?1, ?2)",
            params![hash, tag],
        )?;
        Ok(())
    }

    pub fn gc(&self) -> Result<GcReport, StoreError> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT blob_hash FROM assets")?;
        let referenced: HashSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<HashSet<_>, _>>()?;
        let mut report = GcReport {
            removed: 0,
            kept: 0,
        };
        for entry in std::fs::read_dir(self.blob_dir())? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if referenced.contains(&name) {
                report.kept += 1;
            } else {
                std::fs::remove_file(entry.path())?;
                report.removed += 1;
            }
        }
        Ok(report)
    }

    pub fn resolve_card(&self, prefix: &str) -> Result<String, StoreError> {
        self.resolve_prefix("style_cards", prefix)
    }

    pub fn resolve_asset(&self, prefix: &str) -> Result<String, StoreError> {
        self.resolve_prefix("assets", prefix)
    }

    fn resolve_prefix(&self, table: &str, prefix: &str) -> Result<String, StoreError> {
        if !is_hex_prefix(prefix) {
            return Err(StoreError::Invalid(
                "hash prefix must be at least 8 hex characters".into(),
            ));
        }
        let prefix = prefix.to_ascii_lowercase();
        let sql = format!("SELECT hash FROM {table} WHERE hash LIKE ?1||'%'");
        let mut stmt = self.conn.prepare(&sql)?;
        let matches = stmt
            .query_map(params![prefix], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        match matches.len() {
            0 => Err(StoreError::NotFound(format!(
                "no match for hash prefix {prefix}"
            ))),
            1 => Ok(matches.into_iter().next().unwrap_or_default()),
            n => Err(StoreError::Invalid(format!(
                "ambiguous hash prefix {prefix}: {n} matches"
            ))),
        }
    }

    fn blob_dir(&self) -> PathBuf {
        self.root.join(".conjure").join("blobs")
    }

    fn blob_path(&self, blob_hash: &str) -> PathBuf {
        self.blob_dir().join(blob_hash)
    }
}

#[cfg(test)]
mod tests {
    use conjure_core::{Dimensions, Grid, Outline, PaletteColor, Shading, Style};

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
            "conjure-store-test-{}-{nanos}-{label}",
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

    fn sprite_request(width: u32, height: u32) -> AssetRequest {
        AssetRequest {
            name: "hero".into(),
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
    fn create_builds_schema_and_enforces_foreign_keys() {
        let root = temp_root("schema");
        let store = Store::create(&root.0).unwrap();

        let mut stmt = store
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        for table in ["assets", "style_cards", "tags"] {
            assert!(
                tables.iter().any(|name| name == table),
                "missing table {table}"
            );
        }

        let foreign_keys: i64 = store
            .conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(foreign_keys, 1);

        let bad_insert = store.conn.execute(
            "INSERT INTO assets(hash, style_card_hash, name, kind, width, height, blob_hash, saved_at) \
             VALUES ('deadbeef', 'unknown-card', 'x', 'sprite', 1, 1, 'deadbeef', 0)",
            [],
        );
        assert!(bad_insert.is_err());
    }

    #[test]
    fn failed_verification_writes_nothing() {
        let root = temp_root("verify-fail");
        let store = Store::create(&root.0).unwrap();
        let request = tileset_request(3);
        let one_tile = tiles(1);

        let err = store.save_assets(&request, &one_tile).unwrap_err();
        let StoreError::Verify(report) = err else {
            panic!("expected a verification failure");
        };
        assert!(!report.0.passed);

        let assets: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(assets, 0);
        assert_eq!(blob_count(&root.0), 0);
    }

    #[test]
    fn resaving_leaves_rows_and_blobs_untouched() {
        let root = temp_root("resave");
        let store = Store::create(&root.0).unwrap();
        let request = sprite_request(2, 2);
        let assets = vec![asset(2, 2, vec![0, 1, 1, 0])];

        let hashes = store.save_assets(&request, &assets).unwrap();
        let card_saved_at: i64 = store
            .conn
            .query_row("SELECT saved_at FROM style_cards", [], |row| row.get(0))
            .unwrap();
        let asset_saved_at: i64 = store
            .conn
            .query_row("SELECT saved_at FROM assets", [], |row| row.get(0))
            .unwrap();

        let resaved = store.save_assets(&request, &assets).unwrap();
        assert_eq!(resaved, hashes);
        assert_eq!(blob_count(&root.0), 1);

        let card_saved_again: i64 = store
            .conn
            .query_row("SELECT saved_at FROM style_cards", [], |row| row.get(0))
            .unwrap();
        assert_eq!(card_saved_again, card_saved_at);
        let asset_saved_again: i64 = store
            .conn
            .query_row("SELECT saved_at FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(asset_saved_again, asset_saved_at);
    }

    #[test]
    fn saved_cards_and_assets_round_trip() {
        let root = temp_root("roundtrip");
        let store = Store::create(&root.0).unwrap();
        let card = test_card();
        let hash = store.save_card(&card).unwrap();
        let request = sprite_request(2, 2);
        let original = asset(2, 2, vec![0, 1, 1, 0]);
        let hashes = store
            .save_assets(&request, std::slice::from_ref(&original))
            .unwrap();

        assert_eq!(store.load_card(&hash).unwrap(), card);
        assert_eq!(store.load_asset(&hashes[0]).unwrap(), original);
    }

    #[test]
    fn identical_pixels_at_different_dimensions_share_one_blob() {
        let root = temp_root("shared-blob");
        let store = Store::create(&root.0).unwrap();
        let pixels = vec![0, 1, 1, 0];

        let first = store
            .save_assets(&sprite_request(2, 2), &[asset(2, 2, pixels.clone())])
            .unwrap();
        let second = store
            .save_assets(&sprite_request(4, 1), &[asset(4, 1, pixels.clone())])
            .unwrap();
        assert_ne!(first[0], second[0]);

        let assets: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(assets, 2);
        assert_eq!(blob_count(&root.0), 1);
    }

    #[test]
    fn flipped_blob_byte_is_reported_as_corrupt() {
        let root = temp_root("corrupt-blob");
        let store = Store::create(&root.0).unwrap();
        let pixels = vec![0, 1, 1, 0];
        let hashes = store
            .save_assets(&sprite_request(2, 2), &[asset(2, 2, pixels.clone())])
            .unwrap();

        let mut flipped = pixels.clone();
        flipped[0] = 3;
        let blob_path = root
            .0
            .join(".conjure")
            .join("blobs")
            .join(blob_hash(&pixels));
        std::fs::write(blob_path, flipped.as_slice()).unwrap();

        let err = store.load_asset(&hashes[0]).unwrap_err();
        let StoreError::Corrupt {
            expected, actual, ..
        } = err
        else {
            panic!("expected a corrupt blob report");
        };
        assert_eq!(expected, hashes[0]);
        assert_eq!(actual, asset_hash("card-1", 2, 2, &flipped));
    }

    #[test]
    fn gc_removes_only_unreferenced_blobs() {
        let root = temp_root("gc");
        let store = Store::create(&root.0).unwrap();
        let pixels = vec![0, 1, 1, 0];
        store
            .save_assets(&sprite_request(2, 2), &[asset(2, 2, pixels.clone())])
            .unwrap();

        let blobs = root.0.join(".conjure").join("blobs");
        std::fs::write(blobs.join("unreferenced-one"), b"junk").unwrap();
        std::fs::write(blobs.join("unreferenced-two"), b"junk").unwrap();

        let report = store.gc().unwrap();
        assert_eq!(report.removed, 2);
        assert_eq!(report.kept, 1);
        assert_eq!(blob_count(&root.0), 1);
    }

    #[test]
    fn resolve_rejects_short_unknown_and_ambiguous_prefixes() {
        let root = temp_root("resolve");
        let store = Store::create(&root.0).unwrap();

        let short = store.resolve_asset("1234567").unwrap_err();
        assert!(
            matches!(short, StoreError::Invalid(msg) if msg == "hash prefix must be at least 8 hex characters")
        );
        let unknown = store.resolve_asset("12341234").unwrap_err();
        assert!(
            matches!(unknown, StoreError::NotFound(msg) if msg == "no match for hash prefix 12341234")
        );

        let card_row_hash = "f".repeat(64);
        store
            .conn
            .execute(
                "INSERT INTO style_cards(hash, id, name, json, saved_at) VALUES (?1, 'x', 'x', '{}', 0)",
                params![card_row_hash],
            )
            .unwrap();
        let ambiguous_first = "a".repeat(64);
        let ambiguous_second = format!("{}b", "a".repeat(63));
        for hash in [&ambiguous_first, &ambiguous_second] {
            store
                .conn
                .execute(
                    "INSERT INTO assets(hash, style_card_hash, name, kind, width, height, blob_hash, saved_at) \
                     VALUES (?1, ?2, 'x', 'sprite', 1, 1, 'x', 0)",
                    params![hash, card_row_hash],
                )
                .unwrap();
        }

        let ambiguous = store.resolve_asset("aaaaaaaa").unwrap_err();
        assert!(matches!(ambiguous, StoreError::Invalid(msg) if msg.contains("ambiguous")));
        assert_eq!(
            store.resolve_asset(&ambiguous_first).unwrap(),
            ambiguous_first
        );
        assert_eq!(
            store
                .resolve_asset(&ambiguous_first.to_ascii_uppercase())
                .unwrap(),
            ambiguous_first
        );
        assert_eq!(store.resolve_card(&card_row_hash).unwrap(), card_row_hash);
    }

    #[test]
    fn mismatched_style_card_id_writes_nothing() {
        let root = temp_root("wrong-card-id");
        let store = Store::create(&root.0).unwrap();
        let mut stray = asset(2, 2, vec![0, 1, 1, 0]);
        stray.style_card_id = "other-card".into();

        let err = store
            .save_assets(&sprite_request(2, 2), &[stray])
            .unwrap_err();
        assert!(matches!(err, StoreError::Invalid(msg) if msg.contains("references style card")));

        let assets: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM assets", [], |row| row.get(0))
            .unwrap();
        assert_eq!(assets, 0);
        let cards: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM style_cards", [], |row| row.get(0))
            .unwrap();
        assert_eq!(cards, 0);
        assert_eq!(blob_count(&root.0), 0);
    }

    #[test]
    fn tileset_rows_get_indexed_names_and_tile_kind() {
        let root = temp_root("tileset-names");
        let store = Store::create(&root.0).unwrap();
        let hashes = store.save_assets(&tileset_request(3), &tiles(3)).unwrap();

        let summaries = store.list_assets(&ListFilter { tag: None }).unwrap();
        assert_eq!(summaries.len(), 3);
        let names: Vec<&str> = summaries.iter().map(|s| s.name.as_str()).collect();
        for name in ["floor#0", "floor#1", "floor#2"] {
            assert!(names.contains(&name));
        }
        for summary in &summaries {
            assert_eq!(summary.kind, "tile");
            assert_eq!(store.asset_summary(&summary.hash).unwrap().kind, "tile");
        }
        assert_eq!(hashes.len(), 3);
    }

    #[test]
    fn sprite_row_gets_request_name_and_sprite_kind() {
        let root = temp_root("sprite-name");
        let store = Store::create(&root.0).unwrap();
        let hashes = store
            .save_assets(&sprite_request(2, 2), &[asset(2, 2, vec![0, 1, 1, 0])])
            .unwrap();

        let summary = store.asset_summary(&hashes[0]).unwrap();
        assert_eq!(summary.name, "hero");
        assert_eq!(summary.kind, "sprite");
    }

    #[test]
    fn stores_in_separate_roots_are_isolated() {
        let root_a = temp_root("iso-a");
        let root_b = temp_root("iso-b");
        let store_a = Store::create(&root_a.0).unwrap();
        let store_b = Store::create(&root_b.0).unwrap();
        let hashes = store_a
            .save_assets(&sprite_request(2, 2), &[asset(2, 2, vec![0, 1, 1, 0])])
            .unwrap();

        assert!(
            store_b
                .list_assets(&ListFilter { tag: None })
                .unwrap()
                .is_empty()
        );
        assert!(
            matches!(store_b.load_asset(&hashes[0]), Err(StoreError::NotFound(msg)) if msg == format!("no asset {}", hashes[0]))
        );
    }

    #[test]
    fn tampered_card_json_is_corrupt_and_unknown_asset_not_found() {
        let root = temp_root("tamper-json");
        let store = Store::create(&root.0).unwrap();
        let hash = store.save_card(&test_card()).unwrap();

        let mut other = test_card();
        other.id = "card-2".into();
        let tampered_json = serde_json::to_string(&other).unwrap();
        store
            .conn
            .execute(
                "UPDATE style_cards SET json = ?1 WHERE hash = ?2",
                params![tampered_json, hash],
            )
            .unwrap();

        assert!(matches!(
            store.load_card(&hash),
            Err(StoreError::Corrupt { .. })
        ));

        let unknown = "1234567890abcdef".repeat(4);
        assert!(
            matches!(store.load_asset(&unknown), Err(StoreError::NotFound(msg)) if msg == format!("no asset {unknown}"))
        );
    }

    #[test]
    fn tags_filter_assets_and_survive_repeats() {
        let root = temp_root("tags");
        let store = Store::create(&root.0).unwrap();
        let first = store
            .save_assets(&sprite_request(2, 2), &[asset(2, 2, vec![0, 1, 1, 0])])
            .unwrap();
        let mut second_request = sprite_request(2, 2);
        second_request.name = "sidekick".into();
        let second = store
            .save_assets(&second_request, &[asset(2, 2, vec![1, 0, 0, 1])])
            .unwrap();

        let missing = store.tag("ffffffff", "favorite").unwrap_err();
        assert!(matches!(missing, StoreError::NotFound(msg) if msg == "no asset ffffffff"));

        store.tag(&first[0], "favorite").unwrap();
        store.tag(&first[0], "favorite").unwrap();

        let tagged = store
            .list_assets(&ListFilter {
                tag: Some("favorite".into()),
            })
            .unwrap();
        assert_eq!(tagged.len(), 1);
        assert_eq!(tagged[0].hash, first[0]);

        let summary = store.asset_summary(&first[0]).unwrap();
        assert_eq!(summary.tags, vec!["favorite".to_string()]);

        let all = store.list_assets(&ListFilter { tag: None }).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|s| s.hash == second[0]));
    }
}
