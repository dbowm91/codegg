use egglsp::hunk_context::{HunkDescriptor, HunkLineRange};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseHunkError {
    EmptyInput,
    InvalidHunkHeader(String),
}

impl std::fmt::Display for ParseHunkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyInput => write!(f, "empty diff input"),
            Self::InvalidHunkHeader(h) => write!(f, "invalid hunk header: {h}"),
        }
    }
}

impl std::error::Error for ParseHunkError {}

pub fn parse_unified_diff(diff_text: &str) -> Result<Vec<HunkDescriptor>, ParseHunkError> {
    if diff_text.trim().is_empty() {
        return Err(ParseHunkError::EmptyInput);
    }

    let mut hunks = Vec::new();
    let mut current_file: Option<String> = None;
    let mut hunk_index: usize = 0;

    for line in diff_text.lines() {
        if let Some(path) = parse_diff_file_header(line) {
            current_file = Some(path);
            hunk_index = 0;
            continue;
        }

        if let Some(header_text) = parse_hunk_header(line)? {
            let file_path = current_file.clone().unwrap_or_default();
            match parse_hunk_header_parts(&header_text) {
                Ok((old_range, new_range)) => {
                    let id = if let Some(ref nr) = new_range {
                        format!(
                            "{}:{}:{}-{}",
                            file_path, hunk_index, nr.start_line, nr.end_line
                        )
                    } else {
                        format!("{}:{}:0-0", file_path, hunk_index)
                    };
                    hunks.push(HunkDescriptor {
                        id,
                        file_path,
                        old_range,
                        new_range,
                        header: Some(header_text),
                        added_lines: 0,
                        removed_lines: 0,
                        context_lines: 0,
                    });
                    hunk_index += 1;
                }
                Err(e) => return Err(e),
            }
            continue;
        }

        if let Some(last) = hunks.last_mut() {
            if line.starts_with('+') && !line.starts_with("+++") {
                last.added_lines += 1;
            } else if line.starts_with('-') && !line.starts_with("---") {
                last.removed_lines += 1;
            } else if line.starts_with(' ') {
                last.context_lines += 1;
            }
        }
    }

    Ok(hunks)
}

pub fn parse_multi_file_diff(diff_text: &str) -> Result<Vec<HunkDescriptor>, ParseHunkError> {
    parse_unified_diff(diff_text)
}

fn parse_diff_file_header(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    if let Some(rest) = trimmed.strip_prefix("diff --git ") {
        if let Some((_, b)) = rest.split_once(' ') {
            return Some(strip_diff_prefix(b));
        }
        return Some(strip_diff_prefix(rest));
    }
    if let Some(path) = trimmed.strip_prefix("--- ") {
        if path == "/dev/null" {
            return None;
        }
        return Some(strip_diff_prefix(path));
    }
    None
}

fn strip_diff_prefix(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("a/") {
        return rest.to_string();
    }
    if let Some(rest) = s.strip_prefix("b/") {
        return rest.to_string();
    }
    s.to_string()
}

fn parse_hunk_header(line: &str) -> Result<Option<String>, ParseHunkError> {
    let trimmed = line.trim_end();
    if !trimmed.starts_with("@@ ") {
        return Ok(None);
    }
    if trimmed.contains(" @@") {
        return Ok(Some(trimmed.to_string()));
    }
    // Starts with "@@ " but missing closing " @@" — malformed hunk header
    Err(ParseHunkError::InvalidHunkHeader(trimmed.to_string()))
}

fn parse_hunk_header_parts(
    header: &str,
) -> Result<(Option<HunkLineRange>, Option<HunkLineRange>), ParseHunkError> {
    let rest = header
        .strip_prefix("@@ ")
        .ok_or_else(|| ParseHunkError::InvalidHunkHeader(header.to_string()))?;

    let range_end = rest
        .find(" @@")
        .ok_or_else(|| ParseHunkError::InvalidHunkHeader(header.to_string()))?;

    let range_part = &rest[..range_end];

    let (old_part, new_part) = range_part
        .split_once(' ')
        .ok_or_else(|| ParseHunkError::InvalidHunkHeader(header.to_string()))?;

    let old_range = parse_range_spec(old_part)?;
    let new_range = parse_range_spec(new_part)?;

    Ok((old_range, new_range))
}

