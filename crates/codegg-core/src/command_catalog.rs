//! M007-D: the daemon-owned command catalog.
//!
//! This module is the single definition of what commands exist. ADR-0052
//! (accepted 2026-10-24) moved command ownership into the daemon: the
//! catalog names every command's identity, display name, aliases,
//! description, domain, kind, required capability and argument shape, and
//! the daemon executes the daemon-effect forms under `command.execute`.
//!
//! The catalog is plain data, so `codegg-core` stays UI-free
//! (`scripts/check-core-boundary.sh`).
//!
//! ## The classification unit is the form
//!
//! A command's *form* is one argument shape of that command, and the form
//! — not the bare command — carries the kind. `/agents` is the confirmed
//! case: the bare form opens the agent dialog (a frontend effect) while
//! `show`, `--all`, `diff`, `validate` and `reload` are daemon effects.
//!
//! ## The rule the classification applies
//!
//! > A form is a **daemon effect** when invoking it requires the daemon to
//! > produce content or perform an effect: it mutates daemon-owned state,
//! > reads daemon-owned state as its result, or drives a daemon-mediated
//! > process, store, or service. A form is a **frontend effect** when its
//! > entire observable result is a change to frontend presentation state.
//!
//! The catalog is generated once from the reviewed classification and
//! checked against the TUI's command surface by
//! `scripts/check_command_catalog_ownership.py`, so the two cannot drift.

use serde::{Deserialize, Serialize};

/// The two command kinds ADR-0052 defines. There are exactly two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    /// The daemon owns the effect and the authoritative data. The frontend
    /// submits it; the daemon authorizes it under the capability the
    /// catalog names, executes it, and records a canonical `Run`.
    DaemonEffect,
    /// The command changes only what the frontend is showing. The frontend
    /// dispatches it locally from the declared frontend-action set.
    FrontendEffect,
}

impl CommandKind {
    /// Stable wire name for the kind.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DaemonEffect => "daemon_effect",
            Self::FrontendEffect => "frontend_effect",
        }
    }
}

/// The coarse category a command belongs to.
///
/// This is the older three-way classification, kept for compatibility with
/// the surfaces that still group commands by it. It is presentation
/// metadata only; it does not decide whether a command may execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandCategory {
    Session,
    Agent,
    System,
}

/// The capability a daemon-effect form requires.
///
/// `command.execute` is the only capability any daemon-effect form may
/// name in this milestone: no new capability, scope or policy is invented
/// here (`plans/000-long-term-specification.md:217`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandCapability {
    CommandExecute,
}

impl CommandCapability {
    /// The capability's canonical wire name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CommandExecute => "command.execute",
        }
    }
}

/// Stable user-facing task domains used by discovery surfaces.
///
/// This taxonomy is presentation metadata only; it does not decide whether
/// a command may execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandDomain {
    Project,
    Session,
    Agent,
    Execution,
    GitReview,
    Research,
    Provider,
    Collaboration,
    Memory,
    Diagnostics,
    System,
}

impl std::fmt::Display for CommandDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::Project => "Project",
            Self::Session => "Session",
            Self::Agent => "Agent",
            Self::Execution => "Execution",
            Self::GitReview => "Git/Review",
            Self::Research => "Research",
            Self::Provider => "Provider",
            Self::Collaboration => "Collaboration",
            Self::Memory => "Memory",
            Self::Diagnostics => "Diagnostics",
            Self::System => "System",
        };
        write!(f, "{label}")
    }
}

/// One argument form of a command.
///
/// The form is the classification unit: a command's argument can select a
/// different effect, so the kind lives on the form and not on the command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommandForm {
    /// The form's selector. The empty string is the command's default
    /// form — the one a bare `/name` selects.
    pub form: &'static str,
    /// Whether this form's effect lives in the daemon or in the frontend.
    pub kind: CommandKind,
    /// A one-line hint describing the form's argument, for completion.
    pub argument_hint: &'static str,
    /// Whether the form requires an argument.
    pub argument_required: bool,
}

/// One command's catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDescriptor {
    /// The command's stable identity, shared by every frontend. It is not a
    /// display name: a frontend may present a different name for the same
    /// identity.
    pub command_id: &'static str,
    /// The canonical display name, including its leading slash.
    pub name: &'static str,
    /// Alternate spellings that resolve to the same command.
    pub aliases: &'static [&'static str],
    /// One-line description.
    pub description: &'static str,
    /// The coarse category.
    pub category: CommandCategory,
    /// The discovery domain.
    pub domain: CommandDomain,
    /// The command's kind. A command with forms of both kinds carries the
    /// kind of its **default** form here, and each form carries its own.
    pub kind: CommandKind,
    /// The capability a daemon-effect form requires. `None` for a wholly
    /// frontend-effect command.
    pub capability: Option<CommandCapability>,
    /// The command's forms. Every command has at least one.
    pub forms: &'static [CommandForm],
}

