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
        let old_start =
            parse_hunk_old_start(line).ok_or_else(|| format!("invalid hunk header: {}", line))?;

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

    Ok(output.join("\n"))
}

fn parse_hunk_old_start(header: &str) -> Option<usize> {
    let mut parts = header.split_whitespace();
    let _at1 = parts.next()?;
    let old_part = parts.next()?;
    if !old_part.starts_with('-') {
        return None;
    }
    let old_nums = &old_part[1..];
    let old_start = old_nums.split(',').next()?.parse::<usize>().ok()?;
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
}
