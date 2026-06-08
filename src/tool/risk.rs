//! Deterministic tool risk classifier and output summarizer.
//!
//! Classifies tool calls by risk level based on tool name and input arguments.
//! Provides deterministic output summarization without LLM calls.

use crate::session::events::ToolRisk;

/// Classify a tool call by risk level based on tool name and input.
pub fn classify_tool_risk(tool_name: &str, input: &serde_json::Value) -> ToolRisk {
    match tool_name {
        "read" | "grep" | "glob" | "list" | "codesearch" | "webfetch" | "websearch" => {
            ToolRisk::Read
        }
        "write" | "edit" | "replace" | "multiedit" | "formatter" | "apply_patch" => ToolRisk::Write,
        "git" | "commit" => ToolRisk::GitMutation,
        "bash" | "terminal" => classify_bash_risk(input),
        _ => ToolRisk::Unknown,
    }
}

/// Classify risk of a bash command by inspecting the command string.
fn classify_bash_risk(input: &serde_json::Value) -> ToolRisk {
    let cmd = extract_command_string(input).unwrap_or_default();
    let lower = cmd.to_lowercase();

    if is_destructive_command(&lower) {
        ToolRisk::Destructive
    } else if is_credential_command(&lower) {
        ToolRisk::CredentialAdjacent
    } else if is_dependency_mutation_command(&lower) {
        ToolRisk::DependencyMutation
    } else if is_network_command(&lower) {
        ToolRisk::Network
    } else if is_git_mutation_command(&lower) {
        ToolRisk::GitMutation
    } else if is_readonly_command(&lower) {
        ToolRisk::Read
    } else if is_write_command(&lower) {
        ToolRisk::Write
    } else {
        ToolRisk::Unknown
    }
}

/// Extract the command string from tool input JSON.
fn extract_command_string(input: &serde_json::Value) -> Option<&str> {
    // bash tool uses "command" field
    if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
        return Some(cmd);
    }
    // Some tools use "cmd" field
    if let Some(cmd) = input.get("cmd").and_then(|v| v.as_str()) {
        return Some(cmd);
    }
    // Try "args" as string
    if let Some(cmd) = input.get("args").and_then(|v| v.as_str()) {
        return Some(cmd);
    }
    // If input is a plain string, use it directly
    if let Some(cmd) = input.as_str() {
        return Some(cmd);
    }
    None
}

fn is_destructive_command(cmd: &str) -> bool {
    cmd.contains("rm -rf")
        || cmd.contains("rm -r ")
        || cmd.contains("rm -fr")
        || cmd.contains("dd if=")
        || cmd.contains("mkfs")
        || cmd.contains("> /dev/sd")
        || (cmd.contains("chmod") && cmd.contains("-R") && cmd.contains("/"))
            && !cmd.contains("chmod -R u+r")
}

fn is_credential_command(cmd: &str) -> bool {
    cmd.contains(".env")
        || cmd.contains("credentials")
        || cmd.contains(".ssh")
        || cmd.contains("id_rsa")
        || cmd.contains("id_ed25519")
        || cmd.contains("secret")
        || cmd.contains("token")
        || cmd.contains("password")
        || cmd.contains("api_key")
        || cmd.contains("API_KEY")
}

fn is_dependency_mutation_command(cmd: &str) -> bool {
    cmd.contains("cargo update")
        || cmd.contains("cargo add ")
        || cmd.contains("npm install")
        || cmd.contains("npm i ")
        || cmd.contains("pnpm install")
        || cmd.contains("pnpm add")
        || cmd.contains("uv add")
        || cmd.contains("pip install")
        || cmd.contains("pip install ")
        || cmd.contains("yarn add")
        || cmd.contains("bun add")
        || cmd.contains("bun install")
}

fn is_network_command(cmd: &str) -> bool {
    cmd.contains("curl ")
        || cmd.contains("wget ")
        || cmd.contains("git fetch")
        || cmd.contains("git pull")
        || cmd.contains("ssh ")
        || cmd.contains("scp ")
        || cmd.contains("rsync ")
}

