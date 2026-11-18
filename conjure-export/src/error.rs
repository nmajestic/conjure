#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("png encode: {0}")]
    Encode(#[from] png::EncodingError),
    #[error("png decode: {0}")]
    Decode(#[from] png::DecodingError),
}

#[cfg(test)]
mod tests {
    use super::*;

    // U8.1: each variant's Display string is exact.
    #[test]
    fn display_strings_are_exact() {
        let invalid = ExportError::Invalid("boom".to_string());
        assert_eq!(invalid.to_string(), "invalid: boom");

        let io = ExportError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "nope"));
        assert_eq!(io.to_string(), "io: nope");

        let encode = ExportError::Encode(png::EncodingError::IoError(std::io::Error::other(
            "enc fail",
        )));
        assert_eq!(encode.to_string(), "png encode: enc fail");

        let decode = ExportError::Decode(png::DecodingError::IoError(std::io::Error::other(
            "dec fail",
        )));
        assert_eq!(decode.to_string(), "png decode: dec fail");
    }
}
