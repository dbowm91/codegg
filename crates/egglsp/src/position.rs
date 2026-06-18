//! Position encoding conversion helpers shared across LSP operations.
//!
//! LSP `Position::character` offsets use a *negotiated* position
//! encoding. Per the LSP specification, the encoding is negotiated
//! during the `initialize` handshake via the
//! `PositionEncodingKind` capability (`utf-8`, `utf-16`, or
//! `utf-32`). The vast majority of servers negotiate UTF-16
//! because that is what the protocol was originally designed
//! for (and what VS Code negotiates by default).
//!
//! Rust source code is UTF-8. Converting a UTF-16 offset into
//! a Rust byte offset requires walking the string one character
//! at a time and accumulating `len_utf16()` until the requested
//! unit count is reached. The helper in this module is the
//! canonical implementation; tests live next to it so a
//! regression in any branch is surfaced immediately.

use serde::{Deserialize, Serialize};

/// The set of position encodings the LSP protocol supports.
///
/// Servers negotiate one of these during `initialize`. The
/// crate-wide default is [`PositionEncoding::Utf16`] because
/// every mainstream LSP server (rust-analyzer, basedpyright,
/// gopls, typescript-language-server, clangd) negotiates UTF-16
/// unless the client requests otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionEncoding {
    /// One character == one byte. ASCII-only files use this
    /// implicitly, but the encoding is rare in practice.
    Utf8,
    /// One character == one UTF-16 code unit. BMP code points
    /// are 1 unit; supplementary plane code points are 2 units
    /// (a surrogate pair). The protocol default.
    Utf16,
    /// One character == one UTF-32 code unit (one Unicode
    /// scalar value). Effectively a 1:1 mapping with Rust
    /// `char` indexing.
    Utf32,
}

impl Default for PositionEncoding {
    fn default() -> Self {
        PositionEncoding::Utf16
    }
}

impl PositionEncoding {
    /// Stable string form used by reports and diagnostics. The
    /// values match the LSP `PositionEncodingKind` literal so
    /// they round-trip cleanly through server-reported
    /// capabilities.
    pub fn as_str(&self) -> &'static str {
        match self {
            PositionEncoding::Utf8 => "utf-8",
            PositionEncoding::Utf16 => "utf-16",
            PositionEncoding::Utf32 => "utf-32",
        }
    }
}

/// Convert an LSP position offset expressed in `units` of the
/// given `encoding` to a Rust byte offset within `text`.
///
/// Returns `None` when:
/// - the offset exceeds the string's length in the requested
///   encoding units, or
/// - the offset does not land on a character boundary.
///
/// The function intentionally walks the string one Unicode
/// scalar value at a time and accumulates each character's
/// encoding width, so the conversion is exact for BMP and
/// supplementary-plane code points alike.
///
/// `units` is interpreted as a *non-negative* offset; negative
/// values are treated as out of bounds.
pub fn lsp_units_to_byte_offset(
    text: &str,
    units: u32,
    encoding: PositionEncoding,
) -> Option<usize> {
    match encoding {
        PositionEncoding::Utf8 => lsp_utf8_to_byte_offset(text, units),
        PositionEncoding::Utf16 => lsp_utf16_to_byte_offset(text, units),
        PositionEncoding::Utf32 => lsp_utf32_to_byte_offset(text, units),
    }
}

/// UTF-8 specialization — every byte is exactly one unit, so
/// the offset lands on a byte boundary by definition.
fn lsp_utf8_to_byte_offset(text: &str, units: u32) -> Option<usize> {
    let len = text.len();
    if (units as usize) > len {
        return None;
    }
    Some(units as usize)
}

