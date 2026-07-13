use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("malformed argv: {reason}")]
    MalformedArgv { reason: String },

    #[error("unsupported global option: {option}")]
    UnsupportedGlobalOption { option: String },

    #[error("unsupported subcommand: {subcommand}")]
    UnsupportedSubcommand { subcommand: String },

    #[error("ambiguous syntax: {reason}")]
    AmbiguousSyntax { reason: String },

    #[error("unsafe path/pathspec: {path} — {reason}")]
    UnsafePath { path: String, reason: String },

    #[error("missing required argument: {argument}")]
    MissingRequiredArgument { argument: String },

    #[error("contradictory flags: {reason}")]
    ContradictoryFlags { reason: String },

    #[error("operation requires managed fallback")]
    RequiresManagedFallback,

    #[error("operation must remain raw shell: {reason}")]
    MustRemainRawShell { reason: String },
}
