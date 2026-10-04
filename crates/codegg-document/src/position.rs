#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BytePosition {
    pub line: usize,
    pub byte_column: usize,
}
