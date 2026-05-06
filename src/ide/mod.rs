use similar::{ChangeTag, TextDiff};

pub fn is_vscode() -> bool {
    std::env::var("VSCODE_IPC_HOOK").is_ok()
}

pub fn is_jetbrains() -> bool {
    std::env::var("JETBRAINS_REMOTE").is_ok()
}

pub fn is_ide() -> bool {
    is_vscode() || is_jetbrains()
}

pub fn open_diff(
    _original: &str,
    _modified: &str,
    original_lines: Option<(usize, usize)>,
    modified_lines: Option<(usize, usize)>,
) -> Result<(), String> {
    let mut original_content = std::fs::read_to_string(_original)
        .map_err(|e| format!("failed to read original file: {}", e))?;
    let mut modified_content = std::fs::read_to_string(_modified)
        .map_err(|e| format!("failed to read modified file: {}", e))?;

    if let Some((start, end)) = original_lines {
        let lines: Vec<&str> = original_content.lines().collect();
        let start_idx = start.saturating_sub(1).min(lines.len());
        let end_idx = end.min(lines.len());
        original_content = lines[start_idx..end_idx].join("\n").to_string();
    }

    if let Some((start, end)) = modified_lines {
        let lines: Vec<&str> = modified_content.lines().collect();
        let start_idx = start.saturating_sub(1).min(lines.len());
        let end_idx = end.min(lines.len());
        modified_content = lines[start_idx..end_idx].join("\n").to_string();
    }

    if is_vscode() {
        open_diff_vscode(&original_content, &modified_content)
    } else if is_jetbrains() {
        open_diff_jetbrains(_original, _modified)
    } else {
        open_diff_generic(_original, _modified)
    }
}

fn open_diff_vscode(original_content: &str, modified_content: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::Command;
    use tempfile::Builder;

    let mut original_temp = Builder::new()
        .prefix("codegg_original_")
        .tempfile()
        .map_err(|e| format!("failed to create temp original: {}", e))?;
    let mut modified_temp = Builder::new()
        .prefix("codegg_modified_")
        .tempfile()
        .map_err(|e| format!("failed to create temp modified: {}", e))?;

    original_temp
        .write_all(original_content.as_bytes())
        .map_err(|e| format!("failed to write temp original: {}", e))?;
    modified_temp
        .write_all(modified_content.as_bytes())
        .map_err(|e| format!("failed to write temp modified: {}", e))?;

    let original_path = original_temp.path().to_owned();
    let modified_path = modified_temp.path().to_owned();

    let output = Command::new("code")
        .args([
            "--diff",
            original_path.to_str().unwrap(),
            modified_path.to_str().unwrap(),
        ])
        .output()
        .map_err(|e| format!("failed to open vscode: {}", e))?;

    if !output.status.success() {
        return Err("vscode failed to open diff".to_string());
    }

    Ok(())
}

fn open_diff_jetbrains(original: &str, modified: &str) -> Result<(), String> {
    use std::process::Command;

    let args = vec![
        "diff".to_string(),
        original.to_string(),
        modified.to_string(),
    ];

    let tool = if let Ok(tool) = std::env::var("JETBRAINS_TOOL") {
        tool
    } else if std::path::Path::new("/opt/intellij/bin/idea.sh").exists() {
        "/opt/intellij/bin/idea.sh".to_string()
    } else if std::path::Path::new("/usr/local/bin/idea").exists() {
        "/usr/local/bin/idea".to_string()
    } else {
        "idea".to_string()
    };

    let output = Command::new(&tool)
        .args(&args)
        .output()
        .map_err(|e| format!("failed to open jetbrains: {}", e))?;

    if !output.status.success() {
        return Err("jetbrains failed to open diff".to_string());
    }

    Ok(())
}

fn open_diff_generic(original: &str, modified: &str) -> Result<(), String> {
    if std::env::var("PATH").ok().is_some_and(|path| {
        path.split(':').any(|p| {
            std::path::Path::new(p).join("code").exists()
                || std::path::Path::new(p).join("code.exe").exists()
        })
    }) {
        let output = std::process::Command::new("code")
            .args(["--diff", original, modified])
            .output()
            .map_err(|e| format!("failed to open code: {}", e))?;

        if output.status.success() {
            return Ok(());
        }
    }

    if std::env::var("PATH").ok().is_some_and(|path| {
        path.split(':').any(|p| {
            std::path::Path::new(p).join("idea").exists()
                || std::path::Path::new(p).join("idea.bat").exists()
        })
    }) {
        let output = std::process::Command::new("idea")
            .args(["diff", original, modified])
            .output()
            .map_err(|e| format!("failed to open idea: {}", e))?;

        if output.status.success() {
            return Ok(());
        }
    }

    Err("no IDE diff tool found".to_string())
}

pub fn generate_unified_diff(old: &str, new: &str, path: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();
    result.push_str(&format!("--- a/{}\n", path));
    result.push_str(&format!("+++ b/{}\n", path));

    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        let line = change.value().trim_end_matches('\n');
        result.push_str(&format!("{}{}\n", sign, line));
    }

    let has_changes = result
        .lines()
        .skip(2)
        .any(|line| line.starts_with('+') || line.starts_with('-'));

    if !has_changes {
        return String::from("(no changes)");
    }

    result
}

pub fn generate_side_by_side(old: &str, new: &str, path: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut result = String::new();
    result.push_str(&format!("=== {} ===\n", path));
    result.push_str("─────────────────────────────────────────────────\n");

    for op in diff.grouped_ops(3) {
        for single_op in &op {
            for change in diff.iter_changes(single_op) {
                let (sign, style) = match change.tag() {
                    ChangeTag::Delete => ("-", "31"),
                    ChangeTag::Insert => ("+", "32"),
                    ChangeTag::Equal => (" ", "0"),
                };
                let line = change.value().trim_end_matches('\n');
                result.push_str(&format!("\u{001b}[{}m{}{}\u{001b}[0m\n", style, sign, line));
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vscode_detection() {
        assert!(!is_vscode());
    }

    #[test]
    fn test_jetbrains_detection() {
        assert!(!is_jetbrains());
    }

    #[test]
    fn test_no_changes() {
        let old = "hello\nworld\n";
        let new = "hello\nworld\n";
        let result = generate_unified_diff(old, new, "test.txt");
        assert_eq!(result, "(no changes)");
    }

    #[test]
    fn test_with_changes() {
        let old = "hello\nworld\n";
        let new = "hello\nrust\n";
        let result = generate_unified_diff(old, new, "test.txt");
        assert!(result.contains("-world"));
        assert!(result.contains("+rust"));
    }
}
