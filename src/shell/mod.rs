pub mod digest;
pub mod policy;
pub mod runtime;
pub mod store;
pub mod types;

pub use digest::{ShellDigest, ShellFailure, ShellFailureKind, TruncationReport};
pub use policy::{evaluate_command, HumanShellPolicyDecision};
pub use runtime::{ShellHandle, ShellRuntime};
pub use store::{BoundedOutput, ShellOutputEntry, ShellOutputStore};
pub use types::{
    classify_prompt_submission, PromptSubmissionKind, ShellCapturePolicy, ShellCommandId,
    ShellEvent, ShellOrigin, ShellRequest, ShellStatus, DEFAULT_MAX_BYTES_PER_COMMAND,
    DEFAULT_MAX_HISTORY_ENTRIES, DEFAULT_MAX_TOTAL_BYTES, DEFAULT_TIMEOUT_SECS,
};

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
    }

    #[test]
    fn constants_are_sane() {
        const _: () = assert!(DEFAULT_TIMEOUT_SECS > 0);
        const _: () = assert!(DEFAULT_MAX_BYTES_PER_COMMAND > 0);
        const _: () = assert!(DEFAULT_MAX_TOTAL_BYTES >= DEFAULT_MAX_BYTES_PER_COMMAND);
        const _: () = assert!(DEFAULT_MAX_HISTORY_ENTRIES > 0);
    }
}
