#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("corrupt blob {path}: expected hash {expected}, found {actual}")]
    Corrupt {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("verification failed")]
    Verify(crate::store::Verification),
}

// ValidationReport does not implement std::error::Error, so thiserror's
// #[from] cannot wrap it; the manual conversion keeps `?` working.
impl From<crate::store::Verification> for StoreError {
    fn from(verification: crate::store::Verification) -> Self {
        StoreError::Verify(verification)
    }
}

#[cfg(test)]
mod tests {
    use conjure_core::ValidationReport;
    use rusqlite::Connection;

    use super::StoreError;
    use crate::store::Verification;

    #[test]
    fn variants_render_exact_messages() {
        assert_eq!(
            format!("{}", StoreError::NotFound("thing".into())),
            "not found: thing"
        );
        assert_eq!(
            format!("{}", StoreError::Invalid("bad".into())),
            "invalid: bad"
        );
        assert_eq!(
            format!(
                "{}",
                StoreError::Corrupt {
                    path: ".conjure/blobs/dead".into(),
                    expected: "aaaa".into(),
                    actual: "bbbb".into(),
                }
            ),
            "corrupt blob .conjure/blobs/dead: expected hash aaaa, found bbbb"
        );
        assert_eq!(
            format!(
                "{}",
                StoreError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "nope"))
            ),
            "io: nope"
        );
        let conn = Connection::open_in_memory().unwrap();
        let sqlite_error = conn.prepare("SELECT * FROM nope").unwrap_err();
        assert_eq!(
            format!("{}", StoreError::Sqlite(sqlite_error)),
            "sqlite: no such table: nope"
        );
        assert_eq!(
            format!(
                "{}",
                StoreError::Verify(Verification(ValidationReport::ok()))
            ),
            "verification failed"
        );
    }

    #[test]
    fn verification_converts_into_store_error() {
        let error: StoreError = Verification(ValidationReport::ok()).into();
        assert!(matches!(error, StoreError::Verify(_)));
    }
}
