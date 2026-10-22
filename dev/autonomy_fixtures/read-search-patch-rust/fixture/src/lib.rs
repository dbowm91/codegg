//! A tiny crate that parses delimited record lines.
//!
//! The crate is split in two modules: [`lex`] turns a raw line into its raw
//! fields, and [`parse`] turns those fields into a [`Record`].
//!
//! The record format is simple but strict: **every field is significant**.
//! A trailing empty field is a real empty field and is kept, and the empty
//! record `""` is exactly one empty field. The field count is part of the
//! record, so a consumer can tell `"a,b"` from `"a,b,"`.
//!
//! ```text
//! "a,b,c"  ->  ["a", "b", "c"]
//! "a,b,"   ->  ["a", "b", ""]
//! ""       ->  [""]
//! ```

pub mod lex;
pub mod parse;

pub use lex::{split_raw_fields, trim_field};
pub use parse::{parse_record, Record};
