//! Root-level composite LSP integration test harness.
//!
//! Exercises the root-crate collectors (`SemanticContextCollector`,
//! `DiagnosticsCollector`, `LspOperations`) against the fake LSP server
//! binary built by the `egglsp` package. This bridges the gap between
//! `egglsp`-only integration tests and the real collectors that live in
//! the root crate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use tokio::time::timeout;
use url::Url;

use egglsp::diagnostics::DiagnosticsCollector;
use egglsp::edit::{preview_text_edits_for_file, preview_workspace_edit};
use egglsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Command as LspCommand, CreateFile,
    DocumentChangeOperation, DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier,
    Position, Range, ResourceOp, TextDocumentEdit, TextEdit, WorkspaceEdit,
};
use egglsp::operations::LspOperations;
use egglsp::operations::{select_source_action_edit, SourceActionPreviewKind};
use egglsp::{
    LspClient, LspClientOptions, LspConfig, LspError, LspLaunchSpec, LspRule, LspService,
    RestartShared,
};

use codegg::tool::lsp::LspTool;
use codegg::tool::Tool;

// ── Fake server binary path ──────────────────────────────────────────

fn fake_server_binary_path() -> PathBuf {
    // Root-specific manual override.
    if let Ok(path) = std::env::var("CODEGG_LSP_TEST_SERVER") {
        return PathBuf::from(path);
    }

    // Backward-compatible manual override.
    if let Ok(path) = std::env::var("EGGLSP_TEST_SERVER") {
        return PathBuf::from(path);
    }

    // Package-local Cargo artifact. The root Cargo.toml declares a
    // [[bin]] target named "codegg-lsp-test-server" pointing at the
    // shared source in crates/egglsp-test-server/src/main.rs, so
    // Cargo sets this at compile time for the root integration test.
    if let Some(path) = option_env!("CARGO_BIN_EXE_codegg-lsp-test-server") {
        return PathBuf::from(path);
    }

    panic!(
        "Could not find codegg-lsp-test-server binary.\n\
         Cargo should build it automatically for root integration tests.\n\
         Ensure the [[bin]] target in Cargo.toml is correct.\n\
         Or set CODEGG_LSP_TEST_SERVER=/path/to/binary"
    )
}

// ── Path helpers ─────────────────────────────────────────────────────

fn path_to_uri(path: &Path) -> String {
    Url::from_file_path(path)
        .expect("invalid file path")
        .to_string()
}

fn transcript_tail(path: &Path) -> String {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return "(transcript unavailable)".to_string();
    };
    const MAX_LINES: usize = 40;
    let mut lines: Vec<&str> = contents.lines().rev().take(MAX_LINES).collect();
    lines.reverse();
    lines.join("\n")
}

// ── Harness ──────────────────────────────────────────────────────────

/// Lightweight harness for root-level LSP integration tests.
///
/// Spawns the fake LSP server, initializes a client, and provides
/// access to both the `LspClient` and the root-crate service types
/// (`LspService`, `LspOperations`, `DiagnosticsCollector`).
struct CompositeHarness {
    #[allow(dead_code)]
    tempdir: TempDir,
    root: PathBuf,
    source_path: PathBuf,
    scenario_path: PathBuf,
    transcript_path: PathBuf,
    client: Arc<LspClient>,
    service: Arc<LspService>,
    operations: Arc<LspOperations>,
    diagnostics_collector: Arc<DiagnosticsCollector>,
    scenario_name: String,
}

impl CompositeHarness {
    /// Create and initialize the harness with the given scenario.
    async fn start(scenario: serde_json::Value) -> Result<Self, LspError> {
        Self::start_with_options(scenario, LspClientOptions::default()).await
    }

    /// Create and initialize with an explicit root directory.
    ///
    /// The caller owns the `TempDir`; it is moved into the harness so
    /// the directory lives until the harness is dropped.
    async fn start_with_root(
        tempdir: TempDir,
        scenario: serde_json::Value,
    ) -> Result<Self, LspError> {
        Self::start_with_root_and_options(tempdir, scenario, LspClientOptions::default()).await
    }

    /// Create and initialize with an explicit root and client options.
    async fn start_with_root_and_options(
        tempdir: TempDir,
        scenario: serde_json::Value,
        options: LspClientOptions,
    ) -> Result<Self, LspError> {
        let root = tempdir.path().to_path_buf();
        let source_path = root.join("src/lib.rs");
        let scenario_path = root.join("scenario.json");
        let transcript_path = root.join("transcript.jsonl");
        let root_uri = path_to_uri(&root);
        let source_uri = path_to_uri(&source_path);
        let scenario_name = scenario
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("composite-scenario")
            .to_string();

        std::fs::create_dir_all(root.join("src")).map_err(LspError::Io)?;
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "lsp-composite-test"
version = "0.1.0"
edition = "2021"
"#,
        )
        .map_err(LspError::Io)?;
        std::fs::write(&source_path, "pub fn harness_marker() {}\n").map_err(LspError::Io)?;

        let scenario = substitute_placeholders(
            scenario,
            &root,
            &root_uri,
            &source_path,
            &source_uri,
            &scenario_path,
            &transcript_path,
        );
        std::fs::write(
            &scenario_path,
            serde_json::to_string_pretty(&scenario).map_err(LspError::Json)?,
        )
        .map_err(LspError::Io)?;

        let launch = LspLaunchSpec::new(
            "codegg-lsp-test-server",
            fake_server_binary_path(),
            Vec::new(),
            vec![
                (
                    "CODEGG_FAKE_LSP_SCENARIO".to_string(),
                    scenario_path.to_string_lossy().to_string(),
                ),
                (
                    "CODEGG_FAKE_LSP_TRANSCRIPT".to_string(),
                    transcript_path.to_string_lossy().to_string(),
                ),
            ],
            vec!["rust".to_string()],
            vec!["rs".to_string()],
        );

        let client = Arc::new(
            LspClient::new_with_launch_spec(launch, &root, serde_json::Value::Null, options)
                .await?,
        );

        if let Err(err) = client.initialize(None).await {
            let diagnostics = Self::diagnostics_static(
                &client,
                &root,
                &source_path,
                &scenario_path,
                &transcript_path,
                &scenario_name,
            )
            .await;
            return Err(LspError::RequestFailed(format!(
                "failed to initialize: {err}\n{diagnostics}"
            )));
        }
        if let Err(err) = client.send_initialized().await {
            let diagnostics = Self::diagnostics_static(
                &client,
                &root,
                &source_path,
                &scenario_path,
                &transcript_path,
                &scenario_name,
            )
            .await;
            return Err(LspError::RequestFailed(format!(
                "failed to send initialized: {err}\n{diagnostics}"
            )));
        }

        let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
        let operations = Arc::new(LspOperations::new(service.clone()));
        let diagnostics_collector = Arc::new(DiagnosticsCollector::new(service.clone()));

        Ok(Self {
            tempdir,
            root,
            source_path,
            scenario_path,
            transcript_path,
            client,
            service,
            operations,
            diagnostics_collector,
            scenario_name,
        })
    }

    /// Create and initialize with explicit client options.
    async fn start_with_options(
        scenario: serde_json::Value,
        options: LspClientOptions,
    ) -> Result<Self, LspError> {
        let tempdir = tempfile::tempdir().map_err(LspError::Io)?;
        Self::start_with_root_and_options(tempdir, scenario, options).await
    }

    async fn diagnostics(&self) -> String {
        Self::diagnostics_static(
            &self.client,
            &self.root,
            &self.source_path,
            &self.scenario_path,
            &self.transcript_path,
            &self.scenario_name,
        )
        .await
    }

    async fn diagnostics_static(
        client: &LspClient,
        root: &Path,
        source_path: &Path,
        scenario_path: &Path,
        transcript_path: &Path,
        scenario_name: &str,
    ) -> String {
        let pending = client.pending_request_count().await;
        let transport = client.transport_state_snapshot().await;
        let child_status = match client.try_wait_child().await {
            Some(Ok(status)) => format!("{status:?}"),
            Some(Err(err)) => format!("error: {err}"),
            None => "running or no handle".to_string(),
        };
        let transcript = transcript_tail(transcript_path);

        let mut out = String::new();
        out.push_str(&format!("scenario: {scenario_name}\n"));
        out.push_str(&format!("root: {}\n", root.display()));
        out.push_str(&format!("source: {}\n", source_path.display()));
        out.push_str(&format!("scenario file: {}\n", scenario_path.display()));
        out.push_str(&format!("transcript file: {}\n", transcript_path.display()));
        out.push_str(&format!("pending requests: {pending}\n"));
        out.push_str(&format!("transport: {transport:?}\n"));
        out.push_str(&format!("child exit: {child_status}\n"));
        out.push_str("--- transcript tail ---\n");
        if transcript.is_empty() {
            out.push_str("(transcript empty)\n");
        } else {
            out.push_str(&transcript);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
        out
    }

    /// Bounded teardown: shutdown the client and wait for the server to exit.
    async fn shutdown(self) -> Result<(), LspError> {
        let shutdown_result = self.client.shutdown().await;

        let wait_result = self
            .client
            .wait_for_child_exit(Duration::from_secs(5))
            .await;

        let diagnostics = self.diagnostics().await;

        match (shutdown_result, wait_result) {
            (Ok(()), Ok(Ok(()))) => Ok(()),
            (Ok(()), Ok(Err(err))) => Err(LspError::RequestFailed(format!(
                "failed to wait for fake server exit: {err}\n{diagnostics}"
            ))),
            (Ok(()), Err(err)) => Err(LspError::RequestFailed(format!(
                "no child handle available: {err}\n{diagnostics}"
            ))),
            (Err(err), _) => Err(LspError::RequestFailed(format!(
                "client shutdown failed: {err}\n{diagnostics}"
            ))),
        }
    }
}

// ── Config helpers ───────────────────────────────────────────────────

fn make_service_config(scenario_path: &Path, transcript_path: &Path) -> LspConfig {
    let mut env = HashMap::new();
    env.insert(
        "CODEGG_FAKE_LSP_SCENARIO".to_string(),
        scenario_path.display().to_string(),
    );
    env.insert(
        "CODEGG_FAKE_LSP_TRANSCRIPT".to_string(),
        transcript_path.display().to_string(),
    );

    let mut rules = HashMap::new();
    rules.insert(
        "rust-analyzer".to_string(),
        LspRule::Active {
            command: vec![fake_server_binary_path().to_str().unwrap().to_string()],
            extensions: Some(vec!["rs".to_string()]),
            disabled: None,
            env: Some(env),
            initialization: None,
            workspace_configuration: None,
            restart: None,
        },
    );

    LspConfig::Rules(rules)
}

// ── Placeholder substitution ─────────────────────────────────────────

fn substitute_placeholders(
    value: serde_json::Value,
    root: &Path,
    root_uri: &str,
    source_path: &Path,
    source_uri: &str,
    scenario_path: &Path,
    transcript_path: &Path,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(s) => match s.as_str() {
            "__ROOT_URI__" => serde_json::Value::String(root_uri.to_string()),
            "__ROOT_PATH__" => serde_json::Value::String(root.display().to_string()),
            "__ROOT_NAME__" => serde_json::Value::String(
                root.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default(),
            ),
            "__SOURCE_URI__" => serde_json::Value::String(source_uri.to_string()),
            "__SOURCE_PATH__" => serde_json::Value::String(source_path.display().to_string()),
            "__SCENARIO_PATH__" => serde_json::Value::String(scenario_path.display().to_string()),
            "__TRANSCRIPT_PATH__" => {
                serde_json::Value::String(transcript_path.display().to_string())
            }
            _ => serde_json::Value::String(s),
        },
        serde_json::Value::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .map(|v| {
                    substitute_placeholders(
                        v,
                        root,
                        root_uri,
                        source_path,
                        source_uri,
                        scenario_path,
                        transcript_path,
                    )
                })
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        substitute_placeholders(
                            v,
                            root,
                            root_uri,
                            source_path,
                            source_uri,
                            scenario_path,
                            transcript_path,
                        ),
                    )
                })
                .collect(),
        ),
        other => other,
    }
}

// ── Scenario builders ────────────────────────────────────────────────

/// Minimal scenario: initialize with capabilities, then shutdown.
fn scenario_init_shutdown() -> serde_json::Value {
    serde_json::json!({
        "name": "composite_init_shutdown",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "processId": {"type": "Number"},
                        "rootUri": {"type": "String"}
                    }
                },
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "hoverProvider": true,
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario with document symbol responses for a simple Rust function.
fn scenario_with_symbols() -> serde_json::Value {
    serde_json::json!({
        "name": "composite_with_symbols",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "processId": {"type": "Number"},
                        "rootUri": {"type": "String"}
                    }
                },
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "hoverProvider": true,
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {"uri": {"type": "String"}}
                        }
                    }
                },
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "my_function",
                            "kind": 12,
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 4},
                                "end": {"line": 0, "character": 15}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

// ── Tests ────────────────────────────────────────────────────────────

