pub mod digest;
pub mod policy;
pub mod projection;
pub mod projection_bridge;
pub mod runtime;
pub mod store;
pub mod types;

pub use digest::{ShellDigest, ShellFailure, ShellFailureKind, TruncationReport};
pub use policy::{evaluate_command, HumanShellPolicyDecision};
pub use projection::{
    default_command_projection, default_command_projection_with_budget, CommandExit,
    CommandOutputStore, CommandOutputStoreLimits, CommandOutputStream, CommandRun, CommandRunId,
    OutputCompleteness, OutputEncoding, OutputHandle, ProjectionHandle, RawStream, RedactionState,
    COMMAND_OUTPUT_MAX_HISTORY_ENTRIES, COMMAND_OUTPUT_MAX_RETAINED_BYTES,
    COMMAND_OUTPUT_MAX_SINGLE_STREAM_BYTES, DEFAULT_PROJECTION_BUDGET_BYTES,
};
pub use projection_bridge::ShellCommandRunBridge;
pub use runtime::{ShellHandle, ShellRuntime};
pub use store::{BoundedOutput, ShellOutputEntry, ShellOutputStore};
pub use types::{
    classify_prompt_submission, PromptSubmissionKind, ShellCapturePolicy, ShellCommandId,
    ShellEnvPolicy, ShellEvent, ShellOrigin, ShellPromotionMode, ShellRequest, ShellStatus,
    DEFAULT_MAX_BYTES_PER_COMMAND, DEFAULT_MAX_HISTORY_ENTRIES, DEFAULT_MAX_TOTAL_BYTES,
    DEFAULT_TIMEOUT_SECS,
};

pub fn sanitize_ansi(input: &str, mode: crate::config::schema::AnsiMode) -> String {
    match mode {
        crate::config::schema::AnsiMode::Raw => input.to_string(),
        crate::config::schema::AnsiMode::Strip => {
            let mut out = String::with_capacity(input.len());
            let mut chars = input.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\x1b' {
                    if chars.peek() == Some(&'[') {
                        chars.next();
                        while let Some(&nc) = chars.peek() {
                            chars.next();
                            if nc.is_ascii_alphabetic() {
                                break;
                            }
                        }
                    } else {
                        out.push(c);
                    }
                } else {
                    out.push(c);
                }
            }
            out
        }
        crate::config::schema::AnsiMode::SgrOnly => {
            let mut out = String::with_capacity(input.len());
            let mut chars = input.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '\x1b' {
                    if chars.peek() == Some(&'[') {
                        chars.next();
                        let mut seq = String::new();
                        while let Some(&nc) = chars.peek() {
                            chars.next();
                            seq.push(nc);
                            if nc.is_ascii_alphabetic() {
                                break;
                            }
                        }
                        if seq.starts_with(|c: char| c.is_ascii_digit() || c == ';')
                            && seq.ends_with('m')
                        {
                            out.push('\x1b');
                            out.push('[');
                            out.push_str(&seq);
                        }
                    } else {
                        out.push(c);
                    }
                } else {
                    out.push(c);
                }
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_exports_compile() {
        let _ = std::mem::size_of::<ShellCommandId>();
        let _ = std::mem::size_of::<ShellEvent>();
        let _ = std::mem::size_of::<ShellStatus>();
        let _ = std::mem::size_of::<ShellOrigin>();
        let _ = std::mem::size_of::<ShellCapturePolicy>();
        let _ = std::mem::size_of::<ShellPromotionMode>();
        let _ = std::mem::size_of::<ShellEnvPolicy>();
        let _ = std::mem::size_of::<CommandRunId>();
        let _ = std::mem::size_of::<OutputHandle>();
        let _ = std::mem::size_of::<CommandOutputStream>();
        let _ = std::mem::size_of::<OutputCompleteness>();
        let _ = std::mem::size_of::<OutputEncoding>();
        let _ = std::mem::size_of::<RedactionState>();
    }

    #[test]
    fn constants_are_sane() {
        const _: () = assert!(DEFAULT_TIMEOUT_SECS > 0);
        const _: () = assert!(DEFAULT_MAX_BYTES_PER_COMMAND > 0);
        const _: () = assert!(DEFAULT_MAX_TOTAL_BYTES >= DEFAULT_MAX_BYTES_PER_COMMAND);
        const _: () = assert!(DEFAULT_MAX_HISTORY_ENTRIES > 0);
    }

    #[test]
    fn ansi_strip_removes_csi() {
        let input = "hello\x1b[31mworld\x1b[0m";
        let result = sanitize_ansi(input, crate::config::schema::AnsiMode::Strip);
        assert_eq!(result, "helloworld");
    }

    #[test]
    fn ansi_sgr_only_keeps_color() {
        let input = "hello\x1b[31mworld\x1b[0m";
        let result = sanitize_ansi(input, crate::config::schema::AnsiMode::SgrOnly);
        assert_eq!(result, "hello\x1b[31mworld\x1b[0m");
    }

    #[test]
    fn ansi_sgr_only_removes_cursor() {
        let input = "hello\x1b[2J\x1b[Hworld";
        let result = sanitize_ansi(input, crate::config::schema::AnsiMode::SgrOnly);
        assert_eq!(result, "helloworld");
    }

    #[test]
    fn ansi_raw_passthrough() {
        let input = "hello\x1b[31mworld\x1b[0m";
        let result = sanitize_ansi(input, crate::config::schema::AnsiMode::Raw);
        assert_eq!(result, input);
    }
}
