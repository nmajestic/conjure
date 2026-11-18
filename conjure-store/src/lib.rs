pub mod error;
pub mod hash;
pub mod store;

pub use error::StoreError;
pub use hash::{asset_hash, blob_hash, card_hash};
pub use store::{AssetSummary, CardSummary, GcReport, ListFilter, RecordId, Store, Verification};

pub fn locate(start: &std::path::Path) -> Result<std::path::PathBuf, StoreError> {
    for dir in start.ancestors() {
        if dir.join(".conjure").exists() {
            return Ok(dir.to_path_buf());
        }
    }
    Err(StoreError::NotFound(
        "no .conjure/ found (run conjure init)".into(),
    ))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn temp_dir(label: &str) -> TempDir {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0);
        TempDir(std::env::temp_dir().join(format!(
            "conjure-store-test-{}-{nanos}-{label}",
            std::process::id()
        )))
    }

    #[test]
    fn locate_finds_nearest_conjure_dir_from_descendant() {
        let base = temp_dir("locate");
        std::fs::create_dir_all(base.0.join(".conjure")).unwrap();

        let found = locate(&base.0.join("a").join("b").join("c")).unwrap();
        assert_eq!(found, base.0);
    }

    #[test]
    fn locate_without_conjure_dir_is_not_found() {
        let bare = temp_dir("bare");
        std::fs::create_dir_all(&bare.0).unwrap();

        let err = locate(&bare.0).unwrap_err();
        assert!(
            matches!(err, StoreError::NotFound(msg) if msg == "no .conjure/ found (run conjure init)")
        );
    }
}
