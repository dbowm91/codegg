use similar::{ChangeTag, TextDiff};
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use tempfile::Builder;

const IDE_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

fn run_command_with_timeout(program: &str, args: &[&str]) -> Result<(), String> {
    let program = program.to_string();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

    let mut child = Command::new(&program)
        .args(&args)
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {}", program, e))?;

    let deadline = Instant::now() + IDE_COMMAND_TIMEOUT;

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("{} failed (exit {})", program, status))
                };
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    return Err(format!("{} timed out after {:?}", program, IDE_COMMAND_TIMEOUT));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(format!("wait failed: {}", e));
            }
        }
    }
}

struct TempFilesGuard {
    paths: Vec<PathBuf>,
}

impl TempFilesGuard {
    fn new() -> Self {
        Self { paths: Vec::new() }
    }

    fn add(&mut self, path: PathBuf) {
        self.paths.push(path);
    }
}

impl Drop for TempFilesGuard {
    fn drop(&mut self) {
        for path in &self.paths {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn register_panic_cleanup() {
    static CLEANUP_REGISTERED: std::sync::Once = std::sync::Once::new();
    CLEANUP_REGISTERED.call_once(|| {
        std::panic::set_hook(Box::new(|_| {
            let temp_dir = std::env::temp_dir();
            for entry in std::fs::read_dir(temp_dir).into_iter().flatten().flatten() {
                let name = entry.file_name();
                if name.to_string_lossy().starts_with("codegg_") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }));
    });
}

pub fn is_vscode() -> bool {
    std::env::var("VSCODE_IPC_HOOK").is_ok()
        || std::env::var("VSCODE_INJECTED_ENVIRONMENT").is_ok()
        || std::env::var("TERM_PROGRAM").is_ok_and(|v| v == "vscode")
}

pub fn is_jetbrains() -> bool {
    std::env::var("JETBRAINS_REMOTE").is_ok()
        || std::env::var("JB_PRODUCT_READINESS").is_ok()
        || std::env::var("IDEA_INITIAL_DIRECTORY").is_ok()
        || std::env::var("WEBCLBROWSER_HOST").is_ok()
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
        open_diff_jetbrains(&original_content, &modified_content)
    } else {
        open_diff_generic(&original_content, &modified_content)
    }
}

fn open_diff_vscode(original_content: &str, modified_content: &str) -> Result<(), String> {
    register_panic_cleanup();
    let mut guard = TempFilesGuard::new();

    let original_temp = Builder::new()
        .prefix("codegg_original_")
        .tempfile()
        .map_err(|e| format!("failed to create temp original: {}", e))?;
    let modified_temp = Builder::new()
        .prefix("codegg_modified_")
        .tempfile()
        .map_err(|e| format!("failed to create temp modified: {}", e))?;

    {
        let mut original_file = original_temp.as_file();
        original_file
            .write_all(original_content.as_bytes())
            .map_err(|e| format!("failed to write temp original: {}", e))?;
        original_file.flush().map_err(|e| format!("failed to flush temp original: {}", e))?;
    }

    {
        let mut modified_file = modified_temp.as_file();
        modified_file
            .write_all(modified_content.as_bytes())
            .map_err(|e| format!("failed to write temp modified: {}", e))?;
        modified_file
            .flush()
            .map_err(|e| format!("failed to flush temp modified: {}", e))?;
    }

    let original_path = original_temp.path().to_owned();
    let modified_path = modified_temp.path().to_owned();

    guard.add(original_temp.path().to_owned());
    guard.add(modified_temp.path().to_owned());

    drop(original_temp);
    drop(modified_temp);

    let output = run_command_with_timeout("code", &[
        "--diff",
        original_path.to_string_lossy().as_ref(),
        modified_path.to_string_lossy().as_ref(),
    ])?;

    Ok(())
}

fn open_diff_jetbrains(original: &str, modified: &str) -> Result<(), String> {
    register_panic_cleanup();
    let mut guard = TempFilesGuard::new();

    let original_temp = Builder::new()
        .prefix("codegg_original_")
        .tempfile()
        .map_err(|e| format!("failed to create temp original: {}", e))?;
    let modified_temp = Builder::new()
        .prefix("codegg_modified_")
        .tempfile()
        .map_err(|e| format!("failed to create temp modified: {}", e))?;

    {
        let mut original_file = original_temp.as_file();
        original_file
            .write_all(original.as_bytes())
            .map_err(|e| format!("failed to write temp original: {}", e))?;
        original_file
            .flush()
            .map_err(|e| format!("failed to flush temp original: {}", e))?;
    }

    {
        let mut modified_file = modified_temp.as_file();
        modified_file
            .write_all(modified.as_bytes())
            .map_err(|e| format!("failed to write temp modified: {}", e))?;
        modified_file
            .flush()
            .map_err(|e| format!("failed to flush temp modified: {}", e))?;
    }

    let original_path = original_temp.path().to_owned();
    let modified_path = modified_temp.path().to_owned();

    guard.add(original_temp.path().to_owned());
    guard.add(modified_temp.path().to_owned());

    drop(original_temp);
    drop(modified_temp);

    let tool = if let Ok(tool) = std::env::var("JETBRAINS_TOOL") {
        Some(tool)
    } else if std::path::Path::new("/opt/intellij/bin/idea.sh").exists() {
        Some("/opt/intellij/bin/idea.sh".to_string())
    } else if std::path::Path::new("/usr/local/bin/idea").exists() {
        Some("/usr/local/bin/idea".to_string())
    } else if cfg!(windows) {
        let program_files = std::env::var("PROGRAMFILES").unwrap_or_default();
        let jetbrains_path = std::path::Path::new(&program_files).join("JetBrains");
        if jetbrains_path.exists() {
            let tool_path = jetbrains_path
                .read_dir()
                .ok()
                .and_then(|entries| entries.filter_map(|e| e.ok()).find(|e| e.path().is_dir()))
                .map(|e| e.path().join("bin\\idea.bat"));
            tool_path.and_then(|p| p.exists().then_some(p)).map(|p| p.to_string_lossy().to_string())
        } else {
            None
        }
    } else {
        None
    };

    let tool = tool.unwrap_or_else(|| "idea".to_string());

    run_command_with_timeout(&tool, &[
        "diff",
        original_path.to_string_lossy().as_ref(),
        modified_path.to_string_lossy().as_ref(),
    ])?;

    drop(guard);
    Ok(())
}

fn open_diff_generic(original_content: &str, modified_content: &str) -> Result<(), String> {
    register_panic_cleanup();
    let mut guard = TempFilesGuard::new();
    let has_code = std::env::split_paths(&std::env::var("PATH").unwrap_or_default())
        .any(|p| p.join("code").exists() || p.join("code.exe").exists() || p.join("code.cmd").exists());

    if has_code {
        let original_temp = Builder::new()
            .prefix("codegg_original_")
            .tempfile()
            .map_err(|e| format!("failed to create temp original: {}", e))?;
        let modified_temp = Builder::new()
            .prefix("codegg_modified_")
            .tempfile()
            .map_err(|e| format!("failed to create temp modified: {}", e))?;

        {
            let mut original_file = original_temp.as_file();
            original_file
                .write_all(original_content.as_bytes())
                .map_err(|e| format!("failed to write temp original: {}", e))?;
            original_file
                .flush()
                .map_err(|e| format!("failed to flush temp original: {}", e))?;
        }

        {
            let mut modified_file = modified_temp.as_file();
            modified_file
                .write_all(modified_content.as_bytes())
                .map_err(|e| format!("failed to write temp modified: {}", e))?;
            modified_file
                .flush()
                .map_err(|e| format!("failed to flush temp modified: {}", e))?;
        }

        let original_path = original_temp.path().to_owned();
        let modified_path = modified_temp.path().to_owned();

        guard.add(original_temp.path().to_owned());
        guard.add(modified_temp.path().to_owned());

        drop(original_temp);
        drop(modified_temp);

        let output = run_command_with_timeout("code", &[
            "--diff",
            original_path.to_string_lossy().as_ref(),
            modified_path.to_string_lossy().as_ref(),
        ]);

        if output.is_ok() {
            drop(guard);
            return Ok(());
        }
    }

    let has_idea = std::env::split_paths(&std::env::var("PATH").unwrap_or_default())
        .any(|p| p.join("idea").exists() || p.join("idea.bat").exists() || p.join("idea.cmd").exists());

    if has_idea {
        let original_temp = Builder::new()
            .prefix("codegg_original_")
            .tempfile()
            .map_err(|e| format!("failed to create temp original: {}", e))?;
        let modified_temp = Builder::new()
            .prefix("codegg_modified_")
            .tempfile()
            .map_err(|e| format!("failed to create temp modified: {}", e))?;

        {
            let mut original_file = original_temp.as_file();
            original_file
                .write_all(original_content.as_bytes())
                .map_err(|e| format!("failed to write temp original: {}", e))?;
            original_file
                .flush()
                .map_err(|e| format!("failed to flush temp original: {}", e))?;
        }

        {
            let mut modified_file = modified_temp.as_file();
            modified_file
                .write_all(modified_content.as_bytes())
                .map_err(|e| format!("failed to write temp modified: {}", e))?;
            modified_file
                .flush()
                .map_err(|e| format!("failed to flush temp modified: {}", e))?;
        }

        let original_path = original_temp.path().to_owned();
        let modified_path = modified_temp.path().to_owned();

        guard.add(original_temp.path().to_owned());
        guard.add(modified_temp.path().to_owned());

        drop(original_temp);
        drop(modified_temp);

        let output = run_command_with_timeout("idea", &[
            "diff",
            original_path.to_string_lossy().as_ref(),
            modified_path.to_string_lossy().as_ref(),
        ]);

        if output.is_ok() {
            drop(guard);
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
