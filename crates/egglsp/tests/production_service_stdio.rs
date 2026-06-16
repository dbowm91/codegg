use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egglsp::{
    LspConfig, LspDiagnosticFreshness, LspDiagnosticSource, LspError, LspRule, LspService,
};
use serde_json::json;
use tempfile::TempDir;
use tokio::sync::Barrier;
use tokio::time::sleep;
use url::Url;

mod common;

use common::FakeLspHarness;

const INITIAL_SOURCE_TEXT: &str = "pub fn harness_marker() {}\n";
const UPDATED_SOURCE_TEXT: &str = "pub fn harness_marker() { let _x = 1; }\n";

struct ServiceHarness {
    #[allow(dead_code)]
    tempdir: TempDir,
    root: PathBuf,
    source_path: PathBuf,
    scenario_path: PathBuf,
    transcript_path: PathBuf,
    service: Arc<LspService>,
    scenario_name: String,
}

impl ServiceHarness {
    fn start<F>(build_scenario: F) -> Result<Self, LspError>
    where
        F: FnOnce(&str, &str) -> serde_json::Value,
    {
        let tempdir = tempfile::tempdir()?;
        let root = tempdir.path().to_path_buf();
        let source_path = root.join("src/lib.rs");
        let scenario_path = root.join("scenario.json");
        let transcript_path = root.join("transcript.jsonl");

        std::fs::create_dir_all(root.join("src"))?;
        std::fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "egglsp-service-test"
version = "0.1.0"
edition = "2021"
"#,
        )?;
        std::fs::write(&source_path, INITIAL_SOURCE_TEXT)?;

        let root_uri = path_to_uri(&root);
        let source_uri = path_to_uri(&source_path);
        let scenario = build_scenario(&root_uri, &source_uri);
        let scenario_name = scenario
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("service-scenario")
            .to_string();

        std::fs::write(&scenario_path, serde_json::to_string_pretty(&scenario)?)?;

        let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));

        Ok(Self {
            tempdir,
            root,
            source_path,
            scenario_path,
            transcript_path,
            service,
            scenario_name,
        })
    }

    async fn diagnostics(&self) -> String {
        let mut out = String::new();
        push_line(&mut out, &format!("scenario: {}", self.scenario_name));
        push_line(&mut out, &format!("root: {}", self.root.display()));
        push_line(&mut out, &format!("source: {}", self.source_path.display()));
        push_line(
            &mut out,
            &format!("scenario file: {}", self.scenario_path.display()),
        );
        push_line(
            &mut out,
            &format!("transcript file: {}", self.transcript_path.display()),
        );
        push_line(
            &mut out,
            &format!("client keys: {:?}", self.service.client_keys().await),
        );
        push_line(&mut out, "--- scenario ---");
        match std::fs::read_to_string(&self.scenario_path) {
            Ok(contents) => out.push_str(&contents),
            Err(err) => push_line(&mut out, &format!("(scenario unavailable: {err})")),
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        push_line(&mut out, "--- transcript tail ---");

        let transcript_tail = transcript_tail(&self.transcript_path);
        if transcript_tail.is_empty() {
            push_line(&mut out, "(transcript empty)");
        } else {
            out.push_str(&transcript_tail);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }

        out
    }
}

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
            command: vec![FakeLspHarness::fake_server_path()],
            extensions: Some(vec!["rs".to_string()]),
            disabled: None,
            env: Some(env),
            initialization: None,
            workspace_configuration: None,
        },
    );

    LspConfig::Rules(rules)
}

fn path_to_uri(path: &Path) -> String {
    Url::from_file_path(path)
        .expect("invalid file path")
        .to_string()
}

fn init_params_root_only(_root_uri: &str) -> serde_json::Value {
    json!({
        "type": "ObjectContains",
        "value": {
            "processId": {"type": "Number"},
            "rootUri": {"type": "String"},
            "initializationOptions": {"type": "Null"}
        }
    })
}

