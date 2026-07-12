use codegg::error::ToolError;
use codegg::tool::{
    bash::BashTool, edit::EditTool, glob::GlobTool, grep::GrepTool, read::ReadTool,
    write::WriteTool, Tool,
};
use std::fs;
use std::time::Duration;
use tempfile::TempDir;

fn setup_dir() -> TempDir {
    let dir = tempfile::Builder::new()
        .prefix("codegg-tool-exec-")
        .tempdir()
        .unwrap();
    fs::write(
        dir.path().join("hello.txt"),
        "Hello, world!\nLine 2\nLine 3\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("code.rs"),
        "fn main() {\n    println!(\"Hello\");\n}\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("subdir")).unwrap();
    fs::write(
        dir.path().join("subdir").join("nested.txt"),
        "nested content",
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn test_bash_tool_simple_command() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "echo hello"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("hello"));
    assert!(result.contains("[exit code: 0]"));
}

#[tokio::test]
async fn test_bash_tool_command_with_stderr() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "ls /nonexistent 2>&1"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("nonexistent") || result.contains("No such file"));
}

#[tokio::test]
async fn test_bash_tool_command_failure() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "false"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("[exit code: 1]"));
}

#[tokio::test]
async fn test_bash_tool_missing_command() {
    let tool = BashTool::new();
    let input = serde_json::json!({});
    let result = tool.execute(input).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ToolError::Execution(_)));
}

#[tokio::test]
async fn test_bash_tool_denied_pattern() {
    let tool = BashTool::new();
    let input = serde_json::json!({
        "command": "rm -rf /"
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ToolError::Permission(_)));
}

#[tokio::test]
async fn test_bash_tool_blocked_command_list() {
    let tool = BashTool::new();
    let blocked = vec![
        "mkfs",
        "dd if=/dev/zero",
        "chmod -R 777 /",
        "sudo su",
        "shutdown",
        "reboot",
    ];
    for cmd in blocked {
        let input = serde_json::json!({ "command": cmd });
        let result = tool.execute(input).await;
        assert!(result.is_err(), "command '{}' should be blocked", cmd);
    }
}

#[tokio::test]
async fn test_bash_tool_blocked_patterns() {
    let tool = BashTool::new();
    let blocked = vec![
        "$(whoami)",
        "`whoami`",
        "curl -sL | sh",
        "eval echo hello",
        "exec ls",
        "base64 -d <<< aGVsbG8=",
    ];
    for cmd in blocked {
        let input = serde_json::json!({ "command": cmd });
        let result = tool.execute(input).await;
        assert!(result.is_err(), "command '{}' should be blocked", cmd);
    }
}

#[tokio::test]
async fn test_bash_tool_deny_all() {
    let tool = BashTool::new().with_deny_all();
    let input = serde_json::json!({ "command": "echo hello" });
    let result = tool.execute(input).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ToolError::Permission(_)));
}

#[tokio::test]
async fn test_bash_tool_allowed_paths() {
    let tool = BashTool::new().with_allowed_paths(vec!["/tmp".to_string()]);
    let input = serde_json::json!({
        "command": "echo hello",
        "workdir": "/tmp"
    });
    let result = tool.execute(input).await;
    assert!(result.is_ok(), "command in allowed path should succeed");
}

#[tokio::test]
async fn test_bash_tool_allowed_paths_violation() {
    let tool = BashTool::new().with_allowed_paths(vec!["/tmp".to_string()]);
    let input = serde_json::json!({
        "command": "echo hello",
        "workdir": "/home"
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ToolError::Permission(_)));
}