/// UTF-16 specialization — the common case. Walks the string
/// one character at a time and accumulates `len_utf16()`
/// until the requested unit count is reached. This is the
/// helper that was previously embedded in the
/// `signature_help` operation module under
/// `lsp_units_to_byte_offset`; it has been promoted to the
/// shared `position` module so semantic-token bounds
/// validation (Pass 7) and signature-label parameter offsets
/// share a single implementation.
fn lsp_utf16_to_byte_offset(text: &str, units: u32) -> Option<usize> {
    if units == 0 {
        return Some(0);
    }
    let mut byte_offset: usize = 0;
    let mut unit_offset: u32 = 0;
    for c in text.chars() {
        let char_units = c.len_utf16() as u32;
        // If the target is within this character, it's not on a boundary.
        if unit_offset + char_units > units {
            return None;
        }
        unit_offset += char_units;
        byte_offset += c.len_utf8();
        if unit_offset == units {
            return Some(byte_offset);
        }
    }
    None
}

/// UTF-32 specialization — one `char` == one unit. The
/// conversion is therefore equivalent to finding the byte
/// offset of the `units`-th `char` in the string.
fn lsp_utf32_to_byte_offset(text: &str, units: u32) -> Option<usize> {
    if units == 0 {
        return Some(0);
    }
    let mut byte_offset: usize = 0;
    let mut char_index: u32 = 0;
    for c in text.chars() {
        if char_index == units {
            return Some(byte_offset);
        }
        char_index += 1;
        byte_offset += c.len_utf8();
    }
    None
}

