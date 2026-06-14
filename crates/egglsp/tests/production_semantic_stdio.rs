use std::path::Path;
use std::time::Duration;

use egglsp::lsp_types::{
    CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionTriggerKind, CompletionItem,
    CompletionTriggerKind, Diagnostic, DiagnosticSeverity, DocumentFormattingParams,
    DocumentSymbol, FormattingOptions, GotoDefinitionResponse, HoverContents, Position, Range,
    RenameParams, TextDocumentIdentifier, TextDocumentPositionParams, TextEdit, WorkspaceEdit,
};
use egglsp::{
    ClientTransportSnapshot, LspClientOptions, LspDiagnosticFreshness, LspDiagnosticSource,
    LspError,
};
use tokio::time::sleep;
use url::Url;

mod common;

use common::ProductionClientHarness;

async fn start_harness(
    scenario: serde_json::Value,
    configuration: serde_json::Value,
) -> ProductionClientHarness {
    ProductionClientHarness::start(scenario, LspClientOptions::default(), configuration)
        .await
        .expect("failed to start production harness")
}

async fn expect_ok<T>(harness: &ProductionClientHarness, result: Result<T, LspError>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{}\n{}", err, harness.diagnostics().await),
    }
}

async fn wait_for_diagnostics(
    harness: &ProductionClientHarness,
    uri: &str,
    expected_len: usize,
) -> Vec<Diagnostic> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let diagnostics = harness.client.get_diagnostics(uri).await;
        let current_len = diagnostics.len();
        if current_len >= expected_len {
            return diagnostics;
        }

        if tokio::time::Instant::now() >= deadline {
            let diagnostics_report = harness.diagnostics().await;
            panic!(
                "timed out waiting for {expected_len} diagnostics (got {current_len})\n{diagnostics_report}"
            );
        }

        sleep(Duration::from_millis(25)).await;
    }
}

async fn send_typed_request<T: serde::Serialize>(
    harness: &ProductionClientHarness,
    method: &str,
    params: T,
) -> serde_json::Value {
    expect_ok(
        harness,
        harness
            .client
            .send_request(
                method,
                serde_json::to_value(params).expect("failed to serialize request params"),
            )
            .await,
    )
    .await
}

fn file_uri(path: &Path) -> Url {
    Url::from_file_path(path).expect("valid file path")
}

fn lsp_uri(uri: &Url) -> egglsp::lsp_types::Uri {
    uri.as_str()
        .parse()
        .expect("failed to convert file URI to LSP URI")
}