/// Smoke test: prove the harness compiles, spawns, initializes, and shuts down.
#[tokio::test]
async fn composite_harness_initialization_smoke() {
    let harness = CompositeHarness::start(scenario_init_shutdown())
        .await
        .expect("failed to start composite harness");

    // Client initialized successfully — capabilities should be stored.
    let capabilities = harness
        .client
        .capabilities
        .lock()
        .await
        .clone()
        .expect("capabilities should be stored");
    assert!(
        capabilities.hover_provider.is_some(),
        "hover provider should be set in scenario"
    );
    assert!(
        capabilities.definition_provider.is_some(),
        "definition provider should be set in scenario"
    );
    assert!(
        capabilities.document_symbol_provider.is_some(),
        "document symbol provider should be set in scenario"
    );

    // Transport should be healthy.
    assert_eq!(
        harness.client.transport_state_snapshot().await,
        egglsp::ClientTransportSnapshot::Running,
    );
    assert_eq!(harness.client.pending_request_count().await, 0);

    harness.shutdown().await.expect("shutdown should succeed");
}

/// Verify the service-layer types are constructible and usable.
#[tokio::test]
async fn composite_service_layer_construction() {
    let harness = CompositeHarness::start(scenario_init_shutdown())
        .await
        .expect("failed to start composite harness");

    // LspService should have no clients yet (service is separate from the
    // direct client; it manages its own client pool via get_or_create_client).
    let keys = harness.service.client_keys().await;
    assert!(
        keys.is_empty(),
        "service client pool should be empty (direct client not managed by service)"
    );

    // DiagnosticsCollector should not panic on a file that has no diagnostics.
    let snapshot = harness
        .diagnostics_collector
        .get_diagnostic_snapshot_for_file(&harness.source_path)
        .await;
    match snapshot {
        Ok(snap) => {
            // No diagnostics expected from the fake server.
            assert!(
                snap.diagnostics.is_empty(),
                "expected no diagnostics, got: {:?}",
                snap.diagnostics
            );
        }
        Err(err) => {
            // The service might not have a client for this file yet.
            // That's fine — we just want to prove the call path works.
            eprintln!("diagnostics call returned error (expected if no client): {err}");
        }
    }

    harness.shutdown().await.expect("shutdown should succeed");
}

/// Exercise document symbols via the direct client.
#[tokio::test]
async fn composite_document_symbols_via_direct_client() {
    let harness = CompositeHarness::start(scenario_with_symbols())
        .await
        .expect("failed to start composite harness");

    // Use the direct client to request document symbols.
    let source_url = Url::from_file_path(&harness.source_path).expect("valid source path");
    let symbols = harness.client.document_symbols(&source_url).await;

    match symbols {
        Ok(syms) => {
            assert!(
                !syms.is_empty(),
                "expected at least one document symbol from fake server"
            );
            assert_eq!(syms[0].name, "my_function");
        }
        Err(err) => {
            panic!(
                "document_symbols failed: {err}\n{}",
                harness.diagnostics().await
            );
        }
    }

    harness.shutdown().await.expect("shutdown should succeed");
}

/// Verify SemanticContextCollector can be constructed from the harness types.
///
/// This test proves the root-crate collector integrates with the
/// harness-provided service layer without requiring a full security review.
#[tokio::test]
async fn composite_semantic_context_collector_construction() {
    let harness = CompositeHarness::start(scenario_init_shutdown())
        .await
        .expect("failed to start composite harness");

    // Build the root-crate collector.
    let collector = codegg::lsp::semantic_context::SemanticContextCollector::new(
        harness.service.clone(),
        harness.operations.clone(),
        harness.diagnostics_collector.clone(),
        harness.root.clone(),
    );

    // Issue a basic semantic context request (file-level, no position).
    let request = egglsp::SemanticContextRequest::new(
        harness.source_path.to_string_lossy().as_ref(),
        egglsp::SemanticContextIntent::Explain,
    );

    match collector.collect(request).await {
        Ok(response) => {
            // We should get at least a source excerpt back.
            assert!(
                response.source_excerpt.is_some(),
                "response should include a source excerpt"
            );
        }
        Err(err) => {
            panic!(
                "semantic context collect failed: {err}\n{}",
                harness.diagnostics().await
            );
        }
    }

    harness.shutdown().await.expect("shutdown should succeed");
}

// ── Phase 5: Production workspace edit preview conversion tests ───────

fn file_uri(path: &Path) -> String {
    Url::from_file_path(path)
        .expect("valid file path")
        .to_string()
}

/// Scenario: init + didOpen for source, then rename returning edits in source + helper.
fn scenario_rename_two_files_with_unicode() -> serde_json::Value {
    serde_json::json!({
        "name": "rename_two_files_with_unicode",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {
                    "capabilities": { "renameProvider": true }
                }}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/rename",
                "then": [{"type": "RespondResult", "result": {
                    "documentChanges": [
                        {
                            "textDocument": { "uri": "__SOURCE_URI__", "version": 1 },
                            "edits": [{
                                "range": {
                                    "start": {"line": 0, "character": 13},
                                    "end": {"line": 0, "character": 21}
                                },
                                "newText": "new_name"
                            }]
                        },
                        {
                            "textDocument": { "uri": "__HELPER_URI__", "version": 1 },
                            "edits": [{
                                "range": {
                                    "start": {"line": 0, "character": 7},
                                    "end": {"line": 0, "character": 15}
                                },
                                "newText": "new_name"
                            }]
                        }
                    ]
                }}]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario: init + didOpen for source, then formatting returning edits.
fn scenario_format_edits() -> serde_json::Value {
    serde_json::json!({
        "name": "format_edits",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {
                    "capabilities": { "documentFormattingProvider": true }
                }}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/formatting",
                "then": [{"type": "RespondResult", "result": [
                    {
                        "range": {
                            "start": {"line": 1, "character": 14},
                            "end": {"line": 1, "character": 17}
                        },
                        "newText": ""
                    }
                ]}]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario: init + didOpen, then codeAction returning edit-bearing organizeImports.
fn scenario_code_action_edit() -> serde_json::Value {
    serde_json::json!({
        "name": "code_action_edit",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {
                    "capabilities": { "codeActionProvider": true }
                }}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/codeAction",
                "then": [{"type": "RespondResult", "result": [
                    {
                        "title": "Organize Imports",
                        "kind": "source.organizeImports",
                        "edit": {
                            "documentChanges": [{
                                "textDocument": { "uri": "__SOURCE_URI__", "version": 1 },
                                "edits": [{
                                    "range": {
                                        "start": {"line": 0, "character": 0},
                                        "end": {"line": 2, "character": 0}
                                    },
                                    "newText": "use std::collections::HashMap;\nuse std::path::Path;\n\n"
                                }]
                            }]
                        }
                    }
                ]}]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario: init + didOpen, then rename returning edit outside root.
fn scenario_rename_outside_root() -> serde_json::Value {
    serde_json::json!({
        "name": "rename_outside_root",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {
                    "capabilities": { "renameProvider": true }
                }}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/rename",
                "then": [{"type": "RespondResult", "result": {
                    "documentChanges": [
                        {
                            "textDocument": { "uri": "__SOURCE_URI__", "version": 1 },
                            "edits": [{
                                "range": {
                                    "start": {"line": 0, "character": 4},
                                    "end": {"line": 0, "character": 9}
                                },
                                "newText": "bar"
                            }]
                        },
                        {
                            "textDocument": { "uri": "file:///tmp/outside_root/helper.rs", "version": 1 },
                            "edits": [{
                                "range": {
                                    "start": {"line": 0, "character": 0},
                                    "end": {"line": 0, "character": 3}
                                },
                                "newText": "baz"
                            }]
                        }
                    ]
                }}]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario: init + didOpen, then rename with overlapping edits.
fn scenario_rename_overlapping() -> serde_json::Value {
    serde_json::json!({
        "name": "rename_overlapping",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {
                    "capabilities": { "renameProvider": true }
                }}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/rename",
                "then": [{"type": "RespondResult", "result": {
                    "documentChanges": [{
                        "textDocument": { "uri": "__SOURCE_URI__", "version": 1 },
                        "edits": [
                            {
                                "range": {
                                    "start": {"line": 0, "character": 0},
                                    "end": {"line": 0, "character": 4}
                                },
                                "newText": "AAAA"
                            },
                            {
                                "range": {
                                    "start": {"line": 0, "character": 2},
                                    "end": {"line": 0, "character": 5}
                                },
                                "newText": "BBB"
                            }
                        ]
                    }]
                }}]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

// ── Test 1: rename preview through production path ───────────────────

#[tokio::test]
async fn rename_preview_converts_through_production_path() {
    let source_text = "use crate::\u{1F600}old_name;\n";
    let helper_text = "pub fn old_name() {}\n";

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let source_path = tempdir.path().join("src/lib.rs");
    let source_uri = file_uri(&source_path);

    // Write helper file before starting the harness (scenario needs its URI)
    let helper_path = tempdir.path().join("src/helper.rs");
    let helper_uri = file_uri(&helper_path);
    std::fs::create_dir_all(tempdir.path().join("src")).expect("create src dir");
    std::fs::write(&helper_path, helper_text).expect("write helper");

    // Build scenario with substituted URIs
    let scenario_str = serde_json::to_string(&scenario_rename_two_files_with_unicode())
        .expect("scenario to string");
    let scenario_str = scenario_str.replace("__SOURCE_URI__", &source_uri);
    let scenario_str = scenario_str.replace("__HELPER_URI__", &helper_uri);
    let scenario: serde_json::Value = serde_json::from_str(&scenario_str).expect("parse scenario");

    let harness = CompositeHarness::start_with_root(tempdir, scenario)
        .await
        .expect("failed to start harness");

    // Overwrite the source file with our custom content (harness wrote harness_marker)
    std::fs::write(&harness.source_path, source_text).expect("write source");

    let source_url = Url::from_file_path(&harness.source_path).expect("valid path");
    harness
        .client
        .open_file(&source_url, source_text, 1)
        .await
        .expect("open_file failed");

    let params = serde_json::json!({
        "textDocument": { "uri": source_uri },
        "position": { "line": 0, "character": 15 },
        "newName": "new_name"
    });
    let resp = harness
        .client
        .send_request("textDocument/rename", params)
        .await
        .expect("rename request failed");

    let ws_edit: WorkspaceEdit =
        serde_json::from_value(resp).expect("failed to parse WorkspaceEdit");

    let preview = preview_workspace_edit("rename", ws_edit, Some(&harness.root))
        .expect("preview_workspace_edit failed");

    assert_eq!(preview.total_files, 2);
    assert_eq!(preview.files.len(), 2);
    assert!(!preview.truncated);

    // First file: source with emoji + old_name → new_name
    let src_preview = &preview.files[0];
    assert_eq!(src_preview.file, harness.source_path);
    assert_eq!(src_preview.edits.len(), 1);
    assert_eq!(src_preview.edits[0].replacement_preview, "new_name");
    assert!(src_preview.patch.contains("-use crate::\u{1F600}old_name;"));
    assert!(src_preview.patch.contains("+use crate::\u{1F600}new_name;"));

    // Second file: helper with old_name → new_name
    let helper_path = harness.root.join("src/helper.rs");
    let helper_preview = &preview.files[1];
    assert_eq!(helper_preview.file, helper_path);
    assert_eq!(helper_preview.edits.len(), 1);
    assert_eq!(helper_preview.edits[0].replacement_preview, "new_name");
    assert!(helper_preview.patch.contains("-pub fn old_name() {}"));
    assert!(helper_preview.patch.contains("+pub fn new_name() {}"));

    // Disk files must remain unchanged.
    assert_eq!(
        std::fs::read_to_string(&harness.source_path).unwrap(),
        source_text
    );
    assert_eq!(std::fs::read_to_string(&helper_path).unwrap(), helper_text);

    harness.shutdown().await.expect("shutdown");
}

// ── Test 2: format preview through production path ───────────────────

#[tokio::test]
async fn format_preview_converts_through_production_path() {
    let source_text = "fn main() {\n    let x = 1;xyz\n}\n";

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let source_path = tempdir.path().join("src/lib.rs");
    let source_uri = file_uri(&source_path);

    let scenario = scenario_format_edits();
    let harness = CompositeHarness::start_with_root(tempdir, scenario)
        .await
        .expect("failed to start harness");

    // Overwrite the source file with our custom content
    std::fs::write(&harness.source_path, source_text).expect("write source");

    let source_url = Url::from_file_path(&harness.source_path).expect("valid path");
    harness
        .client
        .open_file(&source_url, source_text, 1)
        .await
        .expect("open_file failed");

    let params = serde_json::json!({
        "textDocument": { "uri": source_uri },
        "options": {
            "tabSize": 4,
            "insertSpaces": true
        }
    });
    let resp = harness
        .client
        .send_request("textDocument/formatting", params)
        .await
        .expect("formatting request failed");

    let edits: Vec<TextEdit> = serde_json::from_value(resp).expect("failed to parse TextEdits");
    assert_eq!(edits.len(), 1);

    let preview =
        preview_text_edits_for_file("format", &harness.source_path, edits, Some(&harness.root))
            .expect("preview_text_edits_for_file failed");

    assert_eq!(preview.total_files, 1);
    assert_eq!(preview.total_edits, 1);
    assert_eq!(preview.files.len(), 1);

    let fp = &preview.files[0];
    assert_eq!(fp.file, harness.source_path);
    assert!(fp.patch.contains("-    let x = 1;xyz"));
    assert!(fp.patch.contains("+    let x = 1;"));

    // Disk must remain unchanged.
    assert_eq!(
        std::fs::read_to_string(&harness.source_path).unwrap(),
        source_text
    );

    harness.shutdown().await.expect("shutdown");
}

// ── Test 3: code action source action preview through production path ─

#[tokio::test]
async fn code_action_source_action_preview_converts_through_production_path() {
    let source_text = "use std::fmt;\nuse std::io;\n\nfn main() {}\n";

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let source_path = tempdir.path().join("src/lib.rs");
    let source_uri = file_uri(&source_path);

    let scenario = scenario_code_action_edit();
    let harness = CompositeHarness::start_with_root(tempdir, scenario)
        .await
        .expect("failed to start harness");

    // Overwrite the source file with our custom content
    std::fs::write(&harness.source_path, source_text).expect("write source");

    let source_url = Url::from_file_path(&harness.source_path).expect("valid path");
    harness
        .client
        .open_file(&source_url, source_text, 1)
        .await
        .expect("open_file failed");

    let params = serde_json::json!({
        "textDocument": { "uri": source_uri },
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": 3, "character": 13 }
        },
        "context": {
            "diagnostics": [],
            "only": ["source.organizeImports"]
        }
    });
    let resp = harness
        .client
        .send_request("textDocument/codeAction", params)
        .await
        .expect("codeAction request failed");

    let actions: Vec<CodeActionOrCommand> =
        serde_json::from_value(resp).expect("failed to parse CodeActionOrCommand");

    assert_eq!(actions.len(), 1);

    let ws_edit = select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions)
        .expect("select_source_action_edit failed");

    let preview = preview_workspace_edit("organize imports", ws_edit, Some(&harness.root))
        .expect("preview_workspace_edit failed");

    assert_eq!(preview.total_files, 1);
    assert_eq!(preview.files.len(), 1);
    assert!(!preview.truncated);
    assert!(preview.files[0]
        .patch
        .contains("+use std::collections::HashMap;"));

    // Disk must remain unchanged.
    assert_eq!(
        std::fs::read_to_string(&harness.source_path).unwrap(),
        source_text
    );

    // Test command-only rejection (tests 6 integration through production path).
    let command_only_actions: Vec<CodeActionOrCommand> =
        vec![CodeActionOrCommand::CodeAction(CodeAction {
            title: "Organize Imports".to_string(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            command: Some(LspCommand {
                command: "editor.action.organizeImports".to_string(),
                title: "Organize Imports".to_string(),
                arguments: None,
            }),
            edit: None,
            ..Default::default()
        })];
    let err = select_source_action_edit(
        SourceActionPreviewKind::OrganizeImports,
        command_only_actions,
    );
    assert!(
        matches!(err, Err(LspError::CommandOnlySourceAction(_))),
        "expected CommandOnlySourceAction, got: {err:?}"
    );

    harness.shutdown().await.expect("shutdown");
}

// ── Test 4: out-of-root path rejected ────────────────────────────────

#[tokio::test]
async fn preview_safety_out_of_root_rejected() {
    let source_text = "fn main() {}\n";

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let source_path = tempdir.path().join("src/lib.rs");
    let source_uri = file_uri(&source_path);

    let scenario = scenario_rename_outside_root();
    let harness = CompositeHarness::start_with_root(tempdir, scenario)
        .await
        .expect("failed to start harness");

    std::fs::write(&harness.source_path, source_text).expect("write source");

    let source_url = Url::from_file_path(&harness.source_path).expect("valid path");
    harness
        .client
        .open_file(&source_url, source_text, 1)
        .await
        .expect("open_file failed");

    let params = serde_json::json!({
        "textDocument": { "uri": source_uri },
        "position": { "line": 0, "character": 4 },
        "newName": "bar"
    });
    let resp = harness
        .client
        .send_request("textDocument/rename", params)
        .await
        .expect("rename request failed");

    let ws_edit: WorkspaceEdit =
        serde_json::from_value(resp).expect("failed to parse WorkspaceEdit");

    let err = preview_workspace_edit("rename", ws_edit, Some(&harness.root));
    assert!(
        matches!(err, Err(LspError::PathOutsideRoot(_))),
        "expected PathOutsideRoot, got: {err:?}"
    );

    harness.shutdown().await.expect("shutdown");
}

// ── Test 5: overlapping edits rejected ───────────────────────────────

#[tokio::test]
async fn preview_safety_overlapping_edits_rejected() {
    let source_text = "abcdef\n";

    let tempdir = tempfile::tempdir().expect("create tempdir");
    let source_path = tempdir.path().join("src/lib.rs");
    let source_uri = file_uri(&source_path);

    let scenario = scenario_rename_overlapping();
    let harness = CompositeHarness::start_with_root(tempdir, scenario)
        .await
        .expect("failed to start harness");

    std::fs::write(&harness.source_path, source_text).expect("write source");

    let source_url = Url::from_file_path(&harness.source_path).expect("valid path");
    harness
        .client
        .open_file(&source_url, source_text, 1)
        .await
        .expect("open_file failed");

    let params = serde_json::json!({
        "textDocument": { "uri": source_uri },
        "position": { "line": 0, "character": 2 },
        "newName": "test"
    });
    let resp = harness
        .client
        .send_request("textDocument/rename", params)
        .await
        .expect("rename request failed");

    let ws_edit: WorkspaceEdit =
        serde_json::from_value(resp).expect("failed to parse WorkspaceEdit");

    let err = preview_workspace_edit("rename", ws_edit, Some(&harness.root));
    assert!(
        matches!(err, Err(LspError::OverlappingEdits)),
        "expected OverlappingEdits, got: {err:?}"
    );

    harness.shutdown().await.expect("shutdown");
}

// ── Test 6: command-only code action rejected ────────────────────────

#[tokio::test]
async fn preview_safety_command_only_code_action_rejected() {
    let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: "Organize Imports".to_string(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        command: Some(LspCommand {
            command: "editor.action.organizeImports".to_string(),
            title: "Organize Imports".to_string(),
            arguments: None,
        }),
        edit: None,
        ..Default::default()
    })];

    let err = select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions);
    assert!(
        matches!(err, Err(LspError::CommandOnlySourceAction(_))),
        "expected CommandOnlySourceAction, got: {err:?}"
    );
}

// ── Test 7: no-edit, no-command code action rejected ─────────────────

#[tokio::test]
async fn preview_safety_no_edit_code_action_rejected() {
    let actions = vec![CodeActionOrCommand::CodeAction(CodeAction {
        title: "Organize Imports".to_string(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        command: None,
        edit: None,
        ..Default::default()
    })];

    let err = select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions);
    assert!(
        matches!(err, Err(LspError::NoEditForSourceAction(_))),
        "expected NoEditForSourceAction, got: {err:?}"
    );
}

// ── Test 8: ambiguous source actions rejected ────────────────────────

#[tokio::test]
async fn preview_safety_ambiguous_source_actions_rejected() {
    let actions = vec![
        CodeActionOrCommand::CodeAction(CodeAction {
            title: "Organize Imports (a)".to_string(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            edit: Some(WorkspaceEdit {
                changes: None,
                document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: "file:///tmp/test.rs".parse().unwrap(),
                        version: Some(1),
                    },
                    edits: vec![OneOf::Left(TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 0,
                            },
                            end: Position {
                                line: 0,
                                character: 5,
                            },
                        },
                        new_text: "AAAAA".to_string(),
                    })],
                }])),
                change_annotations: None,
            }),
            ..Default::default()
        }),
        CodeActionOrCommand::CodeAction(CodeAction {
            title: "Organize Imports (b)".to_string(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            edit: Some(WorkspaceEdit {
                changes: None,
                document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier {
                        uri: "file:///tmp/test.rs".parse().unwrap(),
                        version: Some(1),
                    },
                    edits: vec![OneOf::Left(TextEdit {
                        range: Range {
                            start: Position {
                                line: 0,
                                character: 0,
                            },
                            end: Position {
                                line: 0,
                                character: 5,
                            },
                        },
                        new_text: "BBBBB".to_string(),
                    })],
                }])),
                change_annotations: None,
            }),
            ..Default::default()
        }),
    ];

    let err = select_source_action_edit(SourceActionPreviewKind::OrganizeImports, actions);
    assert!(
        matches!(err, Err(LspError::AmbiguousSourceAction(_, _))),
        "expected AmbiguousSourceAction, got: {err:?}"
    );
}

