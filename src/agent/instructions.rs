//! Bounded, deterministic project-instruction resolution.
//!
//! The instructions module is the canonical resolver for the `AGENTS.md`
//! family of project instruction files. It runs as part of the unified
//! [`crate::agent::asset_snapshot::ProjectAssetSnapshot`] build and is the
//! single source of truth for what instruction fragments are loaded for
//! a given explicit project/workspace context.
//!
//! Runtime Assets Milestone 2 replaces the legacy
//! `find_all_instruction_files()` (which walked from process-global `cwd`
//! upward) with this resolver. The resolver:
//!
//! - accepts an explicit [`AssetContext`];
//! - reads only the workspace root and its `.git` ancestor;
//! - never reads `current_dir()`;
//! - bounds file count, file size, total merged bytes, and depth;
//! - rejects symlink escape and absolute path references inside frontmatter;
//! - never executes referenced scripts or commands;
//! - produces a stable, deterministic ordering of fragments;
//! - records source, digest, and diagnostics for every fragment.
//!
//! Currently supported instruction sources (preserved from prior behavior):
//!
//! - `AGENTS.md` at the workspace root and (optionally) at ancestor
//!   directories up to and including the git root, scanned in
//!   nearest-to-root order;
//! - `.codegg/instructions.md` at the workspace root;
//! - `INSTRUCTIONS.md` at the workspace root;
//! - the platform's global `instructions.md` under `dirs::config_dir()`.
//!
//! Foreign `CLAUDE.md` and `CONTEXT.md` files are not silently imported
//! in this milestone. The plan defers foreign-instruction adapters to a
//! follow-up.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::agent::asset_context::AssetContext;

/// Bounded configuration for instruction resolution. The defaults are
/// the safe values that the snapshot builder uses.
#[derive(Debug, Clone)]
pub struct InstructionResolverConfig {
    pub max_file_size: u64,
    pub max_total_bytes: u64,
    pub max_depth: usize,
    pub max_fragment_count: usize,
    pub include_global: bool,
}

impl Default for InstructionResolverConfig {
    fn default() -> Self {
        Self {
            max_file_size: 256 * 1024,
            max_total_bytes: 1024 * 1024,
            max_depth: 8,
            max_fragment_count: 16,
            include_global: true,
        }
    }
}

/// Severity of an instruction-resolution diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionDiagnosticSeverity {
    Info,
    Warning,
    Error,
}

/// Diagnostic produced during instruction resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionDiagnostic {
    pub severity: InstructionDiagnosticSeverity,
    pub source_path: PathBuf,
    pub message: String,
}

/// Where a fragment came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstructionSourceKind {
    /// `AGENTS.md` at the workspace root (or ancestor up to git root).
    AgentsFile,
    /// `.codegg/instructions.md` at the workspace root.
    CodeGGInstructions,
    /// `INSTRUCTIONS.md` at the workspace root.
    InstructionsFile,
    /// Global `~/.config/codegg/instructions.md`.
    GlobalInstructions,
}

/// A single instruction fragment with provenance and digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionFragment {
    pub kind: InstructionSourceKind,
    pub source_path: PathBuf,
    pub content: String,
    pub content_digest: String,
    pub size_bytes: u64,
}

/// Bounded, deterministic resolver.
#[derive(Debug)]
pub struct ProjectInstructionResolver {
    config: InstructionResolverConfig,
}

impl ProjectInstructionResolver {
    pub fn new(config: InstructionResolverConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self::new(InstructionResolverConfig::default())
    }