/// The catalog: every command the daemon knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandCatalog {
    /// Monotonically increasing version, bumped whenever the catalog's
    /// contents change. A `CommandExecute` carrying a different version is
    /// refused with `StaleCatalog`.
    pub version: u64,
    /// Every command, in registry order.
    pub commands: &'static [CommandDescriptor],
}

/// The catalog's bounds. An over-bound catalog is **refused**, never
/// silently truncated (`plans/000-long-term-specification.md:538`).
pub mod bounds {
    /// Maximum commands in one catalog response.
    pub const MAX_COMMANDS: usize = 256;
    /// Maximum forms across the whole catalog.
    pub const MAX_FORMS: usize = 512;
    /// Maximum serialized catalog size, in bytes.
    pub const MAX_CATALOG_BYTES: usize = 256 * 1024;
    /// Maximum bytes in one command description.
    pub const MAX_DESCRIPTION_BYTES: usize = 256;
    /// Maximum aliases on one command.
    pub const MAX_ALIASES: usize = 8;
    /// Maximum forms on one command.
    pub const MAX_FORMS_PER_COMMAND: usize = 32;
    /// Maximum bytes in one command identity.
    pub const MAX_COMMAND_ID_BYTES: usize = 64;
    /// Maximum bytes in one command argument.
    pub const MAX_ARGUMENT_BYTES: usize = 4 * 1024;
    /// Maximum result lines in one command execution response.
    pub const MAX_RESULT_LINES: usize = 256;
    /// Maximum serialized command execution response size, in bytes.
    pub const MAX_RESULT_BYTES: usize = 64 * 1024;
}

/// Why a catalog was refused as over-bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogBoundError {
    /// The catalog holds more commands than [`bounds::MAX_COMMANDS`].
    TooManyCommands { count: usize },
    /// The catalog holds more forms than [`bounds::MAX_FORMS`].
    TooManyForms { count: usize },
    /// The catalog serializes to more than [`bounds::MAX_CATALOG_BYTES`].
    TooLarge { bytes: usize },
}

impl std::fmt::Display for CatalogBoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyCommands { count } => {
                write!(f, "command catalog holds {count} commands, over the bound")
            }
            Self::TooManyForms { count } => {
                write!(f, "command catalog holds {count} forms, over the bound")
            }
            Self::TooLarge { bytes } => {
                write!(
                    f,
                    "command catalog serializes to {bytes} bytes, over the bound"
                )
            }
        }
    }
}

impl std::error::Error for CatalogBoundError {}