/// Convert a range (start, length) expressed in `units` of the
/// given `encoding` to `(start_byte, end_byte)` byte offsets
/// within `text`. The start and end offsets are both validated
/// against the encoding; `end` is computed via checked
/// addition to reject overflow.
///
/// Returns `None` when:
/// - either offset is out of range for the encoding,
/// - the start offset does not land on a character boundary,
/// - the end offset does not land on a character boundary, or
/// - `start_units > end_units` (range would be negative).
///
/// This is the helper that semantic-token bounds validation
/// uses (Pass 7) and is exposed publicly so the harness can
/// reuse it without duplicating the encoding logic.
pub fn lsp_range_to_byte_offsets(
    text: &str,
    start_units: u32,
    length_units: u32,
    encoding: PositionEncoding,
) -> Option<(usize, usize)> {
    let start = lsp_units_to_byte_offset(text, start_units, encoding)?;
    let end_units = start_units.checked_add(length_units)?;
    let end = lsp_units_to_byte_offset(text, end_units, encoding)?;
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_ascii_is_byte_compatible() {
        let text = "hello world";
        assert_eq!(
            lsp_units_to_byte_offset(text, 0, PositionEncoding::Utf16),
            Some(0)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 5, PositionEncoding::Utf16),
            Some(5)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 11, PositionEncoding::Utf16),
            Some(11)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 12, PositionEncoding::Utf16),
            None
        );
    }

    #[test]
    fn utf16_empty_string() {
        assert_eq!(
            lsp_units_to_byte_offset("", 0, PositionEncoding::Utf16),
            Some(0)
        );
        assert_eq!(
            lsp_units_to_byte_offset("", 1, PositionEncoding::Utf16),
            None
        );
    }

    #[test]
    fn utf16_non_ascii_two_byte_chars() {
        // "café" — the `é` is 2 UTF-16 units (BMP) and 2 UTF-8 bytes.
        let text = "café";
        assert_eq!(
            lsp_units_to_byte_offset(text, 0, PositionEncoding::Utf16),
            Some(0)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 2, PositionEncoding::Utf16),
            Some(2)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 3, PositionEncoding::Utf16),
            Some(3)
        );
        // After the `é`: 3 UTF-16 units = 5 UTF-8 bytes.
        assert_eq!(
            lsp_units_to_byte_offset(text, 4, PositionEncoding::Utf16),
            Some(5)
        );
    }

    #[test]
    fn utf16_cjk_three_byte_chars() {
        // "你好" — each character is 1 UTF-16 unit and 3 UTF-8 bytes.
        let text = "你好";
        assert_eq!(
            lsp_units_to_byte_offset(text, 0, PositionEncoding::Utf16),
            Some(0)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 1, PositionEncoding::Utf16),
            Some(3)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 2, PositionEncoding::Utf16),
            Some(6)
        );
    }

    #[test]
    fn utf16_supplementary_plane_surrogate_pair() {
        // U+1F600 — GRINNING FACE — is 2 UTF-16 units (surrogate pair)
        // and 4 UTF-8 bytes.
        let text = "\u{1F600}!";
        assert_eq!(
            lsp_units_to_byte_offset(text, 0, PositionEncoding::Utf16),
            Some(0)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 1, PositionEncoding::Utf16),
            None
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 2, PositionEncoding::Utf16),
            Some(4)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 3, PositionEncoding::Utf16),
            Some(5)
        );
    }

    #[test]
    fn utf16_offset_in_middle_of_multibyte_char_rejects() {
        let text = "你";
        assert_eq!(
            lsp_units_to_byte_offset(text, 0, PositionEncoding::Utf16),
            Some(0)
        );
        // 1 UTF-16 unit lands in the middle of `你`.
        assert_eq!(
            lsp_units_to_byte_offset(text, 1, PositionEncoding::Utf16),
            None
        );
    }

    #[test]
    fn utf8_ascii_is_identity() {
        let text = "let x = 1;";
        assert_eq!(
            lsp_units_to_byte_offset(text, 0, PositionEncoding::Utf8),
            Some(0)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 5, PositionEncoding::Utf8),
            Some(5)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 10, PositionEncoding::Utf8),
            Some(10)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 11, PositionEncoding::Utf8),
            None
        );
    }

    #[test]
    fn utf8_non_ascii_byte_counts() {
        let text = "café";
        // UTF-8 bytes: c=0, a=1, f=2, é=3,4 — length 5.
        assert_eq!(
            lsp_units_to_byte_offset(text, 4, PositionEncoding::Utf8),
            Some(4)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 5, PositionEncoding::Utf8),
            Some(5)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 6, PositionEncoding::Utf8),
            None
        );
    }

    #[test]
    fn utf32_char_counting() {
        let text = "café";
        // 4 chars; `é` is one char spanning 2 UTF-8 bytes.
        assert_eq!(
            lsp_units_to_byte_offset(text, 0, PositionEncoding::Utf32),
            Some(0)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 3, PositionEncoding::Utf32),
            Some(3)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 4, PositionEncoding::Utf32),
            Some(5)
        );
        assert_eq!(
            lsp_units_to_byte_offset(text, 5, PositionEncoding::Utf32),
            None
        );
    }

    #[test]
    fn range_offsets_reject_overflow() {
        let text = "hello";
        // start + length overflows u32.
        assert_eq!(
            lsp_range_to_byte_offsets(text, u32::MAX, 1, PositionEncoding::Utf16),
            None
        );
    }

    #[test]
    fn range_offsets_negative_rejected() {
        let text = "hello";
        // start > end (start=5, length=0 is valid; start=5, length=1
        // is invalid because there is no char at position 6).
        assert_eq!(
            lsp_range_to_byte_offsets(text, 5, 1, PositionEncoding::Utf16),
            None
        );
    }

    #[test]
    fn range_offsets_cjk() {
        let text = "你好";
        assert_eq!(
            lsp_range_to_byte_offsets(text, 0, 1, PositionEncoding::Utf16),
            Some((0, 3))
        );
        assert_eq!(
            lsp_range_to_byte_offsets(text, 1, 1, PositionEncoding::Utf16),
            Some((3, 6))
        );
    }

    #[test]
    fn encoding_as_str_is_stable() {
        assert_eq!(PositionEncoding::Utf8.as_str(), "utf-8");
        assert_eq!(PositionEncoding::Utf16.as_str(), "utf-16");
        assert_eq!(PositionEncoding::Utf32.as_str(), "utf-32");
    }

    #[test]
    fn default_encoding_is_utf16() {
        assert_eq!(PositionEncoding::default(), PositionEncoding::Utf16);
    }
}
