use codegg_document::DocumentBuffer;

use super::utf16_position;

#[test]
fn completion_positions_count_mixed_unicode_as_utf16_units() {
    let snapshot = DocumentBuffer::new("a😀\n水é").snapshot();

    assert_eq!(utf16_position(&snapshot, "a😀".len()).unwrap(), (0, 3));
    assert_eq!(utf16_position(&snapshot, "a😀\n水é".len()).unwrap(), (1, 2));
}