/// The one catalog. Every command the daemon knows about.
static THE_CATALOG: CommandCatalog = CommandCatalog {
    version: 2,
    commands: &[
        CommandDescriptor {
            command_id: "project_init",
            name: "/init",
            aliases: &[],
            description: "Preview and initialize root AGENTS.md for this project",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "connect",
            name: "/connect",
            aliases: &[],
            description: "Connect provider",
            category: CommandCategory::System,
            domain: CommandDomain::Provider,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "connections",
            name: "/connections",
            aliases: &["/connection", "/select-connection"],
            description: "Select provider connection and model for current session",
            category: CommandCategory::System,
            domain: CommandDomain::Provider,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "exit",
            name: "/exit",
            aliases: &["quit", "q"],
            description: "Exit the app",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "status",
            name: "/status",
            aliases: &[],
            description: "View status",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "policy",
            name: "/policy",
            aliases: &[],
            description: "Show effective approval mode and sandbox profile (/policy)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "approval",
            name: "/approval",
            aliases: &["/approval-mode"],
            description: "Select approval mode: interactive, automatic, or yolo (/approval [mode])",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[mode]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "sandbox",
            name: "/sandbox",
            aliases: &["/sandbox-profile"],
            description: "Select sandbox profile: read-only, workspace-write, or full-host (/sandbox [profile])",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[profile]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "network",
            name: "/network",
            aliases: &[],
            description: "Set child network policy: offline or unrestricted (/network [offline|unrestricted confirm])",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[offline|unrestricted confirm]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "themes",
            name: "/themes",
            aliases: &["/theme"],
            description: "Switch theme (/theme, /theme list, /theme use <name>, /theme reload, /theme diagnostics)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "help",
            name: "/help",
            aliases: &[],
            description: "Help",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "sessions",
            name: "/sessions",
            aliases: &["resume", "continue"],
            description: "Switch session",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "projects",
            name: "/projects",
            aliases: &["/project"],
            description: "Open the project picker (Shift+F in Normal mode)",
            category: CommandCategory::System,
            domain: CommandDomain::Project,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "new",
            name: "/new",
            aliases: &["clear"],
            description: "New session",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "stop",
            name: "/stop",
            aliases: &[],
            description: "Stop the agent's running turn (/stop)",
            category: CommandCategory::Session,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "start_turn",
            name: "/start",
            aliases: &[],
            description: "Carry on after a stopped turn (/start)",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "share",
            name: "/share",
            aliases: &[],
            description: "Share session",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "unshare",
            name: "/unshare",
            aliases: &[],
            description: "Unshare session",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "rename",
            name: "/rename",
            aliases: &[],
            description: "Rename session",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "compact",
            name: "/compact",
            aliases: &["summarize"],
            description: "Compact session",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "timeline",
            name: "/timeline",
            aliases: &[],
            description: "Jump to message",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "fork",
            name: "/fork",
            aliases: &[],
            description: "Fork from message",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "undo",
            name: "/undo",
            aliases: &[],
            description: "Undo previous message",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "redo",
            name: "/redo",
            aliases: &[],
            description: "Redo",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "export",
            name: "/export",
            aliases: &[],
            description: "Export session transcript",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "import",
            name: "/import",
            aliases: &[],
            description: "Import session",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "timestamps",
            name: "/timestamps",
            aliases: &["toggle-timestamps"],
            description: "Toggle timestamps",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "thinking",
            name: "/thinking",
            aliases: &["toggle-thinking"],
            description: "Toggle thinking display",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "reasoning",
            name: "/reasoning",
            aliases: &["reasoning-effort", "thinking-budget"],
            description: "Cycle extended thinking: default / effort / off",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "expand_all_disclosures",
            name: "/expand-all",
            aliases: &["expand-disclosures", "collapse-all"],
            description: "Expand every tool call and reasoning block (or collapse every one)",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "models_refresh",
            name: "/models-refresh",
            aliases: &["refresh-models"],
            description: "Refresh model list",
            category: CommandCategory::System,
            domain: CommandDomain::Provider,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "reload",
            name: "/reload",
            aliases: &["reload", "/skills-refresh", "/agents-refresh"],
            description: "Refresh project runtime assets (/reload skills|agents)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "skills|agents",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "skills",
            name: "/skills",
            aliases: &[],
            description: "Inspect effective skills and shadowed alternatives (/skills [name])",
            category: CommandCategory::System,
            domain: CommandDomain::Memory,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "[name]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "variants",
            name: "/variants",
            aliases: &[],
            description: "Switch model variant",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "agents",
            name: "/agents",
            aliases: &[],
            description: "Manage agents (/agents, /agents --all, show <name>, diff <name>, validate, rebuild)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Agent,
            kind: CommandKind::FrontendEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
                CommandForm {
                    form: "--all",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
                CommandForm {
                    form: "show",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<name>",
                    argument_required: true,
                },
                CommandForm {
                    form: "diff",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<name>",
                    argument_required: true,
                },
                CommandForm {
                    form: "validate",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
                CommandForm {
                    form: "reload",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "agent_select",
            name: "/agent",
            aliases: &[],
            description: "Select active agent (/agent <name>)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Agent,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<name>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "workspaces",
            name: "/workspaces",
            aliases: &[],
            description: "Manage workspaces",
            category: CommandCategory::System,
            domain: CommandDomain::Project,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "tree",
            name: "/tree",
            aliases: &[],
            description: "Show session tree",
            category: CommandCategory::System,
            domain: CommandDomain::Project,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "editor",
            name: "/editor",
            aliases: &[],
            description: "Open editor",
            category: CommandCategory::Agent,
            domain: CommandDomain::Project,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "open_document",
            name: "/open",
            aliases: &[],
            description: "Open a workspace document in the editor view (/open <path>)",
            category: CommandCategory::Session,
            domain: CommandDomain::Project,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "context",
            name: "/context",
            aliases: &[],
            description: "View context window usage",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "cost",
            name: "/cost",
            aliases: &[],
            description: "View token usage and cost",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "usage",
            name: "/usage",
            aliases: &[],
            description: "View rate limits and quota",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "tui_stats",
            name: "/stats",
            aliases: &["/tui-stats"],
            description: "View session analytics and cost breakdown",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "queue",
            name: "/queue",
            aliases: &[],
            description: "Open this session's waiting-prompt box: j/k move, r refresh, a add, e edit, [ ] reorder, d delete, s send now",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "subagent_send",
            name: "/subagent-send",
            aliases: &[],
            description: "Address the next prompt to a running subagent (/subagent-send <name or task id>), or clear it (/subagent-send off)",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<name or task id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "tui",
            name: "/tui",
            aliases: &["fullscreen"],
            description: "Toggle fullscreen mode",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "tts",
            name: "/tts",
            aliases: &["voice"],
            description: "Toggle text-to-speech",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "loop",
            name: "/loop",
            aliases: &[],
            description: "Schedule periodic task (e.g. /loop 5m \"check status\")",
            category: CommandCategory::Agent,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "tasks",
            name: "/tasks",
            aliases: &[],
            description: "Project tasks: running, waiting, attention, and recent WorkOrders (/task, /schedules for low-level schedules)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "task",
            name: "/task",
            aliases: &["/task-view"],
            description: "Open the project Task view (WorkOrders)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "workspace",
            name: "/workspace",
            aliases: &[],
            description: "Global Workspace dashboard: every authorized project with running, future, and attention state (hotkey Ctrl+O / W)",
            category: CommandCategory::System,
            domain: CommandDomain::Project,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "schedules",
            name: "/schedules",
            aliases: &[],
            description: "Low-level schedule diagnostics (recurring Subagent templates; project tasks live in /tasks)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Agent,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "task_del",
            name: "/task-del",
            aliases: &[],
            description: "Delete background task",
            category: CommandCategory::Agent,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "memory",
            name: "/memory",
            aliases: &[],
            description: "Memory dashboard",
            category: CommandCategory::Session,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "memory_search",
            name: "/memory-search",
            aliases: &[],
            description: "Search memories (args: query)",
            category: CommandCategory::Session,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "query",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "memory_list",
            name: "/memory-list",
            aliases: &[],
            description: "List memories (args: namespace)",
            category: CommandCategory::Session,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "namespace",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "memory_remember",
            name: "/memory-remember",
            aliases: &[],
            description: "Remember something (args: text)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "text",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "memory_forget",
            name: "/memory-forget",
            aliases: &[],
            description: "Forget a memory (args: id)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "id",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "memory_consolidate",
            name: "/memory-consolidate",
            aliases: &[],
            description: "Consolidate session into memories",
            category: CommandCategory::Session,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "habits",
            name: "/habits",
            aliases: &[],
            description: "Inspect workflow habit candidates (args: ready)",
            category: CommandCategory::Session,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "ready",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "habit_dismiss",
            name: "/habit-dismiss",
            aliases: &[],
            description: "Dismiss a workflow habit candidate (args: id)",
            category: CommandCategory::Session,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "id",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "skill_promote",
            name: "/skill-promote",
            aliases: &[],
            description: "Draft one skill proposal from a ready habit (args: habit id)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "habit id",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "skill_proposals",
            name: "/skill-proposals",
            aliases: &[],
            description: "List skill proposals",
            category: CommandCategory::Session,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "skill_proposal",
            name: "/skill-proposal",
            aliases: &[],
            description: "Preview, publish, or reject a skill proposal (args: id | publish id [project|global] | reject id)",
            category: CommandCategory::Session,
            domain: CommandDomain::Memory,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "id | publish id [project|global] | reject id",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "goal",
            name: "/goal",
            aliases: &[],
            description: "Manage active long-running goal (/goal set, show, pause, resume, clear, done, checkpoint, from-file, budget [show|raise <axis> <n>])",
            category: CommandCategory::Session,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "set, show, pause, resume, clear, done, checkpoint, from-file, budget [show|raise <axis> <n>]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "plan",
            name: "/plan",
            aliases: &[],
            description: "Manage task plan (/plan, /plan add, done, skip, block, clear)",
            category: CommandCategory::Session,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "state",
            name: "/state",
            aliases: &[],
            description: "Show work/session state view",
            category: CommandCategory::Session,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "diff",
            name: "/diff",
            aliases: &[],
            description: "Show diff for a file (/diff <path>)",
            category: CommandCategory::Session,
            domain: CommandDomain::GitReview,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "tests",
            name: "/tests",
            aliases: &[],
            description: "Show test state (/tests, /tests last, /tests failed)",
            category: CommandCategory::Session,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "test",
            name: "/test",
            aliases: &[],
            description: "Run supervised tests (/test, /test workspace, /test changed, /test package <name>, /test file <path>, /test previous, /test custom <command>)",
            category: CommandCategory::Session,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "revert",
            name: "/revert",
            aliases: &[],
            description: "Revert a file change (/revert <path>)",
            category: CommandCategory::Agent,
            domain: CommandDomain::GitReview,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "research",
            name: "/research",
            aliases: &[],
            description: "Run research on a question (/research <question> [--mode <mode>] [--depth <depth>])",
            category: CommandCategory::Agent,
            domain: CommandDomain::Research,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<question> [--mode <mode>] [--depth <depth>]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "research_runs",
            name: "/research-runs",
            aliases: &[],
            description: "List recent research runs",
            category: CommandCategory::Session,
            domain: CommandDomain::Research,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "research_open",
            name: "/research-open",
            aliases: &[],
            description: "Show artifacts for a research run (/research-open <run_id>)",
            category: CommandCategory::Session,
            domain: CommandDomain::Research,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<run_id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "research_show",
            name: "/research-show",
            aliases: &[],
            description: "Show research runs and details (/research-show report|handoff|claims <run_id>)",
            category: CommandCategory::Session,
            domain: CommandDomain::Research,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "report|handoff|claims <run_id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "search",
            name: "/search",
            aliases: &[],
            description: "Search session transcript",
            category: CommandCategory::Session,
            domain: CommandDomain::Research,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "doctor",
            name: "/doctor",
            aliases: &[],
            description: "Run diagnostics (search backend, MCP, providers)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "logs",
            name: "/logs",
            aliases: &[],
            description: "Show recent daemon log lines and past notifications",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_status",
            name: "/lsp-status",
            aliases: &[],
            description: "Show LSP server status and diagnostics",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "nodes",
            name: "/nodes",
            aliases: &[],
            description: "Show remote Eggwork node host telemetry",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_previews",
            name: "/lsp-previews",
            aliases: &["/preview-list"],
            description: "List LSP preview artifacts",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_preview",
            name: "/lsp-preview",
            aliases: &["/preview-show"],
            description: "Show LSP preview detail (args: <id>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_preview_clear",
            name: "/lsp-preview-clear",
            aliases: &["/preview-clear"],
            description: "Clear LSP preview(s) (args: <id> or --all)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id> or --all",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_preview_refresh",
            name: "/lsp-preview-refresh",
            aliases: &["/preview-refresh"],
            description: "Refresh LSP preview staleness (args: <id>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_preview_apply",
            name: "/lsp-preview-apply",
            aliases: &["/preview-apply"],
            description: "Apply LSP preview patches to disk with hash revalidation (args: <id>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "review",
            name: "/review-change",
            aliases: &["/preview-review"],
            description: "Review a pending agent change before applying it (args: <preview-id>)",
            category: CommandCategory::System,
            domain: CommandDomain::GitReview,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<preview-id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_servers",
            name: "/lsp-servers",
            aliases: &["/lsp-detail"],
            description: "List active LSP servers with status, root, generation",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_capabilities",
            name: "/lsp-capabilities",
            aliases: &[],
            description: "Show effective LSP capabilities (args: server-key)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "server-key",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_errors",
            name: "/lsp-errors",
            aliases: &[],
            description: "Show LSP server errors and health (args: server-key)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "server-key",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_root",
            name: "/lsp-root",
            aliases: &[],
            description: "Diagnose LSP root for a file path (args: <path>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_restart",
            name: "/lsp-restart",
            aliases: &[],
            description: "Restart an LSP server (args: server-key)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "server-key",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_stop",
            name: "/lsp-stop",
            aliases: &[],
            description: "Stop LSP servers (args: server-key or --all)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "server-key or --all",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_cache_status",
            name: "/lsp-cache-status",
            aliases: &[],
            description: "Show LSP semantic cache status and stats",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_cache_clear",
            name: "/lsp-cache-clear",
            aliases: &[],
            description: "Clear LSP semantic cache (args: --all or <root-path>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "--all or <root-path>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_doctor",
            name: "/lsp-doctor",
            aliases: &[],
            description: "Diagnose LSP status for a file path (args: <path>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_context_diagnostics",
            name: "/lsp-context-diagnostics",
            aliases: &[],
            description: "Show LSP context diagnostics for a file path",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_repair_local",
            name: "/lsp-repair-local",
            aliases: &[],
            description: "Repair localized issue (args: <path[:line]>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path[:line]>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_repair_hunk",
            name: "/lsp-repair-hunk",
            aliases: &[],
            description: "Repair code around diff hunks (args: <path> [hunk-id|range])",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path> [hunk-id|range]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_review_file",
            name: "/lsp-review-file",
            aliases: &[],
            description: "Semantic review of a file (args: <path>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_review_diff",
            name: "/lsp-review-diff",
            aliases: &[],
            description: "Review changed files/hunks in current diff",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_security_review",
            name: "/lsp-security-review",
            aliases: &[],
            description: "Enriched security review (args: [path|diff])",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[path|diff]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_impact",
            name: "/lsp-impact",
            aliases: &[],
            description: "Impact analysis for a symbol (args: <path:line:col>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path:line:col>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_test_repair",
            name: "/lsp-test-repair",
            aliases: &[],
            description: "Test failure repair (args: <test-file> [failure-text])",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<test-file> [failure-text]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_interface",
            name: "/lsp-interface",
            aliases: &[],
            description: "API boundary review (args: <path[:symbol]>)",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path[:symbol]>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_cross_repair",
            name: "/lsp-cross-repair",
            aliases: &[],
            description: "Cross-file repair context (args: <primary> [related...])",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<primary> [related...]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "lsp_call_neighbors",
            name: "/lsp-call-neighbors",
            aliases: &[],
            description: "Call neighborhood (args: <path:line:col> [incoming|outgoing|both])",
            category: CommandCategory::System,
            domain: CommandDomain::Diagnostics,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path:line:col> [incoming|outgoing|both]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "tool_backends",
            name: "/tool-backends",
            aliases: &["/tools", "/backends"],
            description: "Show resolved backend for each model-facing tool (Native / MCP / Builtin / Disabled)",
            category: CommandCategory::System,
            domain: CommandDomain::Provider,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "security_review",
            name: "/security-review",
            aliases: &[],
            description: "Security review of changed files (/security-review [--changed] [--base <ref>] [--json] [--prompts-only] [--findings-only] [--no-content] [--no-filename] [--max-findings N] [--max-prompts N] [--enrich] [--panel])",
            category: CommandCategory::Agent,
            domain: CommandDomain::Agent,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[--changed] [--base <ref>] [--json] [--prompts-only] [--findings-only] [--no-content] [--no-filename] [--max-findings N] [--max-prompts N] [--enrich] [--panel]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "security_review_show",
            name: "/security-review-show",
            aliases: &[],
            description: "Reopen the latest security review result panel (no rerun)",
            category: CommandCategory::Agent,
            domain: CommandDomain::Agent,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "security_review_cancel",
            name: "/security-review-cancel",
            aliases: &[],
            description: "Cancel an in-flight security review",
            category: CommandCategory::Agent,
            domain: CommandDomain::Agent,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "shell_list",
            name: "/shell-list",
            aliases: &[],
            description: "List recent shell commands",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "shell_show",
            name: "/shell-show",
            aliases: &[],
            description: "Show detailed info for a shell command (args: <id|last>)",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id|last>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "shell_include",
            name: "/shell-include",
            aliases: &[],
            description: "Include shell output in context (args: <id|last> [--tail N|--stdout|--stderr|--summary])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id|last> [--tail N|--stdout|--stderr|--summary]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "shell_rerun",
            name: "/shell-rerun",
            aliases: &[],
            description: "Re-run a shell command (args: <id|last>)",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id|last>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "shell_kill",
            name: "/shell-kill",
            aliases: &[],
            description: "Kill a running shell command (args: <id|last>)",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id|last>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "shell_ask",
            name: "/shell-ask",
            aliases: &[],
            description: "Ask about shell output (args: <id|last> <question>)",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id|last> <question>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "shell_expand",
            name: "/shell-expand",
            aliases: &[],
            description: "Expand raw shell output from projection handle (args: <id|last> stdout|stderr [start..end])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<id|last> stdout|stderr [start..end]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal",
            name: "/terminal",
            aliases: &["/term"],
            description: "Open an interactive terminal box, starting a shell if none is live (Shift+L docks it into the panel; args: [handle])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_clear",
            name: "/terminal-clear",
            aliases: &[],
            description: "Clear a terminal's scrollback and screen; the process keeps running (args: [handle])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[handle]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_create",
            name: "/terminal-create",
            aliases: &[],
            description: "Create an interactive workspace terminal (args: <command> [args...])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<command> [args...]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_list",
            name: "/terminal-list",
            aliases: &[],
            description: "List interactive terminals in this workspace",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_attach",
            name: "/terminal-attach",
            aliases: &[],
            description: "Attach to an interactive terminal (args: <handle>)",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<handle>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_show",
            name: "/terminal-show",
            aliases: &[],
            description: "Show an interactive terminal (args: <handle>)",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "<handle>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_focus",
            name: "/terminal-focus",
            aliases: &[],
            description: "Focus an interactive terminal so keystrokes go to the process (args: [handle])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "[handle]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_send",
            name: "/terminal-send",
            aliases: &[],
            description: "Send text to an interactive terminal (args: <handle> <text>)",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<handle> <text>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_resize",
            name: "/terminal-resize",
            aliases: &[],
            description: "Resize an interactive terminal (args: <handle> <cols> <rows>)",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<handle> <cols> <rows>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_resume",
            name: "/terminal-resume",
            aliases: &[],
            description: "Resume live terminal output after lag/resync (args: [handle])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[handle]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_detach",
            name: "/terminal-detach",
            aliases: &[],
            description: "Detach from an interactive terminal; the process keeps running (args: [handle])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[handle]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_terminate",
            name: "/terminal-terminate",
            aliases: &[],
            description: "Terminate an interactive terminal process (args: [handle])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[handle]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "terminal_remove",
            name: "/terminal-remove",
            aliases: &[],
            description: "Remove an interactive terminal handle, freeing scrollback (args: [handle])",
            category: CommandCategory::System,
            domain: CommandDomain::Execution,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[handle]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "plugins",
            name: "/plugins",
            aliases: &["/plugin-list", "/plugin-ls"],
            description: "List installed and built-in plugins",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "plugin_info",
            name: "/plugin-info",
            aliases: &[],
            description: "Show plugin runtime, capabilities, trust, and diagnostics (args: <plugin-id-or-name>)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<plugin-id-or-name>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "plugin_enable",
            name: "/plugin-enable",
            aliases: &[],
            description: "Enable a plugin (args: <plugin-id-or-name>)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<plugin-id-or-name>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "plugin_disable",
            name: "/plugin-disable",
            aliases: &[],
            description: "Disable a plugin (args: <plugin-id-or-name>)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<plugin-id-or-name>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "plugin_doctor",
            name: "/plugin-doctor",
            aliases: &[],
            description: "Diagnose plugin configuration and runtime health (args: [plugin-id-or-name])",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[plugin-id-or-name]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "plugin_remove",
            name: "/plugin-remove",
            aliases: &[],
            description: "Remove a local installed plugin (args: <plugin-id-or-name>)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<plugin-id-or-name>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "plugin_install",
            name: "/plugin-install",
            aliases: &[],
            description: "Install a plugin from a local path (args: <path>)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<path>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "collaborators",
            name: "/collaborators",
            aliases: &["/presence"],
            description: "Show collaborators for the active project (/collaborators, /collaborators refresh)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
                // M007-017: the presence panel's own effect.
                CommandForm {
                    form: "presence",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<project-id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "team",
            name: "/team",
            aliases: &[],
            description: "Manage project team membership and chat access (/team, /team add <principal-id> <role>, /team revoke <principal-id> <rev>; LocalOwner: /team principals, /team token-create <principal-id> <label>)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "control",
            name: "/control",
            aliases: &[],
            description: "Inspect and hand off the active-turn controller (/control, /control request [message], /control transfer <principal-id> <revision> [--reason <text>], /control release <revision>, /control takeover <revision> <reason>)",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "observe",
            name: "/observe",
            aliases: &["/watch"],
            description: "Follow another session read-only (/observe <session-id>)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<session-id>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "stop_observing",
            name: "/stop-observing",
            aliases: &["/unwatch"],
            description: "Stop following the observed session",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "panels",
            name: "/panels",
            aliases: &["/panel", "/panel-manager"],
            description: "Open the side-panel manager: pick which panel the side column holds and bind its Ctrl+digit hotkey (hotkey Shift+S in Normal mode)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat",
            name: "/chat",
            aliases: &[],
            description: "Show project chat for the active project (/chat)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_send",
            name: "/chat-send",
            aliases: &[],
            description: "Send a chat message to the active project (/chat-send <text>)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<text>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_reply",
            name: "/chat-reply",
            aliases: &[],
            description: "Reply to a chat message (/chat-reply <message-id> <text>)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<message-id> <text>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_history",
            name: "/chat-history",
            aliases: &[],
            description: "Reload the project chat window (/chat-history)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_sync",
            name: "/chat-sync",
            aliases: &[],
            description: "Incrementally sync project chat from the cached cursor (/chat-sync)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_read",
            name: "/chat-read",
            aliases: &[],
            description: "Mark project chat as read up to the newest message (/chat-read)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_edit",
            name: "/chat-edit",
            aliases: &[],
            description: "Edit your chat message (/chat-edit <message-id> <new-text>)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<message-id> <new-text>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_redact",
            name: "/chat-redact",
            aliases: &[],
            description: "Redact your chat message (/chat-redact <message-id> [reason])",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<message-id> [reason]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_composing",
            name: "/chat-composing",
            aliases: &[],
            description: "Set or clear your typing indicator (/chat-composing on|off)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "on|off",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_action_task",
            name: "/chat-action-task",
            aliases: &[],
            description: "Submit an agent task linked to a chat message (/chat-action-task <message-id> <agent> <prompt>)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<message-id> <agent> <prompt>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_action_review",
            name: "/chat-action-review",
            aliases: &[],
            description: "Request a review linked to a chat message (/chat-action-review <message-id> <agent> <prompt>)",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "<message-id> <agent> <prompt>",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "chat_action_list",
            name: "/chat-action-list",
            aliases: &[],
            description: "List structured chat actions for the active channel (/chat-action-list [message-id])",
            category: CommandCategory::System,
            domain: CommandDomain::Collaboration,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[message-id]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "edit_undo",
            name: "/edit-undo",
            aliases: &[],
            description: "Undo the latest durable edit checkpoint (/edit-undo [checkpoint-id])",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[checkpoint-id]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "edit_reapply",
            name: "/edit-reapply",
            aliases: &[],
            description: "Reapply the latest undone edit checkpoint (/edit-reapply [checkpoint-id])",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "[checkpoint-id]",
                    argument_required: true,
                },
            ],
        },
        CommandDescriptor {
            command_id: "edit_checkpoints",
            name: "/edit-checkpoints",
            aliases: &["/checkpoints", "/history"],
            description: "List durable edit checkpoints (/edit-checkpoints, aliases /checkpoints and /history)",
            category: CommandCategory::Session,
            domain: CommandDomain::Session,
            kind: CommandKind::DaemonEffect,
            capability: Some(CommandCapability::CommandExecute),
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::DaemonEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "tool_contracts",
            name: "/tool-contracts",
            aliases: &[],
            description: "Show tool contract diagnostics (/tool-contracts)",
            category: CommandCategory::System,
            domain: CommandDomain::Provider,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
        CommandDescriptor {
            command_id: "worktree",
            name: "/worktree",
            aliases: &[],
            description: "List worktrees for the active workspace (/worktree)",
            category: CommandCategory::System,
            domain: CommandDomain::System,
            kind: CommandKind::FrontendEffect,
            capability: None,
            forms: &[
                CommandForm {
                    form: "",
                    kind: CommandKind::FrontendEffect,
                    argument_hint: "",
                    argument_required: false,
                },
            ],
        },
    ],
};

/// The daemon's command catalog. This is the single definition of what
/// commands exist; no frontend compiles a second list.
pub fn the_catalog() -> &'static CommandCatalog {
    &THE_CATALOG
}

/// The catalog's current version.
pub fn catalog_version() -> u64 {
    THE_CATALOG.version
}

/// Look one command up by its stable identity.
pub fn find_by_command_id(command_id: &str) -> Option<&'static CommandDescriptor> {
    THE_CATALOG
        .commands
        .iter()
        .find(|command| command.command_id == command_id)
}

/// Look one command up by its canonical name or by any of its aliases.
pub fn find_by_name_or_alias(name: &str) -> Option<&'static CommandDescriptor> {
    THE_CATALOG
        .commands
        .iter()
        .find(|command| command.name == name || command.aliases.contains(&name))
}

