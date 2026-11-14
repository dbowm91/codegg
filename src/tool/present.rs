//! `present`: the model declares this turn's deliverables and the harness
//! verifies every one of them before the declaration is recorded.
//!
//! # The claim this tool checks
//!
//! "This turn delivered `src/foo.rs`" is a claim about three things, and each
//! is checked separately: the path is **contained** in the turn's workspace,
//! the file **exists** and is a regular file the user can open now, and this
//! turn **wrote** it. A claim that fails any of the three is refused with a
//! reason naming the path, and a declaration containing one bad entry is
//! refused in full — trimming the bad entry is how a deliverable list stops
//! meaning anything.
//!
//! # Why the evidence is the checkpoint, not a stream
//!
//! Authorship comes from the durable edit checkpoint
//! ([`crate::snapshot::checkpoint::EditCheckpointManager::paths_written_for_turn`]),
//! which is scoped to `(workspace, session, turn, batch)` and persisted by the
//! tool batch. The global file-change event stream is deliberately *not* used:
//! it is unscoped and not durable, and `src/agent/tool_batch.rs` already
//! refused it as authorship evidence. A file written by an earlier batch of the
//! same turn is found through that record; a file written by a different turn
//! is not found at all.
//!
//! # What this tool does not claim
//!
//! Not correctness, not content, not size, not a hash. It reads metadata and
//! the checkpoint record; it creates, modifies, deletes and moves nothing, and
//! it takes no writer permit.

use crate::error::ToolError;
use crate::snapshot::checkpoint::{EditCheckpointManager, TurnWriteEvidence};
use crate::tool::backend::{StructuredToolResult, ToolExecutionContext, ToolProvenance, ToolTrust};
use crate::tool::contract::{
    IdempotencyClass, ToolCachePolicy, ToolCallerPolicy, ToolContract, ToolEffectClass,
};
use crate::tool::util::validate_path;
use crate::tool::{Tool, ToolCategory};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Maximum files one declaration may record.
pub const MAX_PRESENTED_FILES: usize = 32;

/// Maximum bytes of one declared path.
pub const MAX_PRESENTED_PATH_LEN: usize = 512;

/// Maximum characters kept from a per-file description. The description is
/// user-facing text, not a claim, so it is truncated rather than refused.
pub const MAX_PRESENTED_DESCRIPTION_LEN: usize = 200;

/// How many of the turn's written paths the verification read asks for. Larger
/// than [`MAX_PRESENTED_FILES`] so declaring a few files out of a busy turn's
/// writes does not fall off the end of its own evidence.
const MAX_TURN_WRITE_EVIDENCE: usize = 256;

/// Why one declared file was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentRefusalKind {
    /// Not contained in the turn's workspace, or resolved outside it.
    OutsideWorkspace,
    /// No file exists at the path.
    Missing,
    /// A directory, a socket, a fifo, or anything else that is not a file.
    NotARegularFile,
    /// The turn's write record has no change for the path.
    NotWrittenThisTurn,
    /// The declaration exceeds [`MAX_PRESENTED_FILES`].
    TooManyFiles,
    /// The path exceeds [`MAX_PRESENTED_PATH_LEN`].
    PathTooLong,
}

impl PresentRefusalKind {
    /// Canonical wire spelling. Stable: it is persisted in the tool result.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OutsideWorkspace => "outside_workspace",
            Self::Missing => "missing",
            Self::NotARegularFile => "not_a_regular_file",
            Self::NotWrittenThisTurn => "not_written_this_turn",
            Self::TooManyFiles => "too_many_files",
            Self::PathTooLong => "path_too_long",
        }
    }
}

/// One verified deliverable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentedFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// One refused entry of a declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentRefusal {
    /// The declared path, or `None` for a refusal about the declaration as a
    /// whole (a bound).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub kind: PresentRefusalKind,
    pub reason: String,
}

/// The structured result: what was verified, and what was refused.
///
/// The result *is* the record. There is no table, no second store and no
/// projection field: the tool call's row in the session transcript carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentResult {
    pub delivered: Vec<PresentedFile>,
    pub refused: Vec<PresentRefusal>,
}

impl PresentResult {
    /// Whether the whole declaration was verified. A partial declaration is a
    /// refusal, so this is false whenever `refused` is non-empty.
    pub fn is_verified(&self) -> bool {
        self.refused.is_empty() && !self.delivered.is_empty()
    }

    /// The one message a caller shows. Every refusal names its path.
    pub fn refusal_message(&self) -> String {
        let mut out = format!(
            "present refused {} entr{}; no file was recorded",
            self.refused.len(),
            if self.refused.len() == 1 { "y" } else { "ies" }
        );
        for refusal in &self.refused {
            out.push_str("\n  ");
            match &refusal.path {
                Some(path) => out.push_str(&format!("{}: ", path)),
                None => out.push_str("declaration: "),
            }
            out.push_str(refusal.kind.as_str());
            out.push_str(" — ");
            out.push_str(&refusal.reason);
        }
        out
    }
}

