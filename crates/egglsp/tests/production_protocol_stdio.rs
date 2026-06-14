use std::time::Duration;

use base64::Engine;
use egglsp::{ClientTransportSnapshot, LspClientOptions, LspError};
use lsp_types::Position;
use serde_json::json;
use tokio::time::sleep;
use url::Url;

mod common;

use common::ProductionClientHarness;

async fn start_harness(
    scenario: serde_json::Value,
    options: LspClientOptions,
    configuration: serde_json::Value,
) -> ProductionClientHarness {
    ProductionClientHarness::start(scenario, options, configuration)
        .await
        .expect("failed to start production harness")
}

async fn expect_ok<T>(harness: &ProductionClientHarness, result: Result<T, LspError>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{}\n{}", err, harness.diagnostics().await),
    }
}

fn init_params_root_only() -> serde_json::Value {
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

#[tokio::test]
async fn initialization_handshake() {
    let scenario = json!({
        "name": "init_handshake",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
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
    });

    let harness = start_harness(
        scenario,
        LspClientOptions::default(),
        json!({"rust-analyzer": {"checkOnSave": true}}),
    )
    .await;

    let capabilities = harness
        .client
        .capabilities
        .lock()
        .await
        .clone()
        .expect("capabilities should be stored");
    assert!(capabilities.hover_provider.is_some());
    assert!(matches!(
        harness.client.transport_state_snapshot().await,
        ClientTransportSnapshot::Running
    ));
    assert_eq!(harness.client.pending_request_count().await, 0);

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn server_requests_during_init_and_dynamic_registration() {
    let scenario = json!({
        "name": "server_requests_during_init",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [
                    {
                        "type": "SendRequest",
                        "method": "workspace/configuration",
                        "params": {
                            "items": [{"section": "rust-analyzer"}]
                        }
                    },
                    {
                        "type": "SendRequest",
                        "method": "workspace/workspaceFolders",
                        "params": {}
                    },
                    {
                        "type": "SendRequest",
                        "method": "window/workDoneProgress/create",
                        "params": {"token": "progress-1"}
                    },
                    {
                        "type": "SendRequest",
                        "method": "client/registerCapability",
                        "params": {
                            "registrations": [
                                {
                                    "id": "reg-1",
                                    "method": "textDocument/completion",
                                    "registerOptions": {
                                        "triggerCharacters": ["."]
                                    }
                                }
                            ]
                        }
                    },
                    {"type": "RespondResult", "result": init_result_capabilities()}
                ]
            },
            {
                "type": "ExpectResponse",
                "id": {"type": "Number"},
                "result": [{"checkOnSave": true}]
            },
            {
                "type": "ExpectResponse",
                "id": {"type": "Number"},
                "result": [
                    {
                        "uri": "__ROOT_URI__",
                        "name": "__ROOT_NAME__"
                    }
                ]
            },
            {
                "type": "ExpectResponse",
                "id": {"type": "Number"},
                "result": null
            },
            {
                "type": "ExpectResponse",
                "id": {"type": "Number"},
                "result": null
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
    });

    let harness = start_harness(
        scenario,
        LspClientOptions::default(),
        json!({"rust-analyzer": {"checkOnSave": true}}),
    )
    .await;

    let snapshot = harness.client.dynamic_registration_snapshot().await;
    assert_eq!(snapshot.len(), 1);
    assert_eq!(snapshot[0].id, "reg-1");
    assert_eq!(snapshot[0].method, "textDocument/completion");
    assert_eq!(harness.client.pending_request_count().await, 0);

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn apply_edit_refusal_keeps_client_usable() {
    let scenario = json!({
        "name": "apply_edit_refusal",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [
                    {
                        "type": "SendRequest",
                        "method": "workspace/applyEdit",
                        "params": {
                            "edit": {
                                "documentChanges": [
                                    {
                                        "textDocument": {"uri": "__SOURCE_URI__", "version": 1},
                                        "edits": [
                                            {
                                                "range": {
                                                    "start": {"line": 0, "character": 0},
                                                    "end": {"line": 0, "character": 0}
                                                },
                                                "newText": "// edited\n"
                                            }
                                        ]
                                    }
                                ]
                            }
                        }
                    },
                    {"type": "RespondResult", "result": init_result_capabilities()}
                ]
            },
            {
                "type": "ExpectResponse",
                "id": {"type": "Number"},
                "result": {
                    "type": "ObjectContains",
                    "value": {
                        "applied": false,
                        "failureReason": {"type": "String"}
                    }
                }
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {
                            "kind": "markdown",
                            "value": "**fn** `harness_marker()`"
                        }
                    }
                }]
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
    });

    let harness = start_harness(scenario, LspClientOptions::default(), json!({})).await;
    let original = std::fs::read_to_string(&harness.source_path).expect("source file should exist");
    sleep(Duration::from_millis(100)).await;

    let hover = expect_ok(
        &harness,
        harness
            .client
            .hover(
                &Url::from_file_path(&harness.source_path).unwrap(),
                Position::new(0, 0),
            )
            .await,
    )
    .await;
    assert!(hover.is_some());
    assert_eq!(
        std::fs::read_to_string(&harness.source_path).expect("source file should still exist"),
        original
    );
    assert_eq!(harness.client.pending_request_count().await, 0);

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn concurrent_out_of_order_responses_and_notifications() {
    let scenario = json!({
        "name": "out_of_order_responses",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "SendNotification",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": "__SOURCE_URI__",
                    "diagnostics": [
                        {
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 3}
                            },
                            "message": "example diagnostic",
                            "severity": 1
                        }
                    ]
                }
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {"kind": "markdown", "value": "**fn** `harness_marker()`"}
                    }
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/definition",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 3}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 3}
                            }
                        },
                        {
                            "uri": "__SOURCE_URI__",
                            "range": {
                                "start": {"line": 2, "character": 0},
                                "end": {"line": 2, "character": 3}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "name": "harness_marker",
                            "kind": 12,
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 24}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 7},
                                "end": {"line": 0, "character": 20}
                            }
                        }
                    ]
                }]
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
    });

    let harness = start_harness(scenario, LspClientOptions::default(), json!({})).await;
    let uri = Url::from_file_path(&harness.source_path).unwrap();

    let (hover, definition, references, symbols) = tokio::join!(
        harness.client.hover(&uri, Position::new(0, 0)),
        harness.client.go_to_definition(&uri, Position::new(0, 0)),
        harness.client.find_references(&uri, Position::new(0, 0)),
        harness.client.document_symbols(&uri)
    );

    let hover = expect_ok(&harness, hover).await;
    assert!(hover.is_some());

    let definition = expect_ok(&harness, definition).await;
    assert!(definition.is_some());

    let references = expect_ok(&harness, references).await;
    assert_eq!(references.len(), 2);

    let symbols = expect_ok(&harness, symbols).await;
    assert_eq!(symbols.len(), 1);

    let diagnostics = harness.client.get_diagnostics(&uri.to_string()).await;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(harness.client.pending_request_count().await, 0);

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn request_timeout_and_late_response_are_dropped() {
    let scenario = json!({
        "name": "timeout_and_cancel",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "Delay",
                "millis": 2500
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {"kind": "markdown", "value": "late hover"}
                    }
                }]
            },
            {
                "type": "ExpectNotification",
                "method": "$/cancelRequest",
                "params": {
                    "type": "ObjectContains",
                    "value": {"id": {"type": "Number"}}
                },
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {"kind": "markdown", "value": "fresh hover"}
                    }
                }]
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
    });

    let options = LspClientOptions {
        request_timeout: Duration::from_secs(2),
        server_request_timeout: Duration::from_secs(5),
    };
    let harness = start_harness(scenario, options, json!({})).await;
    let uri = Url::from_file_path(&harness.source_path).unwrap();

    let timeout_err = harness
        .client
        .hover(&uri, Position::new(0, 0))
        .await
        .expect_err("hover should time out");
    assert!(matches!(timeout_err, LspError::RequestTimeout(_)));
    assert_eq!(harness.client.pending_request_count().await, 0);
    assert!(matches!(
        harness.client.transport_state_snapshot().await,
        ClientTransportSnapshot::Running
    ));

    sleep(Duration::from_millis(1000)).await;
    let transcript = std::fs::read_to_string(&harness.transcript_path).unwrap_or_default();
    assert!(
        transcript.contains("$/cancelRequest"),
        "expected client timeout to emit $/cancelRequest, transcript was: {transcript}"
    );
    let hover = expect_ok(
        &harness,
        harness.client.hover(&uri, Position::new(0, 0)).await,
    )
    .await;
    assert!(hover.is_some());
    assert_eq!(harness.client.pending_request_count().await, 0);

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn malformed_frames_fail_transport() {
    let malformed_cases = vec![
        (
            "missing_content_length",
            json!({"type": "SendRawBytes", "bytes_base64": base64::engine::general_purpose::STANDARD.encode(b"X-Test: 1\r\n\r\n{}")}),
        ),
        (
            "invalid_numeric_length",
            json!({"type": "SendRawBytes", "bytes_base64": base64::engine::general_purpose::STANDARD.encode(b"Content-Length: -1\r\n\r\n{}")}),
        ),
        (
            "duplicate_content_length",
            json!({"type": "SendRawBytes", "bytes_base64": base64::engine::general_purpose::STANDARD.encode(b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}")}),
        ),
        (
            "malformed_json",
            json!({"type": "SendRawFrame", "body": "{not json"}),
        ),
        (
            "invalid_utf8",
            json!({"type": "SendRawBytes", "bytes_base64": base64::engine::general_purpose::STANDARD.encode(b"Content-Length: 2\r\n\r\n\xff\xff")}),
        ),
        (
            "eof_mid_header",
            json!({"type": "SendHeaderOnly", "header": "Content-Length: 2\r\n"}),
        ),
        (
            "eof_mid_body",
            json!({
                "type": "SendBodyChunks",
                "header": "Content-Length: 128\r\n\r\n",
                "chunks": ["{\"jsonrpc\":\"2.0\",\"method\":\"oops\""],
                "delay_millis": 0
            }),
        ),
        (
            "oversized_length",
            json!({"type": "SendJsonWithDeclaredLength", "value": {"jsonrpc": "2.0", "method": "oops"}, "declared_length": 70000000}),
        ),
    ];

    for (name, action) in malformed_cases {
        let scenario = json!({
            "name": name,
            "steps": [
                {
                    "type": "ExpectRequest",
                    "method": "initialize",
                    "id": {"type": "Number"},
                    "params": init_params_root_only(),
                    "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
                },
                {"type": "ExpectNotification", "method": "initialized", "then": []},
                {
                    "type": "ExpectRequest",
                    "method": "textDocument/hover",
                    "then": [action, {"type": "Exit", "code": 0}]
                }
            ],
            "exit": {"type": "ExitCode", "code": 0},
            "strict": true
        });

        let harness = start_harness(scenario, LspClientOptions::default(), json!({})).await;
        let uri = Url::from_file_path(&harness.source_path).unwrap();

        let err = harness
            .client
            .hover(&uri, Position::new(0, 0))
            .await
            .expect_err("hover should fail after malformed output");
        assert!(matches!(
            err,
            LspError::RequestFailed(_) | LspError::WriterClosed(_) | LspError::Protocol(_)
        ));
        assert!(matches!(
            harness.client.transport_state_snapshot().await,
            ClientTransportSnapshot::Failed { .. }
        ));
        assert_eq!(harness.client.pending_request_count().await, 0);
    }
}

#[tokio::test]
async fn unknown_json_rpc_frames_are_ignored() {
    let scenario = json!({
        "name": "unknown_json_rpc_frames",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {
                        "type": "SendFramesTogether",
                        "messages": [
                            {"jsonrpc": "2.0", "id": 11},
                            {"jsonrpc": "2.0", "result": null},
                            {"jsonrpc": "2.0", "method": 123},
                            {"jsonrpc": "2.0", "id": {}, "method": "textDocument/hover"},
                            {"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics", "params": {"uri": "__SOURCE_URI__", "diagnostics": []}}
                        ]
                    }
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {"kind": "markdown", "value": "hover after unknown frames"}
                    }
                }]
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
    });

    let harness = start_harness(scenario, LspClientOptions::default(), json!({})).await;
    let uri = Url::from_file_path(&harness.source_path).unwrap();

    let hover = expect_ok(
        &harness,
        harness.client.hover(&uri, Position::new(0, 0)).await,
    )
    .await;
    assert!(hover.is_some());
    assert!(matches!(
        harness.client.transport_state_snapshot().await,
        ClientTransportSnapshot::Running
    ));
    assert_eq!(harness.client.pending_request_count().await, 0);
    assert!(harness
        .client
        .get_diagnostics(&uri.to_string())
        .await
        .is_empty());

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn grouped_frames_and_split_writes_are_processed() {
    let split_body = r#"{"jsonrpc":"2.0","method":"window/logMessage","params":{"type":3,"message":"split body"}}"#;
    let split_header = format!("Content-Length: {}\r\n\r\n", split_body.len());
    let split_midpoint = split_body.len() / 2;
    let split_first = split_body[..split_midpoint].to_string();
    let split_second = split_body[split_midpoint..].to_string();

    let scenario = json!({
        "name": "grouped_frames_and_split_writes",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
            },
            {
                "type": "ExpectNotification",
                "method": "initialized",
                "then": [
                    {
                        "type": "SendFramesTogether",
                        "messages": [
                            {"jsonrpc": "2.0", "method": "window/logMessage", "params": {"type": 3, "message": "first"}},
                            {"jsonrpc": "2.0", "method": "window/logMessage", "params": {"type": 3, "message": "second"}}
                        ]
                    },
                    {
                        "type": "SendBodyChunks",
                        "header": split_header,
                        "chunks": [split_first, split_second],
                        "delay_millis": 0
                    }
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {"kind": "markdown", "value": "hover after grouped frames"}
                    }
                }]
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
    });

    let harness = start_harness(scenario, LspClientOptions::default(), json!({})).await;
    let uri = Url::from_file_path(&harness.source_path).unwrap();

    let hover = expect_ok(
        &harness,
        harness.client.hover(&uri, Position::new(0, 0)).await,
    )
    .await;
    assert!(hover.is_some());
    assert!(matches!(
        harness.client.transport_state_snapshot().await,
        ClientTransportSnapshot::Running
    ));
    assert_eq!(harness.client.pending_request_count().await, 0);

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn diagnostics_lifecycle_tracks_file_changes() {
    let scenario = json!({
        "name": "diagnostics_lifecycle",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {"type": "ExpectNotification", "method": "textDocument/didOpen", "then": []},
            {
                "type": "Delay",
                "millis": 100
            },
            {
                "type": "SendNotification",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": "__SOURCE_URI__",
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
            {"type": "ExpectNotification", "method": "textDocument/didChange", "then": []},
            {"type": "ExpectNotification", "method": "textDocument/didSave", "then": []},
            {"type": "ExpectNotification", "method": "textDocument/didClose", "then": []},
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{"type": "RespondResult", "result": null}]
            },
            {"type": "ExpectNotification", "method": "exit", "then": []}
        ],
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    });

    let harness = start_harness(scenario, LspClientOptions::default(), json!({})).await;
    let uri = Url::from_file_path(&harness.source_path).unwrap();
    let uri_str = uri.to_string();

    expect_ok(
        &harness,
        harness
            .client
            .open_file(&uri, "pub fn harness_marker() {}\n", 1)
            .await,
    )
    .await;
    assert!(
        harness
            .client
            .diagnostics_may_still_be_warming(&uri_str)
            .await
    );
    sleep(Duration::from_millis(150)).await;
    let diagnostics = harness.client.get_diagnostics(&uri_str).await;
    assert_eq!(diagnostics.len(), 1);

    expect_ok(
        &harness,
        harness
            .client
            .update_file(&uri, "pub fn harness_marker() { let _x = 1; }\n", 2)
            .await,
    )
    .await;
    expect_ok(
        &harness,
        harness
            .client
            .save_file(&uri, Some("pub fn harness_marker() { let _x = 1; }\n"))
            .await,
    )
    .await;
    expect_ok(&harness, harness.client.close_file(&uri).await).await;
    assert_eq!(harness.client.pending_request_count().await, 0);

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn server_exit_before_response_and_error_response() {
    let exit_scenario = json!({
        "name": "server_exit_before_response",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{"type": "Exit", "code": 1}]
            }
        ],
        "exit": {"type": "ExitCode", "code": 1},
        "strict": true
    });

    let harness = start_harness(exit_scenario, LspClientOptions::default(), json!({})).await;
    let uri = Url::from_file_path(&harness.source_path).unwrap();
    let err = harness
        .client
        .hover(&uri, Position::new(0, 0))
        .await
        .expect_err("hover should fail after server exit");
    assert!(matches!(
        err,
        LspError::RequestFailed(_) | LspError::WriterClosed(_)
    ));
    assert!(matches!(
        harness.client.transport_state_snapshot().await,
        ClientTransportSnapshot::Failed { .. }
    ));
}

#[tokio::test]
async fn error_response_is_reported() {
    let scenario = json!({
        "name": "error_response",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": {"type": "Number"},
                "params": init_params_root_only(),
                "then": [{"type": "RespondResult", "result": init_result_capabilities()}]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "then": [{
                    "type": "RespondError",
                    "code": -32602,
                    "message": "bad params"
                }]
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
    });

    let harness = start_harness(scenario, LspClientOptions::default(), json!({})).await;
    let uri = Url::from_file_path(&harness.source_path).unwrap();
    let err = harness
        .client
        .hover(&uri, Position::new(0, 0))
        .await
        .expect_err("hover should return server error");
    assert!(matches!(err, LspError::RequestFailed(_)));

    harness.shutdown().await.expect("shutdown should succeed");
}