/// Every form in the catalog.
pub fn catalog_forms() -> impl Iterator<Item = (&'static CommandDescriptor, &'static CommandForm)> {
    THE_CATALOG
        .commands
        .iter()
        .flat_map(|command| command.forms.iter().map(move |form| (command, form)))
}

/// The number of forms in the catalog.
pub fn catalog_form_count() -> usize {
    catalog_forms().count()
}

/// The number of daemon-effect forms in the catalog.
pub fn daemon_effect_form_count() -> usize {
    catalog_forms()
        .filter(|(_, form)| form.kind == CommandKind::DaemonEffect)
        .count()
}

/// The number of frontend-effect forms in the catalog.
pub fn frontend_effect_form_count() -> usize {
    catalog_forms()
        .filter(|(_, form)| form.kind == CommandKind::FrontendEffect)
        .count()
}

/// Check the catalog against its own bounds.
///
/// The bounds are enforced server-side: an over-bound catalog is refused
/// with a typed reason, never truncated silently.
pub fn check_catalog_bounds(catalog: &CommandCatalog) -> Result<(), CatalogBoundError> {
    if catalog.commands.len() > bounds::MAX_COMMANDS {
        return Err(CatalogBoundError::TooManyCommands {
            count: catalog.commands.len(),
        });
    }
    let forms = catalog
        .commands
        .iter()
        .map(|command| command.forms.len())
        .sum::<usize>();
    if forms > bounds::MAX_FORMS {
        return Err(CatalogBoundError::TooManyForms { count: forms });
    }
    for command in catalog.commands {
        if command.forms.len() > bounds::MAX_FORMS_PER_COMMAND {
            return Err(CatalogBoundError::TooManyForms {
                count: command.forms.len(),
            });
        }
        if command.command_id.len() > bounds::MAX_COMMAND_ID_BYTES {
            return Err(CatalogBoundError::TooLarge {
                bytes: command.command_id.len(),
            });
        }
        if command.description.len() > bounds::MAX_DESCRIPTION_BYTES {
            return Err(CatalogBoundError::TooLarge {
                bytes: command.description.len(),
            });
        }
        if command.aliases.len() > bounds::MAX_ALIASES {
            return Err(CatalogBoundError::TooManyCommands {
                count: command.aliases.len(),
            });
        }
    }
    Ok(())
}
