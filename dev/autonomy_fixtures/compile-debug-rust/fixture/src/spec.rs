//! Parsing of textual range specifications.

use crate::range::Range;

/// The error returned when a specification cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpecError {
    /// The text is not a well-formed specification.
    Malformed(String),
}

/// A parsed specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    range: Range,
}

impl Spec {
    /// The range this specification names.
    pub fn range(&self) -> &Range {
        &self.range
    }
}

/// Parses a specification of the form `<lower>..<upper>`.
///
/// # Contract
///
/// * The two fields are separated by exactly one `..` separator.
/// * Each field is a **non-negative** decimal integer. A leading `-` or `+`
///   sign, surrounding whitespace, or any non-digit character makes the
///   specification malformed and **must be rejected**.
/// * The lower bound must not be greater than the upper bound.
pub fn parse_spec(text: &str) -> Result<Spec, SpecError> {
    let (low_text, high_text) = text
        .split_once("..")
        .ok_or_else(|| SpecError::Malformed(text.to_string()))?;
    let low: i64 = low_text
        .parse()
        .map_err(|_| SpecError::Malformed(text.to_string()))?;
    let high: i64 = high_text
        .parse()
        .map_err(|_| SpecError::Malformed(text.to_string()))?;
    if low > high {
        return Err(SpecError::Malformed(text.to_string()));
    }
    Ok(Spec {
        range: Range::new(low, high),
    })
}
