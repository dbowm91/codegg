//! Bounded terminal output and local-path redaction for the agent turn.
//!
//! Physical decomposition of [`super::r#loop`] (M003): the public
//! [`AgentLoopTerminalOutput`] collector plus the path-redaction helpers
//! used before tool text reaches the model or diagnostics. No execution
//! authority lives here.

use std::borrow::Cow;
use std::sync::LazyLock;

use super::r#loop::AgentLoop;
use crate::provider::ChatEvent;

/// Bounded public output collected from one ordinary agent-loop execution.
/// Reasoning deltas are intentionally excluded from this type.
#[derive(Debug, Clone)]
pub struct AgentLoopTerminalOutput {
    pub public_text: String,
    pub stop_reason: String,
    pub usage: Option<crate::provider::TokenUsage>,
    pub tool_event_count: usize,
}

static PATH_REDACTION_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
    let patterns = [
        r"/var/[^\s/]+",
        r"/tmp/[^\s/]+",
        r"C:\\Users\\[^\s\\]+",
        r"C:\\Program Files\\[^\s\\]+",
        r"C:\\Windows\\[^\s\\]+",
    ];
    patterns
        .iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect()
});

pub(super) fn redact_local_paths(
    input: &str,
    local_paths: &(Option<String>, Option<String>),
) -> String {
    let mut result = Cow::Borrowed(input);

    for (path, replacement) in [
        (local_paths.0.as_deref(), "[CWD]"),
        (local_paths.1.as_deref(), "[HOME]"),
    ] {
        if let Some(path) = path.filter(|path| !path.is_empty()) {
            if let Cow::Owned(replaced) = replace_path_prefixes(&result, path, replacement) {
                result = Cow::Owned(replaced);
            }
        }
    }

    for re in PATH_REDACTION_PATTERNS.iter() {
        if re.is_match(&result) {
            result = Cow::Owned(re.replace_all(&result, "[REDACTED_PATH]").into_owned());
        }
    }

    result.into_owned()
}
fn replace_path_prefixes<'a>(input: &'a str, path: &str, replacement: &str) -> Cow<'a, str> {
    let mut output = String::with_capacity(input.len());
    let mut copied_until = 0;
    let mut replaced = false;

    for (start, _) in input.match_indices(path) {
        let end = start + path.len();
        let has_path_boundary = input[end..].chars().next().map_or(true, |character| {
            !character.is_ascii_alphanumeric() && character != '_' && character != '-'
        });
        if !has_path_boundary {
            continue;
        }
        output.push_str(&input[copied_until..start]);
        output.push_str(replacement);
        copied_until = end;
        replaced = true;
    }

    if replaced {
        output.push_str(&input[copied_until..]);
        Cow::Owned(output)
    } else {
        Cow::Borrowed(input)
    }
}

impl AgentLoop {
    /// Collect the final visible output without exposing provider-private
    /// reasoning to host-owned specialized finalizers.
    pub fn terminal_output(events: &[ChatEvent]) -> AgentLoopTerminalOutput {
        let mut public_text = String::new();
        let mut stop_reason = String::from("unknown");
        let mut usage = None;
        let mut tool_event_count = 0;
        for event in events {
            match event {
                ChatEvent::TextDelta(text) => public_text.push_str(text),
                ChatEvent::ToolCall(_) | ChatEvent::ToolResult { .. } => tool_event_count += 1,
                ChatEvent::Finish {
                    stop_reason: reason,
                    usage: turn_usage,
                } => {
                    stop_reason = reason.to_string();
                    usage = Some(turn_usage.clone());
                }
                ChatEvent::ReasoningDelta(_) | ChatEvent::Error(_) => {}
            }
        }
        AgentLoopTerminalOutput {
            public_text,
            stop_reason,
            usage,
            tool_event_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_path_redaction_uses_workspace_and_respects_path_boundaries() {
        let paths = (
            Some("/Users/alice/project".to_string()),
            Some("/Users/alice".to_string()),
        );
        let input = "/Users/alice/project/src/main.rs /Users/alice-other/file";

        assert_eq!(
            redact_local_paths(input, &paths),
            "[CWD]/src/main.rs /Users/alice-other/file"
        );
    }

    #[test]
    fn local_path_redaction_ignores_empty_paths() {
        let paths = (Some(String::new()), Some(String::new()));
        assert_eq!(redact_local_paths("plain text", &paths), "plain text");
    }
}