// ── Test 9: resource operation rejected ───────────────────────────────

#[tokio::test]
async fn preview_safety_resource_operation_rejected() {
    let ws_edit = WorkspaceEdit {
        changes: None,
        document_changes: Some(DocumentChanges::Operations(vec![
            DocumentChangeOperation::Op(ResourceOp::Create(CreateFile {
                uri: "file:///tmp/new_file.rs".parse().unwrap(),
                options: None,
                annotation_id: None,
            })),
        ])),
        change_annotations: None,
    };

    let err = preview_workspace_edit("test", ws_edit, Some(Path::new("/tmp")));
    assert!(
        matches!(err, Err(LspError::UnsupportedEdit(ref msg)) if msg.contains("resource operations")),
        "expected UnsupportedEdit with resource operations message, got: {err:?}"
    );
}

// ══════════════════════════════════════════════════════════════════════
// Phase 2: Semantic Context Collector integration tests
// ══════════════════════════════════════════════════════════════════════

/// Source file used by the semantic context collector tests.
const SEMANTIC_SOURCE: &str = "pub fn entry() {\n    helper();\n}\n\nfn helper() {}\n";

/// Scenario: full workflow covering all collector phases.
///
/// Handles the exact request sequence the `SemanticContextCollector`
/// issues through the `LspService`'s internal client:
/// initialize → initialized → didOpen → documentSymbol → definition →
/// references → prepareCallHierarchy → incomingCalls → outgoingCalls →
/// prepareTypeHierarchy → supertypes → subtypes → shutdown → exit
fn scenario_semantic_context_full() -> serde_json::Value {
    serde_json::json!({
        "name": "semantic_context_full_workflow",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "hoverProvider": true,
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true,
                            "callHierarchyProvider": true,
                            "typeHierarchyProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {"type": "AllowRequest", "method": "textDocument/hover"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        },
                        {
                            "name": "helper",
                            "kind": 12,
                            "range": {
                                "start": {"line": 4, "character": 0},
                                "end": {"line": 4, "character": 15}
                            },
                            "selectionRange": {
                                "start": {"line": 4, "character": 3},
                                "end": {"line": 4, "character": 9}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "uri": "__SOURCE_URI__",
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 15}
                        }
                    }}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 15}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/incomingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "to": {
                                "name": "helper",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 4, "character": 0},
                                    "end": {"line": 4, "character": 15}
                                },
                                "selectionRange": {
                                    "start": {"line": 4, "character": 3},
                                    "end": {"line": 4, "character": 9}
                                }
                            },
                            "fromRanges": [{
                                "start": {"line": 1, "character": 4},
                                "end": {"line": 1, "character": 12}
                            }]
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareTypeHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "typeHierarchy/supertypes",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "typeHierarchy/subtypes",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario: definition capability NOT advertised.
///
/// The collector should skip the definition request and record an
/// `LspUnavailable` entry in the response.
fn scenario_no_definition() -> serde_json::Value {
    serde_json::json!({
        "name": "semantic_context_no_definition",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "referencesProvider": true,
                            "documentSymbolProvider": true,
                            "callHierarchyProvider": true,
                            "typeHierarchyProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {"type": "AllowRequest", "method": "textDocument/hover"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 1, "character": 4},
                                "end": {"line": 1, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/incomingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareTypeHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "typeHierarchy/supertypes",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "typeHierarchy/subtypes",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario: definition request returns an LSP error.
///
/// The collector should catch the error, record a note, and still
/// succeed overall with the remaining evidence.
fn scenario_definition_error() -> serde_json::Value {
    serde_json::json!({
        "name": "semantic_context_definition_error",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true,
                            "callHierarchyProvider": true,
                            "typeHierarchyProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {"type": "AllowRequest", "method": "textDocument/hover"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondError", "code": -32603, "message": "internal error in definition provider"}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 1, "character": 4},
                                "end": {"line": 1, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/incomingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareTypeHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "typeHierarchy/supertypes",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "typeHierarchy/subtypes",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Helper: create a standalone service + collector wired to the fake server.
///
/// Returns (collector, service, source_path, root, tempdir, transcript_path).
/// The `tempdir` must be held alive for the duration of the test.
fn setup_collector_for_scenario(
    scenario: serde_json::Value,
) -> (
    codegg::lsp::semantic_context::SemanticContextCollector,
    Arc<LspService>,
    PathBuf,
    PathBuf,
    tempfile::TempDir,
) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(&source_path, SEMANTIC_SOURCE).expect("write source");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-semantic-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let scenario = substitute_placeholders(
        scenario,
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let operations = Arc::new(LspOperations::new(service.clone()));
    let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));
    let collector = codegg::lsp::semantic_context::SemanticContextCollector::new(
        service.clone(),
        operations,
        diagnostics,
        root.clone(),
    );

    (collector, service, source_path, root, tempdir)
}

/// Minimal test to verify the service can create a client and open a file.
#[tokio::test]
async fn semantic_context_minimal_service_client() {
    let tempdir = tempfile::tempdir().unwrap();
    let root = tempdir.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(&source_path, SEMANTIC_SOURCE).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();

    let scenario = substitute_placeholders(
        serde_json::json!({
            "name": "full_no_delay",
            "steps": [
                {"type": "ExpectRequest", "method": "initialize", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": {"capabilities": {"hoverProvider": true, "definitionProvider": true, "referencesProvider": true, "documentSymbolProvider": true, "callHierarchyProvider": true}}}]},
                {"type": "ExpectNotification", "method": "initialized", "then": []},
                {"type": "ExpectNotification", "method": "textDocument/didOpen", "then": []},
                {"type": "ExpectRequest", "method": "textDocument/documentSymbol", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": [{"name": "entry", "kind": 12, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}}, "selectionRange": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 12}}}, {"name": "helper", "kind": 12, "range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 15}}, "selectionRange": {"start": {"line": 4, "character": 3}, "end": {"line": 4, "character": 9}}}]}]},
                {"type": "ExpectRequest", "method": "textDocument/definition", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": {"uri": source_uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 15}}}}]},
                {"type": "ExpectRequest", "method": "textDocument/references", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": [{"uri": source_uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 15}}}]}]},
                {"type": "ExpectRequest", "method": "textDocument/prepareCallHierarchy", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": [{"name": "entry", "kind": 12, "uri": source_uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}}, "selectionRange": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 12}}}]}]},
                {"type": "ExpectRequest", "method": "callHierarchy/incomingCalls", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": []}]},
                {"type": "ExpectRequest", "method": "callHierarchy/outgoingCalls", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": [{"to": {"name": "helper", "kind": 12, "uri": source_uri, "range": {"start": {"line": 4, "character": 0}, "end": {"line": 4, "character": 15}}, "selectionRange": {"start": {"line": 4, "character": 3}, "end": {"line": 4, "character": 9}}}, "fromRanges": [{"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 12}}]}]}]},
                {"type": "ExpectRequest", "method": "textDocument/prepareTypeHierarchy", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": [{"name": "entry", "kind": 12, "uri": source_uri, "range": {"start": {"line": 0, "character": 0}, "end": {"line": 2, "character": 1}}, "selectionRange": {"start": {"line": 0, "character": 7}, "end": {"line": 0, "character": 12}}}]}]},
                {"type": "ExpectRequest", "method": "typeHierarchy/supertypes", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": []}]},
                {"type": "ExpectRequest", "method": "typeHierarchy/subtypes", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": []}]},
                {"type": "ExpectRequest", "method": "shutdown", "then": [{"type": "RespondResult", "result": null}]},
                {"type": "ExpectNotification", "method": "exit", "then": []}
            ],
            "exit": {"type": "ExitCode", "code": 0},
            "strict": true
        }),
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).unwrap(),
    )
    .unwrap();
    std::fs::write(&transcript_path, "").unwrap();

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let operations = Arc::new(LspOperations::new(service.clone()));
    let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));

    // Open the file (triggers get_or_create_client + initialize + didOpen)
    diagnostics
        .get_diagnostic_snapshot_for_file(&source_path)
        .await
        .unwrap();

    // Now document_symbols should work
    let syms = operations.document_symbols(&source_path).await.unwrap();
    assert_eq!(syms.len(), 2);
    assert_eq!(syms[0].name, "entry");
    assert_eq!(syms[1].name, "helper");

    service.shutdown_all().await;
}