/// The canonical text a verified declaration renders as. The TUI card parses
/// this same text back with [`parse_present_display`], so there is one format
/// and no second copy of what was delivered.
pub fn render_present_display(result: &PresentResult) -> String {
    let count = result.delivered.len();
    let mut out = format!(
        "Presented {} file{} this turn:",
        count,
        if count == 1 { "" } else { "s" }
    );
    for file in &result.delivered {
        out.push_str("\n  ");
        out.push_str(&escape_line_breaks(&file.path));
        if let Some(description) = &file.description {
            out.push_str("\n    ");
            out.push_str(description);
        }
    }
    out
}

/// Parse [`render_present_display`]'s text back into the delivered files.
///
/// Returns `None` for anything that is not exactly that format — a refusal
/// message, a truncated capture, or a transcript from before this tool
/// existed. A reader that "best-effort" guessed here would show files the
/// harness never verified.
pub fn parse_present_display(text: &str) -> Option<Vec<PresentedFile>> {
    let mut lines = text.lines();
    let header = lines.next()?;
    let count: usize = header
        .strip_prefix("Presented ")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let expected_tail = if count == 1 {
        "file this turn:"
    } else {
        "files this turn:"
    };
    if !header.ends_with(expected_tail) {
        return None;
    }
    let mut delivered: Vec<PresentedFile> = Vec::new();
    for line in lines {
        if let Some(body) = line.strip_prefix("    ") {
            delivered.last_mut()?.description = Some(body.to_string());
        } else if let Some(body) = line.strip_prefix("  ") {
            delivered.push(PresentedFile {
                path: unescape_line_breaks(body),
                description: None,
            });
        } else {
            return None;
        }
    }
    (delivered.len() == count).then_some(delivered)
}

