/// Apply a unified diff patch to original text content.
///
/// Returns the patched content or an error describing what went wrong.
pub fn apply_unified_diff(original: &str, patch: &str) -> Result<String, String> {
    let original_lines: Vec<&str> = original.lines().collect();
    let patch_lines: Vec<&str> = patch.lines().collect();

    let mut output: Vec<String> = Vec::new();
    let mut orig_idx: usize = 0;
    let mut patch_idx: usize = 0;
    let mut saw_hunk = false;

    while patch_idx < patch_lines.len() {
        let line = patch_lines[patch_idx];

        if !line.starts_with("@@") {
            patch_idx += 1;
            continue;
        }

        saw_hunk = true;
        let old_start = parse_hunk_old_start(line).ok_or_else(|| {
            format!(
                "invalid hunk header at patch line {}: expected @@ -<line>[,<count>] +<line>[,<count>] @@",
                patch_idx + 1
            )
        })?;

        let target_idx = old_start.saturating_sub(1);
        if target_idx < orig_idx {
            return Err(format!("overlapping hunk at original line {}", old_start));
        }
        while orig_idx < target_idx && orig_idx < original_lines.len() {
            output.push(original_lines[orig_idx].to_string());
            orig_idx += 1;
        }

        patch_idx += 1;
        while patch_idx < patch_lines.len() {
            let hline = patch_lines[patch_idx];
            if hline.starts_with("@@") {
                break;
            }
            if hline.starts_with("--- ") || hline.starts_with("+++ ") {
                patch_idx += 1;
                continue;
            }
            if hline.starts_with("\\ No newline at end of file") {
                patch_idx += 1;
                continue;
            }

            if hline.is_empty() {
                return Err("invalid empty hunk line".to_string());
            }
            let tag = &hline[..1];
            let content = &hline[1..];
            match tag {
                " " => {
                    if orig_idx >= original_lines.len() || original_lines[orig_idx] != content {
                        return Err(format!(
                            "context mismatch at original line {}",
                            orig_idx + 1
                        ));
                    }
                    output.push(content.to_string());
                    orig_idx += 1;
                }
                "-" => {
                    if orig_idx >= original_lines.len() || original_lines[orig_idx] != content {
                        return Err(format!("delete mismatch at original line {}", orig_idx + 1));
                    }
                    orig_idx += 1;
                }
                "+" => output.push(content.to_string()),
                _ => return Err(format!("invalid hunk prefix '{}'", tag)),
            }
            patch_idx += 1;
        }
    }

    if !saw_hunk {
        return Err("patch does not contain any hunks".to_string());
    }

    while orig_idx < original_lines.len() {
        output.push(original_lines[orig_idx].to_string());
        orig_idx += 1;
    }

    let mut result = output.join("\n");
    // `str::lines()` strips the trailing newline; restore it so a patch
    // that does not touch the final newline preserves it (e.g. "old\n"
    // with "old"->"new" must become "new\n", not "new").
    if original.ends_with('\n') && !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    Ok(result)
}

fn parse_hunk_old_start(header: &str) -> Option<usize> {
    let body = header.strip_prefix("@@ ")?;
    let (ranges, suffix) = body.split_once(" @@")?;
    if !suffix.is_empty() && !suffix.starts_with(' ') {
        return None;
    }
    let mut parts = ranges.split_whitespace();
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    if parts.next().is_some() {
        return None;
    }
    let parse_range = |range: &str| -> Option<usize> {
        let mut values = range.split(',');
        let start = values.next()?.parse::<usize>().ok()?;
        if let Some(count) = values.next() {
            count.parse::<usize>().ok()?;
        }
        if values.next().is_some() {
            return None;
        }
        Some(start)
    };
    let old_start = parse_range(old)?;
    parse_range(new)?;
    Some(old_start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_unified_diff_applies_single_hunk() {
        let original = "a\nb\nc";
        let patch = "\
@@ -1,3 +1,3 @@
 a
-b
+B
 c";

        let updated = apply_unified_diff(original, patch).expect("patch should apply");
        assert_eq!(updated, "a\nB\nc");
    }

    #[test]
    fn apply_unified_diff_applies_multiple_hunks() {
        let original = "l1\nl2\nl3\nl4\nl5";
        let patch = "\
@@ -1,2 +1,2 @@
 l1
-l2
+L2
@@ -4,2 +4,2 @@
 l4
-l5
+L5";

        let updated = apply_unified_diff(original, patch).expect("patch should apply");
        assert_eq!(updated, "l1\nL2\nl3\nl4\nL5");
    }

    #[test]
    fn apply_unified_diff_fails_on_context_mismatch() {
        let original = "a\nb\nc";
        let patch = "\
@@ -1,3 +1,3 @@
 a
 x
 c";

        let err = apply_unified_diff(original, patch).expect_err("must fail");
        assert!(err.contains("context mismatch"), "unexpected error: {err}");
    }

    #[test]
    fn apply_unified_diff_fails_on_delete_mismatch() {
        let original = "a\nb\nc";
        let patch = "\
@@ -1,3 +1,2 @@
 a
-x
 c";

        let err = apply_unified_diff(original, patch).expect_err("must fail");
        assert!(err.contains("delete mismatch"), "unexpected error: {err}");
    }

    #[test]
    fn malformed_hunk_reports_location_and_expected_grammar() {
        let err = apply_unified_diff("a", "@@\n-a\n+b").expect_err("bare header rejected");
        assert!(err.contains("patch line 1"));
        assert!(err.contains("@@ -<line>[,<count>] +<line>[,<count>] @@"));
    }

    #[test]
    fn malformed_range_is_not_accepted() {
        let err =
            apply_unified_diff("a", "@@ -x +1 @@\n-a\n+b").expect_err("malformed range rejected");
        assert!(err.contains("patch line 1"));
    }
}
