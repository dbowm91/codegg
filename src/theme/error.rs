//! Theme-related error types.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ThemeError {
    #[error("invalid color '{value}': {reason}")]
    InvalidColor { value: String, reason: String },

    #[error("invalid hex: {0}")]
    InvalidHex(String),

    #[error("toml parse error: {0}")]
    TomlParse(String),

    #[error("unsupported theme format: {0}")]
    UnsupportedFormat(String),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("theme error: {0}")]
    Other(String),
}
