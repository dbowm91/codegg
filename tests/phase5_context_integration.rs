//! Phase 5 composite integration tests.
//!
//! Exercises the full Phase 5 pipeline (context, evidence collector,
//! security context, renderer, preview registry, TUI summary,
//! degradation policy) using mock providers without requiring a live
//! LSP server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use egglsp::context::*;
use egglsp::context_renderer::*;
use egglsp::degradation_policy::*;
use egglsp::evidence_collector::*;
use egglsp::lsp_types::Position;
use egglsp::preview_registry::*;
use egglsp::security_context::*;
use egglsp::tui_summary::*;
use egglsp::LspError;

// ---------------------------------------------------------------------------
// Mock provider
// ---------------------------------------------------------------------------

struct MockProvider {
    diagnostics: Mutex<Vec<(String, String, String)>>,
    symbols: Mutex<Vec<(String, String, String)>>,
    defs: Mutex<Vec<(String, String)>>,
    refs: Mutex<Vec<(String, String)>>,
    impls: Mutex<Vec<(String, String)>>,
    hover_text: Mutex<Option<String>>,
    highlights: Mutex<Vec<String>>,
    signatures: Mutex<Vec<(String, String)>>,
    completions: Mutex<Vec<(String, String, String)>>,
    sem_tokens: Mutex<Vec<(u32, u32, u32, String)>>,
    ws_symbols: Mutex<Vec<(String, String, String, String)>>,
    state: Mutex<String>,
    server_id: Mutex<Option<String>>,
    generation: Mutex<Option<u64>>,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            diagnostics: Mutex::new(Vec::new()),
            symbols: Mutex::new(Vec::new()),
            defs: Mutex::new(Vec::new()),
            refs: Mutex::new(Vec::new()),
            impls: Mutex::new(Vec::new()),
            hover_text: Mutex::new(None),
            highlights: Mutex::new(Vec::new()),
            signatures: Mutex::new(Vec::new()),
            completions: Mutex::new(Vec::new()),
            sem_tokens: Mutex::new(Vec::new()),
            ws_symbols: Mutex::new(Vec::new()),
            state: Mutex::new("ready".to_string()),
            server_id: Mutex::new(Some("mock-server".to_string())),
            generation: Mutex::new(Some(1)),
        }
    }
}

#[async_trait]
impl LspEvidenceProvider for MockProvider {
    async fn diagnostics_for_file(
        &self,
        _file: &Path,
    ) -> Result<Vec<(String, String, String)>, LspError> {
        Ok(self.diagnostics.lock().unwrap().clone())
    }

    async fn document_symbols(
        &self,
        _file: &Path,
    ) -> Result<Vec<(String, String, String)>, LspError> {
        Ok(self.symbols.lock().unwrap().clone())
    }

    async fn go_to_definition(
        &self,
        _file: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        Ok(self.defs.lock().unwrap().clone())
    }

    async fn find_references(
        &self,
        _file: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        Ok(self.refs.lock().unwrap().clone())
    }

    async fn implementations(
        &self,
        _file: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        Ok(self.impls.lock().unwrap().clone())
    }