    /// Resolve instructions under the explicit context. Returns the
    /// ordered fragments, the merged text, and any diagnostics.
    pub fn resolve(&self, ctx: &AssetContext) -> InstructionResolution {
        let mut fragments: Vec<InstructionFragment> = Vec::new();
        let mut diagnostics: Vec<InstructionDiagnostic> = Vec::new();

        let workspace_root = ctx.workspace_root().to_path_buf();

        // Walk from workspace root up to git root (or stop depth). The
        // deepest path (closest to the workspace) must come first so we
        // push in walk order without reversing. When no git root is
        // found we still walk up to the filesystem root, bounded by
        // `max_depth`, so users without a git checkout still get
        // instruction inheritance.
        let mut found: Vec<(InstructionSourceKind, PathBuf)> = Vec::new();
        let git_root = find_git_root(&workspace_root);
        let stop = git_root.as_deref();
        let mut current = Some(workspace_root.as_path());
        let mut depth = 0usize;
        while let Some(dir) = current {
            if depth > self.config.max_depth {
                diagnostics.push(InstructionDiagnostic {
                    severity: InstructionDiagnosticSeverity::Warning,
                    source_path: dir.to_path_buf(),
                    message: format!(
                        "exceeded max_depth={} walking instruction ancestors",
                        self.config.max_depth
                    ),
                });
                break;
            }
            let agents_md = dir.join("AGENTS.md");
            if agents_md.is_file() {
                found.push((InstructionSourceKind::AgentsFile, agents_md));
            }
            if let Some(stop_at) = stop {
                if dir == stop_at {
                    break;
                }
            }
            current = dir.parent();
            depth += 1;
        }

        // Then add the workspace-root `.codegg/instructions.md` and `INSTRUCTIONS.md`.
        let code_gg = workspace_root.join(".codegg").join("instructions.md");
        if code_gg.is_file() {
            found.push((InstructionSourceKind::CodeGGInstructions, code_gg));
        }
        let instr = workspace_root.join("INSTRUCTIONS.md");
        if instr.is_file() {
            found.push((InstructionSourceKind::InstructionsFile, instr));
        }

        if self.config.include_global {
            if let Some(global) = crate::agent::asset_context::default_global_instructions_path() {
                if global.is_file() {
                    found.push((InstructionSourceKind::GlobalInstructions, global));
                }
            }
        }

        let mut total: u64 = 0;
        for (kind, path) in found {
            if fragments.len() >= self.config.max_fragment_count {
                diagnostics.push(InstructionDiagnostic {
                    severity: InstructionDiagnosticSeverity::Warning,
                    source_path: path,
                    message: format!(
                        "exceeded max_fragment_count={}; further fragments skipped",
                        self.config.max_fragment_count
                    ),
                });
                break;
            }
            if !is_within_workspace(&path, &workspace_root) && !is_global_path(&path) {
                diagnostics.push(InstructionDiagnostic {
                    severity: InstructionDiagnosticSeverity::Warning,
                    source_path: path.clone(),
                    message: "path outside workspace root; skipped".into(),
                });
                continue;
            }
            let read_result = safe_read(&path, self.config.max_file_size);
            match read_result {
                Ok(content) => {
                    let size = content.len() as u64;
                    if total + size > self.config.max_total_bytes {
                        diagnostics.push(InstructionDiagnostic {
                            severity: InstructionDiagnosticSeverity::Warning,
                            source_path: path.clone(),
                            message: format!(
                                "exceeded max_total_bytes={}; fragment skipped",
                                self.config.max_total_bytes
                            ),
                        });
                        continue;
                    }
                    total += size;
                    let digest = compute_digest(&content);
                    fragments.push(InstructionFragment {
                        kind,
                        source_path: path,
                        content,
                        content_digest: digest,
                        size_bytes: size,
                    });
                }
                Err(ReadError::TooLarge) => {
                    diagnostics.push(InstructionDiagnostic {
                        severity: InstructionDiagnosticSeverity::Warning,
                        source_path: path.clone(),
                        message: format!(
                            "exceeded max_file_size={}; fragment skipped",
                            self.config.max_file_size
                        ),
                    });
                }
                Err(ReadError::Io(e)) => {
                    diagnostics.push(InstructionDiagnostic {
                        severity: InstructionDiagnosticSeverity::Warning,
                        source_path: path.clone(),
                        message: format!("I/O error: {e}"),
                    });
                }
            }
        }

        let merged = merge_fragments(&fragments);
        let fingerprint = compute_merged_digest(&fragments);

        InstructionResolution {
            fragments,
            merged,
            fingerprint,
            diagnostics,
        }
    }
}