fn init_result_capabilities() -> serde_json::Value {
    json!({
        "capabilities": {
            "hoverProvider": true,
            "definitionProvider": true,
            "referencesProvider": true,
            "documentSymbolProvider": true
        }
    })
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
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

async fn expect_ok<T: std::fmt::Debug>(
    harness: &ServiceHarness,
    label: &str,
    result: Result<T, LspError>,
) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{label}: {}\n{}", err, harness.diagnostics().await),
    }
}

async fn expect_init_cancelled<T: std::fmt::Debug>(
    harness: &ServiceHarness,
    label: &str,
    result: Result<T, LspError>,
) {
    match result {
        Err(LspError::InitializationCancelled(_)) => {}
        other => panic!(
            "{label}: expected InitializationCancelled, got {other:?}\n{}",
            harness.diagnostics().await
        ),
    }
}

async fn expect_request_error<T: std::fmt::Debug>(
    harness: &ServiceHarness,
    label: &str,
    result: Result<T, LspError>,
) -> LspError {
    match result {
        Ok(value) => panic!(
            "{label}: expected request error, got {value:?}\n{}",
            harness.diagnostics().await
        ),
        Err(err) => err,
    }
}

async fn wait_for_transcript_contains(harness: &ServiceHarness, needle: &str, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(contents) = std::fs::read_to_string(&harness.transcript_path) {
            if contents.contains(needle) {
                return;
            }
        }

        if tokio::time::Instant::now() >= deadline {
            panic!(
                "timed out waiting for transcript to contain {needle}\n{}",
                harness.diagnostics().await
            );
        }

        sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn single_flight_init_uses_a_real_child() {
    let harness = ServiceHarness::start(|root_uri, _source_uri| {
        json!({
            "name": "service_single_flight",
            "steps": [
                {"type": "Delay", "millis": 150},
                {
                    "type": "ExpectRequest",
                    "method": "initialize",
                    "id": {"type": "Number"},
                    "params": init_params_root_only(root_uri),
                    "then": [
                        {"type": "RespondResult", "result": init_result_capabilities()}
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
    })
    .expect("failed to start service harness");

    let service = harness.service.clone();
    let barrier = Arc::new(Barrier::new(13));
    let mut handles = Vec::new();

    for _ in 0..12 {
        let service = service.clone();
        let source_path = harness.source_path.clone();
        let barrier = barrier.clone();
        handles.push(tokio::spawn(async move {
            barrier.wait().await;
            service.get_or_create_client_for_file(&source_path).await
        }));
    }

    barrier.wait().await;

    let mut observed_key: Option<String> = None;
    let expected_root =
        std::fs::canonicalize(&harness.root).expect("failed to canonicalize harness root");
    for handle in handles {
        let result = handle.await.expect("task panicked");
        let (key, root) = expect_ok(&harness, "single-flight init", result).await;
        assert_eq!(
            std::fs::canonicalize(&root).expect("failed to canonicalize service root"),
            expected_root
        );
        match &observed_key {
            Some(existing) => assert_eq!(existing, &key),
            None => observed_key = Some(key),
        }
    }

    let key = observed_key.expect("expected a published client key");
    let keys = service.client_keys().await;
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0], key);

    let capabilities = service
        .get_capabilities_for_key(&key)
        .await
        .expect("capabilities should be stored");
    assert!(capabilities.hover_provider.is_some());

    tokio::time::timeout(Duration::from_secs(5), service.shutdown_all())
        .await
        .expect("shutdown_all timed out");

    assert!(service.client_keys().await.is_empty());

    let restart_attempt = service
        .get_or_create_client_for_file(&harness.source_path)
        .await;
    expect_init_cancelled(
        &harness,
        "single-flight restart after shutdown",
        restart_attempt,
    )
    .await;

    let transcript = std::fs::read_to_string(&harness.transcript_path)
        .expect("failed to read service transcript");
    assert_eq!(
        transcript.matches("\"method\":\"initialize\"").count(),
        1,
        "expected exactly one initialize request in transcript\n{transcript}"
    );
    assert_eq!(
        transcript.matches("\"method\":\"initialized\"").count(),
        1,
        "expected exactly one initialized notification in transcript\n{transcript}"
    );
    assert_eq!(
        transcript.matches("\"method\":\"shutdown\"").count(),
        1,
        "expected exactly one shutdown request in transcript\n{transcript}"
    );
}

#[tokio::test]
async fn document_lifecycle_ownership_tracks_open_update_save_close() {
    let harness = ServiceHarness::start(|root_uri, source_uri| {
        json!({
            "name": "service_document_lifecycle",
            "steps": [
                {
                    "type": "ExpectRequest",
                    "method": "initialize",
                    "id": {"type": "Number"},
                    "params": init_params_root_only(root_uri),
                    "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
                },
                {"type": "ExpectNotification", "method": "initialized", "then": []},
                {
                    "type": "ExpectNotification",
                    "method": "textDocument/didOpen",
                    "params": {
                        "type": "ObjectContains",
                        "value": {
                            "textDocument": {
                                "uri": source_uri,
                                "languageId": "rust",
                                "version": 1,
                                "text": INITIAL_SOURCE_TEXT
                            }
                        }
                    },
                    "then": []
                },
                {
                    "type": "ExpectNotification",
                    "method": "textDocument/didChange",
                    "params": {
                        "type": "ObjectContains",
                        "value": {
                            "textDocument": {
                                "uri": source_uri,
                                "version": 2
                            },
                            "contentChanges": [
                                {
                                    "text": UPDATED_SOURCE_TEXT
                                }
                            ]
                        }
                    },
                    "then": []
                },
                {
                    "type": "ExpectNotification",
                    "method": "textDocument/didSave",
                    "params": {
                        "type": "ObjectContains",
                        "value": {
                            "textDocument": {
                                "uri": source_uri
                            },
                            "text": UPDATED_SOURCE_TEXT
                        }
                    },
                    "then": []
                },
                {
                    "type": "ExpectNotification",
                    "method": "textDocument/didClose",
                    "params": {
                        "type": "ObjectContains",
                        "value": {
                            "textDocument": {
                                "uri": source_uri
                            }
                        }
                    },
                    "then": []
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
    })
    .expect("failed to start service harness");

    let service = harness.service.clone();
    let source = harness.source_path.clone();
    let source_uri = path_to_uri(&source);

    expect_ok(
        &harness,
        "document lifecycle open_file",
        service.open_file(&source, INITIAL_SOURCE_TEXT).await,
    )
    .await;

    let keys = service.client_keys().await;
    assert_eq!(keys.len(), 1);
    let key = keys[0].clone();

    assert!(
        expect_ok(
            &harness,
            "document lifecycle is_file_open after open",
            service.is_file_open(&key, &source_uri).await,
        )
        .await,
        "service should own the file after didOpen"
    );

    expect_ok(
        &harness,
        "document lifecycle update_file",
        service.update_file(&source, UPDATED_SOURCE_TEXT).await,
    )
    .await;
    expect_ok(
        &harness,
        "document lifecycle save_file",
        service.save_file(&source, Some(UPDATED_SOURCE_TEXT)).await,
    )
    .await;
    expect_ok(
        &harness,
        "document lifecycle close_file",
        service.close_file(&source).await,
    )
    .await;
    expect_ok(
        &harness,
        "document lifecycle second close_file",
        service.close_file(&source).await,
    )
    .await;

    assert!(
        !expect_ok(
            &harness,
            "document lifecycle is_file_open after close",
            service.is_file_open(&key, &source_uri).await,
        )
        .await,
        "service should release ownership after didClose"
    );

    tokio::time::timeout(Duration::from_secs(5), service.shutdown_all())
        .await
        .expect("shutdown_all timed out");

    assert!(service.client_keys().await.is_empty());
}

#[tokio::test]
async fn diagnostics_propagate_through_service_apis() {
    let harness = ServiceHarness::start(|root_uri, source_uri| {
        json!({
            "name": "service_diagnostics",
            "steps": [
                {
                    "type": "ExpectRequest",
                    "method": "initialize",
                    "id": {"type": "Number"},
                    "params": init_params_root_only(root_uri),
                    "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
                },
                {"type": "ExpectNotification", "method": "initialized", "then": []},
                {
                    "type": "ExpectNotification",
                    "method": "textDocument/didOpen",
                    "params": {
                        "type": "ObjectContains",
                        "value": {
                            "textDocument": {
                                "uri": source_uri,
                                "languageId": "rust",
                                "version": 1,
                                "text": INITIAL_SOURCE_TEXT
                            }
                        }
                    },
                    "then": []
                },
                {"type": "Delay", "millis": 150},
                {
                    "type": "SendNotification",
                    "method": "textDocument/publishDiagnostics",
                    "params": {
                        "uri": source_uri,
                        "diagnostics": [
                            {
                                "range": {
                                    "start": {"line": 0, "character": 0},
                                    "end": {"line": 0, "character": 3}
                                },
                                "message": "warmup warning",
                                "severity": 2
                            }
                        ]
                    }
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
    })
    .expect("failed to start service harness");

    let service = harness.service.clone();
    let source = harness.source_path.clone();
    let source_uri = path_to_uri(&source);

    expect_ok(
        &harness,
        "diagnostics open_file",
        service.open_file(&source, INITIAL_SOURCE_TEXT).await,
    )
    .await;

    let keys = service.client_keys().await;
    assert_eq!(keys.len(), 1);
    let key = keys[0].clone();

    assert!(
        service
            .diagnostics_may_still_be_warming(&key, &source_uri)
            .await,
        "diagnostics should be warming before the fake server publishes"
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        if !service
            .diagnostics_may_still_be_warming(&key, &source_uri)
            .await
        {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("timed out waiting for diagnostics warming to clear");
        }
        sleep(Duration::from_millis(25)).await;
    }

    let diagnostics = expect_ok(
        &harness,
        "diagnostics get_diagnostics_for_key",
        service.get_diagnostics_for_key(&key, &source_uri).await,
    )
    .await;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "warmup warning");

    let snapshot = expect_ok(
        &harness,
        "diagnostics get_diagnostic_snapshot_for_key",
        service
            .get_diagnostic_snapshot_for_key(&key, &source_uri)
            .await,
    )
    .await;
    assert_eq!(snapshot.source, LspDiagnosticSource::Pushed);
    assert_eq!(snapshot.freshness, LspDiagnosticFreshness::Fresh);
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(snapshot.diagnostics[0].message, "warmup warning");
    assert!(snapshot.is_usable_evidence());

    tokio::time::timeout(Duration::from_secs(5), service.shutdown_all())
        .await
        .expect("shutdown_all timed out");

    assert!(service.client_keys().await.is_empty());
}

#[tokio::test]
async fn shutdown_during_delayed_init_cancels_waiters() {
    let harness = ServiceHarness::start(|root_uri, _source_uri| {
        json!({
            "name": "service_shutdown_during_init",
            "steps": [
                {
                    "type": "ExpectRequest",
                    "method": "initialize",
                    "id": {"type": "Number"},
                    "params": init_params_root_only(root_uri),
                    "then": []
                },
                {"type": "Delay", "millis": 1000}
            ],
            "exit": {"type": "ExitCode", "code": 0},
            "strict": true
        })
    })
    .expect("failed to start service harness");

    let service = harness.service.clone();
    let source = harness.source_path.clone();

    let init_task = tokio::spawn({
        let service = service.clone();
        async move { service.get_or_create_client_for_file(&source).await }
    });

    wait_for_transcript_contains(
        &harness,
        "\"method\":\"initialize\"",
        Duration::from_secs(3),
    )
    .await;

    tokio::time::timeout(Duration::from_secs(5), service.shutdown_all())
        .await
        .expect("shutdown_all timed out");

    let init_result = tokio::time::timeout(Duration::from_secs(5), init_task)
        .await
        .expect("init task timed out")
        .expect("init task panicked");
    expect_init_cancelled(&harness, "shutdown during init leader", init_result).await;

    assert!(service.client_keys().await.is_empty());

    let restart_attempt = service
        .get_or_create_client_for_file(&harness.source_path)
        .await;
    expect_init_cancelled(
        &harness,
        "shutdown during init restart after shutdown",
        restart_attempt,
    )
    .await;

    let transcript = std::fs::read_to_string(&harness.transcript_path)
        .expect("failed to read service transcript");
    assert_eq!(
        transcript.matches("\"method\":\"initialize\"").count(),
        1,
        "expected one initialize request before cancellation\n{transcript}"
    );
    assert_eq!(
        transcript.matches("\"method\":\"initialized\"").count(),
        0,
        "initialization should not complete after shutdown\n{transcript}"
    );
    assert_eq!(
        transcript.matches("\"method\":\"shutdown\"").count(),
        0,
        "service should not publish a shutdown request for an unpublished client\n{transcript}"
    );
}

#[tokio::test]
async fn shutdown_with_inflight_request_completes_bounded() {
    let harness = ServiceHarness::start(|root_uri, source_uri| {
        json!({
            "name": "service_shutdown_with_request",
            "steps": [
                {
                    "type": "ExpectRequest",
                    "method": "initialize",
                    "id": {"type": "Number"},
                    "params": init_params_root_only(root_uri),
                    "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
                },
                {"type": "ExpectNotification", "method": "initialized", "then": []},
                {
                    "type": "ExpectRequest",
                    "method": "textDocument/hover",
                    "params": {
                        "type": "ObjectContains",
                        "value": {
                            "textDocument": {
                                "uri": source_uri
                            },
                            "position": {
                                "line": 0,
                                "character": 0
                            }
                        }
                    },
                    "then": []
                },
                {"type": "Delay", "millis": 150},
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
    })
    .expect("failed to start service harness");

    let service = harness.service.clone();
    let source = harness.source_path.clone();
    let source_uri = path_to_uri(&source);

    let (key, _) = expect_ok(
        &harness,
        "shutdown with inflight init",
        service.get_or_create_client_for_file(&source).await,
    )
    .await;

    let hover_handle = tokio::spawn({
        let service = service.clone();
        let key = key.clone();
        let params = json!({
            "textDocument": { "uri": source_uri },
            "position": { "line": 0, "character": 0 }
        });
        async move {
            service
                .send_request(&key, "textDocument/hover", params)
                .await
        }
    });

    wait_for_transcript_contains(
        &harness,
        "\"method\":\"textDocument/hover\"",
        Duration::from_secs(3),
    )
    .await;

    let shutdown_handle = tokio::spawn({
        let service = service.clone();
        async move { service.shutdown_all().await }
    });

    let hover_result = tokio::time::timeout(Duration::from_secs(5), hover_handle)
        .await
        .expect("hover task timed out")
        .expect("hover task panicked");
    let hover_error =
        expect_request_error(&harness, "shutdown with inflight hover", hover_result).await;
    assert!(
        matches!(
            hover_error,
            LspError::RequestFailed(_) | LspError::WriterClosed(_)
        ),
        "unexpected hover error: {hover_error:?}"
    );

    tokio::time::timeout(Duration::from_secs(5), shutdown_handle)
        .await
        .expect("shutdown task timed out")
        .expect("shutdown task panicked");

    assert!(service.client_keys().await.is_empty());

    let restart_attempt = service
        .get_or_create_client_for_file(&harness.source_path)
        .await;
    expect_init_cancelled(
        &harness,
        "shutdown with inflight restart after shutdown",
        restart_attempt,
    )
    .await;
}
