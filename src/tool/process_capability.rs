//! M005 C005 — the effective-runtime-effect classification of a tool.
//!
//! # Why a table and not a flag
//!
//! The restricted shared-checkout profile has to answer one question about
//! every tool: *can this tool start an OS process?* The answer cannot come
//! from the tool's advertised metadata, because the advertisement is not
//! the implementation:
//!
//! * a plugin may declare itself read-only and still be process-backed;
//! * a tool's name says nothing about what it does — `bash` is a command,
//!   `test` is a command, `formatter` is a command, and `job` is a
//!   *submission* that will become a command;
//! * a model's stated intent is not a property of the tool at all.
//!
//! So the classification is CodeGG's own, made against the tool's
//! implementation, and it is **maintained by a guard** rather than by
//! convention. `scripts/check_process_denial.py` fails when a tool that
//! starts a process is missing from this table, and when this table and the
//! disposition manifest disagree.
//!
//! # The three shapes
//!
//! * [`ToolProcessCapability::None`] — the tool starts no process and
//!   creates no child execution that could.
//! * [`ToolProcessCapability::Spawns`] — the tool itself starts a process.
//!   The refusal happens **at the tool**, before the process exists, and
//!   again at the broker's final dispatch.
//! * [`ToolProcessCapability::Delegates`] — the tool creates a child
//!   execution. The child **inherits the narrow profile**: a nested
//!   subagent cannot be handed a wider authority than its parent.
//! * [`ToolProcessCapability::SubmitsProcessWork`] — the tool submits work
//!   whose process class is only known downstream, at the scheduler. The
//!   refusal happens there, where the job kind is known, so a job that
//!   starts no process is not refused with one that does.
//!
//! A Tool Program is classified as [`ToolProcessCapability::Spawns`]: its
//! frozen manifest names broker tools, and a broker tool's effect is not
//! guaranteed by the manifest — a plugin tool may be process-backed — so a
//! Tool Program is a process surface by construction and is refused as one.

use codegg_core::process_admission::ProcessKind;

/// The effective runtime effect of a tool, as C005 classifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolProcessCapability {
    /// The tool starts no process and creates no process-capable child.
    None,
    /// The tool itself starts a process of this kind.
    Spawns(ProcessKind),
    /// The tool creates a child execution that inherits the narrow profile.
    Delegates(ProcessKind),
    /// The tool submits work whose process class the admission authority
    /// decides downstream.
    SubmitsProcessWork,
    /// The tool *uses* a persistent process — a language server, a local
    /// MCP server — rather than starting one.
    ///
    /// A shared checkout admits no unqualified persistent process either:
    /// the server was started under an authority the checkout no longer
    /// has, and it is not CodeGG's to stop. The refusal is therefore at
    /// the *use*, which is the boundary the tool layer owns.
    UsesPersistentProcess(ProcessKind),
}

impl ToolProcessCapability {
    /// The process kind this capability implies, when it implies one.
    pub const fn process_kind(self) -> Option<ProcessKind> {
        match self {
            Self::None => None,
            Self::Spawns(kind) | Self::Delegates(kind) => Some(kind),
            Self::SubmitsProcessWork => Some(ProcessKind::ScheduledJob),
            Self::UsesPersistentProcess(kind) => Some(kind),
        }
    }

    /// Whether the tool itself starts a process.
    pub const fn spawns_itself(self) -> bool {
        matches!(self, Self::Spawns(_))
    }

    /// Whether the tool uses a persistent process the host owns.
    pub const fn uses_persistent_process(self) -> bool {
        matches!(self, Self::UsesPersistentProcess(_))
    }

    /// Whether the tool creates a child execution that must inherit the
    /// narrow profile.
    pub const fn delegates(self) -> bool {
        matches!(self, Self::Delegates(_))
    }
}