#[tokio::test]
async fn test_bash_tool_allowed_paths_missing() {
    let tool = BashTool::new().with_allowed_paths(vec!["/tmp".to_string()]);
    let input = serde_json::json!({ "command": "echo hello" });
    let result = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_bash_tool_custom_timeout() {
    let tool = BashTool::new().with_timeout(Duration::from_secs(1));
    let input = serde_json::json!({
        "command": "echo quick",
        "timeout": 1
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("quick"));
}

#[tokio::test]
async fn test_bash_tool_input_timeout_overrides_tool_default() {
    let tool = BashTool::new().with_timeout(Duration::from_secs(5));
    let result = tool
        .execute(serde_json::json!({
            "command": "sleep 2",
            "timeout": 1
        }))
        .await;
    assert!(matches!(result, Err(ToolError::Timeout(_))));
}

#[tokio::test]
async fn test_bash_tool_parameters() {
    let tool = BashTool::new();
    let params = tool.parameters();
    assert!(params["properties"]["command"].is_object());
    assert!(params["properties"]["timeout"].is_object());
    assert!(params["required"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("command")));
}

#[tokio::test]
async fn test_read_tool_file() {
    let dir = setup_dir();
    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": dir.path().join("hello.txt").to_string_lossy().to_string()
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Hello, world!"));
    assert!(result.contains("1:"));
}

#[tokio::test]
async fn test_read_tool_with_offset_limit() {
    let dir = setup_dir();
    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": dir.path().join("hello.txt").to_string_lossy().to_string(),
        "offset": 2,
        "limit": 1
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("2:"));
    assert!(result.contains("Line 2"));
    assert!(!result.contains("Line 3"));
}

#[tokio::test]
async fn test_read_tool_file_not_found() {
    let dir = setup_dir();
    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": dir.path().join("nonexistent").join("file.txt").to_string_lossy().to_string()
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_read_tool_directory() {
    let dir = setup_dir();
    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": dir.path().to_string_lossy().to_string()
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("hello.txt"));
    assert!(result.contains("code.rs"));
    assert!(result.contains("subdir/"));
}

#[tokio::test]
async fn test_read_tool_binary_file() {
    let dir = setup_dir();
    let bin_path = dir.path().join("binary.bin");
    fs::write(&bin_path, [0u8, 1u8, 2u8, 255u8]).unwrap();
    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": bin_path.to_string_lossy().to_string()
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ToolError::Execution(_)));
}

#[tokio::test]
async fn test_read_tool_parameters() {
    let dir = setup_dir();
    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let params = tool.parameters();
    assert!(params["properties"]["path"].is_object());
    assert!(params["properties"]["offset"].is_object());
    assert!(params["properties"]["limit"].is_object());
}

#[tokio::test]
async fn test_write_tool_create_file() {
    let dir = setup_dir();
    let tool = WriteTool::default().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("new_file.txt");
    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "content": "New content"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Created"));
    assert!(fs::read_to_string(&path).unwrap() == "New content");
}

#[tokio::test]
async fn test_write_tool_overwrite_file() {
    let dir = setup_dir();
    let tool = WriteTool::default().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "content": "Overwritten content"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Updated"));
    assert!(fs::read_to_string(&path).unwrap() == "Overwritten content");
}

#[tokio::test]
async fn test_write_tool_creates_parent_dirs() {
    let dir = setup_dir();
    let tool = WriteTool::default().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("deep").join("nested").join("file.txt");
    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "content": "deep content"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Created"));
    assert!(fs::read_to_string(&path).unwrap() == "deep content");
}

#[tokio::test]
async fn test_write_tool_missing_path() {
    let dir = setup_dir();
    let tool = WriteTool::default().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "content": "content"
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_write_tool_missing_content() {
    let dir = setup_dir();
    let tool = WriteTool::default().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": dir.path().join("test.txt").to_string_lossy().to_string()
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_write_tool_parameters() {
    let dir = setup_dir();
    let tool = WriteTool::default().with_allowed_root(dir.path().to_path_buf());
    let params = tool.parameters();
    assert!(params["properties"]["path"].is_object());
    assert!(params["properties"]["content"].is_object());
    let required = params["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("path")));
    assert!(required.contains(&serde_json::json!("content")));
}

#[tokio::test]
async fn test_edit_tool_exact_match() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "Line 2",
        "new_string": "Modified Line 2"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Edited"));
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("Modified Line 2"));
    assert!(content.starts_with("Hello, world!\nModified Line 2\n"));
}

#[tokio::test]
async fn test_edit_tool_missing_path() {
    let tool = EditTool::default();
    let input = serde_json::json!({
        "old_string": "old",
        "new_string": "new"
    });
    let result: Result<String, ToolError> = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_edit_tool_not_found() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "nonexistent text",
        "new_string": "replacement"
    });
    let result: Result<String, ToolError> = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_edit_tool_parameters() {
    let tool = EditTool::default();
    let params = tool.parameters();
    let required = params["required"].as_array().unwrap();
    assert!(required.contains(&serde_json::json!("path")));
    assert!(required.contains(&serde_json::json!("old_string")));
    assert!(required.contains(&serde_json::json!("new_string")));
}