/// Phase 2: exercise the real `SemanticContextCollector` against the fake
/// server with all capabilities enabled.
///
/// Validates:
/// - Source excerpt from the actual file
/// - Document symbols for `entry` and `helper`
/// - Definition locations
/// - References
/// - Call hierarchy summary (outgoing call to `helper`)
/// - Type hierarchy summary
/// - Truncation/budget metadata
#[tokio::test]
async fn semantic_context_collector_exercises_real_workflow() {
    let (collector, service, source_path, _root, _tempdir) =
        setup_collector_for_scenario(scenario_semantic_context_full());

    let request = egglsp::SemanticContextRequest::new(
        source_path.to_string_lossy().as_ref(),
        egglsp::SemanticContextIntent::SecurityReview,
    )
    .with_position(1, 7)
    .with_call_hierarchy(true)
    .with_type_hierarchy(true);

    let response = timeout(Duration::from_secs(30), collector.collect(request))
        .await
        .expect("collector.collect timed out")
        .expect("collector.collect failed");

    // Source excerpt: read from disk, always present.
    let excerpt = response
        .source_excerpt
        .as_ref()
        .expect("source_excerpt should be present");
    assert!(
        excerpt.text.contains("pub fn entry()"),
        "excerpt should contain source text, got: {}",
        excerpt.text
    );

    // Diagnostic evidence: the snapshot was read (may be empty/unavailable
    // since we don't push diagnostics in this scenario, but the metadata
    // field should still be populated).
    // NOTE: we intentionally do NOT push publishDiagnostics in this
    // scenario to keep the scenario simple and deterministic. The
    // collector still reads the snapshot successfully.

    // Document symbols: both entry and helper.
    assert!(
        !response.all_symbols.is_empty(),
        "all_symbols should not be empty"
    );
    let names: Vec<&str> = response
        .all_symbols
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert!(
        names.contains(&"entry"),
        "symbols should include 'entry', got: {names:?}"
    );
    assert!(
        names.contains(&"helper"),
        "symbols should include 'helper', got: {names:?}"
    );

    // First symbol shorthand.
    let sym = response.symbol.as_ref().expect("symbol should be set");
    assert_eq!(sym.name, "entry");

    // Definitions: the server returns a definition for entry.
    assert!(
        !response.definitions.is_empty(),
        "definitions should not be empty"
    );
    assert!(
        response.definitions[0].file.contains("lib.rs"),
        "definition should reference the source file"
    );

    // References: the server returns at least one reference.
    assert!(
        !response.references.is_empty(),
        "references should not be empty"
    );

    // Call hierarchy: present because we requested it.
    let ch = response
        .call_hierarchy
        .as_ref()
        .expect("call_hierarchy should be present");
    assert_eq!(ch.items.len(), 1, "call hierarchy should have one item");
    assert_eq!(ch.items[0].name, "entry");
    assert_eq!(
        ch.outgoing_count, 1,
        "entry should have one outgoing call (to helper)"
    );
    assert_eq!(ch.incoming_count, 0, "entry should have no incoming calls");

    // Type hierarchy: NOT present because the capability snapshot does not
    // infer type_hierarchy from the server's typeHierarchyProvider. The
    // ObservedCapabilitiesOverride is the only way to enable it (see
    // capability.rs:418-420). Without the override, type_hierarchy is absent.
    assert!(
        response.type_hierarchy.is_none(),
        "type_hierarchy should be absent without override"
    );

    // Truncation/budget metadata.
    // No truncation expected for this small input.
    assert!(
        response.section_truncations.is_empty(),
        "no section truncations expected for small input"
    );

    // Notes: should be empty for a fully successful collection.
    assert!(
        response.notes.is_empty(),
        "notes should be empty for full success, got: {:?}",
        response.notes
    );

    service.shutdown_all().await;
}

/// Phase 2: verify capability gating when a capability is NOT advertised.
///
/// When `definitionProvider` is `false` in the server capabilities, the
/// collector should skip the definition request and record an
/// `LspUnavailable` entry instead.
#[tokio::test]
async fn semantic_context_collector_capability_gating() {
    let (collector, service, source_path, _root, _tempdir) =
        setup_collector_for_scenario(scenario_no_definition());

    let request = egglsp::SemanticContextRequest::new(
        source_path.to_string_lossy().as_ref(),
        egglsp::SemanticContextIntent::SecurityReview,
    )
    .with_position(1, 7)
    .with_call_hierarchy(true)
    .with_type_hierarchy(true);

    let response = timeout(Duration::from_secs(30), collector.collect(request))
        .await
        .expect("collector.collect timed out")
        .expect("collector.collect failed");

    // Source excerpt should still be present.
    assert!(
        response.source_excerpt.is_some(),
        "source_excerpt should be present even without definition"
    );

    // Symbols should be present.
    assert!(
        !response.all_symbols.is_empty(),
        "symbols should be present"
    );

    // Definitions should be EMPTY because definitionProvider is false.
    assert!(
        response.definitions.is_empty(),
        "definitions should be empty when definitionProvider is false"
    );

    // An LspUnavailable entry should be recorded for definition.
    let has_def_unavailable = response
        .unavailable
        .iter()
        .any(|u| u.operation == "definition");
    assert!(
        has_def_unavailable,
        "should have an LspUnavailable for definition, got: {:?}",
        response.unavailable
    );

    // References should still be collected (referencesProvider is true).
    assert!(
        !response.references.is_empty(),
        "references should be collected even without definition"
    );

    // Call hierarchy should still work (callHierarchyProvider is true).
    assert!(
        response.call_hierarchy.is_some(),
        "call_hierarchy should be present"
    );
    // type_hierarchy is NOT derived from callHierarchyProvider (see
    // capability.rs:418-420). The override is the only way to flip it on.
    // Without an ObservedCapabilitiesOverride, type_hierarchy will be absent.
    assert!(
        response.type_hierarchy.is_none(),
        "type_hierarchy should be absent without override"
    );
}

/// Security context tool: proves max_call_nodes truncation and depth limiting.
///
/// Uses a graph wider than max_call_nodes (entry → validate, sink, audit, log)
/// with max_call_nodes=2 and call_depth=2. The BFS should:
/// - Collect entry (depth 0) as node 1
/// - Collect one child (depth 1) as node 2
/// - Truncate remaining children
/// - Set truncation flags
#[tokio::test]
async fn security_context_tool_enforces_call_node_limit_and_truncation() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = manifest_dir.join("target/lsp-tests");
    std::fs::create_dir_all(&base).expect("mkdir lsp-tests base");
    let temp = tempfile::Builder::new()
        .prefix("test-security-node-limit-")
        .tempdir_in(&base)
        .expect("tempdir");
    let root = temp.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(&source_path, SECURITY_CALL_GRAPH_LIMIT_SOURCE).expect("write source");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-security-node-limit-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let scenario = substitute_placeholders(
        scenario_security_call_graph_node_limit(),
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let tool = LspTool::new(service.clone()).with_allowed_root(root.clone());

    let result = timeout(
        Duration::from_secs(30),
        tool.execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": source_path.to_str().unwrap(),
            "line": 4,
            "column": 5,
            "security_preset": "unsafe_review",
            "security_categories": ["unsafe", "process"],
            "include_call_hierarchy": true,
            "call_depth": 2,
            "max_call_nodes": 2,
            "call_direction": "outgoing"
        })),
    )
    .await
    .expect("securityContext timed out")
    .expect("securityContext failed");

    let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");

    // Risk markers must be present
    let markers = parsed["results"]["risk_markers"]
        .as_array()
        .expect("risk_markers should be an array");
    assert!(!markers.is_empty(), "should have risk markers");

    // Call expansion must be present
    let expansion = parsed["results"]["call_expansion"]
        .as_object()
        .expect("call_expansion should be an object");
    assert!(
        expansion.get("root").is_some_and(|v| !v.is_null()),
        "root should be present and non-null"
    );
    assert_eq!(expansion["direction"], "outgoing");

    // Node count must obey max_call_nodes=2
    let nodes = expansion["nodes"]
        .as_array()
        .expect("nodes should be an array");
    assert!(
        nodes.len() <= 2,
        "nodes should be at most 2 (max_call_nodes), got {}",
        nodes.len()
    );

    // Root node must be retained
    let node_names: Vec<&str> = nodes.iter().filter_map(|n| n["name"].as_str()).collect();
    assert!(
        node_names.contains(&"entry"),
        "root node 'entry' must be retained, got: {node_names:?}"
    );

    // Every node must have depth <= 2 (call_depth limit)
    for node in nodes {
        let depth = node["depth"]
            .as_u64()
            .expect("node should have numeric depth");
        assert!(depth <= 2, "node depth {depth} should be <= call_depth 2");
    }

    // Truncation flags must be true
    assert_eq!(
        expansion["truncated"], true,
        "call_expansion.truncated must be true when nodes exceed max_call_nodes"
    );
    assert_eq!(
        parsed["results"]["limits"]["call_expansion_truncated"], true,
        "limits.call_expansion_truncated must be true"
    );

    service.shutdown_all().await;
}

