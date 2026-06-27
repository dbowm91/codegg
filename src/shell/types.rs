use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellOrigin {
    HumanEphemeral,
    HumanPromoted,
    AgentTool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellCapturePolicy {
    DisplayOnly,
    StoreEphemeral,
    StoreAndPromote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShellCommandId(pub u64);

#[derive(Debug, Clone)]
pub struct ShellRequest {
    pub id: ShellCommandId,
    pub origin: ShellOrigin,
    pub command: String,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub capture_policy: ShellCapturePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellEvent {
    Started {
        id: ShellCommandId,
        command: String,
        cwd: PathBuf,
    },
    Stdout {
        id: ShellCommandId,
        bytes: Vec<u8>,
    },
    Stderr {
        id: ShellCommandId,
        bytes: Vec<u8>,
    },
    Exited {
        id: ShellCommandId,
        status: Option<i32>,
        elapsed: Duration,
    },
    TimedOut {
        id: ShellCommandId,
        elapsed: Duration,
    },
    FailedToStart {
        id: ShellCommandId,
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellStatus {
    Running,
    Exited,
    TimedOut,
    FailedToStart,
}

pub const DEFAULT_TIMEOUT_SECS: u64 = 300;
pub const DEFAULT_MAX_BYTES_PER_COMMAND: usize = 1_000_000;
pub const DEFAULT_MAX_TOTAL_BYTES: usize = 8_000_000;
pub const DEFAULT_MAX_HISTORY_ENTRIES: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptSubmissionKind {
    Chat(String),
    Slash(String),
    HumanShell {
        command: String,
        promote_after: bool,
    },
}

pub fn classify_prompt_submission(input: &str) -> PromptSubmissionKind {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return PromptSubmissionKind::Chat(trimmed.to_string());
    }
    if trimmed.starts_with('/') {
        return PromptSubmissionKind::Slash(trimmed.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("!!") {
        let cmd = rest.trim();
        if cmd.is_empty() {
            return PromptSubmissionKind::Chat(trimmed.to_string());
        }
        return PromptSubmissionKind::HumanShell {
            command: cmd.to_string(),
            promote_after: true,
        };
    }
    if let Some(rest) = trimmed.strip_prefix('!') {
        let cmd = rest.trim();
        if cmd.is_empty() {
            return PromptSubmissionKind::Chat(trimmed.to_string());
        }
        return PromptSubmissionKind::HumanShell {
            command: cmd.to_string(),
            promote_after: false,
        };
    }
    PromptSubmissionKind::Chat(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_normal_text() {
        assert_eq!(
            classify_prompt_submission("hello world"),
            PromptSubmissionKind::Chat("hello world".to_string())
        );
    }

    #[test]
    fn slash_command() {
        assert_eq!(
            classify_prompt_submission("/shell-list"),
            PromptSubmissionKind::Slash("/shell-list".to_string())
        );
    }

    #[test]
    fn human_shell_single_bang() {
        assert_eq!(
            classify_prompt_submission("!cargo test"),
            PromptSubmissionKind::HumanShell {
                command: "cargo test".to_string(),
                promote_after: false,
            }
        );
    }

    #[test]
    fn human_shell_double_bang() {
        assert_eq!(
            classify_prompt_submission("!!cargo test"),
            PromptSubmissionKind::HumanShell {
                command: "cargo test".to_string(),
                promote_after: true,
            }
        );
    }

    #[test]
    fn empty_bang_is_chat() {
        assert_eq!(
            classify_prompt_submission("!"),
            PromptSubmissionKind::Chat("!".to_string())
        );
    }

    #[test]
    fn empty_double_bang_is_chat() {
        assert_eq!(
            classify_prompt_submission("!!"),
            PromptSubmissionKind::Chat("!!".to_string())
        );
    }

    #[test]
    fn empty_string_is_chat() {
        assert_eq!(
            classify_prompt_submission(""),
            PromptSubmissionKind::Chat("".to_string())
        );
    }

    #[test]
    fn bang_with_extra_whitespace() {
        assert_eq!(
            classify_prompt_submission("!  cargo test"),
            PromptSubmissionKind::HumanShell {
                command: "cargo test".to_string(),
                promote_after: false,
            }
        );
    }

    #[test]
    fn double_bang_with_extra_whitespace() {
        assert_eq!(
            classify_prompt_submission("!!  cargo test"),
            PromptSubmissionKind::HumanShell {
                command: "cargo test".to_string(),
                promote_after: true,
            }
        );
    }

    #[test]
    fn single_bang_with_pipes() {
        assert_eq!(
            classify_prompt_submission("!grep foo bar | wc -l"),
            PromptSubmissionKind::HumanShell {
                command: "grep foo bar | wc -l".to_string(),
                promote_after: false,
            }
        );
    }

    #[test]
    fn command_id_equality() {
        let a = ShellCommandId(1);
        let b = ShellCommandId(1);
        let c = ShellCommandId(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn command_id_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(ShellCommandId(1));
        set.insert(ShellCommandId(2));
        set.insert(ShellCommandId(1));
        assert_eq!(set.len(), 2);
    }
}