#[tokio::test]
async fn typed_semantic_requests_collect_context_and_freshness() {
    let initial_text = "\
use std::fmt;
use std::io;

pub fn harness_marker() {
    let value = 1;
}
";
    let updated_text = "\
use std::fmt;
use std::io;

pub fn harness_marker() {
    let value = 2;
}
";
    let source_uri = "__SOURCE_URI__";

    let scenario = serde_json::json!({
        "name": "typed_semantic_requests_collect_context_and_freshness",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "capabilities": {
                            "hoverProvider": true,
                            "definitionProvider": true,
                            "referencesProvider": true,
                            "documentSymbolProvider": true,
                            "codeActionProvider": true
                        }
                    }
                }]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri,
                                "languageId": "rust",
                                "version": 1,
                                "text": initial_text
                            }
                        }
                    }
                },
                "then": []
            },
            {
                "type": "Delay",
                "millis": 250
            },
            {
                "type": "SendNotification",
                "method": "textDocument/publishDiagnostics",
                "params": {
                    "uri": source_uri,
                    "diagnostics": [
                        {
                            "range": {
                                "start": {"line": 1, "character": 0},
                                "end": {"line": 1, "character": 11}
                            },
                            "message": "unused import",
                            "severity": 2,
                            "source": "rustc"
                        }
                    ]
                }
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didChange",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri,
                                "version": 2
                            }
                        },
                        "contentChanges": {"type": "ArrayLen", "value": 1}
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
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "text": updated_text
                    }
                },
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "position": {
                            "type": "ObjectContains",
                            "value": {
                                "line": 3,
                                "character": 7
                            }
                        }
                    }
                },
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
                "method": "textDocument/definition",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "position": {
                            "type": "ObjectContains",
                            "value": {
                                "line": 3,
                                "character": 7
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "uri": source_uri,
                        "range": {
                            "start": {"line": 3, "character": 0},
                            "end": {"line": 5, "character": 1}
                        }
                    }
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/references",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "position": {
                            "type": "ObjectContains",
                            "value": {
                                "line": 3,
                                "character": 7
                            }
                        },
                        "context": {
                            "type": "ObjectContains",
                            "value": {
                                "includeDeclaration": true
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "uri": source_uri,
                            "range": {
                                "start": {"line": 3, "character": 3},
                                "end": {"line": 3, "character": 18}
                            }
                        },
                        {
                            "uri": source_uri,
                            "range": {
                                "start": {"line": 4, "character": 8},
                                "end": {"line": 4, "character": 13}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/documentSymbol",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "name": "harness_marker",
                            "kind": 12,
                            "range": {
                                "start": {"line": 3, "character": 0},
                                "end": {"line": 5, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 3, "character": 7},
                                "end": {"line": 3, "character": 22}
                            },
                            "children": [
                                {
                                    "name": "value",
                                    "kind": 13,
                                    "range": {
                                        "start": {"line": 4, "character": 4},
                                        "end": {"line": 4, "character": 18}
                                    },
                                    "selectionRange": {
                                        "start": {"line": 4, "character": 8},
                                        "end": {"line": 4, "character": 13}
                                    },
                                    "children": []
                                }
                            ]
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/completion",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "position": {
                            "type": "ObjectContains",
                            "value": {
                                "line": 4,
                                "character": 4
                            }
                        },
                        "context": {
                            "type": "ObjectContains",
                            "value": {
                                "triggerKind": 2,
                                "triggerCharacter": "."
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "isIncomplete": false,
                        "items": [
                            {
                                "label": "harness_marker",
                                "kind": 3
                            }
                        ]
                    }
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/codeAction",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "range": {
                            "type": "ObjectContains",
                            "value": {
                                "start": {
                                    "type": "ObjectContains",
                                    "value": {
                                        "line": 0,
                                        "character": 0
                                    }
                                },
                                "end": {
                                    "type": "ObjectContains",
                                    "value": {
                                        "line": 2,
                                        "character": 0
                                    }
                                }
                            }
                        },
                        "context": {
                            "type": "ObjectContains",
                            "value": {
                                "diagnostics": {"type": "ArrayLen", "value": 1},
                                "only": {"type": "ArrayLen", "value": 1},
                                "triggerKind": 1
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "title": "organize imports",
                            "kind": "source.organizeImports",
                            "edit": {
                                "changes": {
                                    "__SOURCE_URI__": [
                                        {
                                            "range": {
                                                "start": {"line": 1, "character": 0},
                                                "end": {"line": 1, "character": 11}
                                            },
                                            "newText": "use std::collections::HashMap;"
                                        }
                                    ]
                                }
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didClose",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
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
    });

    let harness = start_harness(scenario, serde_json::json!({})).await;
    let uri = file_uri(&harness.source_path);
    let uri_str = uri.to_string();

    std::fs::write(&harness.source_path, initial_text).expect("failed to seed source file");
    let original_disk_text =
        std::fs::read_to_string(&harness.source_path).expect("source file should exist");

    expect_ok(
        &harness,
        harness.client.open_file(&uri, initial_text, 1).await,
    )
    .await;
    assert!(
        harness
            .client
            .diagnostics_may_still_be_warming(&uri_str)
            .await
    );

    let diagnostics = wait_for_diagnostics(&harness, &uri_str, 1).await;
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "unused import");
    assert!(
        !harness
            .client
            .diagnostics_may_still_be_warming(&uri_str)
            .await
    );

    let snapshot = harness.client.diagnostic_snapshot(&uri_str).await;
    assert_eq!(snapshot.freshness, LspDiagnosticFreshness::Fresh);
    assert_eq!(snapshot.source, LspDiagnosticSource::Pushed);
    assert_eq!(snapshot.diagnostics.len(), 1);
    assert_eq!(snapshot.diagnostics[0].message, "unused import");
    assert!(snapshot.is_usable_evidence());

    expect_ok(
        &harness,
        harness.client.update_file(&uri, updated_text, 2).await,
    )
    .await;
    expect_ok(
        &harness,
        harness.client.save_file(&uri, Some(updated_text)).await,
    )
    .await;
    assert!(
        !harness
            .client
            .diagnostics_may_still_be_warming(&uri_str)
            .await
    );

    let stale_snapshot = harness.client.diagnostic_snapshot(&uri_str).await;
    assert_eq!(
        stale_snapshot.freshness,
        LspDiagnosticFreshness::PossiblyStale
    );
    assert_eq!(stale_snapshot.source, LspDiagnosticSource::Pushed);
    assert_eq!(stale_snapshot.diagnostics.len(), 1);
    assert!(stale_snapshot.is_usable_evidence());
    assert!(harness.client.pending_request_count().await == 0);

    let hover = expect_ok(
        &harness,
        harness.client.hover(&uri, Position::new(3, 7)).await,
    )
    .await;
    let hover = hover.expect("hover should return contents");
    match hover.contents {
        HoverContents::Markup(markup) => {
            assert_eq!(markup.value, "**fn** `harness_marker()`");
        }
        other => panic!("unexpected hover contents: {other:?}"),
    }

    let definition = expect_ok(
        &harness,
        harness
            .client
            .go_to_definition(&uri, Position::new(3, 7))
            .await,
    )
    .await;
    let definition = definition.expect("definition should return a location");
    match definition {
        GotoDefinitionResponse::Scalar(location) => {
            assert_eq!(location.uri.to_string(), uri.to_string());
            assert_eq!(location.range.start.line, 3);
        }
        other => panic!("unexpected definition response: {other:?}"),
    }

    let references = expect_ok(
        &harness,
        harness
            .client
            .find_references(&uri, Position::new(3, 7))
            .await,
    )
    .await;
    assert_eq!(references.len(), 2);
    assert_eq!(references[0].uri.to_string(), uri.to_string());
    assert_eq!(references[1].range.start.line, 4);

    let symbols: Vec<DocumentSymbol> =
        expect_ok(&harness, harness.client.document_symbols(&uri).await).await;
    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "harness_marker");
    let children = symbols[0]
        .children
        .as_ref()
        .expect("symbol should have a child");
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].name, "value");

    let completion: Vec<CompletionItem> = expect_ok(
        &harness,
        harness
            .client
            .completion(
                &uri,
                Position::new(4, 4),
                Some(CompletionTriggerKind::TRIGGER_CHARACTER),
                Some(".".to_string()),
            )
            .await,
    )
    .await;
    assert_eq!(completion.len(), 1);
    assert_eq!(completion[0].label, "harness_marker");

    let code_actions = expect_ok(
        &harness,
        harness
            .client
            .code_actions(
                &uri,
                Range {
                    start: Position::new(0, 0),
                    end: Position::new(2, 0),
                },
                CodeActionContext {
                    diagnostics: vec![Diagnostic {
                        range: Range {
                            start: Position::new(1, 0),
                            end: Position::new(1, 11),
                        },
                        severity: Some(DiagnosticSeverity::WARNING),
                        source: Some("rustc".to_string()),
                        message: "unused import".to_string(),
                        ..Default::default()
                    }],
                    only: Some(vec![CodeActionKind::SOURCE_ORGANIZE_IMPORTS]),
                    trigger_kind: Some(CodeActionTriggerKind::INVOKED),
                },
            )
            .await,
    )
    .await;
    assert_eq!(code_actions.len(), 1);
    match &code_actions[0] {
        CodeActionOrCommand::CodeAction(action) => {
            assert_eq!(action.title, "organize imports");
            assert!(action.edit.is_some());
        }
        other => panic!("unexpected code action response: {other:?}"),
    }

    expect_ok(&harness, harness.client.close_file(&uri).await).await;
    assert_eq!(harness.client.pending_request_count().await, 0);
    assert_eq!(
        std::fs::read_to_string(&harness.source_path).expect("source file should exist"),
        original_disk_text
    );
    assert!(matches!(
        harness.client.transport_state_snapshot().await,
        ClientTransportSnapshot::Running
    ));

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn edit_round_trips_do_not_mutate_disk() {
    let initial_text = "\
use std::fmt;
use std::io;

pub fn harness_marker() {
    let value = 1;
}
";
    let source_uri = "__SOURCE_URI__";

    let scenario = serde_json::json!({
        "name": "edit_round_trips_do_not_mutate_disk",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "capabilities": {
                            "renameProvider": true,
                            "documentFormattingProvider": true,
                            "codeActionProvider": true,
                            "hoverProvider": true
                        }
                    }
                }]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri,
                                "languageId": "rust",
                                "version": 1,
                                "text": initial_text
                            }
                        }
                    }
                },
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/rename",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "position": {
                            "type": "ObjectContains",
                            "value": {
                                "line": 3,
                                "character": 7
                            }
                        },
                        "newName": "renamed_marker"
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "changes": {
                            "__SOURCE_URI__": [
                                {
                                    "range": {
                                        "start": {"line": 3, "character": 7},
                                        "end": {"line": 3, "character": 22}
                                    },
                                    "newText": "renamed_marker"
                                }
                            ]
                        }
                    }
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/formatting",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 13}
                            },
                            "newText": "use std::fmt::{self};"
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/codeAction",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "context": {
                            "type": "ObjectContains",
                            "value": {
                                "triggerKind": 1,
                                "only": {"type": "ArrayLen", "value": 1}
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "title": "organize imports",
                            "kind": "source.organizeImports",
                            "edit": {
                                "changes": {
                                    "__SOURCE_URI__": [
                                        {
                                            "range": {
                                                "start": {"line": 1, "character": 0},
                                                "end": {"line": 1, "character": 11}
                                            },
                                            "newText": "use std::collections::HashMap;"
                                        }
                                    ]
                                }
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/hover",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "position": {
                            "type": "ObjectContains",
                            "value": {
                                "line": 3,
                                "character": 7
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "contents": {
                            "kind": "plaintext",
                            "value": "client still usable"
                        }
                    }
                }]
            },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didClose",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
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
    });

    let harness = start_harness(scenario, serde_json::json!({})).await;
    let uri = file_uri(&harness.source_path);

    std::fs::write(&harness.source_path, initial_text).expect("failed to seed source file");
    let original_disk_text =
        std::fs::read_to_string(&harness.source_path).expect("source file should exist");

    expect_ok(
        &harness,
        harness.client.open_file(&uri, initial_text, 1).await,
    )
    .await;

    let rename_params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: lsp_uri(&uri) },
            position: Position::new(3, 7),
        },
        new_name: "renamed_marker".to_string(),
        work_done_progress_params: Default::default(),
    };
    let rename_value = send_typed_request(&harness, "textDocument/rename", rename_params).await;
    let rename_edit: WorkspaceEdit =
        serde_json::from_value(rename_value).expect("rename should parse as workspace edit");
    #[allow(clippy::mutable_key_type)]
    let rename_changes = rename_edit.changes.expect("rename should use changes");
    assert_eq!(rename_changes.len(), 1);

    let formatting_params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri: lsp_uri(&uri) },
        options: FormattingOptions {
            tab_size: 4,
            insert_spaces: true,
            properties: Default::default(),
            trim_trailing_whitespace: Some(true),
            insert_final_newline: Some(true),
            trim_final_newlines: Some(true),
        },
        work_done_progress_params: Default::default(),
    };
    let formatting_value =
        send_typed_request(&harness, "textDocument/formatting", formatting_params).await;
    let formatting_edits: Vec<TextEdit> =
        serde_json::from_value(formatting_value).expect("formatting should parse as text edits");
    assert_eq!(formatting_edits.len(), 1);

    let code_action_params = egglsp::lsp_types::CodeActionParams {
        text_document: TextDocumentIdentifier { uri: lsp_uri(&uri) },
        range: Range {
            start: Position::new(0, 0),
            end: Position::new(2, 0),
        },
        context: CodeActionContext {
            diagnostics: vec![Diagnostic {
                range: Range {
                    start: Position::new(1, 0),
                    end: Position::new(1, 11),
                },
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("rustc".to_string()),
                message: "unused import".to_string(),
                ..Default::default()
            }],
            only: Some(vec![CodeActionKind::SOURCE_ORGANIZE_IMPORTS]),
            trigger_kind: Some(CodeActionTriggerKind::INVOKED),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let code_action_value =
        send_typed_request(&harness, "textDocument/codeAction", code_action_params).await;
    let code_actions: Vec<CodeActionOrCommand> = serde_json::from_value(code_action_value)
        .expect("code action should parse as typed actions");
    assert_eq!(code_actions.len(), 1);
    match &code_actions[0] {
        CodeActionOrCommand::CodeAction(action) => {
            assert_eq!(action.title, "organize imports");
            assert!(action.edit.is_some());
        }
        other => panic!("unexpected code action response: {other:?}"),
    }

    let hover = expect_ok(
        &harness,
        harness.client.hover(&uri, Position::new(3, 7)).await,
    )
    .await;
    assert!(hover.is_some());

    expect_ok(&harness, harness.client.close_file(&uri).await).await;
    assert_eq!(harness.client.pending_request_count().await, 0);
    assert_eq!(
        std::fs::read_to_string(&harness.source_path).expect("source file should exist"),
        original_disk_text
    );
    assert!(matches!(
        harness.client.transport_state_snapshot().await,
        ClientTransportSnapshot::Running
    ));

    harness.shutdown().await.expect("shutdown should succeed");
}

#[tokio::test]
async fn hierarchy_context_requests_round_trip_through_real_client() {
    let source_uri = "__SOURCE_URI__";

    let scenario = serde_json::json!({
        "name": "hierarchy_context_requests_round_trip_through_real_client",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{
                    "type": "RespondResult",
                    "result": {
                        "capabilities": {
                            "callHierarchyProvider": true,
                            "typeHierarchyProvider": true
                        }
                    }
                }]
            },
            {"type": "ExpectNotification", "method": "initialized", "then": []},
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareCallHierarchy",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "position": {
                            "type": "ObjectContains",
                            "value": {
                                "line": 3,
                                "character": 7
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "name": "call_root",
                            "kind": 12,
                            "uri": source_uri,
                            "range": {
                                "start": {"line": 3, "character": 0},
                                "end": {"line": 5, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 3, "character": 7},
                                "end": {"line": 3, "character": 16}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/incomingCalls",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "item": {
                            "type": "ObjectContains",
                            "value": {
                                "name": "call_root",
                                "uri": source_uri
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "from": {
                                "name": "call_root",
                                "kind": 12,
                                "uri": source_uri,
                                "range": {
                                    "start": {"line": 3, "character": 0},
                                    "end": {"line": 5, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 3, "character": 7},
                                    "end": {"line": 3, "character": 16}
                                }
                            },
                            "fromRanges": [
                                {
                                    "start": {"line": 4, "character": 4},
                                    "end": {"line": 4, "character": 15}
                                }
                            ]
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "callHierarchy/outgoingCalls",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "item": {
                            "type": "ObjectContains",
                            "value": {
                                "name": "call_root",
                                "uri": source_uri
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "to": {
                                "name": "callee",
                                "kind": 12,
                                "uri": source_uri,
                                "range": {
                                    "start": {"line": 10, "character": 0},
                                    "end": {"line": 12, "character": 1}
                                },
                                "selectionRange": {
                                    "start": {"line": 10, "character": 7},
                                    "end": {"line": 10, "character": 13}
                                }
                            },
                            "fromRanges": [
                                {
                                    "start": {"line": 4, "character": 4},
                                    "end": {"line": 4, "character": 15}
                                }
                            ]
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/prepareTypeHierarchy",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {
                                "uri": source_uri
                            }
                        },
                        "position": {
                            "type": "ObjectContains",
                            "value": {
                                "line": 3,
                                "character": 7
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "name": "TypeRoot",
                            "kind": 23,
                            "uri": source_uri,
                            "range": {
                                "start": {"line": 3, "character": 0},
                                "end": {"line": 5, "character": 1}
                            },
                            "selectionRange": {
                                "start": {"line": 3, "character": 7},
                                "end": {"line": 3, "character": 14}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "typeHierarchy/supertypes",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "item": {
                            "type": "ObjectContains",
                            "value": {
                                "name": "TypeRoot",
                                "uri": source_uri
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "name": "BaseType",
                            "kind": 23,
                            "uri": source_uri,
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 10}
                            },
                            "selectionRange": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 8}
                            }
                        }
                    ]
                }]
            },
            {
                "type": "ExpectRequest",
                "method": "typeHierarchy/subtypes",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "item": {
                            "type": "ObjectContains",
                            "value": {
                                "name": "TypeRoot",
                                "uri": source_uri
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "name": "ChildOne",
                            "kind": 23,
                            "uri": source_uri,
                            "range": {
                                "start": {"line": 20, "character": 0},
                                "end": {"line": 20, "character": 10}
                            },
                            "selectionRange": {
                                "start": {"line": 20, "character": 0},
                                "end": {"line": 20, "character": 8}
                            }
                        },
                        {
                            "name": "ChildTwo",
                            "kind": 23,
                            "uri": source_uri,
                            "range": {
                                "start": {"line": 30, "character": 0},
                                "end": {"line": 30, "character": 10}
                            },
                            "selectionRange": {
                                "start": {"line": 30, "character": 0},
                                "end": {"line": 30, "character": 8}
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

    let harness = start_harness(scenario, serde_json::json!({})).await;
    let uri = file_uri(&harness.source_path);

    let prepared = expect_ok(
        &harness,
        harness
            .client
            .prepare_call_hierarchy(&uri, Position::new(3, 7))
            .await,
    )
    .await;
    assert_eq!(prepared.len(), 1);
    assert_eq!(prepared[0].name, "call_root");

    let incoming = expect_ok(
        &harness,
        harness
            .client
            .incoming_calls(prepared[0].clone())
            .await,
    )
    .await;
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].from.name, "call_root");

    let outgoing = expect_ok(
        &harness,
        harness
            .client
            .outgoing_calls(prepared[0].clone())
            .await,
    )
    .await;
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].to.name, "callee");

    let prepared_types = expect_ok(
        &harness,
        harness
            .client
            .prepare_type_hierarchy(&uri, Position::new(3, 7))
            .await,
    )
    .await;
    assert_eq!(prepared_types.len(), 1);
    assert_eq!(prepared_types[0].name, "TypeRoot");

    let supertypes = expect_ok(
        &harness,
        harness
            .client
            .supertypes(prepared_types[0].clone())
            .await,
    )
    .await;
    assert_eq!(supertypes.len(), 1);
    assert_eq!(supertypes[0].name, "BaseType");

    let subtypes = expect_ok(
        &harness,
        harness
            .client
            .subtypes(prepared_types[0].clone())
            .await,
    )
    .await;
    assert_eq!(subtypes.len(), 2);
    assert_eq!(subtypes[0].name, "ChildOne");
    assert_eq!(subtypes[1].name, "ChildTwo");

    assert_eq!(harness.client.pending_request_count().await, 0);
    assert!(matches!(
        harness.client.transport_state_snapshot().await,
        ClientTransportSnapshot::Running
    ));

    harness.shutdown().await.expect("shutdown should succeed");
}