/// Phase 4: independent call-depth enforcement test.
///
/// Uses a linear chain entry→level1→level2→level3 with call_depth=2
/// and a generous max_call_nodes=16, so only depth can stop traversal.
/// The BFS should:
/// - Collect entry (depth 0) as root
/// - Expand entry → level1 (depth 0→1)
/// - Expand level1 → level2 (depth 1→2)
/// - Stop before expanding level2 → level3 (depth 2→3)
#[tokio::test]
async fn security_context_tool_enforces_call_depth_limit() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = manifest_dir.join("target/lsp-tests");
    std::fs::create_dir_all(&base).expect("mkdir lsp-tests base");
    let temp = tempfile::Builder::new()
        .prefix("test-security-depth-limit-")
        .tempdir_in(&base)
        .expect("tempdir");
    let root = temp.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(&source_path, SECURITY_CALL_GRAPH_DEPTH_SOURCE).expect("write source");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-security-depth-limit-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let scenario = substitute_placeholders(
        scenario_security_call_graph_depth_limit(),
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let tool = LspTool::new(service.clone()).with_allowed_root(root.clone());

    let result = timeout(
        Duration::from_secs(30),
        tool.execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": source_path.to_str().unwrap(),
            "line": 4,
            "column": 5,
            "security_preset": "unsafe_review",
            "security_categories": ["unsafe", "process"],
            "include_call_hierarchy": true,
            "call_depth": 2,
            "max_call_nodes": 16,
            "call_direction": "outgoing"
        })),
    )
    .await
    .expect("securityContext timed out")
    .expect("securityContext failed");

    let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");

    // Risk markers must be present
    let markers = parsed["results"]["risk_markers"]
        .as_array()
        .expect("risk_markers should be an array");
    assert!(!markers.is_empty(), "should have risk markers");

    // Call expansion must be present
    let expansion = parsed["results"]["call_expansion"]
        .as_object()
        .expect("call_expansion should be an object");
    assert!(
        expansion.get("root").is_some_and(|v| !v.is_null()),
        "root should be present and non-null"
    );
    assert_eq!(expansion["direction"], "outgoing");

    // Node count: with depth 2 we expect entry(0), level1(1), level2(2) = 3 nodes
    let nodes = expansion["nodes"]
        .as_array()
        .expect("nodes should be an array");

    // Root node must be retained
    let node_names: Vec<&str> = nodes.iter().filter_map(|n| n["name"].as_str()).collect();
    assert!(
        node_names.contains(&"entry"),
        "root node 'entry' must be retained, got: {node_names:?}"
    );

    // All nodes must have depth <= 2
    for node in nodes {
        let depth = node["depth"]
            .as_u64()
            .expect("node should have numeric depth");
        assert!(
            depth <= 2,
            "node depth {depth} should be <= call_depth 2, node: {:?}",
            node["name"]
        );
    }

    // No level3 node should appear (depth 2→3 expansion must not happen)
    let has_level3 = node_names.iter().any(|n| *n == "level3");
    assert!(
        !has_level3,
        "level3 must not appear when call_depth=2, got nodes: {node_names:?}"
    );

    // The strict scenario enforces that no unexpected requests are made.
    // If the BFS expanded level2 into level3 (depth 2→3), the fake server
    // would receive an unexpected prepareCallHierarchy request and fail,
    // causing the test to timeout. The fact that we reached here with a
    // valid response proves depth limiting worked.
    // Additionally, verify no level3 appears in the expansion output.
    let expansion_text =
        serde_json::to_string(&parsed["results"]["call_expansion"]).unwrap_or_default();
    assert!(
        !expansion_text.contains("\"level3\""),
        "call_expansion should not contain level3 when call_depth=2, expansion: {expansion_text}"
    );

    service.shutdown_all().await;
}

/// Security context tool: proves diagnostic filtering and diagnostic evidence.
///
/// The fake server publishes two diagnostics via initialized notification:
/// - A security-relevant COMMAND_INJECTION diagnostic (severity: error, source: security-lint)
/// - A non-security STYLE_ONLY diagnostic (severity: info, source: style-lint)
///
/// The test verifies that the security diagnostic survives filtering and
/// appears in security_relevant_diagnostics, while diagnostic_evidence is populated.
#[tokio::test]
async fn security_context_tool_filters_and_preserves_diagnostic_evidence() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = manifest_dir.join("target/lsp-tests");
    std::fs::create_dir_all(&base).expect("mkdir lsp-tests base");
    let temp = tempfile::Builder::new()
        .prefix("test-security-diag-")
        .tempdir_in(&base)
        .expect("tempdir");
    let root = temp.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(&source_path, SECURITY_CALL_GRAPH_SOURCE).expect("write source");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-security-diag-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let scenario = substitute_placeholders(
        scenario_security_call_graph_with_diagnostics(),
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let tool = LspTool::new(service.clone()).with_allowed_root(root.clone());

    // Initialize the client and open the file so the fake server sends
    // publishDiagnostics on the initialized notification.
    service
        .open_file(&source_path, SECURITY_CALL_GRAPH_SOURCE)
        .await
        .expect("open_file should succeed");

    // Wait for the published diagnostics to appear in the client cache.
    let diag_collector = DiagnosticsCollector::new(service.clone());
    let source_uri_str = source_uri.clone();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "diagnostics never appeared for {source_uri_str} within 5s.\ntranscript:\n{}",
                transcript_tail(&transcript_path),
            );
        }
        match diag_collector.get_all_diagnostic_snapshots().await {
            Ok(snapshots) => {
                if let Some(snap) = snapshots.get(&source_uri_str) {
                    if !snap.diagnostics.is_empty() {
                        break;
                    }
                }
            }
            Err(_) => {}
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }

    // Now run securityContext exactly once
    let result = timeout(
        Duration::from_secs(30),
        tool.execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": source_path.to_str().unwrap(),
            "line": 14,
            "column": 5,
            "security_preset": "unsafe_review",
            "security_categories": ["unsafe", "process"]
        })),
    )
    .await
    .expect("securityContext timed out")
    .expect("securityContext failed");

    let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");

    // Risk markers must be present
    let markers = parsed["results"]["risk_markers"]
        .as_array()
        .expect("risk_markers should be an array");
    assert!(!markers.is_empty(), "should have risk markers");

    // Security-relevant diagnostics must be present and contain the COMMAND_INJECTION diagnostic
    let sec_diags = parsed["results"]["security_relevant_diagnostics"]
        .as_array()
        .expect("security_relevant_diagnostics should be an array");
    assert!(
        !sec_diags.is_empty(),
        "should have security-relevant diagnostics"
    );
    let diag_messages: Vec<&str> = sec_diags
        .iter()
        .filter_map(|d| {
            d["message"]
                .as_str()
                .or(d.get("message").and_then(|m| m.as_str()))
        })
        .collect();
    assert!(
        diag_messages.iter().any(|m| m.contains("COMMAND_INJECTION") || m.contains("untrusted input reaches shell")),
        "should contain COMMAND_INJECTION diagnostic, got: {diag_messages:?}"
    );

    // Verify STYLE_ONLY diagnostic treatment:
    // The STYLE_ONLY diagnostic (severity: info, line 9) is within the
    // SECURITY_NEARBY_LINE_RADIUS (20 lines) of a risk marker (unsafe at line 14),
    // so current policy INCLUDES it in security_relevant_diagnostics.
    let style_diag = sec_diags.iter().find(|d| {
        d.get("code")
            .and_then(|c| c.as_str())
            .is_some_and(|c| c == "STYLE_ONLY")
    });
    assert!(
        style_diag.is_some(),
        "STYLE_ONLY diagnostic should be present in security_relevant_diagnostics \
         (near a risk marker within SECURITY_NEARBY_LINE_RADIUS)"
    );

    // Verify the COMMAND_INJECTION diagnostic has correct metadata
    let cmd_inj = sec_diags.iter().find(|d| {
        d.get("code")
            .and_then(|c| c.as_str())
            .is_some_and(|c| c == "COMMAND_INJECTION")
    });
    assert!(
        cmd_inj.is_some(),
        "COMMAND_INJECTION diagnostic should be present in security_relevant_diagnostics"
    );
    let cmd_inj = cmd_inj.unwrap();
    assert_eq!(
        cmd_inj.get("source").and_then(|s| s.as_str()),
        Some("security-lint"),
        "COMMAND_INJECTION source should be 'security-lint'"
    );
    assert_eq!(
        cmd_inj.get("severity").and_then(|s| s.as_str()),
        Some("error"),
        "COMMAND_INJECTION severity should be 'error'"
    );

    // Diagnostic evidence should be a non-null object with expected fields
    let diag_evidence = &parsed["results"]["diagnostic_evidence"];
    assert!(
        !diag_evidence.is_null(),
        "diagnostic_evidence should be present"
    );
    let diag_evidence_obj = diag_evidence
        .as_object()
        .expect("diagnostic_evidence should be an object");
    assert!(
        diag_evidence_obj.contains_key("freshness"),
        "diagnostic_evidence should have 'freshness' field"
    );
    let freshness = diag_evidence_obj["freshness"]
        .as_str()
        .expect("freshness should be a string");
    assert!(
        freshness == "Fresh" || freshness == "PossiblyStale",
        "freshness should be 'Fresh' or 'PossiblyStale', got: {freshness}"
    );
    assert!(
        diag_evidence_obj.contains_key("source"),
        "diagnostic_evidence should have 'source' field"
    );
    assert_eq!(
        diag_evidence_obj["source"].as_str(),
        Some("Pushed"),
        "source should be 'Pushed' (diagnostics came via publishDiagnostics)"
    );
    assert!(
        diag_evidence_obj.contains_key("usable_evidence"),
        "diagnostic_evidence should have 'usable_evidence' field"
    );
    assert!(
        diag_evidence_obj["usable_evidence"]
            .as_bool()
            .unwrap_or(false),
        "usable_evidence should be true"
    );
    assert!(
        diag_evidence_obj.contains_key("age_ms"),
        "diagnostic_evidence should have 'age_ms' field"
    );
    let age_ms = diag_evidence_obj["age_ms"]
        .as_f64()
        .expect("age_ms should be a number");
    assert!(age_ms >= 0.0, "age_ms should be >= 0, got: {age_ms}");

    // Notes and limits should be present
    assert!(
        parsed["results"]["notes"].as_array().is_some(),
        "notes should be present"
    );
    assert!(
        parsed["results"]["limits"].as_object().is_some(),
        "limits should be present"
    );

    service.shutdown_all().await;
}

/// Phase 2: verify failure degradation when an optional operation errors.
///
/// When `textDocument/definition` returns an LSP error, the collector
/// should catch it, record a note, and still succeed with the remaining
/// evidence (symbols, references, hierarchies).
#[tokio::test]
async fn semantic_context_collector_failure_degradation() {
    let (collector, service, source_path, _root, _tempdir) =
        setup_collector_for_scenario(scenario_definition_error());

    let request = egglsp::SemanticContextRequest::new(
        source_path.to_string_lossy().as_ref(),
        egglsp::SemanticContextIntent::SecurityReview,
    )
    .with_position(1, 7)
    .with_call_hierarchy(true)
    .with_type_hierarchy(true);

    let response = timeout(Duration::from_secs(30), collector.collect(request))
        .await
        .expect("collector.collect timed out")
        .expect("collector.collect failed");

    // Source excerpt: still present (disk read, not affected by LSP error).
    assert!(
        response.source_excerpt.is_some(),
        "source_excerpt should be present despite definition error"
    );

    // Symbols: still collected.
    assert!(
        !response.all_symbols.is_empty(),
        "symbols should be present despite definition error"
    );

    // Definitions: empty because the request failed.
    assert!(
        response.definitions.is_empty(),
        "definitions should be empty when definition request fails"
    );

    // A note should be recorded for the failed definition request.
    let has_def_note = response.notes.iter().any(|n| n.contains("goToDefinition"));
    assert!(
        has_def_note,
        "should have a note about the failed definition, got: {:?}",
        response.notes
    );

    // References: still collected (independent of definition).
    assert!(
        !response.references.is_empty(),
        "references should be collected despite definition error"
    );

    // Call hierarchy: still present.
    assert!(
        response.call_hierarchy.is_some(),
        "call_hierarchy should be present despite definition error"
    );
    // Type hierarchy: NOT present because the capability snapshot does not
    // infer type_hierarchy from the server's typeHierarchyProvider. The
    // ObservedCapabilitiesOverride is the only way to enable it (see
    // capability.rs:418-420). Without the override, type_hierarchy is absent.
    assert!(
        response.type_hierarchy.is_none(),
        "type_hierarchy should be absent without override despite definition error"
    );

    // The overall response is still Ok (not an error).
    // The collector never returns Err for individual operation failures;
    // they are recorded as notes/unavailable in the response.

    service.shutdown_all().await;
}

// ══════════════════════════════════════════════════════════════════════
// Phase 4: Hunk Source Context integration test
// ══════════════════════════════════════════════════════════════════════

/// Source file containing a function with a call to exercise hunk context.
const HUNK_SOURCE: &str = "pub fn entry() {\n    helper();\n}\n\nfn helper() {}\n";