fn is_git_mutation_command(cmd: &str) -> bool {
    cmd.contains("git commit")
        || cmd.contains("git push")
        || cmd.contains("git merge")
        || cmd.contains("git rebase")
        || cmd.contains("git reset")
        || cmd.contains("git checkout --")
        || cmd.contains("git clean")
        || cmd.contains("git tag")
        || cmd.contains("git branch -d")
        || cmd.contains("git branch -D")
}

fn is_readonly_command(cmd: &str) -> bool {
    cmd.starts_with("ls")
        || cmd.starts_with("cat ")
        || cmd.starts_with("head ")
        || cmd.starts_with("tail ")
        || cmd.starts_with("wc ")
        || cmd.starts_with("file ")
        || cmd.starts_with("stat ")
        || cmd.starts_with("which ")
        || cmd.starts_with("whereis ")
        || cmd.starts_with("type ")
        || cmd.starts_with("echo ")
        || cmd.starts_with("pwd")
        || cmd.starts_with("env")
        || cmd.starts_with("printenv")
        || cmd.contains("git status")
        || cmd.contains("git log")
        || cmd.contains("git diff")
        || cmd.contains("git show")
        || cmd.contains("git branch")
        || cmd.contains("git remote")
        || cmd.contains("git stash list")
        || cmd.contains("cargo test")
        || cmd.contains("cargo check")
        || cmd.contains("cargo clippy")
        || cmd.contains("cargo build")
        || cmd.contains("cargo fmt --check")
        || cmd.contains("npm test")
        || cmd.contains("npm run lint")
        || cmd.contains("make ")
        || cmd.contains("grep ")
        || cmd.contains("rg ")
        || cmd.contains("find ")
        || cmd.contains("tree ")
}

fn is_write_command(cmd: &str) -> bool {
    cmd.contains("mkdir")
        || cmd.contains("touch ")
        || cmd.contains("cp ")
        || cmd.contains("mv ")
        || cmd.contains("chmod")
        || cmd.contains("chown")
        || cmd.contains("ln -s")
        || cmd.contains("cargo fmt")
        || cmd.contains("cargo fix")
}

/// Summarize tool output deterministically (no LLM calls).
pub fn summarize_tool_output(tool_name: &str, output: &str, success: bool) -> Option<String> {
    if output.is_empty() {
        return Some("empty output".to_string());
    }

    let lines: Vec<&str> = output.lines().collect();
    let line_count = lines.len();

    // For failed commands, extract error context
    if !success {
        return summarize_failure(tool_name, &lines);
    }

    // Detect specific command patterns and summarize accordingly
    if tool_name == "bash" || tool_name == "terminal" {
        return summarize_bash_output(&lines, line_count);
    }

    if tool_name == "cargo" || tool_name == "bash" {
        // Already handled above for bash
    }

    if line_count <= 3 {
        Some(lines.join("\n"))
    } else {
        Some(format!("{} lines of output", line_count))
    }
}

fn summarize_failure(_tool_name: &str, lines: &[&str]) -> Option<String> {
    // For cargo test failures, find failed test lines
    if let Some(summary) = summarize_test_failure(lines) {
        return Some(summary);
    }

    // For compiler errors
    if let Some(summary) = summarize_compiler_errors(lines) {
        return Some(summary);
    }

    // General failure: first few error lines
    let error_lines: Vec<&str> = lines
        .iter()
        .filter(|l| {
            l.contains("error")
                || l.contains("Error")
                || l.contains("FAILED")
                || l.contains("panic")
                || l.contains("failed")
        })
        .take(3)
        .copied()
        .collect();

    if !error_lines.is_empty() {
        Some(error_lines.join("; "))
    } else if lines.len() > 3 {
        Some(format!("{} lines of error output", lines.len()))
    } else {
        Some(lines.join("\n"))
    }
}