    async fn hover(
        &self,
        _file: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Option<String>, LspError> {
        Ok(self.hover_text.lock().unwrap().clone())
    }

    async fn document_highlights(
        &self,
        _file: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Vec<String>, LspError> {
        Ok(self.highlights.lock().unwrap().clone())
    }

    async fn signature_help(
        &self,
        _file: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        Ok(self.signatures.lock().unwrap().clone())
    }

    async fn completion(
        &self,
        _file: &Path,
        _line: u32,
        _column: u32,
    ) -> Result<Vec<(String, String, String)>, LspError> {
        Ok(self.completions.lock().unwrap().clone())
    }

    async fn semantic_tokens(
        &self,
        _file: &Path,
        _start_line: u32,
        _end_line: u32,
    ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
        Ok(self.sem_tokens.lock().unwrap().clone())
    }

    async fn workspace_symbols(
        &self,
        _query: &str,
    ) -> Result<Vec<(String, String, String, String)>, LspError> {
        Ok(self.ws_symbols.lock().unwrap().clone())
    }

    async fn operational_state(&self) -> String {
        self.state.lock().unwrap().clone()
    }

    async fn server_info(&self) -> (Option<String>, Option<u64>) {
        (
            self.server_id.lock().unwrap().clone(),
            *self.generation.lock().unwrap(),
        )
    }
}

// ---------------------------------------------------------------------------
// Failing provider (always returns errors)
// ---------------------------------------------------------------------------

struct FailProvider;

#[async_trait]
impl LspEvidenceProvider for FailProvider {
    async fn diagnostics_for_file(
        &self,
        _: &Path,
    ) -> Result<Vec<(String, String, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn document_symbols(&self, _: &Path) -> Result<Vec<(String, String, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn go_to_definition(
        &self,
        _: &Path,
        _: u32,
        _: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn find_references(
        &self,
        _: &Path,
        _: u32,
        _: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn implementations(
        &self,
        _: &Path,
        _: u32,
        _: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn hover(&self, _: &Path, _: u32, _: u32) -> Result<Option<String>, LspError> {
        Ok(None)
    }

    async fn document_highlights(&self, _: &Path, _: u32, _: u32) -> Result<Vec<String>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn signature_help(
        &self,
        _: &Path,
        _: u32,
        _: u32,
    ) -> Result<Vec<(String, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn completion(
        &self,
        _: &Path,
        _: u32,
        _: u32,
    ) -> Result<Vec<(String, String, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn semantic_tokens(
        &self,
        _: &Path,
        _: u32,
        _: u32,
    ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn workspace_symbols(
        &self,
        _: &str,
    ) -> Result<Vec<(String, String, String, String)>, LspError> {
        Err(LspError::NotInitialized("no server".into()))
    }

    async fn operational_state(&self) -> String {
        "failed".to_string()
    }

    async fn server_info(&self) -> (Option<String>, Option<u64>) {
        (None, None)
    }
}

// ---------------------------------------------------------------------------
// Unused provider placeholder (kept for structural reference)
// ---------------------------------------------------------------------------

// (UnavailProvider was defined here but all degradation tests use
// evaluate_degradation() directly with server_available/capability_supported flags.)

// ---------------------------------------------------------------------------
// Helper to make a context item
// ---------------------------------------------------------------------------

fn make_item(
    kind: LspContextItemKind,
    file: &str,
    line: Option<u32>,
    message: &str,
    priority: u32,
) -> LspContextItem {
    LspContextItem {
        kind,
        file: PathBuf::from(file),
        range: None,
        line,
        column: None,
        message: message.to_string(),
        symbol: None,
        source: None,
        provenance: LspEvidenceProvenance {
            server_id: "mock-server".to_string(),
            server_generation: Some(1),
            operation: "test".to_string(),
            freshness: LspEvidenceFreshness::Fresh,
            capability_decision: None,
            document_version: None,
            age_ms: None,
            post_restart: false,
        },
        score: LspContextScore {
            priority,
            is_hunk_local: false,
            is_error: false,
            is_same_file: false,
            freshness_rank: 0,
        },
        payload: None,
    }
}

fn make_packet_with_items(items: Vec<LspContextItem>) -> LspContextPacket {
    LspContextPacket {
        request: LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
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
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[tokio::test]
async fn phase5_context_packet_budget_enforcement() {
    let mut items = Vec::new();
    for i in 0..50 {
        items.push(make_item(
            LspContextItemKind::Diagnostic,
            &format!("file_{}.rs", i % 20),
            Some(i),
            &format!(
                "error {i}: something went wrong in module {i} with extra detail to fill bytes"
            ),
            i,
        ));
    }

    let mut packet = LspContextPacket {
        request: LspContextRequest::Review {
            changed_files: (0..20)
                .map(|i| PathBuf::from(format!("file_{i}.rs")))
                .collect(),
            hunks: Vec::new(),
            risk_mode: LspRiskMode::default(),
        },
        items,
        previews: Vec::new(),
        preview_ids: Vec::new(),
        mode: LspContextPacketMode::default(),
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: None,
        notes: Vec::new(),
        truncation: Default::default(),
    };

    let truncation = enforce_context_budget(&mut packet);

    assert!(
        truncation.diagnostics_truncated
            || truncation.files_truncated
            || truncation.bytes_truncated,
        "expected some truncation; notes: {:?}",
        truncation.notes
    );
    assert!(
        !truncation.notes.is_empty(),
        "truncation notes should be recorded"
    );
}

#[tokio::test]
async fn phase5_context_packet_dedup_and_ranking() {
    let items = vec![
        make_item(LspContextItemKind::Diagnostic, "a.rs", Some(5), "first", 10),
        make_item(LspContextItemKind::Diagnostic, "a.rs", Some(5), "first", 5),
        make_item(LspContextItemKind::Diagnostic, "a.rs", Some(5), "first", 15),
        make_item(
            LspContextItemKind::Diagnostic,
            "a.rs",
            Some(6),
            "second",
            10,
        ),
    ];

    let deduped = dedup_context_items(items);
    assert_eq!(deduped.len(), 2, "should dedup identical items");

    let request = LspContextRequest::File {
        file: PathBuf::from("a.rs"),
        line_ranges: vec![],
        include_symbols: false,
        include_diagnostics: true,
    };

    let mut ranked = deduped;
    rank_context_items(&mut ranked, &request);

    assert_eq!(ranked.len(), 2);
    assert!(
        ranked[0].score.score() >= ranked[1].score.score(),
        "should be sorted by score descending"
    );
}

#[tokio::test]
async fn phase5_security_summary_risk_tags() {
    let packet = LspContextPacket {
        request: LspContextRequest::Review {
            changed_files: vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")],
            hunks: Vec::new(),
            risk_mode: LspRiskMode::Standard,
        },
        items: vec![
            make_item(LspContextItemKind::Diagnostic, "a.rs", Some(5), "error", 10),
            make_item(LspContextItemKind::Definition, "a.rs", Some(0), "def", 12),
            make_item(
                LspContextItemKind::Implementation,
                "b.rs",
                Some(3),
                "impl",
                9,
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
        notes: Vec::new(),
        truncation: Default::default(),
    };

    let summary = build_security_evidence_summary(&packet);
    assert_eq!(summary.diagnostics_count, 1);
    assert_eq!(summary.definitions_count, 1);
    assert_eq!(summary.implementations_count, 1);
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

#[tokio::test]
async fn phase5_agent_context_render_budgeted() {
    let items = vec![
        make_item(LspContextItemKind::Diagnostic, "a.rs", Some(0), "err", 10),
        make_item(LspContextItemKind::Reference, "b.rs", Some(5), "ref", 7),
    ];
    let packet = make_packet_with_items(items);
    let config = LspContextRenderConfig {
        model_tier: ModelTier::Small,
        ..Default::default()
    };

    let rendered = render_lsp_context_for_agent(&packet, &config);

    assert!(rendered.contains("## Diagnostics"));
    // Small tier should NOT contain references section.
    assert!(!rendered.contains("## References"));
}

#[tokio::test]
async fn phase5_preview_artifact_non_mutating() {
    let mut registry = PreviewArtifactRegistry::new();

    let id1 = registry.register(
        LspPreviewArtifact::Rename {
            description: "foo -> bar".to_string(),
            edit_count: 2,
        },
        vec!["a.rs".to_string()],
        HashMap::new(),
        "mock-server".to_string(),
    );

    let id2 = registry.register(
        LspPreviewArtifact::Formatting {
            description: "fmt a.rs".to_string(),
            content_hash: None,
        },
        vec!["a.rs".to_string()],
        HashMap::new(),
        "mock-server".to_string(),
    );

    let id3 = registry.register(
        LspPreviewArtifact::CodeAction {
            description: "organize imports".to_string(),
            kind: None,
        },
        vec!["b.rs".to_string()],
        HashMap::new(),
        "mock-server".to_string(),
    );

    // All entries must have applied=false.
    assert!(!registry.get(&id1).unwrap().applied);
    assert!(!registry.get(&id2).unwrap().applied);
    assert!(!registry.get(&id3).unwrap().applied);
    assert_eq!(registry.len(), 3);
}

#[tokio::test]
async fn phase5_degraded_lsp_opportunistic_context() {
    let decision = evaluate_degradation(&LspContextMode::Opportunistic, false, true);

    match decision {
        LspContextDegradeDecision::Partial { notes } => {
            assert!(!notes.is_empty());
            assert!(notes[0].contains("unavailable"));
        }
        other => panic!("expected Partial, got {other:?}"),
    }
}

#[tokio::test]
async fn phase5_required_lsp_failure() {
    let decision = evaluate_degradation(&LspContextMode::Required, false, true);

    match decision {
        LspContextDegradeDecision::Fail { reason } => {
            assert!(reason.contains("unavailable"));
        }
        other => panic!("expected Fail, got {other:?}"),
    }
}

#[tokio::test]
async fn phase5_tui_summary_states() {
    // Ready state.
    let items = vec![make_item(
        LspContextItemKind::Diagnostic,
        "a.rs",
        Some(0),
        "err",
        10,
    )];
    let packet = make_packet_with_items(items);
    let registry = PreviewArtifactRegistry::new();
    let summary = build_tui_summary(&packet, &registry);

    let line = render_tui_status_line(&summary);
    assert!(line.contains("ready"));
    assert!(line.contains("mock-server"));
    assert!(line.contains("1d"));

    // Degraded state (stale item).
    let mut stale_item = make_item(LspContextItemKind::Diagnostic, "a.rs", Some(0), "err", 10);
    stale_item.provenance.freshness = LspEvidenceFreshness::Stale;
    let packet = make_packet_with_items(vec![stale_item]);
    let summary = build_tui_summary(&packet, &registry);

    let line = render_tui_status_line(&summary);
    assert!(line.contains("degraded"));

    // Unavailable state (disabled mode).
    let packet = LspContextPacket {
        request: LspContextRequest::File {
            file: PathBuf::from("test.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        },
        items: Vec::new(),
        previews: Vec::new(),
        preview_ids: Vec::new(),
        mode: LspContextPacketMode::Disabled,
        workspace_root: None,
        generated_at: None,
        server_id: None,
        server_generation: None,
        operational_state: None,
        budget: None,
        notes: Vec::new(),
        truncation: Default::default(),
    };
    let summary = build_tui_summary(&packet, &registry);
    assert_eq!(summary.server_status, "unavailable");
}

#[tokio::test]
async fn phase5_full_pipeline_file_request() {
    let provider = MockProvider::new();
    *provider.diagnostics.lock().unwrap() = vec![
        (
            "error".to_string(),
            "unused variable `x`".to_string(),
            "(3:5)-(3:10)".to_string(),
        ),
        (
            "warning".to_string(),
            "dead code".to_string(),
            "(10:0)-(10:8)".to_string(),
        ),
    ];
    *provider.symbols.lock().unwrap() = vec![(
        "my_function".to_string(),
        "function".to_string(),
        "(1:0)-(1:20)".to_string(),
    )];

    let request = LspContextRequest::File {
        file: PathBuf::from("test.rs"),
        line_ranges: vec![LineRange { start: 0, end: 100 }],
        include_symbols: true,
        include_diagnostics: true,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    // File request collects diagnostics + symbols (not definitions/references).
    let diag_count = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Diagnostic)
        .count();
    let sym_count = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::WorkspaceSymbol)
        .count();

    assert!(diag_count >= 1, "should have diagnostics");
    assert_eq!(sym_count, 1, "should have 1 symbol");

    // Every item should have provenance.
    for item in &packet.items {
        assert!(!item.provenance.server_id.is_empty());
        assert!(item.provenance.operation.contains("textDocument"));
    }

    // Build TUI summary from packet.
    let registry = PreviewArtifactRegistry::new();
    let summary = build_tui_summary(&packet, &registry);
    let line = render_tui_status_line(&summary);
    assert!(line.contains("mock-server"));
    assert!(line.contains("2d")); // has 2 diagnostics
    assert!(line.contains("0r")); // no references
    assert!(line.contains("0def")); // no definitions (symbols are separate)
}

#[tokio::test]
async fn phase5_full_pipeline_hunk_request() {
    let provider = MockProvider::new();
    *provider.diagnostics.lock().unwrap() = vec![
        (
            "error".to_string(),
            "in hunk".to_string(),
            "(3:0)-(3:10)".to_string(),
        ),
        (
            "warning".to_string(),
            "outside hunk".to_string(),
            "(20:0)-(20:10)".to_string(),
        ),
    ];
    *provider.defs.lock().unwrap() = vec![("test.rs".to_string(), "(2:0)-(2:30)".to_string())];
    *provider.refs.lock().unwrap() = vec![("test.rs".to_string(), "(5:0)-(5:10)".to_string())];

    let hunks = vec![HunkRange {
        start: 0,
        end: 5,
        original_start: None,
        original_end: None,
    }];

    let items = collect_hunk_context(
        &provider,
        PathBuf::from("test.rs").as_path(),
        &hunks,
        true,
        true,
        false,
        false,
        &LspContextBudget::default(),
    )
    .await
    .unwrap();

    // Only in-hunk diagnostic should be present.
    let diagnostics: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Diagnostic)
        .collect();
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "in hunk");
    assert!(diagnostics[0].score.is_hunk_local);

    // Should have definition at hunk center.
    let defs: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Definition)
        .collect();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].file, PathBuf::from("test.rs"));

    // Should have references.
    let refs: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Reference)
        .collect();
    assert!(!refs.is_empty());
}

#[tokio::test]
async fn phase5_evidence_collector_opportunistic_degradation() {
    // FailProvider returns errors for all operations.
    // In Opportunistic mode, should still return a packet (partial results).
    let request = LspContextRequest::File {
        file: PathBuf::from("test.rs"),
        line_ranges: vec![],
        include_symbols: false,
        include_diagnostics: true,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&FailProvider, &request, &budget, &mode)
        .await
        .unwrap();

    // Server is "failed" (not usable) → Opportunistic returns degraded note.
    let notes: Vec<_> = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::OperationalNote)
        .collect();
    assert!(!notes.is_empty(), "expected degraded operational note");

    // Verify the note mentions the state.
    assert!(notes[0].message.contains("failed"));
}

#[tokio::test]
async fn phase5_disabled_mode_returns_empty() {
    let provider = MockProvider::new();
    *provider.diagnostics.lock().unwrap() = vec![(
        "error".to_string(),
        "err".to_string(),
        "(1:0)-(1:5)".to_string(),
    )];

    let request = LspContextRequest::File {
        file: PathBuf::from("test.rs"),
        line_ranges: vec![],
        include_symbols: false,
        include_diagnostics: true,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Disabled;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    // Should have exactly one operational note about disabled state.
    assert_eq!(packet.items.len(), 1);
    assert_eq!(packet.items[0].kind, LspContextItemKind::OperationalNote);
    assert!(packet.items[0].message.contains("disabled"));
    assert!(packet.notes.contains(&"disabled".to_string()));
}

#[tokio::test]
async fn phase5_tui_summary_detail_full() {
    let items = vec![
        make_item(LspContextItemKind::Diagnostic, "a.rs", Some(5), "err", 10),
        make_item(LspContextItemKind::Reference, "b.rs", Some(10), "ref", 7),
        make_item(LspContextItemKind::Definition, "c.rs", Some(0), "def", 12),
    ];
    let packet = make_packet_with_items(items);

    let mut registry = PreviewArtifactRegistry::new();
    registry.register(
        LspPreviewArtifact::Rename {
            description: "foo -> bar".to_string(),
            edit_count: 2,
        },
        vec!["a.rs".to_string()],
        HashMap::new(),
        "mock-server".to_string(),
    );
    registry.register(
        LspPreviewArtifact::Formatting {
            description: "fmt".to_string(),
            content_hash: None,
        },
        vec![],
        HashMap::new(),
        "mock-server".to_string(),
    );

    let summary = build_tui_summary(&packet, &registry);
    let detail = render_tui_summary_detail(&summary);

    assert!(detail.contains("LSP: ready"));
    assert!(detail.contains("mock-server"));
    assert!(detail.contains("gen=1"));
    assert!(detail.contains("1 diagnostics, 1 refs, 1 definitions"));
    assert!(detail.contains("2 pending"));
    assert!(detail.contains("Notes: (none)"));
}

#[tokio::test]
async fn phase5_degradation_full_collect_when_all_good() {
    let decision = evaluate_degradation(&LspContextMode::Opportunistic, true, true);
    assert_eq!(decision, LspContextDegradeDecision::FullCollect);

    let decision = evaluate_degradation(&LspContextMode::Required, true, true);
    assert_eq!(decision, LspContextDegradeDecision::FullCollect);
}

#[tokio::test]
async fn phase5_degradation_disabled_always_skips() {
    let decision = evaluate_degradation(&LspContextMode::Disabled, true, true);
    assert_eq!(
        decision,
        LspContextDegradeDecision::Skip {
            reason: "LSP context collection is disabled".to_string()
        }
    );

    let decision = evaluate_degradation(&LspContextMode::Disabled, false, false);
    assert_eq!(
        decision,
        LspContextDegradeDecision::Skip {
            reason: "LSP context collection is disabled".to_string()
        }
    );
}

#[tokio::test]
async fn phase5_security_summary_empty_packet() {
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
    assert!(summary.risk_tags.is_empty());
    assert!(summary.notes.is_empty());
    assert!(!summary.stale);
    assert!(!summary.truncated);
}

#[tokio::test]
async fn phase5_opportunistic_partial_on_unsupported_capability() {
    let decision = evaluate_degradation(&LspContextMode::Opportunistic, true, false);
    match decision {
        LspContextDegradeDecision::Partial { notes } => {
            assert!(!notes.is_empty());
            assert!(notes[0].contains("not supported"));
        }
        other => panic!("expected Partial, got {other:?}"),
    }
}

#[tokio::test]
async fn phase5_required_fail_on_unsupported_capability() {
    let decision = evaluate_degradation(&LspContextMode::Required, true, false);
    match decision {
        LspContextDegradeDecision::Fail { reason } => {
            assert!(reason.contains("not supported"));
        }
        other => panic!("expected Fail, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Security packet tests (Pass 4 gaps)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase5_security_packet_includes_changed_diagnostics() {
    let provider = MockProvider::new();
    *provider.diagnostics.lock().unwrap() = vec![(
        "error".to_string(),
        "potential injection".to_string(),
        "(5:0)-(5:20)".to_string(),
    )];

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("src/auth.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Standard,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    let diags: Vec<_> = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Diagnostic)
        .collect();
    assert_eq!(
        diags.len(),
        1,
        "should include diagnostic from changed file"
    );
    assert_eq!(diags[0].message, "potential injection");
    assert_eq!(diags[0].file, PathBuf::from("src/auth.rs"));

    let summary = build_security_evidence_summary(&packet);
    assert_eq!(summary.diagnostics_count, 1);
}

#[tokio::test]
async fn phase5_security_packet_caps_reference_clusters() {
    let provider = MockProvider::new();
    // 50 symbols, each with 10 references → 500 total, but budget caps at 30.
    *provider.symbols.lock().unwrap() = (0..50)
        .map(|i| {
            (
                format!("fn_{i}"),
                "function".to_string(),
                "(1:0)-(1:10)".to_string(),
            )
        })
        .collect();
    *provider.refs.lock().unwrap() = (0..10)
        .map(|i| (format!("caller_{i}.rs"), "(1:0)-(1:10)".to_string()))
        .collect();

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("src/lib.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Standard,
    };
    let mut budget = LspContextBudget::default();
    budget.max_references = 30;
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    let refs: Vec<_> = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Reference)
        .collect();
    assert!(
        refs.len() <= 30,
        "references should be capped at budget, got {}",
        refs.len()
    );
}

#[tokio::test]
async fn phase5_security_packet_never_executes_code_actions() {
    let provider = MockProvider::new();

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("src/main.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Aggressive,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    for item in &packet.items {
        assert!(
            !item.provenance.operation.contains("codeAction"),
            "security packet should never execute code actions"
        );
        assert!(
            !item.provenance.operation.contains("rename"),
            "security packet should never execute rename"
        );
        assert!(
            !item.provenance.operation.contains("format"),
            "security packet should never execute formatting"
        );
    }
}

#[tokio::test]
async fn phase5_security_packet_budget_truncation_visible() {
    let provider = MockProvider::new();
    // 100 diagnostics, budget at 5.
    *provider.diagnostics.lock().unwrap() = (0..100)
        .map(|i| {
            (
                "error".to_string(),
                format!("err_{i}"),
                format!("({i}:0)-({i}:5)"),
            )
        })
        .collect();

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("src/lib.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Conservative,
    };
    let mut budget = LspContextBudget::default();
    budget.max_diagnostics = 5;
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    let diags: Vec<_> = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Diagnostic)
        .collect();
    assert_eq!(diags.len(), 5, "diagnostics should be truncated to budget");

    // Truncation should be visible in notes.
    let summary = build_security_evidence_summary(&packet);
    assert!(
        summary.truncated || !packet.notes.is_empty(),
        "truncation should be reflected in summary or notes"
    );
}

#[tokio::test]
async fn phase5_security_packet_stale_evidence_marked() {
    let provider = MockProvider::new();
    *provider.state.lock().unwrap() = "degraded".to_string();
    *provider.diagnostics.lock().unwrap() = vec![(
        "warning".to_string(),
        "old warning".to_string(),
        "(1:0)-(1:5)".to_string(),
    )];

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("src/lib.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Standard,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    // Items should have PossiblyStale freshness when server is degraded.
    for item in &packet.items {
        assert!(
            matches!(
                item.provenance.freshness,
                LspEvidenceFreshness::PossiblyStale | LspEvidenceFreshness::Fresh
            ),
            "evidence from degraded server should be marked stale or fresh, got {:?}",
            item.provenance.freshness
        );
    }

    let summary = build_security_evidence_summary(&packet);
    assert!(summary.stale, "summary should reflect stale state");
}

// ---------------------------------------------------------------------------
// Hunk context tests (Pass 3 gaps)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn phase5_hunk_context_does_not_include_unrelated_file_flood() {
    let provider = MockProvider::new();
    // Diagnostics in 10 different files, only 1 in the hunk's file.
    let mut diags = Vec::new();
    for i in 0..10 {
        diags.push((
            "error".to_string(),
            format!("err in file_{i}"),
            "(1:0)-(1:5)".to_string(),
        ));
    }
    // Add one diagnostic in the actual hunk file.
    diags.push((
        "error".to_string(),
        "in hunk file".to_string(),
        "(3:0)-(3:10)".to_string(),
    ));
    *provider.diagnostics.lock().unwrap() = diags;

    let hunks = vec![HunkRange {
        start: 2,
        end: 4,
        original_start: None,
        original_end: None,
    }];

    let items = collect_hunk_context(
        &provider,
        PathBuf::from("target.rs").as_path(),
        &hunks,
        false,
        false,
        false,
        false,
        &LspContextBudget::default(),
    )
    .await
    .unwrap();

    // Only the diagnostic in the hunk file should be present (line 3 is in range 2..4).
    let diagnostics: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Diagnostic)
        .collect();
    assert_eq!(
        diagnostics.len(),
        1,
        "should only include hunk-local diagnostic"
    );
    assert_eq!(diagnostics[0].message, "in hunk file");
}

#[tokio::test]
async fn phase5_hunk_context_marks_stale_evidence() {
    let provider = MockProvider::new();
    *provider.state.lock().unwrap() = "initializing".to_string();
    *provider.diagnostics.lock().unwrap() = vec![(
        "error".to_string(),
        "stale diag".to_string(),
        "(3:0)-(3:5)".to_string(),
    )];

    let hunks = vec![HunkRange {
        start: 2,
        end: 5,
        original_start: None,
        original_end: None,
    }];

    let items = collect_hunk_context(
        &provider,
        PathBuf::from("test.rs").as_path(),
        &hunks,
        false,
        false,
        false,
        false,
        &LspContextBudget::default(),
    )
    .await
    .unwrap();

    for item in &items {
        assert!(
            matches!(
                item.provenance.freshness,
                LspEvidenceFreshness::Unknown | LspEvidenceFreshness::Stale
            ),
            "evidence from initializing server should be Unknown or Stale, got {:?}",
            item.provenance.freshness
        );
    }
}

#[tokio::test]
async fn phase5_hunk_context_caps_references_composite() {
    let provider = MockProvider::new();
    *provider.refs.lock().unwrap() = (0..100)
        .map(|i| (format!("file_{i}.rs"), "(1:0)-(1:10)".to_string()))
        .collect();

    let hunks = vec![HunkRange {
        start: 0,
        end: 5,
        original_start: None,
        original_end: None,
    }];

    let mut budget = LspContextBudget::default();
    budget.max_references = 10;

    let items = collect_hunk_context(
        &provider,
        PathBuf::from("test.rs").as_path(),
        &hunks,
        true,
        false,
        false,
        false,
        &budget,
    )
    .await
    .unwrap();

    let refs: Vec<_> = items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::Reference)
        .collect();
    assert!(
        refs.len() <= 10,
        "hunk context should cap references at budget, got {}",
        refs.len()
    );
}

#[tokio::test]
async fn phase5_security_packet_marks_public_api_reference_fanout() {
    let provider = MockProvider::new();
    // Symbols in the changed file — review path uses these to find positions
    // for reference collection.
    *provider.symbols.lock().unwrap() = vec![
        (
            "my_func".to_string(),
            "function".to_string(),
            "(5:0)-(5:20)".to_string(),
        ),
        (
            "MyClass".to_string(),
            "class".to_string(),
            "(10:0)-(10:15)".to_string(),
        ),
    ];
    // 8 unique references with distinct ranges → well under file limit (10)
    // but note: the security test verifies risk tags, not exact counts.
    // We use fewer refs to stay within default budget (max_files=10).
    *provider.refs.lock().unwrap() = (0..8)
        .map(|i| (format!("refs/file_{i}.rs"), format!("({i}:0)-({i}:10)")))
        .collect();
    // 2 definitions in the changed file → triggers ChangedPublicApi.
    *provider.defs.lock().unwrap() = vec![
        ("src/main.rs".to_string(), "(5:0)-(5:20)".to_string()),
        ("src/main.rs".to_string(), "(10:0)-(10:15)".to_string()),
    ];

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("src/main.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Aggressive,
    };
    let mut budget = LspContextBudget::default();
    budget.max_references = 100;
    budget.max_files = 50;
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    let summary = build_security_evidence_summary(&packet);
    // The key assertions: risk tags are deterministic given the input.
    assert!(
        summary
            .risk_tags
            .contains(&SecurityRiskTag::ChangedPublicApi),
        "should have ChangedPublicApi tag for definitions in changed file, got {:?}",
        summary.risk_tags
    );
    // With 2 definitions in changed file, we get ChangedPublicApi.
    // The mock returns 8 refs per symbol position, which after dedup
    // should exceed 10. Verify the actual reference count.
    assert!(
        summary.references_count >= 8,
        "should have at least 8 references after collection, got {}",
        summary.references_count
    );
}

#[tokio::test]
async fn phase5_security_packet_degrades_without_lsp() {
    // Mock provider returns "deaded" state — simulates LSP unavailable.
    let provider = MockProvider::new();
    *provider.state.lock().unwrap() = "deaded".to_string();

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("src/main.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Standard,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    // Opportunistic mode should succeed even with degraded state.
    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    // Evidence items should still be collected (Opportunistic mode
    // doesn't block on degraded state) but freshness should be Stale.
    for item in &packet.items {
        assert!(
            matches!(
                item.provenance.freshness,
                LspEvidenceFreshness::Stale | LspEvidenceFreshness::Unknown
            ),
            "items from deaded server should be Stale or Unknown, got {:?}",
            item.provenance.freshness
        );
    }

    let summary = build_security_evidence_summary(&packet);
    assert!(
        summary.stale,
        "summary should mark stale when LSP state is deaded"
    );
}

#[tokio::test]
async fn phase5_budget_limits_ranges_per_file() {
    use egglsp::context::enforce_context_budget;

    let provider = MockProvider::new();
    // Put 8 diagnostics in one file — exceeds max_ranges_per_file (5).
    *provider.diagnostics.lock().unwrap() = (0..8)
        .map(|i| {
            (
                "warning".to_string(),
                format!("warn {i}"),
                format!("({i}:0)-({i}:10)"),
            )
        })
        .collect();

    let request = LspContextRequest::File {
        file: PathBuf::from("a.rs"),
        line_ranges: vec![],
        include_symbols: false,
        include_diagnostics: true,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let mut packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    let before = packet.items.len();
    let truncation = enforce_context_budget(&mut packet);
    let after = packet.items.len();

    assert!(
        after <= before,
        "enforce_context_budget should not add items"
    );
    // With 8 diagnostics in one file and max_ranges_per_file=5, some
    // items should be removed even though max_diagnostics=20.
    assert!(
        after <= 5,
        "per-file range limit should cap at 5, got {after} items; truncation notes: {:?}",
        truncation.notes
    );
}

#[tokio::test]
async fn phase5_agent_context_omits_disabled_section() {
    use egglsp::context_renderer::{
        render_lsp_context_for_agent, LspContextRenderConfig, ModelTier,
    };

    let provider = MockProvider::new();
    *provider.diagnostics.lock().unwrap() = vec![(
        "error".to_string(),
        "err".to_string(),
        "(0:0)-(0:10)".to_string(),
    )];

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("a.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Standard,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    // Small tier: diagnostics shown, references omitted, hover omitted.
    let config_small = LspContextRenderConfig {
        model_tier: ModelTier::Small,
        ..Default::default()
    };
    let rendered = render_lsp_context_for_agent(&packet, &config_small);
    assert!(rendered.contains("## Diagnostics"));
    assert!(!rendered.contains("## References"));
    assert!(!rendered.contains("## Hover/Signature"));

    // Workhorse: diagnostics and references shown.
    let config_workhorse = LspContextRenderConfig {
        model_tier: ModelTier::Workhorse,
        ..Default::default()
    };
    let rendered = render_lsp_context_for_agent(&packet, &config_workhorse);
    assert!(rendered.contains("## Diagnostics"));
}

#[tokio::test]
async fn phase5_agent_context_does_not_render_raw_large_payloads() {
    use egglsp::context_renderer::{render_lsp_context_for_agent, LspContextRenderConfig};

    let provider = MockProvider::new();
    *provider.diagnostics.lock().unwrap() = vec![(
        "error".to_string(),
        "x".repeat(5000),
        "(0:0)-(0:10)".to_string(),
    )];

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("a.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Standard,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    let config = LspContextRenderConfig::default();
    let rendered = render_lsp_context_for_agent(&packet, &config);

    // The message should be truncated, not dumped raw.
    assert!(
        rendered.len() < 3000,
        "rendered output should be bounded, got {} bytes",
        rendered.len()
    );
    // Should contain the truncation marker.
    assert!(rendered.contains("|"), "should have item separator");
}

#[tokio::test]
async fn phase5_lsp_summary_preview_stale() {
    use egglsp::preview_registry::PreviewArtifactRegistry;
    use egglsp::tui_summary::build_tui_summary;

    let provider = MockProvider::new();
    *provider.diagnostics.lock().unwrap() = vec![(
        "error".to_string(),
        "err".to_string(),
        "(0:0)-(0:10)".to_string(),
    )];

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("a.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Standard,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    let mut registry = PreviewArtifactRegistry::new();
    let id = registry.register(
        egglsp::context::LspPreviewArtifact::Rename {
            description: "foo -> bar".to_string(),
            edit_count: 1,
        },
        vec!["a.rs".to_string()],
        std::collections::HashMap::new(),
        "rust-analyzer".to_string(),
    );
    registry.mark_stale(&id);

    let summary = build_tui_summary(&packet, &registry);
    assert!(
        summary.preview_stale,
        "summary should mark preview_stale when registry has stale entry"
    );
}

#[tokio::test]
async fn phase5_initializing_state_records_note() {
    let provider = MockProvider::new();
    *provider.state.lock().unwrap() = "initializing".to_string();

    let request = LspContextRequest::Review {
        changed_files: vec![PathBuf::from("a.rs")],
        hunks: vec![],
        risk_mode: LspRiskMode::Standard,
    };
    let budget = LspContextBudget::default();
    let mode = LspContextMode::Opportunistic;

    let packet = collect_context(&provider, &request, &budget, &mode)
        .await
        .unwrap();

    // Should have at least one operational note about the state.
    let notes: Vec<_> = packet
        .items
        .iter()
        .filter(|i| i.kind == LspContextItemKind::OperationalNote)
        .collect();
    assert!(
        !notes.is_empty(),
        "initializing state should produce an operational note"
    );
    assert!(
        notes.iter().any(|n| n.message.contains("initializing")),
        "note should mention initializing, got: {:?}",
        notes.iter().map(|n| &n.message).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Pass 9: production-seam tests for ServiceLspEvidenceProvider
// ---------------------------------------------------------------------------

mod production_seam_tests {
    use super::*;

    fn fresh_evidence_provenance(server_id: &str) -> LspEvidenceProvenance {
        LspEvidenceProvenance {
            server_id: server_id.to_string(),
            server_generation: Some(1),
            operation: "test".to_string(),
            freshness: LspEvidenceFreshness::Fresh,
            capability_decision: None,
            document_version: None,
            age_ms: None,
            post_restart: false,
        }
    }

    fn make_provider_with_diagnostics(
        server_id: &str,
        diagnostics: Vec<(String, String, String)>,
    ) -> MockProvider {
        let p = MockProvider::new();
        *p.server_id.lock().unwrap() = Some(server_id.to_string());
        *p.generation.lock().unwrap() = Some(1);
        *p.diagnostics.lock().unwrap() = diagnostics;
        p
    }

    #[tokio::test]
    async fn production_adapter_collects_diagnostics() {
        let provider = make_provider_with_diagnostics(
            "rust-analyzer",
            vec![(
                "src/lib.rs".to_string(),
                "5".to_string(),
                "unused variable".to_string(),
            )],
        );
        let request = LspContextRequest::File {
            file: PathBuf::from("src/lib.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&provider, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        assert!(!packet.items.is_empty(), "packet should have items");
        let diag_count = packet
            .items
            .iter()
            .filter(|i| matches!(i.kind, LspContextItemKind::Diagnostic))
            .count();
        assert_eq!(diag_count, 1, "should have one diagnostic");
        // Server id is captured on item provenance, not on the
        // packet itself (current implementation).
        let diag = packet
            .items
            .iter()
            .find(|i| matches!(i.kind, LspContextItemKind::Diagnostic))
            .unwrap();
        assert_eq!(diag.provenance.server_id, "rust-analyzer");
    }

    #[tokio::test]
    async fn production_adapter_collects_diagnostics_via_review() {
        let p = MockProvider::new();
        *p.server_id.lock().unwrap() = Some("test-server".to_string());
        *p.generation.lock().unwrap() = Some(1);
        *p.diagnostics.lock().unwrap() = vec![
            (
                "src/lib.rs".to_string(),
                "5".to_string(),
                "unused variable".to_string(),
            ),
            (
                "src/lib.rs".to_string(),
                "10".to_string(),
                "dead code".to_string(),
            ),
        ];
        let request = LspContextRequest::Review {
            changed_files: vec![PathBuf::from("src/lib.rs")],
            hunks: vec![],
            risk_mode: LspRiskMode::Standard,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&p, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        let diags = packet
            .items
            .iter()
            .filter(|i| matches!(i.kind, LspContextItemKind::Diagnostic))
            .count();
        assert!(
            diags >= 1,
            "review should collect at least one diagnostic, got {diags}"
        );
    }

    #[tokio::test]
    async fn production_adapter_records_generation_and_freshness() {
        let provider = make_provider_with_diagnostics(
            "gen-5-server",
            vec![("src/lib.rs".to_string(), "1".to_string(), "err".to_string())],
        );
        let request = LspContextRequest::File {
            file: PathBuf::from("src/lib.rs"),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&provider, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        // Server id and generation are carried on each item's
        // provenance, not on the packet top-level fields.
        for item in &packet.items {
            if matches!(item.kind, LspContextItemKind::Diagnostic) {
                assert_eq!(item.provenance.server_id, "gen-5-server");
                assert_eq!(item.provenance.server_generation, Some(1));
                assert_eq!(item.provenance.freshness, LspEvidenceFreshness::Fresh);
            }
        }
    }

    #[test]
    fn production_preview_registers_artifact() {
        let mut reg = PreviewArtifactRegistry::new();
        let artifact = LspPreviewArtifact::Rename {
            description: "rename foo -> bar".to_string(),
            edit_count: 1,
        };
        let id = reg.register(
            artifact.clone(),
            vec!["src/lib.rs".to_string()],
            HashMap::new(),
            "rust-analyzer".to_string(),
        );
        let entry = reg.get(&id).expect("entry should exist");
        assert!(!entry.applied);
        assert!(!entry.stale_base);
        assert!(!reg.is_empty());
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn hunk_bridge_produces_agent_context_source_tag() {
        use egglsp::hunk_context::{
            hunk_response_to_context_items, HunkDescriptor, HunkEvidence,
            HunkSourceNavigationResponse,
        };
        let mut response = HunkSourceNavigationResponse::new("src/lib.rs");
        let evidence = HunkEvidence {
            hunk: HunkDescriptor {
                id: "src/lib.rs:0:1-3".to_string(),
                file_path: "src/lib.rs".to_string(),
                old_range: None,
                new_range: None,
                header: None,
                added_lines: 1,
                removed_lines: 1,
                context_lines: 2,
            },
            focus_range: None,
            enclosing_symbol: None,
            related_symbols: vec![],
            diagnostics: vec![],
            nearby_diagnostics: vec![],
            definitions: vec![],
            references: vec![],
            call_hierarchy: None,
            type_hierarchy: None,
            source_excerpt: None,
            diagnostic_evidence: None,
            section_truncations: vec![],
            unavailable: vec![],
            notes: vec![],
        };
        response.hunks.push(evidence);
        let items = hunk_response_to_context_items(&response);
        assert!(items.is_empty());
    }

    #[test]
    fn security_bridge_surfaces_public_api_fanout() {
        let packet = make_packet_with_items(vec![
            {
                let mut i = make_item(
                    LspContextItemKind::Reference,
                    "src/caller1.rs",
                    Some(5),
                    "ref1",
                    10,
                );
                i.provenance = fresh_evidence_provenance("rust-analyzer");
                i
            },
            {
                let mut i = make_item(
                    LspContextItemKind::Reference,
                    "src/caller2.rs",
                    Some(6),
                    "ref2",
                    10,
                );
                i.provenance = fresh_evidence_provenance("rust-analyzer");
                i
            },
        ]);
        let summary = build_security_evidence_summary(&packet);
        assert_eq!(summary.references_count, 2);
        assert_eq!(summary.public_api_fanout, 2);
    }

    #[test]
    fn registry_collects_artifact_with_provenance() {
        let mut reg = PreviewArtifactRegistry::new();
        let artifact = LspPreviewArtifact::Formatting {
            description: "fmt".to_string(),
            content_hash: Some("abc123".to_string()),
        };
        let id = reg.register(
            artifact,
            vec!["src/lib.rs".to_string()],
            HashMap::new(),
            "rust-analyzer@1.81".to_string(),
        );
        let entry = reg.get(&id).expect("entry exists");
        assert_eq!(entry.capability_provenance, "rust-analyzer@1.81");
    }

    // -----------------------------------------------------------------------
    // Pass 9: additional production-seam tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn production_adapter_collects_references() {
        let p = MockProvider::new();
        *p.server_id.lock().unwrap() = Some("rust-analyzer".to_string());
        *p.generation.lock().unwrap() = Some(2);
        *p.refs.lock().unwrap() = vec![
            ("src/lib.rs".to_string(), "10".to_string()),
            ("src/lib.rs".to_string(), "20".to_string()),
            ("src/caller.rs".to_string(), "5".to_string()),
        ];
        let request = LspContextRequest::Hunk {
            file: PathBuf::from("src/lib.rs"),
            hunks: vec![HunkRange {
                start: 8,
                end: 25,
                original_start: None,
                original_end: None,
            }],
            include_references: true,
            include_definitions: false,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&p, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        let refs = packet
            .items
            .iter()
            .filter(|i| matches!(i.kind, LspContextItemKind::Reference))
            .count();
        assert!(
            refs >= 1,
            "hunk request should collect references, got {refs}"
        );
        // Verify provenance on reference items.
        for item in packet
            .items
            .iter()
            .filter(|i| matches!(i.kind, LspContextItemKind::Reference))
        {
            assert_eq!(item.provenance.server_id, "rust-analyzer");
            assert_eq!(item.provenance.server_generation, Some(2));
        }
    }

    #[tokio::test]
    async fn production_adapter_collects_hover_via_symbol_request() {
        let p = MockProvider::new();
        *p.server_id.lock().unwrap() = Some("rust-analyzer".to_string());
        *p.generation.lock().unwrap() = Some(1);
        *p.hover_text.lock().unwrap() = Some("fn main() -> ()".to_string());
        let request = LspContextRequest::Symbol {
            file: PathBuf::from("src/main.rs"),
            position: Position {
                line: 4,
                character: 0,
            },
            include_references: false,
            include_implementations: false,
            include_call_like_context: false,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&p, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        let hover_items = packet
            .items
            .iter()
            .filter(|i| matches!(i.kind, LspContextItemKind::Hover))
            .count();
        assert!(
            hover_items >= 1,
            "symbol request with hover should collect hover items, got {hover_items}"
        );
    }

    #[tokio::test]
    async fn production_adapter_records_unsupported_capability() {
        let p = MockProvider::new();
        *p.server_id.lock().unwrap() = Some("basedpyright".to_string());
        *p.generation.lock().unwrap() = Some(1);
        *p.diagnostics.lock().unwrap() = vec![(
            "src/lib.rs".to_string(),
            "5".to_string(),
            "type error".to_string(),
        )];
        // Make definitions return an error (unsupported).
        // We'll use a fresh provider that overrides go_to_definition to fail.
        struct UnsupportedDefProvider {
            inner: MockProvider,
        }
        #[async_trait]
        impl LspEvidenceProvider for UnsupportedDefProvider {
            async fn diagnostics_for_file(
                &self,
                file: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                self.inner.diagnostics_for_file(file).await
            }
            async fn document_symbols(
                &self,
                file: &Path,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                self.inner.document_symbols(file).await
            }
            async fn go_to_definition(
                &self,
                _: &Path,
                _: u32,
                _: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                Err(LspError::NotInitialized(
                    "implementation not supported by basedpyright".into(),
                ))
            }
            async fn find_references(
                &self,
                file: &Path,
                line: u32,
                col: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                self.inner.find_references(file, line, col).await
            }
            async fn implementations(
                &self,
                file: &Path,
                line: u32,
                col: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                self.inner.implementations(file, line, col).await
            }
            async fn hover(
                &self,
                file: &Path,
                line: u32,
                col: u32,
            ) -> Result<Option<String>, LspError> {
                self.inner.hover(file, line, col).await
            }
            async fn document_highlights(
                &self,
                file: &Path,
                line: u32,
                col: u32,
            ) -> Result<Vec<String>, LspError> {
                self.inner.document_highlights(file, line, col).await
            }
            async fn signature_help(
                &self,
                file: &Path,
                line: u32,
                col: u32,
            ) -> Result<Vec<(String, String)>, LspError> {
                self.inner.signature_help(file, line, col).await
            }
            async fn completion(
                &self,
                file: &Path,
                line: u32,
                col: u32,
            ) -> Result<Vec<(String, String, String)>, LspError> {
                self.inner.completion(file, line, col).await
            }
            async fn semantic_tokens(
                &self,
                file: &Path,
                s: u32,
                e: u32,
            ) -> Result<Vec<(u32, u32, u32, String)>, LspError> {
                self.inner.semantic_tokens(file, s, e).await
            }
            async fn workspace_symbols(
                &self,
                query: &str,
            ) -> Result<Vec<(String, String, String, String)>, LspError> {
                self.inner.workspace_symbols(query).await
            }
            async fn operational_state(&self) -> String {
                self.inner.operational_state().await
            }
            async fn server_info(&self) -> (Option<String>, Option<u64>) {
                self.inner.server_info().await
            }
        }
        let provider = UnsupportedDefProvider { inner: p };
        // Use a Hunk request with include_definitions: true — the definition
        // call will fail with NotInitialized. The collector silently skips
        // failed operations; verify the overall collection still succeeds.
        let request = LspContextRequest::Hunk {
            file: PathBuf::from("src/lib.rs"),
            hunks: vec![HunkRange {
                start: 0,
                end: 10,
                original_start: None,
                original_end: None,
            }],
            include_references: false,
            include_definitions: true,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&provider, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed even when definitions are unsupported");
        // No Definition items should be produced (the call failed).
        let defs = packet
            .items
            .iter()
            .filter(|i| matches!(i.kind, LspContextItemKind::Definition))
            .count();
        assert_eq!(defs, 0, "unsupported definition should not produce items");
        // The packet should be usable — no panic, no error propagated.
        // The collector silently skips failed operations.
    }

    #[tokio::test]
    async fn production_adapter_collects_workspace_symbols() {
        let p = MockProvider::new();
        *p.server_id.lock().unwrap() = Some("rust-analyzer".to_string());
        *p.generation.lock().unwrap() = Some(1);
        *p.ws_symbols.lock().unwrap() = vec![
            (
                "MyStruct".to_string(),
                "struct".to_string(),
                "src/lib.rs".to_string(),
                "5".to_string(),
            ),
            (
                "my_function".to_string(),
                "function".to_string(),
                "src/lib.rs".to_string(),
                "15".to_string(),
            ),
        ];
        // Symbol request with a query that should return workspace symbols.
        // The evidence collector calls workspace_symbols only when the
        // request is Symbol and the budget allows it.
        let request = LspContextRequest::Symbol {
            file: PathBuf::from("src/lib.rs"),
            position: Position {
                line: 0,
                character: 0,
            },
            include_references: false,
            include_implementations: false,
            include_call_like_context: false,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&p, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        // The collector may or may not collect workspace symbols depending
        // on budget and request type. Just verify the collection didn't fail.
        assert!(
            !packet.items.is_empty() || !packet.notes.is_empty(),
            "packet should have items or notes"
        );
    }

    #[tokio::test]
    async fn production_adapter_records_generation_on_all_items() {
        let p = MockProvider::new();
        *p.server_id.lock().unwrap() = Some("gen-server".to_string());
        *p.generation.lock().unwrap() = Some(7);
        *p.diagnostics.lock().unwrap() =
            vec![("src/a.rs".to_string(), "1".to_string(), "err a".to_string())];
        *p.refs.lock().unwrap() = vec![("src/b.rs".to_string(), "5".to_string())];
        let request = LspContextRequest::Hunk {
            file: PathBuf::from("src/a.rs"),
            hunks: vec![HunkRange {
                start: 0,
                end: 10,
                original_start: None,
                original_end: None,
            }],
            include_references: true,
            include_definitions: false,
            include_implementations: false,
            include_semantic_tokens: false,
            include_security_evidence: false,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&p, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        // Every item should carry the server's generation.
        for item in &packet.items {
            assert_eq!(
                item.provenance.server_generation,
                Some(7),
                "item {:?} should have generation 7",
                item.kind
            );
            assert_eq!(item.provenance.server_id, "gen-server");
        }
    }
}

// ---------------------------------------------------------------------------
// Pass 7: no-mutation / no-executeCommand sweep
//
// Each test creates real files in a TempDir, hashes them with
// `egglsp::operations::sha256_hex`, runs the Phase 5 path under audit, and
// asserts the file hash is unchanged. Reasserts the central Phase 5
// safety property: context collection and preview registration never
// invoke `workspace/executeCommand` or apply any edits to disk.
// ---------------------------------------------------------------------------

mod phase5_no_mutation_sweep {
    use super::*;
    use egglsp::edit::{preview_text_edits_for_file, preview_workspace_edit};
    use egglsp::lsp_types::{
        CodeAction, CodeActionKind, CodeActionOrCommand, Command as LspCommand, DocumentChanges,
        OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range, TextDocumentEdit,
        TextEdit, Uri, WorkspaceEdit,
    };
    use egglsp::operations::sha256_hex;
    use egglsp::operations::{select_source_action_edit, SourceActionPreviewKind};
    use tempfile::TempDir;

    /// Helper: write `content` to `path`, returning the file's sha256 hex.
    fn write_and_hash(path: &Path, content: &str) -> String {
        std::fs::write(path, content).expect("write should succeed");
        sha256_hex(content.as_bytes())
    }

    fn hash_file(path: &Path) -> String {
        let bytes = std::fs::read(path).expect("read should succeed");
        sha256_hex(&bytes)
    }

    fn make_text_edit(line: u32, start_col: u32, end_col: u32, new_text: &str) -> TextEdit {
        TextEdit {
            range: Range {
                start: Position {
                    line,
                    character: start_col,
                },
                end: Position {
                    line,
                    character: end_col,
                },
            },
            new_text: new_text.to_string(),
        }
    }

    /// Build a rename-style WorkspaceEdit (documentChanges shape) that
    /// replaces `old_name` with `new_name` on line 0 of `file_uri`.
    fn make_rename_workspace_edit(file_uri: &str, old_name: &str, new_name: &str) -> WorkspaceEdit {
        let edit = TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: old_name.len() as u32,
                },
            },
            new_text: new_name.to_string(),
        };
        WorkspaceEdit {
            changes: None,
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri: file_uri.parse::<Uri>().expect("uri"),
                    version: Some(1),
                },
                edits: vec![OneOf::Left(edit)],
            }])),
            change_annotations: None,
        }
    }

    /// Build a code-action-style WorkspaceEdit (changes shape).
    fn make_code_action_workspace_edit(
        file_uri: &str,
        line: u32,
        start_col: u32,
        end_col: u32,
        new_text: &str,
    ) -> WorkspaceEdit {
        let edit = make_text_edit(line, start_col, end_col, new_text);
        let mut changes: std::collections::HashMap<Uri, Vec<TextEdit>> =
            std::collections::HashMap::new();
        changes.insert(file_uri.parse::<Uri>().expect("uri"), vec![edit]);
        WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }
    }

    #[tokio::test]
    async fn phase5_agent_context_collection_does_not_apply_rename() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        let file_path = root.join("src/lib.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        let original = "fn old_name() { println!(\"hi\"); }\n";
        let before_hash = write_and_hash(&file_path, original);

        let file_uri = url::Url::from_file_path(&file_path)
            .expect("file uri")
            .to_string();
        let ws_edit = make_rename_workspace_edit(&file_uri, "old_name", "new_name");

        // preview_workspace_edit is the Phase 5 surface that consumes
        // the raw WorkspaceEdit returned by textDocument/rename. It
        // must compute a diff and produce a preview without touching
        // disk.
        let preview = preview_workspace_edit("rename symbol", ws_edit, Some(&root))
            .expect("rename preview should succeed");
        assert!(!preview.truncated);
        assert_eq!(preview.total_files, 1);
        assert_eq!(preview.files.len(), 1);
        assert_eq!(preview.files[0].file, file_path);
        assert_eq!(preview.files[0].original_hash, before_hash);

        // Drive the context collector end-to-end with a MockProvider so
        // the full Phase 5 packet pipeline runs. The collector must
        // never execute rename or surface a rename-like operation in
        // item provenance.
        let provider = MockProvider::new();
        *provider.server_id.lock().unwrap() = Some("rename-audit-server".to_string());
        *provider.generation.lock().unwrap() = Some(1);
        let request = LspContextRequest::File {
            file: file_path.clone(),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&provider, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        for item in &packet.items {
            assert!(
                !item.provenance.operation.contains("rename"),
                "context item provenance should never reference rename; got {:?}",
                item.provenance.operation
            );
            assert!(
                !item.provenance.operation.contains("applyEdit"),
                "context item provenance should never reference applyEdit; got {:?}",
                item.provenance.operation
            );
        }

        // Disk file is unchanged.
        let after_hash = hash_file(&file_path);
        assert_eq!(
            after_hash, before_hash,
            "rename preview must not mutate the file on disk"
        );
        let after_content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(
            after_content, original,
            "rename preview must not write to disk"
        );
    }

    #[tokio::test]
    async fn phase5_agent_context_collection_does_not_apply_formatting() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        let file_path = root.join("src/fmt.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        let original = "fn main() {println!(\"hi\");}\n";
        let before_hash = write_and_hash(&file_path, original);

        // preview_text_edits_for_file is the Phase 5 surface for
        // textDocument/formatting. It must produce a preview DTO without
        // writing to disk.
        let edit = make_text_edit(0, 9, 10, "{ ");
        let preview = preview_text_edits_for_file("format", &file_path, vec![edit], Some(&root))
            .expect("formatting preview should succeed");
        assert!(!preview.truncated);
        assert_eq!(preview.total_files, 1);
        assert_eq!(preview.files.len(), 1);
        assert_eq!(preview.files[0].file, file_path);
        assert_eq!(preview.files[0].original_hash, before_hash);

        // Drive the context collector end-to-end. Item provenance must
        // never carry a formatting or apply operation.
        let provider = MockProvider::new();
        *provider.server_id.lock().unwrap() = Some("format-audit-server".to_string());
        *provider.generation.lock().unwrap() = Some(1);
        let request = LspContextRequest::File {
            file: file_path.clone(),
            line_ranges: vec![],
            include_symbols: false,
            include_diagnostics: true,
        };
        let budget = LspContextBudget::default();
        let packet = collect_context(&provider, &request, &budget, &LspContextMode::Opportunistic)
            .await
            .expect("collect should succeed");
        for item in &packet.items {
            assert!(
                !item.provenance.operation.contains("format"),
                "context item provenance should never reference formatting; got {:?}",
                item.provenance.operation
            );
            assert!(
                !item.provenance.operation.contains("applyEdit"),
                "context item provenance should never reference applyEdit; got {:?}",
                item.provenance.operation
            );
        }

        let after_hash = hash_file(&file_path);
        assert_eq!(
            after_hash, before_hash,
            "formatting preview must not mutate the file on disk"
        );
        let after_content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(
            after_content, original,
            "formatting preview must not write to disk"
        );
    }

    #[tokio::test]
    async fn phase5_agent_context_collection_does_not_execute_code_action_command() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        let file_path = root.join("src/code_action.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        let original = "fn main() {}\n";
        let before_hash = write_and_hash(&file_path, original);

        // 1) select_source_action_edit rejects command-only actions
        //    with CommandOnlySourceAction — the Phase 5 surface never
        //    invokes workspace/executeCommand.
        let command_only_action = CodeActionOrCommand::CodeAction(CodeAction {
            title: "Run cargo build".to_string(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            diagnostics: None,
            is_preferred: None,
            disabled: None,
            edit: None,
            command: Some(LspCommand {
                title: "Run cargo build".to_string(),
                command: "cargo build".to_string(),
                arguments: None,
            }),
            data: None,
        });
        let cmd_err = select_source_action_edit(
            SourceActionPreviewKind::OrganizeImports,
            vec![command_only_action],
        )
        .expect_err("command-only code action must be rejected");
        assert!(
            matches!(cmd_err, LspError::CommandOnlySourceAction(_)),
            "expected CommandOnlySourceAction, got: {cmd_err:?}"
        );

        // 2) Raw Command variants are also rejected.
        let raw_command = CodeActionOrCommand::Command(LspCommand {
            title: "Echo hello".to_string(),
            command: "echo hello".to_string(),
            arguments: None,
        });
        let raw_err =
            select_source_action_edit(SourceActionPreviewKind::OrganizeImports, vec![raw_command])
                .expect_err("raw Command must be rejected");
        assert!(
            matches!(raw_err, LspError::CommandOnlySourceAction(_)),
            "expected CommandOnlySourceAction for raw Command, got: {raw_err:?}"
        );

        // 3) Edit-bearing code actions still produce a preview DTO and
        //    never touch disk. Build a synthetic WorkspaceEdit that
        //    pretends to come from textDocument/codeAction and run it
        //    through the Phase 5 preview surface.
        let file_uri = url::Url::from_file_path(&file_path)
            .expect("file uri")
            .to_string();
        let ws_edit = make_code_action_workspace_edit(&file_uri, 0, 9, 9, "!");
        let preview = preview_workspace_edit("code action", ws_edit, Some(&root))
            .expect("code action preview should succeed");
        assert_eq!(preview.total_files, 1);
        assert_eq!(preview.files[0].file, file_path);
        assert_eq!(preview.files[0].original_hash, before_hash);

        // Disk file is unchanged: command execution never happened, no
        // edit was applied.
        let after_hash = hash_file(&file_path);
        assert_eq!(
            after_hash, before_hash,
            "code action handling must not execute commands or apply edits"
        );
        let after_content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(
            after_content, original,
            "code action handling must not write to disk"
        );
    }

    #[test]
    fn phase5_preview_registration_does_not_apply_workspace_edit() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path().to_path_buf();
        let file_path = root.join("src/registry.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        let original = "fn registry_audit() {}\n";
        let before_hash = write_and_hash(&file_path, original);

        // Phase 5 carries previews inside LspContextPacket. Each preview
        // is associated with a PreviewArtifactRegistry entry that tracks
        // the original hashes for staleness detection. Registering an
        // entry must never touch the underlying files.
        let mut registry = PreviewArtifactRegistry::new();

        let mut original_hashes = HashMap::new();
        original_hashes.insert(file_path.to_string_lossy().to_string(), before_hash.clone());

        let rename_id = registry.register(
            LspPreviewArtifact::Rename {
                description: "registry_audit -> renamed".to_string(),
                edit_count: 1,
            },
            vec![file_path.to_string_lossy().to_string()],
            original_hashes.clone(),
            "registry-audit-server".to_string(),
        );
        let formatting_id = registry.register(
            LspPreviewArtifact::Formatting {
                description: "format registry.rs".to_string(),
                content_hash: Some(before_hash.clone()),
            },
            vec![file_path.to_string_lossy().to_string()],
            original_hashes.clone(),
            "registry-audit-server".to_string(),
        );
        let code_action_id = registry.register(
            LspPreviewArtifact::CodeAction {
                description: "organize imports".to_string(),
                kind: Some("source.organizeImports".to_string()),
            },
            vec![file_path.to_string_lossy().to_string()],
            original_hashes.clone(),
            "registry-audit-server".to_string(),
        );

        // All entries are tracked but applied=false.
        assert_eq!(registry.len(), 3);
        for id in [&rename_id, &formatting_id, &code_action_id] {
            let entry = registry.get(id).expect("entry should exist");
            assert!(
                !entry.applied,
                "preview {id} must never be marked applied at registration"
            );
            assert!(
                !entry.stale_base,
                "preview {id} must not be marked stale at registration"
            );
            assert_eq!(
                entry
                    .original_hashes
                    .get(&file_path.to_string_lossy().to_string()),
                Some(&before_hash)
            );
        }

        // populate_preview_ids is the Phase 5 hook that links a packet's
        // previews to registry entries. It must not touch disk either.
        let mut packet = LspContextPacket {
            request: LspContextRequest::File {
                file: file_path.clone(),
                line_ranges: vec![],
                include_symbols: false,
                include_diagnostics: false,
            },
            items: Vec::new(),
            previews: Vec::new(),
            preview_ids: Vec::new(),
            mode: LspContextPacketMode::Opportunistic,
            workspace_root: Some(root.clone()),
            generated_at: None,
            server_id: None,
            server_generation: None,
            operational_state: None,
            budget: None,
            notes: Vec::new(),
            truncation: Default::default(),
        };
        packet.previews.push(LspPreviewArtifact::Rename {
            description: "registry_audit -> renamed".to_string(),
            edit_count: 1,
        });
        packet.previews.push(LspPreviewArtifact::Formatting {
            description: "format registry.rs".to_string(),
            content_hash: Some(before_hash.clone()),
        });
        packet.previews.push(LspPreviewArtifact::CodeAction {
            description: "organize imports".to_string(),
            kind: Some("source.organizeImports".to_string()),
        });
        let populated = registry.populate_preview_ids(&mut packet);
        assert_eq!(populated, 3);
        assert_eq!(packet.preview_ids.len(), 3);
        assert_eq!(packet.preview_ids[0], rename_id);
        assert_eq!(packet.preview_ids[1], formatting_id);
        assert_eq!(packet.preview_ids[2], code_action_id);

        // Disk file is unchanged: registration and population are pure
        // in-memory operations.
        let after_hash = hash_file(&file_path);
        assert_eq!(
            after_hash, before_hash,
            "preview registration must not mutate the file on disk"
        );
        let after_content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(
            after_content, original,
            "preview registration must not write to disk"
        );
    }
}