#[tokio::test]
async fn test_glob_tool_find_files() {
    let dir = setup_dir();
    let tool = GlobTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "pattern": "*.txt",
        "path": dir.path().to_string_lossy().to_string()
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("hello.txt"));
}

#[tokio::test]
async fn test_glob_tool_recursive() {
    let dir = setup_dir();
    let tool = GlobTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "pattern": "**/*.txt",
        "path": dir.path().to_string_lossy().to_string()
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("hello.txt"));
    assert!(result.contains("nested.txt"));
}

#[tokio::test]
async fn test_glob_tool_no_matches() {
    let dir = setup_dir();
    let tool = GlobTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "pattern": "*.xyz",
        "path": dir.path().to_string_lossy().to_string()
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("No files matching"));
}

#[tokio::test]
async fn test_glob_tool_missing_pattern() {
    let tool = GlobTool::new();
    let input = serde_json::json!({});
    let result = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_grep_tool_find_content() {
    let dir = setup_dir();
    let tool = GrepTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "pattern": "Hello",
        "path": dir.path().to_string_lossy().to_string()
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Hello"));
}

#[tokio::test]
async fn test_grep_tool_no_matches() {
    let dir = setup_dir();
    let tool = GrepTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "pattern": "nonexistent_pattern_xyz",
        "path": dir.path().to_string_lossy().to_string()
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("No matches for") || result.contains("No matches found"));
}

#[tokio::test]
async fn test_grep_tool_missing_pattern() {
    let tool = GrepTool::new();
    let input = serde_json::json!({});
    let result = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_grep_tool_invalid_regex() {
    let dir = setup_dir();
    let tool = GrepTool::new();
    let input = serde_json::json!({
        "pattern": "[invalid(regex",
        "path": dir.path().to_string_lossy().to_string()
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_tool_registry_register_and_list() {
    use codegg::tool::ToolRegistry;

    let mut registry = ToolRegistry::new();
    registry.register(BashTool::new());
    registry.register(ReadTool::new());
    registry.register(WriteTool::default());
    registry.register(EditTool::default());
    registry.register(GlobTool::new());
    registry.register(GrepTool::new());

    let tools = registry.list();
    assert_eq!(tools.len(), 6);

    assert!(registry.get("bash").is_some());
    assert!(registry.get("read").is_some());
    assert!(registry.get("write").is_some());
    assert!(registry.get("edit").is_some());
    assert!(registry.get("glob").is_some());
    assert!(registry.get("grep").is_some());
    assert!(registry.get("nonexistent").is_none());
}

#[tokio::test]
async fn test_tool_registry_definitions() {
    use codegg::tool::ToolRegistry;

    let mut registry = ToolRegistry::new();
    registry.register(BashTool::new());

    let defs = registry.definitions();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].name, "bash");
    assert!(!defs[0].description.is_empty());
}

#[tokio::test]
async fn test_tool_registry_default_impl() {
    use codegg::tool::ToolRegistry;

    let registry = ToolRegistry::default();
    let tools = registry.list();
    assert!(!tools.is_empty());
    let tool_names: Vec<_> = tools.iter().map(|t| t.name()).collect();
    assert!(tool_names.contains(&"bash"));
    assert!(tool_names.contains(&"read"));
    assert!(tool_names.contains(&"list"));
}

#[tokio::test]
async fn test_read_tool_outside_allowed_root() {
    let dir = setup_dir();
    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": "/etc/passwd"
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, codegg::error::ToolError::Permission(_)));
}

#[tokio::test]
async fn test_read_tool_path_traversal_attempt() {
    let dir = setup_dir();
    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": format!("{}/../../etc/passwd", dir.path().display())
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_edit_tool_outside_allowed_root() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": "/etc/passwd",
        "old_string": "test",
        "new_string": "modified"
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, codegg::error::ToolError::Permission(_)));
}