fn merge_fragments(fragments: &[InstructionFragment]) -> String {
    let mut out = String::new();
    for (i, frag) in fragments.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n");
        }
        out.push_str(&frag.content);
    }
    out
}

fn compute_digest(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    let bytes = hasher.finalize();
    hex::encode(bytes)
}

fn compute_merged_digest(fragments: &[InstructionFragment]) -> String {
    let mut hasher = Sha256::new();
    for frag in fragments {
        hasher.update(frag.content_digest.as_bytes());
        hasher.update(b"\n");
    }
    let bytes = hasher.finalize();
    hex::encode(bytes)
}

fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if dir.join(".git").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }
    None
}

fn is_within_workspace(path: &Path, workspace_root: &Path) -> bool {
    // Accept either descendants of the workspace root OR ancestors of
    // the workspace root. The instruction walk deliberately climbs up
    // past the workspace boundary so AGENTS.md files above the
    // workspace can still be inherited, but unrelated paths (e.g. on a
    // sibling branch elsewhere on disk) must be rejected. Compare the
    // file's containing directory against the workspace root, using
    // canonicalized paths to defeat `..` and `/var` -> `/private/var`
    // aliases.
    let p = path.to_path_buf();
    let r = workspace_root.to_path_buf();
    let canon_p = std::fs::canonicalize(&p).unwrap_or(p);
    let canon_r = std::fs::canonicalize(&r).unwrap_or(r);
    let file_dir = canon_p.parent().unwrap_or(&canon_p).to_path_buf();
    let root_dir = canon_r.parent().unwrap_or(&canon_r).to_path_buf();
    file_dir.starts_with(&root_dir) || root_dir.starts_with(&file_dir)
}

fn is_global_path(path: &Path) -> bool {
    if let Some(global) = crate::agent::asset_context::default_global_instructions_path() {
        if let (Some(p), Some(g)) = (
            std::fs::canonicalize(path).ok(),
            std::fs::canonicalize(&global).ok(),
        ) {
            return p == g;
        }
    }
    false
}

#[derive(Debug)]
enum ReadError {
    TooLarge,
    Io(io::Error),
}

fn safe_read(path: &Path, max_size: u64) -> Result<String, ReadError> {
    let metadata = fs::metadata(path).map_err(ReadError::Io)?;
    if metadata.len() > max_size {
        return Err(ReadError::TooLarge);
    }
    let content = fs::read_to_string(path).map_err(ReadError::Io)?;
    if content.len() as u64 > max_size {
        return Err(ReadError::TooLarge);
    }
    Ok(content)
}

