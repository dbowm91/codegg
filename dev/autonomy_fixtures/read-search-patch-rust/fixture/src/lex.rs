//! Raw lexing: split a record line into its raw fields.
//!
//! The lexer is deliberately literal. It knows one delimiter and it keeps
//! every field the line contains, empty ones included, because the number of
//! fields in a record is part of the record. Normalizing a field is a
//! separate step, and it belongs to the parser.

/// The delimiter that separates the fields of a record line.
pub const SEPARATOR: char = ',';

/// Split `line` into its raw fields.
///
/// Every delimiter is a field boundary, so `"a,,b"` has three fields and the
/// middle one is empty. A trailing delimiter means the line ends with an
/// empty field, and this function keeps it.
pub fn split_raw_fields(line: &str) -> Vec<&str> {
    line.split(SEPARATOR).collect()
}

/// Trim the whitespace that surrounds a single raw field.
pub fn trim_field(field: &str) -> &str {
    field.trim()
}