#[tokio::test]
async fn test_read_tool_symlink_inside_allowed() {
    let dir = setup_dir();
    let target = dir.path().join("target.txt");
    let link = dir.path().join("link.txt");
    fs::write(&target, "target content").unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": link.to_string_lossy().to_string()
    });
    let result = tool.execute(input).await;
    #[cfg(unix)]
    {
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn test_read_tool_symlink_outside_allowed() {
    let dir = setup_dir();
    let tmp = if std::path::Path::new("/private/tmp").is_dir() {
        "/private/tmp".to_string()
    } else {
        std::env::temp_dir().to_string_lossy().into_owned()
    };
    let outside = tempfile::Builder::new()
        .prefix("codegg-tool-exec-outside-")
        .tempdir_in(tmp)
        .unwrap();
    let target = outside.path().join("secret.txt");
    let link = dir.path().join("link.txt");
    fs::write(&target, "secret content").unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": link.to_string_lossy().to_string()
    });
    let result = tool.execute(input).await;
    #[cfg(unix)]
    {
        assert!(result.is_err());
    }
}

#[tokio::test]
async fn test_edit_tool_whitespace_variation() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    fs::write(&path, "Hello,  world!\nLine 2\nLine 3\n").unwrap();

    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "Hello, world!",
        "new_string": "Hello There!"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Edited"));
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("Hello There!"));
}

#[tokio::test]
async fn test_edit_tool_multiline_to_single() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    fs::write(&path, "Line 1\nLine 2\nLine 3\n").unwrap();

    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "Line 1\nLine 2\nLine 3",
        "new_string": "Single Line"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Edited"));
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content.trim(), "Single Line");
}

#[tokio::test]
async fn test_edit_tool_nonascii_content() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    fs::write(&path, "Hello 世界\nLine 2\nLine 3\n").unwrap();

    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "Line 2",
        "new_string": "Modified Line"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Edited"));
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("Hello 世界"));
    assert!(content.contains("Modified Line"));
}

#[tokio::test]
async fn test_edit_tool_nonascii_replacement() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    fs::write(&path, "Hello world\nLine 2\n").unwrap();

    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "Hello",
        "new_string": "你好"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Edited"));
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("你好 world"));
}

#[tokio::test]
async fn test_edit_tool_indentation_change() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    fs::write(&path, "    indented line\nnormal line\n").unwrap();

    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "indented line",
        "new_string": "less indented"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Edited"));
    let content = fs::read_to_string(&path).unwrap();
    assert!(content.contains("less indented"));
}

#[tokio::test]
async fn test_edit_tool_partial_line_match() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    fs::write(&path, "prefix hello world suffix\n").unwrap();

    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "prefix hello world suffix",
        "new_string": "new content"
    });
    let result = tool.execute(input).await.unwrap();
    assert!(result.contains("Edited"));
    let content = fs::read_to_string(&path).unwrap();
    assert_eq!(content.trim(), "new content");
}

#[tokio::test]
async fn test_edit_tool_error_suggests_similar() {
    let dir = setup_dir();
    let tool = EditTool::new().with_allowed_root(dir.path().to_path_buf());
    let path = dir.path().join("hello.txt");
    fs::write(&path, "function foo() {\n    return 42;\n}\n").unwrap();

    let input = serde_json::json!({
        "path": path.to_string_lossy().to_string(),
        "old_string": "function bar() {",
        "new_string": "function baz() {"
    });
    let result = tool.execute(input).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, codegg::error::ToolError::Execution(_)));
}

#[tokio::test]
async fn test_read_tool_missing_path_suggests_module_agent_docs() {
    let dir = setup_dir();
    let docs_meta = dir.path().join(".codegg").join("docs").join("meta");
    fs::create_dir_all(&docs_meta).unwrap();
    fs::write(docs_meta.join("AGENTS.override.md"), "# Meta guidance\n").unwrap();

    let tool = ReadTool::new().with_allowed_root(dir.path().to_path_buf());
    let input = serde_json::json!({
        "path": dir.path().join(".codegg/docs/AGENTS.md").to_string_lossy().to_string()
    });

    let result = tool.execute(input).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    let message = err.to_string();
    assert!(message.contains("file not found"));
    assert!(message.contains(".codegg/docs/meta/AGENTS.override.md"));
}