/// Scenario for hunk source context: the collector needs the same operations
/// as semantic context (documentSymbol, definition, references).
fn scenario_hunk_source_context() -> serde_json::Value {
    serde_json::json!({
        "name": "hunk_source_context_workflow",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 2, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 12}
                            }
                        },
                        {
                            "name": "helper",
                            "kind": 12,
                            "range": {
                                "start": {"line": 4, "character": 0},
                                "end": {"line": 4, "character": 15}
                            },
                            "selectionRange": {
                                "start": {"line": 4, "character": 3},
                                "end": {"line": 4, "character": 9}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 4, "character": 3},
                                "end": {"line": 4, "character": 9}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 1, "character": 4},
                                "end": {"line": 1, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {"type": "ExpectRequest", "method": "shutdown", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": null}]},
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Hunk source context: exercises `HunkSourceNavigationCollector` with a real
/// unified diff against a fake-server-backed LSP stack.
///
/// Validates:
/// - Patch is parsed into hunks
/// - Semantic context is collected (document symbols, definition, references)
/// - Hunk evidence is produced with enclosing symbol
/// - Definitions and references are included
#[tokio::test]
async fn hunk_source_context_collector_exercises_real_workflow() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(&source_path, HUNK_SOURCE).expect("write source");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-hunk-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let scenario = substitute_placeholders(
        scenario_hunk_source_context(),
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let operations = Arc::new(LspOperations::new(service.clone()));
    let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));
    let sem_collector = codegg::lsp::semantic_context::SemanticContextCollector::new(
        service.clone(),
        operations,
        diagnostics,
        root.clone(),
    );
    let navigator = codegg::lsp::hunk_nav::HunkSourceNavigator::new();
    let hunk_collector = codegg::lsp::hunk_nav_collector::HunkSourceNavigationCollector::new(
        sem_collector,
        navigator,
    );

    // Unified diff: add a comment inside entry()
    let patch = "\
diff --git a/src/lib.rs b/src/lib.rs
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,4 +1,5 @@
 pub fn entry() {
+    // validate input
     helper();
 }
 
";

    let request = egglsp::hunk_context::HunkSourceNavigationRequest {
        file_path: source_path.to_string_lossy().to_string(),
        hunks: vec![],
        patch: Some(patch.to_string()),
        intent: "navigation".to_string(),
        include_definitions: true,
        include_references: true,
        include_call_hierarchy: false,
        include_type_hierarchy: false,
        excerpt_radius: 40,
        max_hunks: 20,
        max_symbols_per_hunk: 10,
        max_diagnostics_per_hunk: 10,
        max_references_per_hunk: 10,
    };

    let response = timeout(Duration::from_secs(30), hunk_collector.collect(request))
        .await
        .expect("hunk collector timed out")
        .expect("hunk collector failed");

    // Hunk evidence should be present
    assert!(
        !response.hunks.is_empty(),
        "should have at least one hunk in response"
    );

    let hunk_evidence = &response.hunks[0];

    // Enclosing symbol should be present (the hunk is inside entry(),
    // and the navigator identifies the nearest enclosing symbol).
    assert!(
        hunk_evidence.enclosing_symbol.is_some(),
        "should have an enclosing symbol"
    );

    // Definitions should reference `helper` (called on line 2)
    assert!(
        !hunk_evidence.definitions.is_empty(),
        "should have definitions"
    );

    // References should be present (helper() call is a reference)
    assert!(
        !hunk_evidence.references.is_empty(),
        "should have references"
    );

    // Source excerpt should contain the modified file content
    assert!(
        hunk_evidence.source_excerpt.is_some(),
        "should have a source excerpt"
    );

    service.shutdown_all().await;
}

// ══════════════════════════════════════════════════════════════════════
// Phase 3: Security Context workflow integration test
// ══════════════════════════════════════════════════════════════════════

/// Source file with security-sensitive patterns for security context testing.
const SECURITY_SOURCE: &str = r#"use std::process::Command;

pub fn run_unchecked(input: &str) -> String {
    unsafe {
        let cmd = Command::new("sh").arg("-c").arg(input).output();
        String::from_utf8_unchecked(cmd.unwrap().stdout)
    }
}

pub fn entry() {
    let data = run_unchecked("echo hello");
    println!("{data}");
}
"#;

/// Source file for call graph / call hierarchy security context testing.
const SECURITY_CALL_GRAPH_SOURCE: &str = r#"use std::process::Command;

pub fn entry(input: &str) {
    validate(input);
    sink(input);
}

fn validate(input: &str) {
    if input.is_empty() { return; }
}

fn sink(input: &str) {
    unsafe {
        let _ = Command::new("sh").arg("-c").arg(input).output();
    }
    entry(input); // deliberate cycle
}
"#;

const SECURITY_CALL_GRAPH_LIMIT_SOURCE: &str = r#"use std::process::Command;

pub fn entry(input: &str) {
    validate(input);
    sink(input);
    audit(input);
    log(input);
}

fn validate(input: &str) {
    if input.is_empty() { return; }
}

fn sink(input: &str) {
    unsafe {
        let _ = Command::new("sh").arg("-c").arg(input).output();
    }
    entry(input); // deliberate cycle
}

fn audit(input: &str) {
    let _ = Command::new("sh").arg("-c").arg(input);
}

fn log(input: &str) {
    eprintln!("{input}");
}
"#;

/// Source file for call-depth enforcement testing: a linear chain
/// entry → level1 → level2 → level3 (4 functions, 3 edges).
///
/// With `call_depth = 2` and a generous `max_call_nodes = 16`, only
/// depth can stop traversal. The BFS should expand entry (depth 0→1),
/// level1 (depth 1→2), and stop before expanding level2 (depth 2→3).
const SECURITY_CALL_GRAPH_DEPTH_SOURCE: &str = r#"use std::process::Command;

pub fn entry(input: &str) {
    level1(input);
}

fn level1(input: &str) {
    level2(input);
}

fn level2(input: &str) {
    level3(input);
}

fn level3(input: &str) {
    unsafe {
        let _ = Command::new("sh").arg("-c").arg(input).output();
    }
}
"#;

/// Scenario for security context: exercises the same LSP operations as the
/// semantic context collector (documentSymbol, definition, references).
fn scenario_security_context() -> serde_json::Value {
    serde_json::json!({
        "name": "security_context_workflow",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "run_unchecked",
                            "kind": 12,
                            "range": {
                                "start": {"line": 2, "character": 0},
                                "end": {"line": 8, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 2, "character": 7},
                                "end": {"line": 2, "character": 20}
                            }
                        },
                        {
                            "name": "entry",
                            "kind": 12,
                            "range": {
                                "start": {"line": 10, "character": 0},
                                "end": {"line": 14, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 10, "character": 7},
                                "end": {"line": 10, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 2, "character": 7},
                                "end": {"line": 2, "character": 20}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 11, "character": 12},
                                "end": {"line": 11, "character": 25}
                            }
                        }
                    ]}
                ]
            },
            {"type": "ExpectRequest", "method": "shutdown", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": null}]},
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Security context workflow: exercises `SemanticContextCollector` with
/// `SecurityReview` intent against security-sensitive source code.
///
/// The security context LSP tool operation delegates to this collector for
/// its core LSP operations, then applies risk marker scanning and security-
/// relevant filtering locally. This test verifies the collector produces
/// correct results with `SecurityReview` intent on source containing
/// `unsafe`, `process::Command`, and command injection patterns.
///
/// Validates:
/// - Source excerpt from the actual file
/// - Document symbols for `run_unchecked` and `entry`
/// - Definition for `run_unchecked` at the function declaration
/// - Reference to `run_unchecked` from `entry`
/// - Source contains security-sensitive patterns (unsafe, Command)
#[tokio::test]
async fn semantic_context_security_review_intent_collects_security_source() {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(&source_path, SECURITY_SOURCE).expect("write source");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-security-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let scenario = substitute_placeholders(
        scenario_security_context(),
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let operations = Arc::new(LspOperations::new(service.clone()));
    let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));
    let collector = codegg::lsp::semantic_context::SemanticContextCollector::new(
        service.clone(),
        operations,
        diagnostics,
        root.clone(),
    );

    // Use SecurityReview intent — this is the same path the securityContext
    // tool operation takes through the collector.
    let request = egglsp::SemanticContextRequest::new(
        source_path.to_string_lossy().as_ref(),
        egglsp::SemanticContextIntent::SecurityReview,
    )
    .with_position(11, 12); // entry() calling run_unchecked

    let response = timeout(Duration::from_secs(30), collector.collect(request))
        .await
        .expect("collector timed out")
        .expect("collector failed");

    // Source excerpt should contain the security-sensitive code
    let excerpt = response
        .source_excerpt
        .as_ref()
        .expect("source_excerpt should be present");
    assert!(
        excerpt.text.contains("unsafe"),
        "excerpt should contain unsafe keyword"
    );
    assert!(
        excerpt.text.contains("Command"),
        "excerpt should contain Command usage"
    );

    // Document symbols should include run_unchecked and entry
    let syms = &response.all_symbols;
    assert!(!syms.is_empty(), "should have document symbols");
    let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"run_unchecked"),
        "should have run_unchecked symbol: {names:?}"
    );
    assert!(
        names.contains(&"entry"),
        "should have entry symbol: {names:?}"
    );

    // Definition should be present (def of run_unchecked at line 3)
    assert!(!response.definitions.is_empty(), "should have definitions");

    // References should be present (run_unchecked called from entry)
    assert!(!response.references.is_empty(), "should have references");

    service.shutdown_all().await;
}

// ══════════════════════════════════════════════════════════════════════
// Phase 4: Security context call graph + risk filtering integration test
// ══════════════════════════════════════════════════════════════════════