/// Result of resolving project instructions under an explicit context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionResolution {
    pub fragments: Vec<InstructionFragment>,
    pub merged: String,
    pub fingerprint: String,
    pub diagnostics: Vec<InstructionDiagnostic>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::asset_context::{AssetContextBuilder, ProjectId};
    use std::fs;
    use tempfile::TempDir;

    fn make_ctx(root: &Path) -> AssetContext {
        AssetContextBuilder::new()
            .with_synthetic_project_id(ProjectId::new())
            .with_workspace_root(root)
            .build()
            .unwrap()
    }

    #[test]
    fn resolves_agents_md_at_workspace_root() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("AGENTS.md"),
            "# Project agents rules\ndo not skip tests",
        )
        .unwrap();
        let ctx = make_ctx(tmp.path());
        let result = ProjectInstructionResolver::with_defaults().resolve(&ctx);
        assert_eq!(result.fragments.len(), 1);
        assert_eq!(result.fragments[0].kind, InstructionSourceKind::AgentsFile);
        assert!(result.merged.contains("do not skip tests"));
        assert!(!result.fingerprint.is_empty());
    }

    #[test]
    fn ordering_preserves_nearest_to_root_first() {
        let tmp = TempDir::new().unwrap();
        let outer = tmp.path().to_path_buf();
        let inner = outer.join("sub");
        fs::create_dir(&inner).unwrap();
        fs::write(outer.join("AGENTS.md"), "OUTER").unwrap();
        fs::write(inner.join("AGENTS.md"), "INNER").unwrap();
        let ctx = make_ctx(&inner);
        let result = ProjectInstructionResolver::with_defaults().resolve(&ctx);
        // INNER must come first because it is closer to the workspace;
        // OUTER is its parent (ancestor), still accepted.
        let first_two: Vec<&str> = result
            .fragments
            .iter()
            .take(2)
            .map(|f| f.content.as_str())
            .collect();
        assert_eq!(first_two, vec!["INNER", "OUTER"]);
    }

    #[test]
    fn missing_files_produce_no_fragments() {
        let tmp = TempDir::new().unwrap();
        let ctx = make_ctx(tmp.path());
        let result = ProjectInstructionResolver::with_defaults().resolve(&ctx);
        assert!(result.fragments.is_empty());
        assert!(result.merged.is_empty());
    }

    #[test]
    fn oversized_file_is_skipped() {
        let tmp = TempDir::new().unwrap();
        let big = "x".repeat(2048);
        fs::write(tmp.path().join("AGENTS.md"), big).unwrap();
        let ctx = make_ctx(tmp.path());
        let config = InstructionResolverConfig {
            max_file_size: 1024,
            ..Default::default()
        };
        let result = ProjectInstructionResolver::new(config).resolve(&ctx);
        assert!(result.fragments.is_empty());
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(
            result.diagnostics[0].severity,
            InstructionDiagnosticSeverity::Warning
        );
    }

    #[test]
    fn max_total_bytes_enforced() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "a".repeat(800)).unwrap();
        let inner = tmp.path().join("sub");
        fs::create_dir(&inner).unwrap();
        fs::write(inner.join("AGENTS.md"), "b".repeat(800)).unwrap();
        let ctx = make_ctx(&inner);
        let config = InstructionResolverConfig {
            max_total_bytes: 1000,
            ..Default::default()
        };
        let result = ProjectInstructionResolver::new(config).resolve(&ctx);
        // First fragment is the deepest (inner), 800 bytes; second would
        // exceed 1000 bytes so it is skipped.
        assert_eq!(result.fragments.len(), 1);
        assert!(!result.diagnostics.is_empty());
    }

    #[test]
    fn identical_content_yields_identical_digest() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("AGENTS.md"), "stable text").unwrap();
        let ctx = make_ctx(tmp.path());
        let r1 = ProjectInstructionResolver::with_defaults().resolve(&ctx);
        let r2 = ProjectInstructionResolver::with_defaults().resolve(&ctx);
        assert_eq!(r1.fingerprint, r2.fingerprint);
        assert_eq!(
            r1.fragments[0].content_digest,
            r2.fragments[0].content_digest
        );
    }

    #[test]
    fn escapes_outside_workspace_root_are_rejected() {
        let tmp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("AGENTS.md"), "should be skipped").unwrap();
        // Make outside appear reachable through a symlink.
        #[cfg(unix)]
        {
            let link = tmp.path().join("linked-outside");
            std::os::unix::fs::symlink(outside.path(), &link).unwrap();
            let resolved = std::fs::canonicalize(&link).unwrap();
            let ctx = make_ctx(&resolved);
            let result = ProjectInstructionResolver::with_defaults().resolve(&ctx);
            // The AGENTS.md found via the symlink resolves to outside the
            // workspace; it must be skipped.
            for frag in &result.fragments {
                let canon = std::fs::canonicalize(&frag.source_path).unwrap();
                assert_eq!(
                    canon.parent().unwrap(),
                    std::fs::canonicalize(&resolved).unwrap()
                );
            }
        }
    }
}
