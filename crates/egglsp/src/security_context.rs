//! Security review context integration.
//!
//! Provides compact risk-tagged summaries from [`LspContextPacket`]
//! for use in the security review workflow.

use std::path::PathBuf;

use crate::context::{LineRange, LspContextItem, LspContextItemKind, LspContextPacket};

// ---------------------------------------------------------------------------
// Security risk tags
// ---------------------------------------------------------------------------

/// Risk classification tags derived from LSP evidence for security review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SecurityRiskTag {
    /// Definitions or declarations are in changed files — public API surface.
    ChangedPublicApi,
    /// Auth/security-sensitive files have definitions or declarations.
    ChangedAuthSecuritySensitive,
    /// Unsafe, FFI, network, filesystem, or process-related code is touched.
    ChangedUnsafeFfiNetworkFsProcess,
    /// More than 10 references found — broad call surface.
    BroadReferences,
    /// Diagnostics introduced within a hunk range.
    DiagnosticsIntroducedInHunk,
    /// Implementations or hierarchy items are affected by the change.
    ImplementationHierarchyAffected,
}

impl std::fmt::Display for SecurityRiskTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ChangedPublicApi => write!(f, "changed_public_api"),
            Self::ChangedAuthSecuritySensitive => write!(f, "changed_auth_security_sensitive"),
            Self::ChangedUnsafeFfiNetworkFsProcess => {
                write!(f, "changed_unsafe_ffi_network_fs_process")
            }
            Self::BroadReferences => write!(f, "broad_references"),
            Self::DiagnosticsIntroducedInHunk => write!(f, "diagnostics_introduced_in_hunk"),
            Self::ImplementationHierarchyAffected => write!(f, "implementation_hierarchy_affected"),
        }
    }
}

// ---------------------------------------------------------------------------
// Security evidence summary
// ---------------------------------------------------------------------------

/// Compact summary extracted from a context packet for security review use.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SecurityEvidenceSummary {
    /// Number of diagnostic items in the packet.
    pub diagnostics_count: usize,
    /// Number of reference items.
    pub references_count: usize,
    /// Number of definition items.
    pub definitions_count: usize,
    /// Number of implementation items.
    pub implementations_count: usize,
    /// Number of distinct files touched by reference items
    /// (the public API fanout from a changed definition).
    pub public_api_fanout: usize,
    /// Risk tags applied to this evidence.
    pub risk_tags: Vec<SecurityRiskTag>,
    /// Whether any evidence item is stale.
    pub stale: bool,
    /// Whether any evidence was truncated.
    pub truncated: bool,
    /// Server that produced the evidence.
    pub server_id: Option<String>,
    /// Accumulated notes from evidence collection.
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------

/// Extract a compact [`SecurityEvidenceSummary`] from a context packet.
pub fn build_security_evidence_summary(packet: &LspContextPacket) -> SecurityEvidenceSummary {
    let mut diagnostics_count = 0usize;
    let mut references_count = 0usize;
    let mut definitions_count = 0usize;
    let mut implementations_count = 0usize;
    let mut stale = false;
    let mut server_id: Option<String> = None;
    let mut public_api_fanout_files: std::collections::HashSet<PathBuf> =
        std::collections::HashSet::new();

    for item in &packet.items {
        match item.kind {
            LspContextItemKind::Diagnostic => diagnostics_count += 1,
            LspContextItemKind::Reference => {
                references_count += 1;
                // Each reference contributes to the public API fanout.
                public_api_fanout_files.insert(item.file.clone());
            }
            LspContextItemKind::Definition | LspContextItemKind::Declaration => {
                definitions_count += 1
            }
            LspContextItemKind::Implementation => implementations_count += 1,
            _ => {}
        }

        if matches!(
            item.provenance.freshness,
            crate::context::LspEvidenceFreshness::Stale
                | crate::context::LspEvidenceFreshness::PossiblyStale
        ) {
            stale = true;
        }

        if server_id.is_none() && !item.provenance.server_id.is_empty() {
            server_id = Some(item.provenance.server_id.clone());
        }
    }

    let changed_files: Vec<PathBuf> = match &packet.request {
        crate::context::LspContextRequest::Review { changed_files, .. } => changed_files.clone(),
        crate::context::LspContextRequest::File { file, .. } => vec![file.clone()],
        _ => Vec::new(),
    };

    let risk_tags = tag_security_risks(&packet.items, &changed_files);

    let truncated = packet.truncation.bytes_truncated
        || packet.truncation.files_truncated
        || packet.truncation.diagnostics_truncated
        || packet.truncation.references_truncated;

    let notes = packet.notes.clone();

    SecurityEvidenceSummary {
        diagnostics_count,
        references_count,
        definitions_count,
        implementations_count,
        public_api_fanout: public_api_fanout_files.len(),
        risk_tags,
        stale,
        truncated,
        server_id,
        notes,
    }
}