/// Newlines are escaped so one path is always one line; everything else is
/// rendered as it is.
fn escape_line_breaks(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn unescape_line_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[derive(Debug, Deserialize)]
struct PresentInput {
    files: Vec<PresentEntry>,
}

#[derive(Debug, Deserialize)]
struct PresentEntry {
    path: String,
    #[serde(default)]
    description: Option<String>,
}

/// Declaration of a turn's deliverables, verified against the turn's own
/// durable write record before anything is rendered.
pub struct PresentTool {
    root: PathBuf,
    workspace_id: String,
    session_id: String,
    /// The turn this registry was built for. `None` cannot prove authorship,
    /// so every entry is refused rather than checked against a weaker rule.
    turn_id: Option<String>,
    /// `None` when no durable checkpoint store is attached; same refusal.
    checkpoints: Option<Arc<EditCheckpointManager>>,
}

impl PresentTool {
    pub fn new(
        root: PathBuf,
        workspace_id: String,
        session_id: String,
        turn_id: Option<String>,
        checkpoints: Option<Arc<EditCheckpointManager>>,
    ) -> Self {
        Self {
            root,
            workspace_id,
            session_id,
            turn_id,
            checkpoints,
        }
    }

    fn output_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "delivered": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "description": {"type": "string"}
                        },
                        "required": ["path"]
                    }
                },
                "refused": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "kind": {"type": "string"},
                            "reason": {"type": "string"}
                        },
                        "required": ["kind", "reason"]
                    }
                }
            },
            "required": ["delivered", "refused"]
        })
    }

    /// The turn's written paths, or the reason they cannot be established.
    ///
    /// Fails closed on every arm: an absent turn, an absent store and an
    /// unreadable store are all "authorship is unproven", never "no paths were
    /// written" and never "assume it was this turn".
    async fn turn_write_evidence(&self) -> Result<TurnWriteEvidence, String> {
        let Some(turn_id) = self.turn_id.as_deref() else {
            return Err("no turn is bound to this call, so authorship cannot be proven".into());
        };
        let Some(checkpoints) = self.checkpoints.as_deref() else {
            return Err("no durable checkpoint store is attached to this call".into());
        };
        checkpoints
            .paths_written_for_turn(
                &self.workspace_id,
                &self.session_id,
                turn_id,
                MAX_TURN_WRITE_EVIDENCE,
            )
            .await
    }

    /// Verify one declared entry. The checks run in the order a reader would
    /// ask them: is this a path we would even look at, is there a file there,
    /// is it contained, and did this turn write it.
    fn verify_entry(
        &self,
        entry: &PresentEntry,
        evidence: &Result<TurnWriteEvidence, String>,
    ) -> Result<PresentedFile, PresentRefusal> {
        let declared = entry.path.as_str();
        let refuse = |kind: PresentRefusalKind, reason: String| PresentRefusal {
            path: Some(declared.to_string()),
            kind,
            reason,
        };

        if declared.trim().is_empty() {
            return Err(refuse(
                PresentRefusalKind::Missing,
                "the path is empty".into(),
            ));
        }
        if declared.len() > MAX_PRESENTED_PATH_LEN {
            return Err(refuse(
                PresentRefusalKind::PathTooLong,
                format!("the path is longer than {MAX_PRESENTED_PATH_LEN} bytes"),
            ));
        }
        let candidate = Path::new(declared);
        // The lexical escapes are refused before any filesystem call, so a
        // `..` path never causes a stat outside the workspace.
        if candidate.is_absolute() {
            return Err(refuse(
                PresentRefusalKind::OutsideWorkspace,
                "absolute paths are not accepted; declare a path relative to the workspace root"
                    .into(),
            ));
        }
        if candidate
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
        {
            return Err(refuse(
                PresentRefusalKind::OutsideWorkspace,
                "the path leaves the workspace root".into(),
            ));
        }

        let joined = self.root.join(candidate);
        match std::fs::symlink_metadata(&joined) {
            Err(error) => {
                return Err(refuse(
                    PresentRefusalKind::Missing,
                    format!("nothing exists at this path ({error})"),
                ))
            }
            Ok(metadata) if metadata.is_dir() => {
                return Err(refuse(
                    PresentRefusalKind::NotARegularFile,
                    "the path is a directory".into(),
                ))
            }
            Ok(metadata) if !metadata.is_file() && !metadata.file_type().is_symlink() => {
                return Err(refuse(
                    PresentRefusalKind::NotARegularFile,
                    "the path is not a regular file".into(),
                ))
            }
            Ok(_) => {}
        }

        // `validate_path` is the only resolver: it refuses a symlink in the
        // path and requires the resolved target to stay under the root.
        let resolved = match validate_path(candidate, &self.root) {
            Ok(resolved) => resolved,
            Err(ToolError::Permission(detail)) => {
                return Err(refuse(
                    PresentRefusalKind::OutsideWorkspace,
                    format!("the resolver refused it: {detail}"),
                ))
            }
            Err(other) => {
                return Err(refuse(
                    PresentRefusalKind::Missing,
                    format!("the resolver could not resolve it: {other}"),
                ))
            }
        };
        // A symlink whose target is a directory resolves inside the root and
        // must still not be presented as a file.
        match std::fs::metadata(&resolved) {
            Ok(metadata) if metadata.is_file() => {}
            Ok(_) => {
                return Err(refuse(
                    PresentRefusalKind::NotARegularFile,
                    "the resolved target is not a regular file".into(),
                ))
            }
            Err(error) => {
                return Err(refuse(
                    PresentRefusalKind::Missing,
                    format!("the resolved target could not be read ({error})"),
                ))
            }
        }

        let root = self.root.canonicalize().map_err(|error| PresentRefusal {
            path: Some(declared.to_string()),
            kind: PresentRefusalKind::OutsideWorkspace,
            reason: format!("the workspace root could not be resolved ({error})"),
        })?;
        let relative = resolved.strip_prefix(&root).map_err(|_| PresentRefusal {
            path: Some(declared.to_string()),
            kind: PresentRefusalKind::OutsideWorkspace,
            reason: "the resolved path is not under the workspace root".into(),
        })?;
        let relative = normalise_relative(relative).ok_or_else(|| PresentRefusal {
            path: Some(declared.to_string()),
            kind: PresentRefusalKind::OutsideWorkspace,
            reason: "the resolved path is not a normalised path under the workspace root".into(),
        })?;

        match evidence {
            Err(reason) => Err(refuse(
                PresentRefusalKind::NotWrittenThisTurn,
                format!("authorship cannot be proven: {reason}"),
            )),
            Ok(evidence) if evidence.contains(&relative) => Ok(PresentedFile {
                path: declared.to_string(),
                description: entry
                    .description
                    .as_deref()
                    .map(normalise_description)
                    .filter(|description| !description.is_empty()),
            }),
            Ok(evidence) if evidence.truncated => Err(refuse(
                PresentRefusalKind::NotWrittenThisTurn,
                format!(
                    "the turn's write record was truncated at {} paths, so authorship cannot be proven",
                    evidence.paths.len()
                ),
            )),
            Ok(_) => Err(refuse(
                PresentRefusalKind::NotWrittenThisTurn,
                "the turn's write record has no change for this path".into(),
            )),
        }
    }

    /// Parse, bound and verify a declaration.
    ///
    /// A malformed declaration is kept distinct from a refused one: it names no
    /// path to refuse, and only the latter carries typed per-path refusals.
    async fn verify(&self, input: &serde_json::Value) -> PresentOutcome {
        let parsed: PresentInput = match serde_json::from_value(input.clone()) {
            Ok(parsed) => parsed,
            Err(error) => {
                return PresentOutcome::Malformed(format!(
                    "present expects {{\"files\": [{{\"path\": string, \"description\": string?}}]}}: {error}"
                ))
            }
        };
        if parsed.files.is_empty() {
            return PresentOutcome::Malformed(
                "present needs at least one file; a declaration that names nothing records nothing"
                    .into(),
            );
        }
        if parsed.files.len() > MAX_PRESENTED_FILES {
            return PresentOutcome::Refused(PresentResult {
                delivered: Vec::new(),
                refused: vec![PresentRefusal {
                    path: None,
                    kind: PresentRefusalKind::TooManyFiles,
                    reason: format!(
                        "{} files declared, the maximum is {MAX_PRESENTED_FILES}",
                        parsed.files.len()
                    ),
                }],
            });
        }

        let evidence = self.turn_write_evidence().await;
        let mut delivered = Vec::new();
        let mut refused = Vec::new();
        for entry in &parsed.files {
            match self.verify_entry(entry, &evidence) {
                Ok(file) => delivered.push(file),
                Err(refusal) => refused.push(refusal),
            }
        }
        // One bad entry refuses the declaration: reporting the good half as a
        // success is the "trimming" that makes a deliverable list a lie.
        if !refused.is_empty() {
            return PresentOutcome::Refused(PresentResult {
                delivered: Vec::new(),
                refused,
            });
        }
        PresentOutcome::Verified(PresentResult { delivered, refused })
    }
}

