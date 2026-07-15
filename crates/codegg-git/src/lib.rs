//! Typed Git operation model, argv parser, and risk classification.
//!
//! `codegg-git` establishes a stable typed vocabulary for Git commands that
//! later native-tool, Bash-routing, policy, execution, projection, and
//! provenance work consumes. It is intentionally architecture-first and
//! side-effect free: parsing and rendering never execute commands.
//!
//! # Crate boundary
//!
//! This crate must **not** depend on TUI, provider, Bash implementation,
//! or agent types. It is a pure data-model and parser library.

pub mod error;
pub mod operation;
pub mod origin;
pub mod parser;
pub mod path;
pub mod process_policy;
pub mod ref_name;
pub mod render;
pub mod risk;
pub mod sensitive;

pub use error::ParseError;
pub use operation::GitOperation;
pub use origin::GitCommandOrigin;
pub use parser::parse_git_argv;
pub use path::{RepoPath, RepoRoot};
pub use process_policy::{
    is_allowed as is_allowed_env, is_stripped as is_stripped_env, ALWAYS_STRIPPED_ENV_VARS,
    ALLOWED_ENV_VARS,
};
pub use ref_name::{BranchName, ObjectId, RefName, RemoteName};
pub use render::render_argv;
pub use risk::{GitRiskClass, RiskSet};
pub use sensitive::{redact_url_credentials, AuditSafeArgv, RedactedUrl};