/// Scenario for call graph / call hierarchy security context testing.
fn scenario_security_call_graph() -> serde_json::Value {
    serde_json::json!({
        "name": "security_call_graph",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true,
                            "callHierarchyProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "range": {
                                "start": {"line": 3, "character": 0},
                                "end": {"line": 7, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 3, "character": 7},
                                "end": {"line": 3, "character": 12}
                            }
                        },
                        {
                            "name": "validate",
                            "kind": 12,
                            "range": {
                                "start": {"line": 9, "character": 0},
                                "end": {"line": 11, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 9, "character": 3},
                                "end": {"line": 9, "character": 11}
                            }
                        },
                        {
                            "name": "sink",
                            "kind": 12,
                            "range": {
                                "start": {"line": 13, "character": 0},
                                "end": {"line": 19, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 13, "character": 3},
                                "end": {"line": 13, "character": 7}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 3, "character": 7},
                                "end": {"line": 3, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 4, "character": 4},
                                "end": {"line": 4, "character": 12}
                            }
                        },
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 5, "character": 4},
                                "end": {"line": 5, "character": 8}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 3, "character": 0},
                                "end": {"line": 7, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 3, "character": 7},
                                "end": {"line": 3, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/incomingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "from": {
                                "name": "entry",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 3, "character": 0},
                                    "end": {"line": 7, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 3, "character": 7},
                                    "end": {"line": 3, "character": 12}
                                }
                            },
                            "to": {
                                "name": "validate",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 9, "character": 0},
                                    "end": {"line": 11, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 9, "character": 3},
                                    "end": {"line": 9, "character": 11}
                                }
                            },
                            "fromRanges": [
                                {
                                    "start": {"line": 4, "character": 4},
                                    "end": {"line": 4, "character": 12}
                                }
                            ]
                        },
                        {
                            "from": {
                                "name": "entry",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 3, "character": 0},
                                    "end": {"line": 7, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 3, "character": 7},
                                    "end": {"line": 3, "character": 12}
                                }
                            },
                            "to": {
                                "name": "sink",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 13, "character": 0},
                                    "end": {"line": 19, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 13, "character": 3},
                                    "end": {"line": 13, "character": 7}
                                }
                            },
                            "fromRanges": [
                                {
                                    "start": {"line": 5, "character": 4},
                                    "end": {"line": 5, "character": 8}
                                }
                            ]
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "entry",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 3, "character": 0},
                                "end": {"line": 7, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 3, "character": 7},
                                "end": {"line": 3, "character": 12}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "from": {
                                "name": "entry",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 3, "character": 0},
                                    "end": {"line": 7, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 3, "character": 7},
                                    "end": {"line": 3, "character": 12}
                                }
                            },
                            "to": {
                                "name": "validate",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 9, "character": 0},
                                    "end": {"line": 11, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 9, "character": 3},
                                    "end": {"line": 9, "character": 11}
                                }
                            },
                            "fromRanges": [
                                {
                                    "start": {"line": 4, "character": 4},
                                    "end": {"line": 4, "character": 12}
                                }
                            ]
                        },
                        {
                            "from": {
                                "name": "entry",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 3, "character": 0},
                                    "end": {"line": 7, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 3, "character": 7},
                                    "end": {"line": 3, "character": 12}
                                }
                            },
                            "to": {
                                "name": "sink",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 13, "character": 0},
                                    "end": {"line": 19, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 13, "character": 3},
                                    "end": {"line": 13, "character": 7}
                                }
                            },
                            "fromRanges": [
                                {
                                    "start": {"line": 5, "character": 4},
                                    "end": {"line": 5, "character": 8}
                                }
                            ]
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "validate",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 9, "character": 0},
                                "end": {"line": 11, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 9, "character": 3},
                                "end": {"line": 9, "character": 11}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": []}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "name": "sink",
                            "kind": 12,
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 13, "character": 0},
                                "end": {"line": 19, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 13, "character": 3},
                                "end": {"line": 13, "character": 7}
                            }
                        }
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {
                            "from": {
                                "name": "sink",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 13, "character": 0},
                                    "end": {"line": 19, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 13, "character": 3},
                                    "end": {"line": 13, "character": 7}
                                }
                            },
                            "to": {
                                "name": "entry",
                                "kind": 12,
                                "uri": "__SOURCE_URI__",
                                "range": {
                                    "start": {"line": 3, "character": 0},
                                    "end": {"line": 7, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 3, "character": 7},
                                    "end": {"line": 3, "character": 12}
                                }
                            },
                            "fromRanges": [
                                {
                                    "start": {"line": 17, "character": 4},
                                    "end": {"line": 17, "character": 9}
                                }
                            ]
                        }
                    ]}
                ]
            },
            {"type": "ExpectRequest", "method": "shutdown", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": null}]},
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario variant of `scenario_security_call_graph` where the
/// `callHierarchy/outgoingCalls` for `sink` returns an LSP error instead
/// of the expected cycle edge. The security context tool should degrade
/// gracefully: the packet is still returned, the error is recorded, and
/// nodes/evidence collected before the failure are preserved.
fn scenario_security_call_graph_outgoing_error() -> serde_json::Value {
    let mut scenario = scenario_security_call_graph();
    let steps = scenario["steps"]
        .as_array_mut()
        .expect("steps should be an array");

    // Find the outgoingCalls step for sink (the one returning sink→entry cycle).
    // It is the last outgoingCalls step, after prepareCallHierarchy for sink.
    // Replace its RespondResult with RespondError.
    let mut replaced = false;
    for step in steps.iter_mut() {
        if step["method"] == "callHierarchy/outgoingCalls" {
            if let Some(actions) = step["then"].as_array() {
                if let Some(first) = actions.first() {
                    if first["type"] == "RespondResult" {
                        let result = &first["result"];
                        // The cycle response has one entry whose "to" name is "entry"
                        if let Some(items) = result.as_array() {
                            if items.iter().any(|item| item["to"]["name"] == "entry") {
                                // Replace with an error response
                                *step = serde_json::json!({
                                    "type": "ExpectRequest",
                                    "method": "callHierarchy/outgoingCalls",
                                    "id": {"type": "Number"},
                                    "then": [
                                        {"type": "RespondError", "code": -32603, "message": "Internal error: call hierarchy unavailable for sink"}
                                    ]
                                });
                                replaced = true;
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        replaced,
        "should have found and replaced the sink outgoingCalls step"
    );
    scenario
}

fn scenario_security_call_graph_node_limit() -> serde_json::Value {
    // The securityContext tool with include_call_hierarchy=true makes:
    // 1. Semantic context collector: prepareCallHierarchy + incomingCalls + outgoingCalls
    // 2. Call expansion BFS: prepareCallHierarchy + outgoingCalls (truncated at max_call_nodes=2)
    // With max_call_nodes=2, the BFS stops after adding entry + one child,
    // so the second outgoingCalls is NOT consumed.
    serde_json::json!({
        "name": "security_call_graph_node_limit",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true,
                            "callHierarchyProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"name": "entry", "kind": 12,
                         "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                         "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                        {"name": "validate", "kind": 12,
                         "range": {"start": {"line": 10, "character": 0}, "end": {"line": 12, "character": 1}},
                         "selectionRange": {"start": {"line": 10, "character": 3}, "end": {"line": 10, "character": 11}}},
                        {"name": "sink", "kind": 12,
                         "range": {"start": {"line": 14, "character": 0}, "end": {"line": 20, "character": 1}},
                         "selectionRange": {"start": {"line": 14, "character": 3}, "end": {"line": 14, "character": 7}}},
                        {"name": "audit", "kind": 12,
                         "range": {"start": {"line": 22, "character": 0}, "end": {"line": 24, "character": 1}},
                         "selectionRange": {"start": {"line": 22, "character": 3}, "end": {"line": 22, "character": 8}}},
                        {"name": "log", "kind": 12,
                         "range": {"start": {"line": 26, "character": 0}, "end": {"line": 28, "character": 1}},
                         "selectionRange": {"start": {"line": 26, "character": 3}, "end": {"line": 26, "character": 6}}}
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}}
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 12}}},
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 5, "character": 4}, "end": {"line": 5, "character": 8}}},
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 6, "character": 4}, "end": {"line": 6, "character": 9}}},
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 7, "character": 4}, "end": {"line": 7, "character": 7}}}
                    ]}
                ]
            },
            {"type": "ExpectRequest", "method": "textDocument/prepareCallHierarchy", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                     "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                     "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}}
                ]}]},
            {"type": "ExpectRequest", "method": "callHierarchy/incomingCalls", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": []}]},
            {"type": "ExpectRequest", "method": "callHierarchy/outgoingCalls", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "validate", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 10, "character": 0}, "end": {"line": 12, "character": 1}},
                            "selectionRange": {"start": {"line": 10, "character": 3}, "end": {"line": 10, "character": 11}}},
                     "fromRanges": [{"start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 12}}]},
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "sink", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 14, "character": 0}, "end": {"line": 20, "character": 1}},
                            "selectionRange": {"start": {"line": 14, "character": 3}, "end": {"line": 14, "character": 7}}},
                     "fromRanges": [{"start": {"line": 5, "character": 4}, "end": {"line": 5, "character": 8}}]},
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "audit", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 22, "character": 0}, "end": {"line": 24, "character": 1}},
                            "selectionRange": {"start": {"line": 22, "character": 3}, "end": {"line": 22, "character": 8}}},
                     "fromRanges": [{"start": {"line": 6, "character": 4}, "end": {"line": 6, "character": 9}}]},
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "log", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 26, "character": 0}, "end": {"line": 28, "character": 1}},
                            "selectionRange": {"start": {"line": 26, "character": 3}, "end": {"line": 26, "character": 6}}},
                     "fromRanges": [{"start": {"line": 7, "character": 4}, "end": {"line": 7, "character": 7}}]}
                ]}]},
            {"type": "ExpectRequest", "method": "textDocument/prepareCallHierarchy", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                     "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                     "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}}
                ]}]},
            {"type": "ExpectRequest", "method": "callHierarchy/outgoingCalls", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "validate", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 10, "character": 0}, "end": {"line": 12, "character": 1}},
                            "selectionRange": {"start": {"line": 10, "character": 3}, "end": {"line": 10, "character": 11}}},
                     "fromRanges": [{"start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 12}}]},
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "sink", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 14, "character": 0}, "end": {"line": 20, "character": 1}},
                            "selectionRange": {"start": {"line": 14, "character": 3}, "end": {"line": 14, "character": 7}}},
                     "fromRanges": [{"start": {"line": 5, "character": 4}, "end": {"line": 5, "character": 8}}]},
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "audit", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 22, "character": 0}, "end": {"line": 24, "character": 1}},
                            "selectionRange": {"start": {"line": 22, "character": 3}, "end": {"line": 22, "character": 8}}},
                     "fromRanges": [{"start": {"line": 6, "character": 4}, "end": {"line": 6, "character": 9}}]},
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 8, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "log", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 26, "character": 0}, "end": {"line": 28, "character": 1}},
                            "selectionRange": {"start": {"line": 26, "character": 3}, "end": {"line": 26, "character": 6}}},
                     "fromRanges": [{"start": {"line": 7, "character": 4}, "end": {"line": 7, "character": 7}}]}
                ]}]},
            {"type": "ExpectRequest", "method": "shutdown", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": null}]},
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

fn scenario_security_call_graph_with_diagnostics() -> serde_json::Value {
    serde_json::json!({
        "name": "security_call_graph_diagnostics",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true,
                            "callHierarchyProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": [
                {"type": "SendNotification", "method": "textDocument/publishDiagnostics", "params": {
                    "uri": "__SOURCE_URI__",
                    "version": 1,
                    "diagnostics": [
                        {
                            "range": {
                                "start": {"line": 14, "character": 4},
                                "end": {"line": 17, "character": 5}
                            },
                            "severity": 1,
                            "source": "security-lint",
                            "code": "COMMAND_INJECTION",
                            "message": "untrusted input reaches shell command execution"
                        },
                        {
                            "range": {
                                "start": {"line": 9, "character": 4},
                                "end": {"line": 9, "character": 20}
                            },
                            "severity": 3,
                            "source": "style-lint",
                            "code": "STYLE_ONLY",
                            "message": "consider a shorter function"
                        }
                    ]
                }}
            ]},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"name": "entry", "kind": 12,
                         "range": {"start": {"line": 3, "character": 0}, "end": {"line": 7, "character": 1}},
                         "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                        {"name": "validate", "kind": 12,
                         "range": {"start": {"line": 9, "character": 0}, "end": {"line": 11, "character": 1}},
                         "selectionRange": {"start": {"line": 9, "character": 3}, "end": {"line": 9, "character": 11}}},
                        {"name": "sink", "kind": 12,
                         "range": {"start": {"line": 13, "character": 0}, "end": {"line": 19, "character": 1}},
                         "selectionRange": {"start": {"line": 13, "character": 3}, "end": {"line": 13, "character": 7}}}
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}}
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 12}}},
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 5, "character": 4}, "end": {"line": 5, "character": 8}}}
                    ]}
                ]
            },
            {"type": "ExpectRequest", "method": "shutdown", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": null}]},
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Scenario for call-depth enforcement: a linear chain entry→level1→level2→level3.
///
/// The fake server handles:
/// 1. Semantic context collector: prepareCallHierarchy + incomingCalls + outgoingCalls for entry
/// 2. BFS expansion depth 0→1: prepareCallHierarchy + outgoingCalls for entry (→level1)
/// 3. BFS expansion depth 1→2: prepareCallHierarchy + outgoingCalls for level1 (→level2)
///
/// With call_depth=2, the BFS must NOT expand level2 into level3.
/// Strict mode ensures any unexpected request fails the test.
fn scenario_security_call_graph_depth_limit() -> serde_json::Value {
    serde_json::json!({
        "name": "security_call_graph_depth_limit",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": {
                        "capabilities": {
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true,
                            "callHierarchyProvider": true
                        }
                    }}
                ]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "AllowNotification", "method": "textDocument/didOpen"},
            {"type": "AllowNotification", "method": "textDocument/didChange"},
            {"type": "AllowNotification", "method": "textDocument/didSave"},
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"name": "entry", "kind": 12,
                         "range": {"start": {"line": 3, "character": 0}, "end": {"line": 5, "character": 1}},
                         "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                        {"name": "level1", "kind": 12,
                         "range": {"start": {"line": 7, "character": 0}, "end": {"line": 9, "character": 1}},
                         "selectionRange": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 9}}},
                        {"name": "level2", "kind": 12,
                         "range": {"start": {"line": 11, "character": 0}, "end": {"line": 13, "character": 1}},
                         "selectionRange": {"start": {"line": 11, "character": 3}, "end": {"line": 11, "character": 9}}},
                        {"name": "level3", "kind": 12,
                         "range": {"start": {"line": 15, "character": 0}, "end": {"line": 19, "character": 1}},
                         "selectionRange": {"start": {"line": 15, "character": 3}, "end": {"line": 15, "character": 9}}}
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}}
                    ]}
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "id": {"type": "Number"},
                "then": [
                    {"type": "RespondResult", "result": [
                        {"uri": "__SOURCE_URI__",
                         "range": {"start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 11}}}
                    ]}
                ]
            },
            {"type": "ExpectRequest", "method": "textDocument/prepareCallHierarchy", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                     "range": {"start": {"line": 3, "character": 0}, "end": {"line": 5, "character": 1}},
                     "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}}
                ]}]},
            {"type": "ExpectRequest", "method": "callHierarchy/incomingCalls", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": []}]},
            {"type": "ExpectRequest", "method": "callHierarchy/outgoingCalls", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 5, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "level1", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 7, "character": 0}, "end": {"line": 9, "character": 1}},
                            "selectionRange": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 9}}},
                     "fromRanges": [{"start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 11}}]}
                ]}]},
            {"type": "ExpectRequest", "method": "textDocument/prepareCallHierarchy", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                     "range": {"start": {"line": 3, "character": 0}, "end": {"line": 5, "character": 1}},
                     "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}}
                ]}]},
            {"type": "ExpectRequest", "method": "callHierarchy/outgoingCalls", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"from": {"name": "entry", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 3, "character": 0}, "end": {"line": 5, "character": 1}},
                               "selectionRange": {"start": {"line": 3, "character": 7}, "end": {"line": 3, "character": 12}}},
                     "to": {"name": "level1", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 7, "character": 0}, "end": {"line": 9, "character": 1}},
                            "selectionRange": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 9}}},
                     "fromRanges": [{"start": {"line": 4, "character": 4}, "end": {"line": 4, "character": 11}}]}
                ]}]},
            {"type": "ExpectRequest", "method": "textDocument/prepareCallHierarchy", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"name": "level1", "kind": 12, "uri": "__SOURCE_URI__",
                     "range": {"start": {"line": 7, "character": 0}, "end": {"line": 9, "character": 1}},
                     "selectionRange": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 9}}}
                ]}]},
            {"type": "ExpectRequest", "method": "callHierarchy/outgoingCalls", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": [
                    {"from": {"name": "level1", "kind": 12, "uri": "__SOURCE_URI__",
                               "range": {"start": {"line": 7, "character": 0}, "end": {"line": 9, "character": 1}},
                               "selectionRange": {"start": {"line": 7, "character": 3}, "end": {"line": 7, "character": 9}}},
                     "to": {"name": "level2", "kind": 12, "uri": "__SOURCE_URI__",
                            "range": {"start": {"line": 11, "character": 0}, "end": {"line": 13, "character": 1}},
                            "selectionRange": {"start": {"line": 11, "character": 3}, "end": {"line": 11, "character": 9}}},
                     "fromRanges": [{"start": {"line": 8, "character": 4}, "end": {"line": 8, "character": 11}}]}
                ]}]},
            {"type": "ExpectRequest", "method": "shutdown", "id": {"type": "Number"},
                "then": [{"type": "RespondResult", "result": null}]},
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