/// The built-in tools that start a process.
///
/// Every entry here is a **built-in**. A plugin tool is classified at
/// invocation time by the plugin runtime's kind, because a plugin's
/// advertised metadata is not evidence of its effect.
const BUILTIN_PROCESS_CAPABILITIES: &[(&str, ToolProcessCapability)] = &[
    ("bash", ToolProcessCapability::Spawns(ProcessKind::Command)),
    ("terminal", ToolProcessCapability::Spawns(ProcessKind::Pty)),
    (
        "python_script",
        ToolProcessCapability::Spawns(ProcessKind::Command),
    ),
    ("test", ToolProcessCapability::Spawns(ProcessKind::Command)),
    (
        "verify",
        ToolProcessCapability::Spawns(ProcessKind::Command),
    ),
    (
        "ci_watch",
        ToolProcessCapability::Spawns(ProcessKind::Command),
    ),
    (
        "formatter",
        ToolProcessCapability::Spawns(ProcessKind::Command),
    ),
    (
        "tool_program",
        ToolProcessCapability::Spawns(ProcessKind::ToolProgram),
    ),
    (
        "task",
        ToolProcessCapability::Delegates(ProcessKind::Command),
    ),
    ("job", ToolProcessCapability::SubmitsProcessWork),
    (
        "lsp",
        ToolProcessCapability::UsesPersistentProcess(ProcessKind::LanguageServer),
    ),
    (
        "lsp_read",
        ToolProcessCapability::UsesPersistentProcess(ProcessKind::LanguageServer),
    ),
];

/// The process capability of a built-in tool.
pub fn process_capability(tool_name: &str) -> ToolProcessCapability {
    BUILTIN_PROCESS_CAPABILITIES
        .iter()
        .find(|(name, _)| *name == tool_name)
        .map(|(_, capability)| *capability)
        .unwrap_or(ToolProcessCapability::None)
}

/// The built-in tools that start a process, in declaration order.
pub fn process_capable_tool_names() -> Vec<&'static str> {
    BUILTIN_PROCESS_CAPABILITIES
        .iter()
        .filter(|(_, capability)| capability.spawns_itself())
        .map(|(name, _)| *name)
        .collect()
}

/// The built-in tools a shared checkout denies.
///
/// A shared checkout admits no tool that **starts** a process in it and no
/// tool that **uses** a persistent process the host owns: the server was
/// started under an authority the checkout no longer has, and it is not
/// CodeGG's to stop.
///
/// A tool that *delegates* to a child execution (`task`) or *submits* work
/// (`job`) is deliberately not on this list. The child runs in its own
/// scope and the job's executor has its own admission, so neither writes the
/// shared checkout — and a shared group is *made of* delegating children, so
/// denying the delegating tool would deny the profile to itself.
pub fn shared_profile_denied_tools(base: &[String]) -> Vec<String> {
    let mut denied = base.to_vec();
    denied.extend(
        BUILTIN_PROCESS_CAPABILITIES
            .iter()
            .filter(|(_, capability)| {
                capability.spawns_itself() || capability.uses_persistent_process()
            })
            .map(|(name, _)| (*name).to_owned()),
    );
    denied.sort();
    denied.dedup();
    denied
}

/// Every built-in tool with a declared process capability.
pub fn declared_process_capabilities() -> &'static [(&'static str, ToolProcessCapability)] {
    BUILTIN_PROCESS_CAPABILITIES
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_process_tools_are_classified_by_effect() {
        assert_eq!(
            process_capability("bash"),
            ToolProcessCapability::Spawns(ProcessKind::Command)
        );
        assert_eq!(
            process_capability("terminal"),
            ToolProcessCapability::Spawns(ProcessKind::Pty)
        );
        assert_eq!(
            process_capability("python_script"),
            ToolProcessCapability::Spawns(ProcessKind::Command)
        );
    }

    #[test]
    fn a_read_only_tool_is_not_process_capable() {
        assert_eq!(process_capability("read"), ToolProcessCapability::None);
        assert_eq!(process_capability("grep"), ToolProcessCapability::None);
    }

    #[test]
    fn an_unknown_tool_is_not_assumed_to_be_process_capable() {
        // The guard is what keeps this honest: a *built-in* tool that starts
        // a process must be declared, and a plugin tool is classified by
        // its runtime kind.
        assert_eq!(
            process_capability("some_plugin_tool"),
            ToolProcessCapability::None
        );
    }

    #[test]
    fn delegation_is_distinguished_from_a_direct_spawn() {
        assert!(process_capability("task").delegates());
        assert!(!process_capability("task").spawns_itself());
        assert!(process_capability("job").process_kind().is_some());
    }
}
