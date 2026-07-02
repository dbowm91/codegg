use crate::shell::store::ShellOutputEntry;
use codegg_protocol::ui::{ContainerNode, KeyValueEntry, KeyValueNode, TextNode, UiNode};

pub fn shell_detail_node(entry: &ShellOutputEntry) -> UiNode {
    let mut info_entries = vec![
        KeyValueEntry {
            key: "ID".into(),
            value: entry.id.0.to_string(),
        },
        KeyValueEntry {
            key: "Command".into(),
            value: entry.command.clone(),
        },
        KeyValueEntry {
            key: "CWD".into(),
            value: entry.cwd.display().to_string(),
        },
        KeyValueEntry {
            key: "Status".into(),
            value: format!("{:?}", entry.status),
        },
        KeyValueEntry {
            key: "Exit code".into(),
            value: match entry.exit_code {
                Some(code) => code.to_string(),
                None => "(none)".into(),
            },
        },
        KeyValueEntry {
            key: "Elapsed".into(),
            value: match entry.elapsed {
                Some(d) => format!("{}.{:03}s", d.as_secs(), d.subsec_millis()),
                None => "(pending)".into(),
            },
        },
        KeyValueEntry {
            key: "Promoted".into(),
            value: if entry.promoted { "yes" } else { "no" }.into(),
        },
        KeyValueEntry {
            key: "Truncated".into(),
            value: if entry.stdout.omitted_bytes > 0 || entry.stderr.omitted_bytes > 0 {
                "yes".into()
            } else {
                "no".into()
            },
        },
    ];

    if entry.promote_after {
        info_entries.push(KeyValueEntry {
            key: "Promote after".into(),
            value: "yes".into(),
        });
    }

    let stdout_text = bounded_to_text(&entry.stdout);
    let stderr_text = bounded_to_text(&entry.stderr);

    UiNode::Container(ContainerNode {
        title: Some("Shell Command".into()),
        children: vec![
            UiNode::KeyValue(KeyValueNode {
                entries: info_entries,
            }),
            UiNode::Container(ContainerNode {
                title: Some("Stdout".into()),
                children: vec![UiNode::Text(TextNode { text: stdout_text })],
            }),
            UiNode::Container(ContainerNode {
                title: Some("Stderr".into()),
                children: vec![UiNode::Text(TextNode { text: stderr_text })],
            }),
        ],
    })
}

fn bounded_to_text(output: &crate::shell::store::BoundedOutput) -> String {
    if output.is_empty() {
        return "(empty)".into();
    }
    let mut text = output.head_str_lossy();
    if output.omitted_bytes > 0 {
        text.push_str(&format!(
            "\n... ({} bytes omitted) ...\n",
            output.omitted_bytes
        ));
        text.push_str(&output.tail_str_lossy());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::store::BoundedOutput;
    use crate::shell::types::{ShellCommandId, ShellStatus};
    use crate::tui::components::ui_node_renderer::UiNodeRenderer;
    use std::path::PathBuf;
    use std::time::Duration;

    fn make_entry(
        command: &str,
        stdout: &[u8],
        stderr: &[u8],
        exit_code: Option<i32>,
    ) -> ShellOutputEntry {
        let mut so = BoundedOutput::new();
        so.append(stdout);
        let mut se = BoundedOutput::new();
        se.append(stderr);
        ShellOutputEntry {
            id: ShellCommandId(1),
            command: command.to_string(),
            cwd: PathBuf::from("/tmp"),
            started_at: std::time::SystemTime::now(),
            finished_at: Some(std::time::SystemTime::now()),
            status: if exit_code.is_some() {
                ShellStatus::Exited
            } else {
                ShellStatus::Running
            },
            exit_code,
            stdout: so,
            stderr: se,
            elapsed: Some(Duration::from_secs(1)),
            promoted: false,
            promote_after: false,
            capture_policy: crate::shell::types::ShellCapturePolicy::StoreEphemeral,
        }
    }

    #[test]
    fn test_shell_detail_node_includes_command_info() {
        let entry = make_entry("cargo test", b"ok\n", b"", Some(0));
        let node = shell_detail_node(&entry);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        assert!(text.contains("Shell Command"), "missing title: {text}");
        assert!(text.contains("cargo test"), "missing command: {text}");
        assert!(text.contains("/tmp"), "missing cwd: {text}");
        assert!(text.contains("Exited"), "missing status: {text}");
        assert!(text.contains("Exit code: 0"), "missing exit code: {text}");
    }

    #[test]
    fn test_shell_detail_node_empty_stdout_shows_empty_placeholder() {
        let entry = make_entry("true", b"", b"", Some(0));
        let node = shell_detail_node(&entry);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        assert!(
            text.contains("(empty)"),
            "should show empty placeholder: {text}"
        );
    }

    #[test]
    fn test_shell_detail_node_empty_stderr_shows_empty_placeholder() {
        let entry = make_entry("echo hi", b"hi\n", b"", Some(0));
        let node = shell_detail_node(&entry);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        let stderr_idx = text.find("Stderr:").expect("should have stderr section");
        let after_stderr = &text[stderr_idx..];
        assert!(
            after_stderr.contains("(empty)"),
            "stderr should show empty: {after_stderr}"
        );
    }

    #[test]
    fn test_shell_detail_node_failed_exit_code_shown() {
        let entry = make_entry("cargo check", b"", b"error\n", Some(1));
        let node = shell_detail_node(&entry);
        let lines = UiNodeRenderer::node_to_lines(&node);
        let text = lines.join("\n");
        assert!(
            text.contains("Exit code: 1"),
            "should show exit code 1: {text}"
        );
    }

    #[test]
    fn test_shell_detail_node_renders_to_lines_without_panic() {
        let entry = make_entry(
            "long command",
            b"stdout line 1\nstdout line 2\n",
            b"stderr line 1\n",
            Some(0),
        );
        let node = shell_detail_node(&entry);
        let lines = UiNodeRenderer::node_to_lines(&node);
        assert!(!lines.is_empty(), "should produce output lines");
    }
}