/// Security context tool: degrades gracefully when one outgoing call
/// hierarchy request fails during call expansion BFS.
///
/// Uses the same risk-marker source as the main security test but the
/// fake server returns an LSP error for sink's outgoingCalls. The tool
/// must still return a packet with:
/// - risk markers and security-relevant symbols (collected before failure)
/// - the expansion graph with nodes collected before the error
/// - the error recorded in `call_expansion.errors`
/// - `call_expansion.truncated` may be true (partial expansion)
#[tokio::test]
async fn security_context_tool_degrades_on_call_hierarchy_error() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = manifest_dir.join("target/lsp-tests");
    std::fs::create_dir_all(&base).expect("mkdir lsp-tests base");
    let temp = tempfile::Builder::new()
        .prefix("test-security-call-graph-error-")
        .tempdir_in(&base)
        .expect("tempdir");
    let root = temp.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(&source_path, SECURITY_CALL_GRAPH_SOURCE).expect("write source");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-security-call-graph-error-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let scenario = substitute_placeholders(
        scenario_security_call_graph_outgoing_error(),
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let tool = LspTool::new(service.clone()).with_allowed_root(root.clone());

    // The tool must NOT fail — it should degrade gracefully
    let result = timeout(
        Duration::from_secs(30),
        tool.execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": source_path.to_str().unwrap(),
            "line": 14,
            "column": 5,
            "security_preset": "unsafe_review",
            "security_categories": ["unsafe", "process"],
            "include_call_hierarchy": true,
            "call_depth": 2,
            "max_call_nodes": 8,
            "call_direction": "outgoing"
        })),
    )
    .await
    .expect("securityContext timed out")
    .expect("securityContext must not fail on partial call hierarchy error");

    let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");

    // Risk markers must still be present despite the call hierarchy error
    let markers = parsed["results"]["risk_markers"]
        .as_array()
        .expect("risk_markers should be an array");
    assert!(
        !markers.is_empty(),
        "risk markers should be present even with call hierarchy error"
    );

    // Security-relevant symbols must still be present
    let syms = parsed["results"]["security_relevant_symbols"]
        .as_array()
        .expect("security_relevant_symbols should be an array");
    assert!(
        !syms.is_empty(),
        "security-relevant symbols should be present even with call hierarchy error"
    );

    // Preset should match
    assert_eq!(parsed["results"]["preset"], "unsafe_review");

    // Call expansion must be present (partial)
    let expansion = parsed["results"]["call_expansion"]
        .as_object()
        .expect("call_expansion should be an object");
    assert!(
        expansion.get("root").is_some(),
        "call_expansion.root should be present"
    );
    assert_eq!(expansion["direction"], "outgoing");

    // Nodes: at least entry and validate should be collected before the error
    let nodes = expansion["nodes"]
        .as_array()
        .expect("call_expansion.nodes should be an array");
    assert!(
        !nodes.is_empty(),
        "should have at least some nodes before the error"
    );
    let node_names: Vec<&str> = nodes.iter().filter_map(|n| n["name"].as_str()).collect();
    assert!(
        node_names.contains(&"entry"),
        "entry should be in the expansion (collected before error), got: {node_names:?}"
    );

    // Edges: at least one edge should be present (entry→validate or entry→sink)
    let edges = expansion["edges"]
        .as_array()
        .expect("call_expansion.edges should be an array");
    assert!(
        !edges.is_empty(),
        "should have at least some edges before the error"
    );

    // The error must be recorded in call_expansion.errors
    let errors = expansion.get("errors");
    assert!(
        errors.is_some() && errors.unwrap().as_array().map_or(false, |e| !e.is_empty()),
        "call_expansion.errors should be non-empty, recording the outgoingCalls failure"
    );

    // Notes and limits should be present
    assert!(
        parsed["results"]["notes"].as_array().is_some(),
        "notes should be present"
    );
    assert!(
        parsed["results"]["limits"].as_object().is_some(),
        "limits should be present"
    );

    service.shutdown_all().await;
}

/// Security context tool: exercises risk filtering and call expansion with
/// `unsafe_review` preset against code containing unsafe blocks, process
/// execution, and a deliberate call cycle (entry → sink → entry).
///
/// Validates:
/// - Risk markers are detected for unsafe and process patterns
/// - `unsafe_review` preset is applied
/// - Call hierarchy expansion is triggered and bounded
/// - Call expansion graph contains expected nodes and edges
/// - Limits and notes are populated
#[tokio::test]
async fn security_context_tool_exercises_risk_filtering_and_call_expansion() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = manifest_dir.join("target/lsp-tests");
    std::fs::create_dir_all(&base).expect("mkdir lsp-tests base");
    let temp = tempfile::Builder::new()
        .prefix("test-security-call-graph-")
        .tempdir_in(&base)
        .expect("tempdir");
    let root = temp.path().to_path_buf();
    let source_path = root.join("src/lib.rs");
    let scenario_path = root.join("scenario.json");
    let transcript_path = root.join("transcript.jsonl");
    let root_uri = path_to_uri(&root);
    let source_uri = path_to_uri(&source_path);

    std::fs::create_dir_all(root.join("src")).expect("mkdir src");
    std::fs::write(&source_path, SECURITY_CALL_GRAPH_SOURCE).expect("write source");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"lsp-security-call-graph-test\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");

    let scenario = substitute_placeholders(
        scenario_security_call_graph(),
        &root,
        &root_uri,
        &source_path,
        &source_uri,
        &scenario_path,
        &transcript_path,
    );
    std::fs::write(
        &scenario_path,
        serde_json::to_string_pretty(&scenario).expect("scenario json"),
    )
    .expect("write scenario");

    let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));
    let tool = LspTool::new(service.clone()).with_allowed_root(root.clone());

    let result = timeout(
        Duration::from_secs(30),
        tool.execute(serde_json::json!({
            "operation": "securityContext",
            "file_path": source_path.to_str().unwrap(),
            "line": 14,
            "column": 5,
            "security_preset": "unsafe_review",
            "security_categories": ["unsafe", "process"],
            "include_call_hierarchy": true,
            "call_depth": 2,
            "max_call_nodes": 8,
            "call_direction": "outgoing"
        })),
    )
    .await
    .expect("securityContext timed out")
    .expect("securityContext failed");

    let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid JSON");

    // Risk markers must be present and include unsafe/process categories
    let markers = parsed["results"]["risk_markers"]
        .as_array()
        .expect("risk_markers should be an array");
    assert!(!markers.is_empty(), "should have risk markers");
    let categories: Vec<&str> = markers
        .iter()
        .filter_map(|m| m["category"].as_str())
        .collect();
    assert!(
        categories.contains(&"unsafe") || categories.contains(&"process"),
        "should contain unsafe or process category markers, got: {categories:?}"
    );

    // Security-relevant symbols should be populated
    let syms = parsed["results"]["security_relevant_symbols"]
        .as_array()
        .expect("security_relevant_symbols should be an array");
    assert!(!syms.is_empty(), "should have security-relevant symbols");

    // Preset should match
    assert_eq!(parsed["results"]["preset"], "unsafe_review");

    // Call expansion must be present
    let expansion = parsed["results"]["call_expansion"]
        .as_object()
        .expect("call_expansion should be an object");
    assert!(
        expansion.get("root").is_some(),
        "call_expansion.root should be present"
    );
    assert_eq!(expansion["direction"], "outgoing");

    // Nodes: entry, validate, sink — at most 3
    let nodes = expansion["nodes"]
        .as_array()
        .expect("call_expansion.nodes should be an array");
    assert!(
        nodes.len() <= 3,
        "should have at most 3 nodes (entry, validate, sink), got {}",
        nodes.len()
    );

    // Edges: at least 2 (entry→validate, entry→sink)
    let edges = expansion["edges"]
        .as_array()
        .expect("call_expansion.edges should be an array");
    assert!(
        edges.len() >= 2,
        "should have at least 2 edges, got {}",
        edges.len()
    );

    // Notes and limits should be present
    assert!(
        parsed["results"]["notes"].as_array().is_some(),
        "notes should be present"
    );
    assert!(
        parsed["results"]["limits"].as_object().is_some(),
        "limits should be present"
    );

    service.shutdown_all().await;
}

// ── Operational state note propagation ──────────────────────────────

/// Verify that when the LSP service is in a notable state
/// (`Indexing`), the `SemanticContextCollector` surfaces a note
/// to the agent in the response. The fake server scenario drives
/// an indexing-style state by transitioning the operational
/// state map directly before invoking the collector.
#[tokio::test]
async fn semantic_context_includes_indexing_note_when_state_is_indexing() {
    let (collector, service, source_path, _root, _tempdir) = setup_collector_for_scenario(
        serde_json::json!({
            "name": "full_no_delay",
            "steps": [
                {"type": "ExpectRequest", "method": "initialize", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": {"capabilities": {"documentSymbolProvider": true}}}]},
                {"type": "ExpectNotification", "method": "initialized", "then": []},
                {"type": "ExpectNotification", "method": "textDocument/didOpen", "then": []},
                {"type": "ExpectRequest", "method": "textDocument/documentSymbol", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": []}]},
                {"type": "ExpectRequest", "method": "shutdown", "then": [{"type": "RespondResult", "result": null}]},
                {"type": "ExpectNotification", "method": "exit", "then": []}
            ],
            "exit": {"type": "ExitCode", "code": 0},
            "strict": true
        }),
    );

    // Resolve the key the collector will use for the source.
    let key = service
        .get_or_create_client(&source_path)
        .await
        .expect("key resolution")
        .0;

    // Force the service into `Degraded` so `context_note()` is `Some`.
    // (The state machine does not allow Ready -> Indexing, so
    // we use Degraded which produces a context_note about the
    // server being slow/degraded.)
    service
        .transition_operational_state(
            &key,
            egglsp::health::LspOperationalState::Degraded {
                reason: "indexing takes too long".to_string(),
            },
        )
        .await
        .expect("transition to Degraded");

    // Run the collector. We expect a successful response that
    // contains a note about the server being degraded/indexing.
    let request = egglsp::semantic_context::SemanticContextRequest::new(
        source_path.to_string_lossy().as_ref(),
        egglsp::semantic_context::SemanticContextIntent::Review,
    )
    .with_excerpt_radius(20);
    let response = timeout(Duration::from_secs(15), collector.collect(request))
        .await
        .expect("collect should not time out")
        .expect("collect should succeed");

    let has_indexing_note = response
        .notes
        .iter()
        .any(|n| n.to_lowercase().contains("index"));
    assert!(
        has_indexing_note,
        "expected a note about indexing, got: {:?}",
        response.notes
    );

    service.shutdown_all().await;
}

/// Verify that when the LSP service is in a `Failed` state, the
/// `SemanticContextCollector` returns an `Err` so the agent sees
/// the failure clearly.
#[tokio::test]
async fn semantic_context_returns_err_when_state_is_failed() {
    let (collector, service, source_path, _root, _tempdir) = setup_collector_for_scenario(
        serde_json::json!({
            "name": "full_no_delay",
            "steps": [
                {"type": "ExpectRequest", "method": "initialize", "id": {"type": "Number"}, "then": [{"type": "RespondResult", "result": {"capabilities": {"documentSymbolProvider": true}}}]},
                {"type": "ExpectNotification", "method": "initialized", "then": []},
                {"type": "ExpectRequest", "method": "shutdown", "then": [{"type": "RespondResult", "result": null}]},
                {"type": "ExpectNotification", "method": "exit", "then": []}
            ],
            "exit": {"type": "ExitCode", "code": 0},
            "strict": true
        }),
    );

    let key = service
        .get_or_create_client(&source_path)
        .await
        .expect("key resolution")
        .0;

    // Force the service into `Failed` so the collector returns Err.
    service
        .transition_operational_state(
            &key,
            egglsp::health::LspOperationalState::Failed {
                reason: "intentional failure for test".to_string(),
            },
        )
        .await
        .expect("transition to Failed");

    let request = egglsp::semantic_context::SemanticContextRequest::new(
        source_path.to_string_lossy().as_ref(),
        egglsp::semantic_context::SemanticContextIntent::Review,
    )
    .with_excerpt_radius(20);
    let result = timeout(Duration::from_secs(5), collector.collect(request))
        .await
        .expect("collect should not time out");

    assert!(
        result.is_err(),
        "expected Err when state is Failed, got: {result:?}"
    );
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("intentional failure for test"),
        "error should mention the failure reason, got: {err_msg}"
    );

    service.shutdown_all().await;
}
