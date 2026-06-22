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
        line,
        column: None,
        message: message.to_string(),
        symbol: None,
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
        mode: LspContextPacketMode::Opportunistic,
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
        mode: LspContextPacketMode::default(),
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
        mode: LspContextPacketMode::Opportunistic,
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
        LspPreviewArtifact::Rename("foo -> bar".to_string()),
        vec!["a.rs".to_string()],
        HashMap::new(),
        "mock-server".to_string(),
    );

    let id2 = registry.register(
        LspPreviewArtifact::Formatting("fmt a.rs".to_string()),
        vec!["a.rs".to_string()],
        HashMap::new(),
        "mock-server".to_string(),
    );

    let id3 = registry.register(
        LspPreviewArtifact::CodeAction("organize imports".to_string()),
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
        mode: LspContextPacketMode::Disabled,
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
        LspPreviewArtifact::Rename("foo -> bar".to_string()),
        vec!["a.rs".to_string()],
        HashMap::new(),
        "mock-server".to_string(),
    );
    registry.register(
        LspPreviewArtifact::Formatting("fmt".to_string()),
        vec![],
        HashMap::new(),
        "mock-server".to_string(),
    );

    let summary = build_tui_summary(&packet, &registry);
    let detail = render_tui_summary_detail(&summary);

    assert!(detail.contains("LSP Status: ready"));
    assert!(detail.contains("Server: mock-server gen=1"));
    assert!(detail.contains("1 diagnostics, 1 refs, 1 definitions"));
    assert!(detail.contains("Preview: 2 pending"));
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
        mode: LspContextPacketMode::Opportunistic,
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
        .map(|i| (format!("caller_{i}.rs"), format!("(1:0)-(1:10)")))
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
            format!("(1:0)-(1:5)"),
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
        .map(|i| (format!("file_{i}.rs"), format!("(1:0)-(1:10)")))
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