fn summarize_test_failure(lines: &[&str]) -> Option<String> {
    let failed_tests: Vec<&str> = lines
        .iter()
        .filter(|l| {
            l.contains("FAILED") || l.contains("failures:") || l.contains("test result: FAILED")
        })
        .take(3)
        .copied()
        .collect();

    if !failed_tests.is_empty() {
        return Some(failed_tests.join("; "));
    }

    // Count passed/failed from test result line
    for line in lines {
        if line.contains("test result:") {
            return Some(line.trim().to_string());
        }
    }

    None
}

fn summarize_compiler_errors(lines: &[&str]) -> Option<String> {
    let errors: Vec<&str> = lines
        .iter()
        .filter(|l| l.contains("error[") || l.contains("error:"))
        .take(2)
        .copied()
        .collect();

    if !errors.is_empty() {
        let error_count = lines.iter().filter(|l| l.contains("error[")).count();
        if error_count > errors.len() {
            Some(format!(
                "{} (and {} more errors)",
                errors.join("; "),
                error_count - errors.len()
            ))
        } else {
            Some(errors.join("; "))
        }
    } else {
        None
    }
}

fn summarize_bash_output(lines: &[&str], line_count: usize) -> Option<String> {
    // Detect cargo test output
    if let Some(s) = summarize_test_failure(lines) {
        return Some(s);
    }

    // Detect compiler errors
    if let Some(s) = summarize_compiler_errors(lines) {
        return Some(s);
    }

    // Detect git status output
    let has_git_branch = lines.iter().any(|l| l.contains("On branch"));
    if has_git_branch {
        let branch_line = lines.iter().find(|l| l.contains("On branch")).unwrap();
        let changes = lines
            .iter()
            .filter(|l| {
                l.contains("modified:") || l.contains("new file:") || l.contains("deleted:")
            })
            .count();
        return Some(format!(
            "{}, {} changed file(s)",
            branch_line.trim(),
            changes
        ));
    }

    // General: summarize by line count and first significant lines
    if line_count <= 2 {
        Some(lines.join("\n"))
    } else {
        // Find first non-empty, non-header line as preview
        let preview = lines
            .iter()
            .find(|l| !l.trim().is_empty() && !l.starts_with('#') && !l.starts_with("//"))
            .map(|l| {
                if l.len() > 80 {
                    format!("{}...", &l[..77])
                } else {
                    l.to_string()
                }
            })
            .unwrap_or_else(|| format!("{} lines", line_count));

        if line_count > 5 {
            Some(format!("{} ({} lines total)", preview, line_count))
        } else {
            Some(preview)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classify_readonly_tools() {
        assert_eq!(classify_tool_risk("read", &json!({})), ToolRisk::Read);
        assert_eq!(classify_tool_risk("grep", &json!({})), ToolRisk::Read);
        assert_eq!(classify_tool_risk("glob", &json!({})), ToolRisk::Read);
        assert_eq!(classify_tool_risk("list", &json!({})), ToolRisk::Read);
        assert_eq!(classify_tool_risk("codesearch", &json!({})), ToolRisk::Read);
        assert_eq!(classify_tool_risk("webfetch", &json!({})), ToolRisk::Read);
        assert_eq!(classify_tool_risk("websearch", &json!({})), ToolRisk::Read);
    }

    #[test]
    fn classify_write_tools() {
        assert_eq!(classify_tool_risk("write", &json!({})), ToolRisk::Write);
        assert_eq!(classify_tool_risk("edit", &json!({})), ToolRisk::Write);
        assert_eq!(classify_tool_risk("replace", &json!({})), ToolRisk::Write);
        assert_eq!(classify_tool_risk("multiedit", &json!({})), ToolRisk::Write);
        assert_eq!(
            classify_tool_risk("apply_patch", &json!({})),
            ToolRisk::Write
        );
    }

    #[test]
    fn classify_git_tools() {
        assert_eq!(classify_tool_risk("git", &json!({})), ToolRisk::GitMutation);
        assert_eq!(
            classify_tool_risk("commit", &json!({})),
            ToolRisk::GitMutation
        );
    }

    #[test]
    fn classify_bash_destructive() {
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "rm -rf /tmp/junk"})),
            ToolRisk::Destructive
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "rm -r build/"})),
            ToolRisk::Destructive
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "dd if=/dev/zero of=/dev/sda"})),
            ToolRisk::Destructive
        );
    }

    #[test]
    fn classify_bash_dependency_mutation() {
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "cargo update"})),
            ToolRisk::DependencyMutation
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "npm install"})),
            ToolRisk::DependencyMutation
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "pnpm add lodash"})),
            ToolRisk::DependencyMutation
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "pip install requests"})),
            ToolRisk::DependencyMutation
        );
    }

    #[test]
    fn classify_bash_network() {
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "curl https://example.com"})),
            ToolRisk::Network
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "wget https://example.com/file"})),
            ToolRisk::Network
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "git fetch origin"})),
            ToolRisk::Network
        );
    }

    #[test]
    fn classify_bash_credential() {
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "cat .env"})),
            ToolRisk::CredentialAdjacent
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "cat ~/.ssh/id_rsa"})),
            ToolRisk::CredentialAdjacent
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "echo $API_KEY"})),
            ToolRisk::CredentialAdjacent
        );
    }

    #[test]
    fn classify_bash_git_mutation() {
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "git commit -m 'fix'"})),
            ToolRisk::GitMutation
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "git push origin main"})),
            ToolRisk::GitMutation
        );
    }

    #[test]
    fn classify_bash_readonly() {
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "ls -la"})),
            ToolRisk::Read
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "cat src/main.rs"})),
            ToolRisk::Read
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "git status"})),
            ToolRisk::Read
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "git log --oneline -5"})),
            ToolRisk::Read
        );
    }

    #[test]
    fn classify_bash_write() {
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "mkdir -p src/utils"})),
            ToolRisk::Write
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "touch src/new_file.rs"})),
            ToolRisk::Write
        );
        assert_eq!(
            classify_tool_risk("bash", &json!({"command": "cp file1 file2"})),
            ToolRisk::Write
        );
    }

    #[test]
    fn classify_unknown_tool() {
        assert_eq!(
            classify_tool_risk("some_new_tool", &json!({})),
            ToolRisk::Unknown
        );
    }

    #[test]
    fn summarize_empty_output() {
        assert_eq!(
            summarize_tool_output("bash", "", true),
            Some("empty output".to_string())
        );
    }

    #[test]
    fn summarize_test_failure_output() {
        let output = "running 3 tests\ntest test_a ... ok\ntest test_b ... FAILED\ntest test_c ... ok\n\nfailures:\n\n---- test_b stdout ----\nthread 'test_b' panicked at 'assertion failed'\n\nerror: test failed";
        assert!(summarize_tool_output("bash", output, false).is_some());
        let summary = summarize_tool_output("bash", output, false).unwrap();
        assert!(summary.contains("FAILED") || summary.contains("failed"));
    }

    #[test]
    fn summarize_compiler_error_output() {
        let output = "error[E0596]: cannot borrow `x` as mutable, as it is not declared as mutable\n --> src/main.rs:5:5\n  |\n3 | let x = &mut 5;\n  |         ------- first mutable borrow occurs here\n\nerror: aborting due to previous error";
        let summary = summarize_tool_output("bash", output, false).unwrap();
        assert!(summary.contains("error["));
    }

    #[test]
    fn summarize_short_output() {
        let output = "hello world";
        assert_eq!(
            summarize_tool_output("bash", output, true),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn summarize_long_output() {
        let output = (0..100)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let summary = summarize_tool_output("bash", &output, true).unwrap();
        assert!(summary.contains("100 lines"));
    }
}