fn parse_range_spec(spec: &str) -> Result<Option<HunkLineRange>, ParseHunkError> {
    let rest = if let Some(r) = spec.strip_prefix('-') {
        r
    } else if let Some(r) = spec.strip_prefix('+') {
        r
    } else {
        spec
    };

    let has_comma = rest.contains(',');
    let (start_str, end_str) = if let Some((s, l)) = rest.split_once(',') {
        (s, Some(l))
    } else {
        (rest, None)
    };

    let start: u32 = start_str
        .parse()
        .map_err(|_| ParseHunkError::InvalidHunkHeader(format!("bad start: {start_str}")))?;

    let len = if let Some(l) = end_str {
        l.parse::<u32>()
            .map_err(|_| ParseHunkError::InvalidHunkHeader(format!("bad len: {l}")))?
    } else if has_comma {
        0
    } else {
        1
    };

    if start == 0 || len == 0 {
        return Ok(None);
    }

    let end = start + len - 1;

    Ok(Some(HunkLineRange {
        start_line: start,
        end_line: end,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_hunk() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,6 +10,8 @@ fn main() {
     let x = 1;
     let y = 2;
+    let z = 3;
+    let w = 4;
     println!(\"{x} {y}\");
 }";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].file_path, "src/main.rs");
        assert_eq!(
            hunks[0].old_range,
            Some(HunkLineRange {
                start_line: 10,
                end_line: 15
            })
        );
        assert_eq!(
            hunks[0].new_range,
            Some(HunkLineRange {
                start_line: 10,
                end_line: 17
            })
        );
        assert_eq!(hunks[0].added_lines, 2);
        assert_eq!(hunks[0].removed_lines, 0);
        assert_eq!(hunks[0].context_lines, 4);
        assert!(hunks[0].id.contains("src/main.rs"));
    }

    #[test]
    fn parse_multiple_hunks_same_file() {
        let diff = "\
diff --git a/src/foo.rs b/src/foo.rs
--- a/src/foo.rs
+++ b/src/foo.rs
@@ -5,3 +5,4 @@
 line1
+added1
 line2
@@ -20,3 +21,3 @@
 line20
-old20
+new20
 line21
 ";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].file_path, "src/foo.rs");
        assert_eq!(hunks[1].file_path, "src/foo.rs");
        assert_eq!(hunks[0].id, "src/foo.rs:0:5-8");
        assert_eq!(hunks[1].id, "src/foo.rs:1:21-23");
        assert_eq!(hunks[0].added_lines, 1);
        assert_eq!(hunks[1].removed_lines, 1);
        assert_eq!(hunks[1].added_lines, 1);
    }

    #[test]
    fn parse_multi_file_diff() {
        let diff = "\
diff --git a/src/a.rs b/src/a.rs
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,2 +1,3 @@
 foo
+bar
 baz
diff --git a/src/b.rs b/src/b.rs
--- a/src/b.rs
+++ b/src/b.rs
@@ -5,3 +5,2 @@
 old
-delete
 keep
 ";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 2);
        assert_eq!(hunks[0].file_path, "src/a.rs");
        assert_eq!(hunks[1].file_path, "src/b.rs");
        assert_eq!(hunks[0].added_lines, 1);
        assert_eq!(hunks[1].removed_lines, 1);
    }

    #[test]
    fn parse_hunk_with_omitted_length() {
        let diff = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -10 +10,2 @@
 line
+added
 ";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0].old_range,
            Some(HunkLineRange {
                start_line: 10,
                end_line: 10
            })
        );
        assert_eq!(
            hunks[0].new_range,
            Some(HunkLineRange {
                start_line: 10,
                end_line: 11
            })
        );
    }

    #[test]
    fn parse_additions_only_hunk() {
        let diff = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,0 +1,3 @@
+line1
+line2
+line3
 ";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].old_range.is_none());
        assert_eq!(
            hunks[0].new_range,
            Some(HunkLineRange {
                start_line: 1,
                end_line: 3
            })
        );
        assert_eq!(hunks[0].added_lines, 3);
        assert_eq!(hunks[0].removed_lines, 0);
    }

    #[test]
    fn parse_deletions_only_hunk() {
        let diff = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +0,0 @@
-line1
-line2
-line3
 ";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0].old_range,
            Some(HunkLineRange {
                start_line: 1,
                end_line: 3
            })
        );
        assert!(hunks[0].new_range.is_none());
        assert_eq!(hunks[0].removed_lines, 3);
        assert_eq!(hunks[0].added_lines, 0);
    }

    #[test]
    fn parse_empty_diff_returns_error() {
        assert!(parse_unified_diff("").is_err());
        assert!(parse_unified_diff("  \n  ").is_err());
    }

    #[test]
    fn parse_malformed_header_returns_invalid_hunk_header() {
        let diff = "@@ bad header\n";
        let result = parse_unified_diff(diff);
        match result {
            Err(ParseHunkError::InvalidHunkHeader(h)) => {
                assert!(h.contains("@@ bad header"));
            }
            other => panic!("expected InvalidHunkHeader, got {:?}", other),
        }
    }

    #[test]
    fn parse_malformed_header_missing_closing_at_at() {
        let diff = "@@ -1,3 +1,4\n";
        let result = parse_unified_diff(diff);
        match result {
            Err(ParseHunkError::InvalidHunkHeader(h)) => {
                assert!(h.contains("@@ -1,3 +1,4"));
            }
            other => panic!("expected InvalidHunkHeader, got {:?}", other),
        }
    }

    #[test]
    fn valid_hunk_header_with_context_still_parses() {
        let diff = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@ fn main() {
 line1
+added
 line2
 line3
 ";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].header.as_deref().unwrap().contains("fn main()"));
    }

    #[test]
    fn non_hunk_text_does_not_error() {
        let diff = "some random text\nno hunk here\n";
        let result = parse_unified_diff(diff).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn deterministic_ids() {
        let diff = "\
diff --git a/src/main.rs b/src/main.rs
--- a/src/main.rs
+++ b/src/main.rs
@@ -10,6 +10,8 @@ fn main() {
     let x = 1;
+    let z = 3;
     println!(\"{x}\");
 }
 ";
        let hunks1 = parse_unified_diff(diff).unwrap();
        let hunks2 = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks1[0].id, hunks2[0].id);
    }

    #[test]
    fn hunk_header_with_context_text() {
        let diff = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@ fn main() {
 line1
+added
 line2
 line3
 ";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].header.as_deref().unwrap().contains("fn main()"));
    }

    #[test]
    fn old_range_none_for_zero_start() {
        let diff = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -0,5 +1,5 @@
+added
+added
+added
+added
+added
 ";
        let hunks = parse_unified_diff(diff).unwrap();
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].old_range.is_none());
    }
}