/// Build an [`LspContextRequest::Review`] tailored for security review.
///
/// Always uses [`LspRiskMode::Aggressive`] so the budget prioritises
/// risk-bearing items (changed public API, unsafe code, auth
/// patterns, broad references, diagnostics) over generic symbols.
/// Preview artifacts are NOT included — security review never
/// executes code actions, and the renderer omits previews.
pub fn build_security_lsp_context_request(
    changed_files: &[PathBuf],
    hunks: &[crate::hunk_context::HunkDescriptor],
) -> crate::context::LspContextRequest {
    crate::context::LspContextRequest::Review {
        changed_files: changed_files.to_vec(),
        hunks: hunks.to_vec(),
        risk_mode: crate::context::LspRiskMode::Aggressive,
    }
}

/// Deterministically tag security risks based on context items and changed files.
pub fn tag_security_risks(
    items: &[LspContextItem],
    changed_files: &[PathBuf],
) -> Vec<SecurityRiskTag> {
    let mut tags = Vec::new();

    let changed_set: std::collections::HashSet<&PathBuf> = changed_files.iter().collect();

    // Definitions/declarations in changed files → ChangedPublicApi.
    let has_defs_in_changed = items.iter().any(|i| {
        matches!(
            i.kind,
            LspContextItemKind::Definition | LspContextItemKind::Declaration
        ) && changed_set.contains(&i.file)
    });
    if has_defs_in_changed {
        tags.push(SecurityRiskTag::ChangedPublicApi);
    }

    // Auth/security-sensitive file detection.
    let auth_patterns = [
        "auth",
        "login",
        "session",
        "token",
        "password",
        "credential",
        "oauth",
        "jwt",
        "csrf",
        "permission",
        "role",
        "rbac",
    ];
    let has_auth_sensitive = changed_files.iter().any(|f| {
        let name = f.to_string_lossy().to_lowercase();
        auth_patterns.iter().any(|p| name.contains(p))
    }) || items.iter().any(|i| {
        let msg = i.message.to_lowercase();
        auth_patterns.iter().any(|p| msg.contains(p))
    });
    if has_auth_sensitive {
        tags.push(SecurityRiskTag::ChangedAuthSecuritySensitive);
    }

    // Unsafe/FFI/network/fs/process code detection.
    let unsafe_patterns = [
        "unsafe",
        "ffi",
        "extern",
        "raw_ptr",
        "transmute",
        "network",
        "socket",
        "tcp",
        "udp",
        "http",
        "filesystem",
        "fs::",
        "std::fs",
        "File::",
        "process",
        "Command::",
        "std::process",
    ];
    let has_unsafe_code = items.iter().any(|i| {
        let msg = i.message.to_lowercase();
        unsafe_patterns.iter().any(|p| msg.contains(p))
    }) || changed_files.iter().any(|f| {
        let name = f.to_string_lossy().to_lowercase();
        ["unsafe", "ffi", "extern", "syscall", "process"]
            .iter()
            .any(|p| name.contains(p))
    });
    if has_unsafe_code {
        tags.push(SecurityRiskTag::ChangedUnsafeFfiNetworkFsProcess);
    }

    // Implementations/hierarchy affected.
    let has_impls = items
        .iter()
        .any(|i| matches!(i.kind, LspContextItemKind::Implementation));
    if has_impls {
        tags.push(SecurityRiskTag::ImplementationHierarchyAffected);
    }

    // Diagnostics in changed files → DiagnosticsIntroducedInHunk.
    let has_diag_in_changed = items
        .iter()
        .any(|i| i.kind == LspContextItemKind::Diagnostic && changed_set.contains(&i.file));
    if has_diag_in_changed {
        tags.push(SecurityRiskTag::DiagnosticsIntroducedInHunk);
    }

    // Broad references (> 10).
    let ref_count = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Reference)
        .count();
    if ref_count > 10 {
        tags.push(SecurityRiskTag::BroadReferences);
    }

    tags
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{
        LspContextPacket, LspContextPacketMode, LspContextRequest, LspContextScore,
        LspEvidenceFreshness, LspEvidenceProvenance, LspRiskMode,
    };

    fn make_item(
        kind: LspContextItemKind,
        file: &str,
        line: Option<u32>,
        message: &str,
    ) -> LspContextItem {
        LspContextItem {
            kind,
            file: PathBuf::from(file),
            range: line.map(|l| LineRange {
                start: l,
                end: l + 1,
            }),
            line,
            column: None,
            message: message.to_string(),
            symbol: None,
            source: None,
            provenance: LspEvidenceProvenance {
                server_id: "test-server".to_string(),
                server_generation: Some(1),
                operation: "test".to_string(),
                freshness: LspEvidenceFreshness::Fresh,
                capability_decision: None,
                document_version: None,
                age_ms: None,
                post_restart: false,
            },
            score: LspContextScore {
                priority: 10,
                is_hunk_local: false,
                is_error: false,
                is_same_file: false,
                freshness_rank: 0,
            },
            payload: None,
        }
    }

    #[test]
    fn test_security_summary_from_context_packet() {
        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")],
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Standard,
            },
            items: vec![
                make_item(
                    LspContextItemKind::Diagnostic,
                    "a.rs",
                    Some(5),
                    "error: unused",
                ),
                make_item(
                    LspContextItemKind::Diagnostic,
                    "b.rs",
                    Some(10),
                    "warn: dead_code",
                ),
                make_item(LspContextItemKind::Reference, "c.rs", Some(1), "ref: foo"),
                make_item(LspContextItemKind::Definition, "a.rs", Some(0), "def: bar"),
                make_item(
                    LspContextItemKind::Implementation,
                    "d.rs",
                    Some(3),
                    "impl: baz",
                ),
            ],
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: vec!["test note".to_string()],
            truncation: Default::default(),
        };

        let summary = build_security_evidence_summary(&packet);
        assert_eq!(summary.diagnostics_count, 2);
        assert_eq!(summary.references_count, 1);
        assert_eq!(summary.definitions_count, 1);
        assert_eq!(summary.implementations_count, 1);
        assert_eq!(summary.server_id, Some("test-server".to_string()));
        assert_eq!(summary.notes, vec!["test note"]);
        assert!(!summary.truncated);
        assert!(!summary.stale);
        assert!(summary
            .risk_tags
            .contains(&SecurityRiskTag::ChangedPublicApi));
        assert!(summary
            .risk_tags
            .contains(&SecurityRiskTag::DiagnosticsIntroducedInHunk));
        assert!(summary
            .risk_tags
            .contains(&SecurityRiskTag::ImplementationHierarchyAffected));
    }

    #[test]
    fn test_security_risk_tagging() {
        let items = vec![
            make_item(LspContextItemKind::Definition, "a.rs", Some(0), "def"),
            make_item(LspContextItemKind::Diagnostic, "a.rs", Some(5), "err"),
        ];
        let changed_files = vec![PathBuf::from("a.rs")];
        let tags = tag_security_risks(&items, &changed_files);
        assert!(tags.contains(&SecurityRiskTag::ChangedPublicApi));
        assert!(tags.contains(&SecurityRiskTag::DiagnosticsIntroducedInHunk));
        assert!(!tags.contains(&SecurityRiskTag::BroadReferences));
    }

    #[test]
    fn test_security_summary_empty_packet() {
        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: Vec::new(),
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Standard,
            },
            items: Vec::new(),
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: Default::default(),
        };

        let summary = build_security_evidence_summary(&packet);
        assert_eq!(summary.diagnostics_count, 0);
        assert_eq!(summary.references_count, 0);
        assert_eq!(summary.definitions_count, 0);
        assert_eq!(summary.implementations_count, 0);
        assert!(summary.risk_tags.is_empty());
        assert!(summary.notes.is_empty());
        assert!(!summary.stale);
        assert!(!summary.truncated);
    }

    #[test]
    fn test_broad_references_tag() {
        let items: Vec<LspContextItem> = (0..15)
            .map(|i| {
                make_item(
                    LspContextItemKind::Reference,
                    &format!("f{i}.rs"),
                    Some(i),
                    &format!("ref {i}"),
                )
            })
            .collect();
        let tags = tag_security_risks(&items, &[]);
        assert!(tags.contains(&SecurityRiskTag::BroadReferences));
    }

    #[test]
    fn test_stale_detection() {
        let mut item = make_item(LspContextItemKind::Diagnostic, "a.rs", Some(0), "err");
        item.provenance.freshness = LspEvidenceFreshness::Stale;

        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("a.rs")],
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Standard,
            },
            items: vec![item],
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: Default::default(),
        };

        let summary = build_security_evidence_summary(&packet);
        assert!(summary.stale);
    }

    #[test]
    fn test_public_api_fanout_counts_distinct_files() {
        let items = vec![
            make_item(LspContextItemKind::Reference, "a.rs", Some(1), "ref"),
            make_item(LspContextItemKind::Reference, "a.rs", Some(2), "ref"),
            make_item(LspContextItemKind::Reference, "b.rs", Some(3), "ref"),
            make_item(LspContextItemKind::Reference, "c.rs", Some(4), "ref"),
            make_item(LspContextItemKind::Reference, "c.rs", Some(5), "ref"),
        ];
        let packet = LspContextPacket {
            request: LspContextRequest::Review {
                changed_files: vec![PathBuf::from("lib.rs")],
                hunks: Vec::new(),
                risk_mode: LspRiskMode::Aggressive,
            },
            items,
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: None,
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: Default::default(),
        };
        let summary = build_security_evidence_summary(&packet);
        assert_eq!(summary.references_count, 5);
        assert_eq!(summary.public_api_fanout, 3);
    }

    #[test]
    fn test_build_security_lsp_context_request_uses_aggressive() {
        use crate::context::{LspContextRequest, LspRiskMode};
        use crate::hunk_context::HunkDescriptor;
        let files = vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")];
        let hunks = vec![HunkDescriptor {
            id: "a.rs:0:1-3".to_string(),
            file_path: "a.rs".to_string(),
            old_range: None,
            new_range: None,
            header: Some("@@ -1,3 +1,3 @@".to_string()),
            added_lines: 1,
            removed_lines: 1,
            context_lines: 2,
        }];
        let req = build_security_lsp_context_request(&files, &hunks);
        match req {
            LspContextRequest::Review {
                changed_files,
                hunks: req_hunks,
                risk_mode,
            } => {
                assert_eq!(changed_files, files);
                assert_eq!(req_hunks.len(), 1);
                assert_eq!(risk_mode, LspRiskMode::Aggressive);
            }
            _ => panic!("expected Review request"),
        }
    }

    #[test]
    fn test_build_security_lsp_context_request_with_empty_inputs() {
        use crate::context::{LspContextRequest, LspRiskMode};
        let req = build_security_lsp_context_request(&[], &[]);
        match req {
            LspContextRequest::Review {
                changed_files,
                hunks,
                risk_mode,
            } => {
                assert!(changed_files.is_empty());
                assert!(hunks.is_empty());
                assert_eq!(risk_mode, LspRiskMode::Aggressive);
            }
            _ => panic!("expected Review request"),
        }
    }
}