/// The three shapes a declaration can produce, kept distinct because they have
/// different terminal statuses and different structured values.
enum PresentOutcome {
    /// Every entry was verified.
    Verified(PresentResult),
    /// At least one entry was refused, with a typed reason per entry.
    Refused(PresentResult),
    /// The declaration itself was not usable.
    Malformed(String),
}

/// Rebuild a path from its normal components, dropping `.` and refusing
/// anything that is not a plain relative descent.
fn normalise_relative(path: &Path) -> Option<String> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out.to_string_lossy().into_owned())
    }
}

/// Collapse a description to one bounded line. It is presentation text, so the
/// bound is a truncation that keeps the first characters rather than a refusal
/// that would let a long sentence erase a real claim.
fn normalise_description(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX_PRESENTED_DESCRIPTION_LEN {
        return collapsed;
    }
    let mut bounded: String = collapsed
        .chars()
        .take(MAX_PRESENTED_DESCRIPTION_LEN)
        .collect();
    bounded.push('…');
    bounded
}

#[async_trait]
impl Tool for PresentTool {
    fn name(&self) -> &str {
        "present"
    }

    fn description(&self) -> &str {
        "Declare the files this turn delivers. Every entry is verified before it is recorded: the path must be inside the workspace, the file must exist and be a regular file, and this turn must have written it. One invalid entry refuses the whole declaration. The tool records a declaration; it does not create, modify, move or publish anything."
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "files": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": MAX_PRESENTED_FILES,
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "maxLength": MAX_PRESENTED_PATH_LEN,
                                "description": "Workspace-relative path of a file this turn wrote."
                            },
                            "description": {
                                "type": "string",
                                "maxLength": MAX_PRESENTED_DESCRIPTION_LEN,
                                "description": "One line of user-facing context. Not a claim about the file's contents."
                            }
                        },
                        "required": ["path"]
                    }
                }
            },
            "required": ["files"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    fn contract(&self, tool_name: &str, input_schema: serde_json::Value) -> ToolContract {
        ToolContract {
            name: tool_name.to_string(),
            // A declaration is made by the turn itself; a nested program has no
            // turn-scoped delivery claim to make.
            caller_policy: ToolCallerPolicy::DirectOnly,
            effect_class: ToolEffectClass::ReadOnly,
            idempotency: IdempotencyClass::Idempotent,
            // The answer depends on this turn's writes, so a cached answer can
            // only be stale.
            cache_policy: ToolCachePolicy::default(),
            output_schema: Some(Self::output_schema()),
            ..ToolContract::legacy(tool_name, input_schema)
        }
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        match self.verify(&input).await {
            PresentOutcome::Verified(result) => Ok(render_present_display(&result)),
            PresentOutcome::Refused(result) => Err(ToolError::Format(result.refusal_message())),
            PresentOutcome::Malformed(message) => Err(ToolError::Format(message)),
        }
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        // c005-denial: tool-boundary
        crate::tool::process_admission::admit_tool_process(self.name(), ctx.as_ref()).await?;
        let provenance = ToolProvenance {
            backend: "native".to_string(),
            implementation: "codegg/present".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            elapsed_ms: None,
            truncated: false,
            trust: ToolTrust::LocalTrusted,
        };
        match self.verify(&input).await {
            PresentOutcome::Verified(result) => {
                let value = serde_json::to_value(&result).unwrap_or(serde_json::Value::Null);
                Ok(StructuredToolResult::with_value(
                    render_present_display(&result),
                    value,
                    true,
                    Some(provenance),
                ))
            }
            PresentOutcome::Refused(result) => {
                // A refusal is a structured error, not an infrastructure
                // failure: the declaration was understood and rejected, and the
                // typed kinds stay in the value so a consumer can tell which
                // check failed.
                let value = serde_json::to_value(&result).unwrap_or(serde_json::Value::Null);
                Ok(StructuredToolResult::with_value(
                    result.refusal_message(),
                    value,
                    false,
                    Some(provenance),
                ))
            }
            PresentOutcome::Malformed(message) => Ok(StructuredToolResult::with_value(
                message,
                serde_json::json!({"delivered": [], "refused": []}),
                false,
                Some(provenance),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::checkpoint::{EditCheckpoint, EditFileState, FileState};
    use codegg_core::session::schema::migrate;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;

    const WORKSPACE: &str = "workspace-test";
    const SESSION: &str = "session-test";

    /// A fresh migrated pool. `foreign_keys(true)` matches production, so the
    /// checkpoint insert needs the session row the migration's FK names.
    async fn test_pool(workspace_id: &str, session_id: &str) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("pool");
        migrate(&pool).await.expect("migrate");
        sqlx::query(
            "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) \
             VALUES ('project-test', '/tmp', 0, 0, '[]')",
        )
        .execute(&pool)
        .await
        .expect("project");
        sqlx::query(
            "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated, workspace_id) \
             VALUES (?, 'project-test', 'test', '/tmp', 'test', '1', 0, 0, ?)",
        )
        .bind(session_id)
        .bind(workspace_id)
        .execute(&pool)
        .await
        .expect("session");
        pool
    }

    /// One turn's workspace: a tempdir root, a migrated pool, and a tool bound
    /// to the same identities the daemon binds.
    struct Turn {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        manager: Arc<EditCheckpointManager>,
        tool: PresentTool,
    }

    impl Turn {
        /// The workspace root is a subdirectory of the tempdir, so a test can
        /// place a file *outside* the workspace it is declaring.
        async fn rooted_at(tmp: tempfile::TempDir, turn: Option<&str>) -> Self {
            let root = tmp.path().join("workspace");
            std::fs::create_dir_all(&root).expect("workspace root");
            let manager = Arc::new(EditCheckpointManager::new(
                test_pool(WORKSPACE, SESSION).await,
                root.clone(),
            ));
            let tool = PresentTool::new(
                root.clone(),
                WORKSPACE.to_string(),
                SESSION.to_string(),
                turn.map(str::to_string),
                Some(manager.clone()),
            );
            Self {
                _tmp: tmp,
                root,
                manager,
                tool,
            }
        }

        async fn new() -> Self {
            Self::rooted_at(tempfile::tempdir().expect("tempdir"), Some("turn-1")).await
        }

        /// Write `relative` into the workspace and record it as this turn's
        /// write in checkpoint `batch`. Both halves are needed: a file on disk
        /// with no record is a claim the turn did not write, and a record with
        /// no file is a claim the user cannot open.
        async fn write(&self, relative: &str, batch: i64) {
            self.write_in_turn(relative, "turn-1", batch).await;
        }

        async fn write_in_turn(&self, relative: &str, turn: &str, batch: i64) {
            let absolute = self.root.join(relative);
            if let Some(parent) = absolute.parent() {
                std::fs::create_dir_all(parent).expect("parent dirs");
            }
            std::fs::write(&absolute, "written by the turn").expect("write file");
            self.record(turn, batch, relative).await;
        }

        async fn record(&self, turn: &str, batch: i64, relative: &str) {
            self.manager
                .persist_checkpoint(EditCheckpoint {
                    id: format!("cp-{turn}-{batch}-{relative}"),
                    workspace_id: WORKSPACE.to_string(),
                    session_id: SESSION.to_string(),
                    turn_id: Some(turn.to_string()),
                    batch_seq: batch,
                    created_at: batch,
                    files: vec![EditFileState {
                        path: relative.to_string(),
                        pre: FileState::Absent,
                        post: FileState::Present {
                            hash: "h".into(),
                            content: "written by the turn".into(),
                        },
                    }],
                })
                .await
                .expect("persist checkpoint");
        }
    }

    /// One `present` call's outcome, with the typed result recovered from the
    /// structured value when the call produced one.
    struct Call {
        success: bool,
        output: String,
        result: PresentResult,
    }

    async fn declare(tool: &PresentTool, files: &[(&str, Option<&str>)]) -> Call {
        let entries: Vec<serde_json::Value> = files
            .iter()
            .map(|(path, description)| match description {
                Some(description) => serde_json::json!({"path": path, "description": description}),
                None => serde_json::json!({"path": path}),
            })
            .collect();
        let structured = tool
            .execute_structured(serde_json::json!({"files": entries}), None)
            .await
            .expect("present reports refusals as a structured error, not a transport error");
        let result = serde_json::from_value(
            structured
                .value
                .clone()
                .expect("present always carries a structured value"),
        )
        .expect("the structured value is a PresentResult");
        Call {
            success: structured.success,
            output: structured.output,
            result,
        }
    }

    fn kinds(result: &PresentResult) -> Vec<PresentRefusalKind> {
        result.refused.iter().map(|refusal| refusal.kind).collect()
    }

    #[tokio::test]
    async fn a_declaration_of_a_file_this_turn_wrote_is_recorded() {
        let turn = Turn::new().await;
        turn.write("src/a.rs", 1).await;

        let call = declare(&turn.tool, &[("src/a.rs", Some("the new module"))]).await;

        assert!(call.success, "output: {}", call.output);
        assert_eq!(
            call.result.delivered,
            vec![PresentedFile {
                path: "src/a.rs".into(),
                description: Some("the new module".into()),
            }]
        );
        assert!(call.result.refused.is_empty());
        assert!(call.result.is_verified());
    }

    #[tokio::test]
    async fn a_path_written_in_an_earlier_batch_of_the_same_turn_is_accepted() {
        let turn = Turn::new().await;
        turn.write("src/early.rs", 1).await;
        turn.write("src/late.rs", 2).await;

        let call = declare(&turn.tool, &[("src/early.rs", None)]).await;

        assert!(call.success, "output: {}", call.output);
        assert_eq!(call.result.delivered[0].path, "src/early.rs");
    }

    #[tokio::test]
    async fn a_path_written_by_another_turn_is_refused() {
        let turn = Turn::new().await;
        // Present on disk and recorded, but recorded for turn-2.
        turn.write_in_turn("src/other.rs", "turn-2", 1).await;

        let call = declare(&turn.tool, &[("src/other.rs", None)]).await;

        assert!(!call.success);
        assert!(call.result.delivered.is_empty());
        assert_eq!(
            kinds(&call.result),
            vec![PresentRefusalKind::NotWrittenThisTurn]
        );
        assert_eq!(call.result.refused[0].path.as_deref(), Some("src/other.rs"));
        assert!(call.output.contains("src/other.rs"));
    }

    #[tokio::test]
    async fn a_path_this_turn_did_not_write_is_refused() {
        let turn = Turn::new().await;
        // The repository contains it; the turn did not write it. This is the
        // claim the milestone exists to make checkable.
        std::fs::create_dir_all(turn.root.join("src")).expect("src dir");
        std::fs::write(turn.root.join("src/pre_existing.rs"), "old").expect("pre-existing file");

        let call = declare(&turn.tool, &[("src/pre_existing.rs", None)]).await;

        assert!(!call.success);
        assert_eq!(
            kinds(&call.result),
            vec![PresentRefusalKind::NotWrittenThisTurn]
        );
        assert!(
            call.output.contains("no change for this path"),
            "the reason names the check: {}",
            call.output
        );
    }

    #[tokio::test]
    async fn a_path_outside_the_workspace_is_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        // A real file that really exists, one level above the workspace root.
        let outside = tmp.path().join("escape.txt");
        std::fs::write(&outside, "outside").expect("outside file");
        let turn = Turn::rooted_at(tmp, Some("turn-1")).await;

        let absolute = declare(&turn.tool, &[(outside.to_str().unwrap(), None)]).await;
        let traversing = declare(&turn.tool, &[("../escape.txt", None)]).await;

        assert!(!absolute.success);
        assert_eq!(
            kinds(&absolute.result),
            vec![PresentRefusalKind::OutsideWorkspace]
        );
        assert!(!traversing.success);
        assert_eq!(
            kinds(&traversing.result),
            vec![PresentRefusalKind::OutsideWorkspace]
        );
        assert!(traversing.output.contains("../escape.txt"));
    }

    #[tokio::test]
    async fn a_missing_path_is_refused() {
        let turn = Turn::new().await;
        // Recorded as written, then deleted: the claim is about a file the
        // user can open now.
        turn.record("turn-1", 1, "src/gone.rs").await;

        let call = declare(&turn.tool, &[("src/gone.rs", None)]).await;

        assert!(!call.success);
        assert_eq!(kinds(&call.result), vec![PresentRefusalKind::Missing]);
        assert!(call.output.contains("src/gone.rs"));
        // The plain string path refuses too, so a caller that never looks at
        // the structured value cannot record a missing file either.
        let legacy = turn
            .tool
            .execute(serde_json::json!({"files": [{"path": "src/gone.rs"}]}))
            .await;
        assert!(matches!(legacy, Err(ToolError::Format(_))));
    }

    #[tokio::test]
    async fn a_directory_is_refused_as_not_a_regular_file() {
        let turn = Turn::new().await;
        std::fs::create_dir_all(turn.root.join("src/pkg")).expect("package dir");
        turn.record("turn-1", 1, "src/pkg").await;

        let call = declare(&turn.tool, &[("src/pkg", None)]).await;

        assert!(!call.success);
        assert_eq!(
            kinds(&call.result),
            vec![PresentRefusalKind::NotARegularFile]
        );
        assert!(call.output.contains("src/pkg"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_symlink_that_escapes_the_workspace_is_refused() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let outside = tmp.path().join("escape.txt");
        std::fs::write(&outside, "outside").expect("outside file");
        let turn = Turn::rooted_at(tmp, Some("turn-1")).await;
        // Inside the workspace by path, outside it by target.
        std::os::unix::fs::symlink(&outside, turn.root.join("link.txt")).expect("symlink");
        turn.record("turn-1", 1, "link.txt").await;

        let call = declare(&turn.tool, &[("link.txt", None)]).await;

        assert!(!call.success);
        assert_eq!(
            kinds(&call.result),
            vec![PresentRefusalKind::OutsideWorkspace]
        );
        assert!(call.output.contains("link.txt"));
    }

    #[tokio::test]
    async fn too_many_files_is_refused() {
        let turn = Turn::new().await;
        let paths: Vec<String> = (0..MAX_PRESENTED_FILES + 1)
            .map(|index| format!("src/f{index}.rs"))
            .collect();
        let declared: Vec<(&str, Option<&str>)> =
            paths.iter().map(|path| (path.as_str(), None)).collect();

        let call = declare(&turn.tool, &declared).await;

        assert!(!call.success);
        assert_eq!(kinds(&call.result), vec![PresentRefusalKind::TooManyFiles]);
        assert!(
            call.result.refused[0].path.is_none(),
            "a bound names no path"
        );
        assert!(call.output.contains(&format!("{MAX_PRESENTED_FILES}")));
    }

    #[tokio::test]
    async fn an_over_long_path_is_refused() {
        let turn = Turn::new().await;
        let long = format!("src/{}.rs", "x".repeat(MAX_PRESENTED_PATH_LEN));

        let call = declare(&turn.tool, &[(long.as_str(), None)]).await;

        assert!(!call.success);
        assert_eq!(kinds(&call.result), vec![PresentRefusalKind::PathTooLong]);
        assert!(call
            .output
            .contains(&format!("{MAX_PRESENTED_PATH_LEN} bytes")));
    }

    #[tokio::test]
    async fn a_partial_declaration_is_refused_in_full() {
        let turn = Turn::new().await;
        turn.write("src/good.rs", 1).await;
        // It exists, so the refusal is about authorship rather than existence:
        // the turn never wrote it.
        std::fs::write(turn.root.join("src/never_written.rs"), "old").expect("file");

        let call = declare(
            &turn.tool,
            &[("src/good.rs", None), ("src/never_written.rs", None)],
        )
        .await;

        assert!(!call.success);
        assert!(
            call.result.delivered.is_empty(),
            "a partial declaration records nothing rather than the good half"
        );
        assert_eq!(
            kinds(&call.result),
            vec![PresentRefusalKind::NotWrittenThisTurn]
        );
        assert_eq!(
            call.result.refused[0].path.as_deref(),
            Some("src/never_written.rs")
        );
        assert!(
            !call.output.contains("Presented"),
            "the refusal must not read like a successful declaration: {}",
            call.output
        );
    }

    #[tokio::test]
    async fn a_declaration_with_no_bound_turn_is_refused() {
        let turn = Turn::rooted_at(tempfile::tempdir().expect("tempdir"), None).await;
        std::fs::write(turn.root.join("a.rs"), "x").expect("file");

        let call = declare(&turn.tool, &[("a.rs", None)]).await;

        assert!(!call.success);
        assert_eq!(
            kinds(&call.result),
            vec![PresentRefusalKind::NotWrittenThisTurn]
        );
        assert!(
            call.output.contains("no turn is bound"),
            "the reason states why authorship is unprovable: {}",
            call.output
        );
    }

    #[tokio::test]
    async fn an_empty_declaration_is_malformed() {
        let turn = Turn::new().await;

        let call = declare(&turn.tool, &[]).await;

        assert!(!call.success);
        assert!(call.result.refused.is_empty());
        assert!(call.output.contains("at least one file"));
    }

    #[tokio::test]
    async fn a_declaration_leaves_the_writer_permit_store_untouched() {
        let turn = Turn::new().await;
        turn.write("src/a.rs", 1).await;
        let count = |pool: SqlitePool| async move {
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM writer_permit")
                .fetch_one(&pool)
                .await
                .expect("count writer_permit");
            count
        };
        let before = count(turn.manager.pool()).await;

        let call = declare(&turn.tool, &[("src/a.rs", None)]).await;
        let after = count(turn.manager.pool()).await;

        assert!(call.success, "output: {}", call.output);
        assert_eq!(before, 0);
        assert_eq!(after, before, "verifying a claim takes no writer permit");
    }

    #[test]
    fn the_tool_is_read_only_and_absent_from_the_child_registry() {
        let tool = PresentTool::new(
            crate::tool::util::unresolved_workspace_root(),
            WORKSPACE.to_string(),
            SESSION.to_string(),
            None,
            None,
        );

        assert_eq!(tool.category(), ToolCategory::ReadOnly);
        assert_eq!(
            crate::permission::tool_category_for_name("present"),
            ToolCategory::ReadOnly
        );
        let contract = Tool::contract(&tool, tool.name(), tool.parameters());
        assert_eq!(contract.effect_class, ToolEffectClass::ReadOnly);
        assert_eq!(contract.caller_policy, ToolCallerPolicy::DirectOnly);

        // A delegated child builds its registry through `with_options`, never
        // through the daemon's per-turn factory, so this is the denial the
        // child's registry gives it: absent by construction.
        let child = crate::tool::ToolRegistry::with_defaults();
        assert!(!child.tool_names().any(|name| name == "present"));

        // And a profile that keeps only an explicit allowlist keeps it out even
        // when a parent registry holds one.
        let mut parent = crate::tool::ToolRegistry::with_defaults();
        parent.register(PresentTool::new(
            crate::tool::util::unresolved_workspace_root(),
            WORKSPACE.to_string(),
            SESSION.to_string(),
            Some("turn-1".to_string()),
            None,
        ));
        assert!(parent.tool_names().any(|name| name == "present"));
        parent.retain_only(&["read", "list", "glob"]);
        assert!(!parent.tool_names().any(|name| name == "present"));
    }

    #[tokio::test]
    async fn the_daemon_session_registry_registers_present() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("workspace");
        std::fs::create_dir_all(&root).expect("workspace root");
        let workspace = Arc::new(codegg_core::workspace::WorkspaceRecord {
            id: codegg_core::workspace::WorkspaceId::parse(WORKSPACE).expect("workspace id"),
            canonical_root: root,
            display_name: "present-test".to_string(),
            created_at: chrono::Utc::now(),
            last_opened_at: chrono::Utc::now(),
            archived_at: None,
        });
        let execution = codegg_core::workspace::ExecutionContext::new(
            workspace,
            Some(SESSION.to_string()),
            tokio_util::sync::CancellationToken::new(),
        );
        let (registry, _) = crate::tool::factory::build_session_tool_registry(
            &crate::config::schema::Config::default(),
            Some(test_pool(WORKSPACE, SESSION).await),
            SESSION,
            None,
            crate::model_profile::types::TaskStatePolicy::explicit_todo(),
            None,
            execution,
            crate::tool::factory::SessionToolContext {
                turn_id: Some("turn-1".to_string()),
                ..Default::default()
            },
        );

        // Registration is the half a denial test cannot prove: without it the
        // capability is inert in every production turn.
        assert!(registry.tool_names().any(|name| name == "present"));
        assert!(registry
            .definitions()
            .iter()
            .any(|definition| definition.name == "present"));
    }

    #[test]
    fn the_display_round_trips_through_the_one_format() {
        let result = PresentResult {
            delivered: vec![
                PresentedFile {
                    path: "src/a.rs".into(),
                    description: Some("the new module".into()),
                },
                PresentedFile {
                    path: "src/weird\\name.rs".into(),
                    description: None,
                },
            ],
            refused: Vec::new(),
        };

        let rendered = render_present_display(&result);

        assert_eq!(parse_present_display(&rendered), Some(result.delivered));
    }

    #[test]
    fn a_refusal_message_does_not_parse_as_a_declaration() {
        let refused = PresentResult {
            delivered: Vec::new(),
            refused: vec![PresentRefusal {
                path: Some("src/gone.rs".into()),
                kind: PresentRefusalKind::Missing,
                reason: "nothing exists at this path".into(),
            }],
        };

        let rendered = refused.refusal_message();

        assert!(rendered.contains("src/gone.rs"));
        assert_eq!(parse_present_display(&rendered), None);
    }
}
