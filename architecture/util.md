# Util Module

The `util` module provides common utility functions.

## Overview

**Location**: `src/util/`

**Key Responsibilities**:
- Clipboard operations
- Fuzzy string matching
- File statistics
- Text truncation

## Components

### clipboard.rs

```rust
pub fn copy(text: &str) -> Result<()>;
pub fn paste() -> Result<String>;
```

### fuzzy.rs

```rust
pub fn fuzzy_match(pattern: &str, text: &str) -> Option<(usize, usize)>;

pub struct FuzzyScore {
    pub score: i64,
    pub matches: Vec<(usize, usize)>,
}

pub fn fuzzy_score(pattern: &str, text: &str) -> FuzzyScore;
```

### stat_core.rs

```rust
pub fn file_stats(path: &Path) -> Result<FileStats>;

pub struct FileStats {
    pub size: u64,
    pub modified: DateTime<Utc>,
    pub created: DateTime<Utc>,
    pub is_dir: bool,
    pub is_symlink: bool,
}
```

### truncate.rs

```rust
pub fn truncate(text: &str, max_len: usize) -> String;
pub fn truncate_with_ellipsis(text: &str, max_len: usize) -> String;
```

## See Also

- [tool.md](tool.md) - Tools using utilities
