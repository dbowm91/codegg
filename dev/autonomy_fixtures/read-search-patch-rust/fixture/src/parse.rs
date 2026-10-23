//! Turning lexed fields into a parsed [`Record`].

use crate::lex::{split_raw_fields, trim_field};

/// A parsed record: the ordered fields of one input line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The record's fields, in source order.
    pub fields: Vec<String>,
}

impl Record {
    /// The number of fields in the record.
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether the record carries no field at all.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
}

/// Parse one delimited record line into a [`Record`].
///
/// Every field is significant: a trailing empty field is a real empty field
/// and is preserved, and the empty record `""` is exactly one empty field.
pub fn parse_record(line: &str) -> Record {
    let raw = split_raw_fields(line);
    let mut fields: Vec<String> = raw
        .iter()
        .map(|field| trim_field(field).to_string())
        .collect();
    settle(&mut fields);
    Record { fields }
}

/// Fold the record's trailing fields into the last significant one.
fn settle(fields: &mut Vec<String>) {
    while let Some(last) = fields.last() {
        if !last.is_empty() {
            break;
        }
        fields.pop();
    }
}
