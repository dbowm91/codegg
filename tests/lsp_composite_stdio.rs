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

use egglsp::{
    LspClient, LspClientOptions, LspConfig, LspError, LspLaunchSpec, LspRule, LspService,
};
use egglsp::diagnostics::DiagnosticsCollector;
use egglsp::edit::{preview_text_edits_for_file, preview_workspace_edit};
use egglsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Command as LspCommand,
    CreateFile, DocumentChangeOperation, DocumentChanges,
    OptionalVersionedTextDocumentIdentifier, OneOf, Position, Range, ResourceOp,
    TextDocumentEdit, TextEdit, WorkspaceEdit,
};
use egglsp::operations::LspOperations;
use egglsp::operations::{select_source_action_edit, SourceActionPreviewKind};

// ── Fake server binary path ──────────────────────────────────────────

fn fake_server_binary_path() -> PathBuf {
    // Allow manual override.
    if let Ok(path) = std::env::var("EGGLSP_TEST_SERVER") {
        return PathBuf::from(path);
    }

    // When running `cargo test -p egglsp`, Cargo sets this env var at compile time.
    if let Some(path) = option_env!("CARGO_BIN_EXE_egglsp-test-server") {
        return PathBuf::from(path);
    }

    // Fallback: look in the workspace target directory for the binary.
    // This covers root-crate tests where CARGO_BIN_EXE_* isn't set.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let debug_path = PathBuf::from(manifest_dir)
        .join("target")
        .join("debug")
        .join("egglsp-test-server");
    if debug_path.exists() {
        return debug_path;
    }

    panic!(
        "Could not find egglsp-test-server binary.\n\
         Build it with: cargo build -p egglsp --bin egglsp-test-server\n\
         Or set EGGLSP_TEST_SERVER=/path/to/binary"
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
            "egglsp-test-server",
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

        let service = Arc::new(LspService::new(make_service_config(
            &scenario_path,
            &transcript_path,
        )));
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
        let child_status = {
            let mut process = client.process.lock().await;
            match process.child.try_wait() {
                Ok(Some(status)) => format!("{status:?}"),
                Ok(None) => "running".to_string(),
                Err(err) => format!("error: {err}"),
            }
        };
        let transcript = transcript_tail(transcript_path);

        let mut out = String::new();
        out.push_str(&format!("scenario: {scenario_name}\n"));
        out.push_str(&format!("root: {}\n", root.display()));
        out.push_str(&format!("source: {}\n", source_path.display()));
        out.push_str(&format!("scenario file: {}\n", scenario_path.display()));
        out.push_str(&format!(
            "transcript file: {}\n",
            transcript_path.display()
        ));
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

        let wait_result = {
            let mut process = self.client.process.lock().await;
            timeout(Duration::from_secs(5), process.child.wait()).await
        };

        let diagnostics = self.diagnostics().await;

        match (shutdown_result, wait_result) {
            (Ok(()), Ok(Ok(_status))) => Ok(()),
            (Ok(()), Ok(Err(err))) => Err(LspError::RequestFailed(format!(
                "failed to wait for fake server exit: {err}\n{diagnostics}"
            ))),
            (Ok(()), Err(_elapsed)) => Err(LspError::RequestFailed(format!(
                "timed out waiting for fake server exit\n{diagnostics}"
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
            command: vec![fake_server_binary_path()
                .to_str()
                .unwrap()
                .to_string()],
            extensions: Some(vec!["rs".to_string()]),
            disabled: None,
            env: Some(env),
            initialization: None,
            workspace_configuration: None,
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
                        v, root, root_uri, source_path, source_uri, scenario_path, transcript_path,
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
                            v, root, root_uri, source_path, source_uri, scenario_path,
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
    let symbols = harness
        .client
        .document_symbols(&source_url)
        .await;

    match symbols {
        Ok(syms) => {
            assert!(
                !syms.is_empty(),
                "expected at least one document symbol from fake server"
            );
            assert_eq!(syms[0].name, "my_function");
        }
        Err(err) => {
            panic!("document_symbols failed: {err}\n{}", harness.diagnostics().await);
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
    let scenario: serde_json::Value =
        serde_json::from_str(&scenario_str).expect("parse scenario");

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
    assert_eq!(
        std::fs::read_to_string(&helper_path).unwrap(),
        helper_text
    );

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

    let edits: Vec<TextEdit> =
        serde_json::from_value(resp).expect("failed to parse TextEdits");
    assert_eq!(edits.len(), 1);

    let preview = preview_text_edits_for_file(
        "format",
        &harness.source_path,
        edits,
        Some(&harness.root),
    )
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
    assert!(preview.files[0].patch.contains("+use std::collections::HashMap;"));

    // Disk must remain unchanged.
    assert_eq!(
        std::fs::read_to_string(&harness.source_path).unwrap(),
        source_text
    );

    // Test command-only rejection (tests 6 integration through production path).
    let command_only_actions: Vec<CodeActionOrCommand> = vec![CodeActionOrCommand::CodeAction(
        CodeAction {
            title: "Organize Imports".to_string(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            command: Some(LspCommand {
                command: "editor.action.organizeImports".to_string(),
                title: "Organize Imports".to_string(),
                arguments: None,
            }),
            edit: None,
            ..Default::default()
        },
    )];
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
                document_changes: Some(DocumentChanges::Edits(vec![
                    TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier {
                            uri: "file:///tmp/test.rs".parse().unwrap(),
                            version: Some(1),
                        },
                        edits: vec![OneOf::Left(TextEdit {
                            range: Range {
                                start: Position { line: 0, character: 0 },
                                end: Position { line: 0, character: 5 },
                            },
                            new_text: "AAAAA".to_string(),
                        })],
                    },
                ])),
                change_annotations: None,
            }),
            ..Default::default()
        }),
        CodeActionOrCommand::CodeAction(CodeAction {
            title: "Organize Imports (b)".to_string(),
            kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
            edit: Some(WorkspaceEdit {
                changes: None,
                document_changes: Some(DocumentChanges::Edits(vec![
                    TextDocumentEdit {
                        text_document: OptionalVersionedTextDocumentIdentifier {
                            uri: "file:///tmp/test.rs".parse().unwrap(),
                            version: Some(1),
                        },
                        edits: vec![OneOf::Left(TextEdit {
                            range: Range {
                                start: Position { line: 0, character: 0 },
                                end: Position { line: 0, character: 5 },
                            },
                            new_text: "BBBBB".to_string(),
                        })],
                    },
                ])),
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

    let service = Arc::new(LspService::new(make_service_config(
        &scenario_path,
        &transcript_path,
    )));
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
    std::fs::write(&scenario_path, serde_json::to_string_pretty(&scenario).unwrap()).unwrap();
    std::fs::write(&transcript_path, "").unwrap();

    let service = Arc::new(LspService::new(make_service_config(&scenario_path, &transcript_path)));
    let operations = Arc::new(LspOperations::new(service.clone()));
    let diagnostics = Arc::new(DiagnosticsCollector::new(service.clone()));

    // Open the file (triggers get_or_create_client + initialize + didOpen)
    diagnostics.get_diagnostic_snapshot_for_file(&source_path).await.unwrap();

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
    let names: Vec<&str> = response.all_symbols.iter().map(|s| s.name.as_str()).collect();
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
    eprintln!("DEBUG call_hierarchy: items={}, incoming={}, outgoing={}, prepare_error={:?}, incoming_error={:?}, outgoing_error={:?}",
        ch.items.len(), ch.incoming_count, ch.outgoing_count, ch.prepare_error, ch.incoming_error, ch.outgoing_error);
    // Print transcript for debugging
    let transcript_path = _root.join("transcript.jsonl");
    if let Ok(t) = std::fs::read_to_string(&transcript_path) {
        eprintln!("DEBUG transcript (last 20 lines):");
        for line in t.lines().rev().take(20) {
            eprintln!("  {line}");
        }
    }
    assert_eq!(ch.items.len(), 1, "call hierarchy should have one item");
    assert_eq!(ch.items[0].name, "entry");
    assert_eq!(
        ch.outgoing_count, 1,
        "entry should have one outgoing call (to helper)"
    );
    assert_eq!(ch.incoming_count, 0, "entry should have no incoming calls");

    // Type hierarchy: present because we requested it.
    let th = response
        .type_hierarchy
        .as_ref()
        .expect("type_hierarchy should be present");
    assert_eq!(th.items.len(), 1, "type hierarchy should have one item");
    assert_eq!(th.items[0].name, "entry");

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
    // type_hierarchy is derived from callHierarchyProvider in the capability
    // snapshot heuristic (see capability.rs), so it IS populated when
    // callHierarchyProvider is advertised. The server scenario still handles
    // the prepareTypeHierarchy/supertypes/subtypes requests.
    assert!(
        response.type_hierarchy.is_some(),
        "type_hierarchy should be present (derived from callHierarchyProvider)"
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
    let has_def_note = response
        .notes
        .iter()
        .any(|n| n.contains("goToDefinition"));
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

    // Call/type hierarchy: still present.
    assert!(
        response.call_hierarchy.is_some(),
        "call_hierarchy should be present despite definition error"
    );
    assert!(
        response.type_hierarchy.is_some(),
        "type_hierarchy should be present despite definition error"
    );

    // The overall response is still Ok (not an error).
    // The collector never returns Err for individual operation failures;
    // they are recorded as notes/unavailable in the response.

    service.shutdown_all().await;
}
