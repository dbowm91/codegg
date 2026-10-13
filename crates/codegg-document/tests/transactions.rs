use codegg_document::{
    BytePosition, DocumentBuffer, DocumentError, DocumentLimits, TextEdit, TextTransaction,
};
use proptest::prelude::*;

fn limits() -> DocumentLimits {
    DocumentLimits::default()
}

#[test]
fn multi_edit_inverse_restores_unicode_and_mixed_line_endings() {
    let original = "a🌍\r\nbeta\ngamma";
    let mut buffer = DocumentBuffer::new(original);
    let transaction =
        TextTransaction::new(vec![TextEdit::new(1..5, "world"), TextEdit::new(7..8, "B")]);
    let applied = buffer.apply(&transaction, limits()).unwrap();
    assert_eq!(applied.snapshot.to_string(), "aworld\r\nBeta\ngamma");
    buffer.apply(&applied.inverse, limits()).unwrap();
    assert_eq!(buffer.snapshot().to_string(), original);
    assert_eq!(buffer.revision().get(), 2);
}

#[test]
fn empty_transaction_is_a_noop() {
    let mut buffer = DocumentBuffer::new("text");
    let result = buffer.apply(&TextTransaction::default(), limits()).unwrap();
    assert_eq!(result.revision.get(), 0);
    assert_eq!(result.snapshot.to_string(), "text");
}

#[test]
fn invalid_boundaries_overlap_and_bounds_are_rejected() {
    let mut buffer = DocumentBuffer::new("éx");
    assert_eq!(
        buffer
            .apply(
                &TextTransaction::new(vec![TextEdit::new(1..2, "")]),
                limits()
            )
            .unwrap_err(),
        DocumentError::InvalidBoundary
    );
    let mut ascii = DocumentBuffer::new("abcd");
    assert_eq!(
        ascii
            .apply(
                &TextTransaction::new(vec![TextEdit::new(0..2, ""), TextEdit::new(1..3, "")]),
                limits()
            )
            .unwrap_err(),
        DocumentError::OverlappingEdits
    );
    assert_eq!(
        buffer
            .apply(
                &TextTransaction::new(vec![TextEdit::new(0..4, "")]),
                limits()
            )
            .unwrap_err(),
        DocumentError::OutOfBounds
    );
}

#[test]
fn line_positions_are_byte_based_and_preserve_crlf() {
    let snapshot = DocumentBuffer::new("é\r\nx\n").snapshot();
    assert_eq!(snapshot.len_lines(), 3);
    assert_eq!(
        snapshot.byte_to_position(2).unwrap(),
        BytePosition {
            line: 0,
            byte_column: 2
        }
    );
    assert_eq!(
        snapshot
            .position_to_byte(BytePosition {
                line: 1,
                byte_column: 1
            })
            .unwrap(),
        5
    );
    assert_eq!(snapshot.to_string(), "é\r\nx\n");
}

proptest! {
    #[test]
    fn insertion_inverse_round_trips(prefix in ".{0,30}", suffix in ".{0,30}", insert in ".{0,10}") {
        let original = format!("{prefix}{suffix}");
        let offset = prefix.len();
        let mut buffer = DocumentBuffer::new(&original);
        let applied = buffer.apply(&TextTransaction::new(vec![TextEdit::new(offset..offset, insert)]), limits()).unwrap();
        buffer.apply(&applied.inverse, limits()).unwrap();
        prop_assert_eq!(buffer.snapshot().to_string(), original);
    }
}

fn snapshot_of(text: &str) -> codegg_document::DocumentSnapshot {
    DocumentBuffer::new(text).snapshot()
}

#[test]
fn utf16_column_is_exact_for_ascii_tabs_combining_and_astral() {
    // "a\té\u{301}🌍x": a=1, tab=1, é=1, combining mark=1, astral=2, x=1.
    let text = "a\té\u{301}🌍x";
    let snapshot = snapshot_of(text);
    let bytes = text.len();
    assert_eq!(snapshot.utf16_column_bounded(0, bytes).unwrap(), 7);
    // Prefix "a\té\u{301}" is 1 + 1 + 2 + 2 = 6 bytes, 4 units.
    assert_eq!(snapshot.utf16_column_bounded(0, 6).unwrap(), 4);
    assert_eq!(snapshot.utf16_column_bounded(0, 0).unwrap(), 0);
}

#[test]
fn utf16_column_refuses_past_the_prefix_budget() {
    let budget = codegg_document::DocumentSnapshot::MAX_COMPLETION_PREFIX_BYTES;
    let line = "a".repeat(budget + 1);
    let snapshot = snapshot_of(&line);
    assert_eq!(
        snapshot.utf16_column_bounded(0, budget + 1).unwrap_err(),
        DocumentError::CompletionPrefixTooLong
    );
    // Exactly at budget still resolves precisely, with no refusal.
    assert_eq!(
        snapshot.utf16_column_bounded(0, budget).unwrap(),
        budget as u32
    );
    assert_eq!(snapshot.utf16_column_bounded(0, 0).unwrap(), 0);
}

#[test]
fn utf16_column_rejects_bad_ranges_without_reading() {
    let snapshot = snapshot_of("héllo");
    assert_eq!(
        snapshot.utf16_column_bounded(3, 1).unwrap_err(),
        DocumentError::ReversedRange
    );
    assert_eq!(
        snapshot.utf16_column_bounded(0, 999).unwrap_err(),
        DocumentError::OutOfBounds
    );
    // Offset 1 splits 'h'(1 byte) from 'é'(2 bytes): byte 2 is inside é.
    assert_eq!(
        snapshot.utf16_column_bounded(0, 2).unwrap_err(),
        DocumentError::InvalidBoundary
    );
}
