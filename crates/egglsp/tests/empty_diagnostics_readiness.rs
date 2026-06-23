//! Pass 7 — Empty-diagnostics readiness tests.
//!
//! The previous test in `real_server_smoke.rs` named
//! `empty_diagnostics_readiness_passes` was actually testing
//! the *missing* notifications branch (the test process was
//! `/bin/sh`, not an LSP server, so no `publishDiagnostics`
//! notification was observed). That test was renamed to
//! `missing_diagnostics_readiness_times_out`.
//!
//! The new tests in this file use the scripted fake LSP server
//! to prove two distinct readiness contracts:
//!
//! 1. `wait_for_first_diagnostics` returns `true` when the
//!    server publishes a `publishDiagnostics` notification
//!    with an empty `diagnostics` array (the "empty
//!    diagnostics" case).
//! 2. `wait_for_first_diagnostics` returns `false` when the
//!    server does NOT publish any `publishDiagnostics`
//!    notification within the timeout window (the "missing
//!    diagnostics" case, equivalent to the renamed
//!    `missing_diagnostics_readiness_times_out` test but
//!    driven through the fake server rather than `/bin/sh`).

#![cfg(feature = "lsp-test-support")]

use std::time::Duration;

use common::ProductionClientHarness;
use egglsp::LspClientOptions;
use serde_json::json;

mod common;

/// Build a scenario JSON that the scripted fake LSP server
/// will execute. The server publishes a `publishDiagnostics`
/// notification with an empty `diagnostics` array in
/// response to `textDocument/didOpen`. The server then waits
/// for `shutdown`/`exit`.
fn empty_diagnostics_scenario(root_uri: &str, source_uri: &str) -> serde_json::Value {
    json!({
        "name": "empty_diagnostics_readiness",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": { "type": "Number" },
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "processId": { "type": "Number" },
                        "rootUri": root_uri,
                    }
                },
                "then": [
                    {
                        "type": "RespondResult",
                        "result": {
                            "capabilities": {}
                        }
                    }
                ]
            },
            { "type": "ExpectNotification", "method": "initialized", "then": [] },
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
                        }
                    }
                },
                "then": [
                    {
                        "type": "SendNotification",
                        "method": "textDocument/publishDiagnostics",
                        "params": {
                            "uri": source_uri,
                            "diagnostics": []
                        }
                    }
                ]
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{ "type": "RespondResult", "result": null }]
            },
            { "type": "ExpectNotification", "method": "exit", "then": [] }
        ],
        "exit": { "type": "ExitCode", "code": 0 },
        "strict": true
    })
}

/// Pass 7 — When the server publishes a
/// `publishDiagnostics` notification with an empty
/// `diagnostics` array, `wait_for_first_diagnostics` MUST
/// return `true`. The cache must contain an entry with an
/// empty vector.
#[tokio::test]
async fn empty_publish_diagnostics_satisfies_readiness() {
    let harness = ProductionClientHarness::start(
        empty_diagnostics_scenario("__ROOT_URI__", "__SOURCE_URI__"),
        LspClientOptions::default(),
        serde_json::Value::Null,
    )
    .await
    .expect("harness start");

    // Drive a didOpen through the client (not the service)
    // because we are testing the LspClient readiness primitive
    // directly.
    let client = harness.client.clone();
    let source_uri = format!(
        "file://{}",
        harness
            .source_path
            .to_string_lossy()
            .trim_start_matches('/')
    );
    let did_open = json!({
        "textDocument": {
            "uri": source_uri,
            "languageId": "rust",
            "version": 1,
            "text": "pub fn harness_marker() {}\n"
        }
    });
    client
        .send_notification("textDocument/didOpen", did_open)
        .await
        .expect("didOpen send");

    // The fake server publishes empty diagnostics in
    // response. `wait_for_first_diagnostics` must return true.
    let passed = client
        .wait_for_first_diagnostics(Duration::from_secs(3))
        .await;
    assert!(
        passed,
        "wait_for_first_diagnostics must return true after empty publishDiagnostics"
    );

    // Clean up.
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        let _ = client.shutdown().await;
        let _ = client.wait_for_child_exit(Duration::from_secs(5)).await;
    })
    .await;
    let _ = harness.shutdown().await;
}

/// Pass 7 — When the server does NOT publish any
/// `publishDiagnostics` notification, the readiness helper
/// times out and returns `false`. Distinct from the empty-
/// diagnostics case.
#[tokio::test]
async fn missing_diagnostics_notification_times_out() {
    let scenario = json!({
        "name": "missing_diagnostics_readiness",
        // emit_progress: false — by default the scripted fake
        // server emits `$/progress begin/end` AND an empty
        // `textDocument/publishDiagnostics` (uri: "file:///dummy")
        // when it sees `initialized`. Disable that for this test
        // so the readiness wait truly receives no diagnostics.
        "emit_progress": false,
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "id": { "type": "Number" },
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "processId": { "type": "Number" },
                        "rootUri": "__ROOT_URI__",
                    }
                },
                "then": [
                    {
                        "type": "RespondResult",
                        "result": { "capabilities": {} }
                    }
                ]
            },
            { "type": "ExpectNotification", "method": "initialized", "then": [] },
            {
                "type": "ExpectNotification",
                "method": "textDocument/didOpen",
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "shutdown",
                "then": [{ "type": "RespondResult", "result": null }]
            },
            { "type": "ExpectNotification", "method": "exit", "then": [] }
        ],
        "exit": { "type": "ExitCode", "code": 0 },
        "strict": true
    });

    let harness = ProductionClientHarness::start(
        scenario,
        LspClientOptions::default(),
        serde_json::Value::Null,
    )
    .await
    .expect("harness start");

    let client = harness.client.clone();
    let source_uri = format!(
        "file://{}",
        harness
            .source_path
            .to_string_lossy()
            .trim_start_matches('/')
    );
    let did_open = json!({
        "textDocument": {
            "uri": source_uri,
            "languageId": "rust",
            "version": 1,
            "text": "pub fn harness_marker() {}\n"
        }
    });
    client
        .send_notification("textDocument/didOpen", did_open)
        .await
        .expect("didOpen send");

    // The fake server accepts didOpen but does NOT publish
    // diagnostics. `wait_for_first_diagnostics` must return
    // false within the timeout.
    let passed = client
        .wait_for_first_diagnostics(Duration::from_millis(500))
        .await;
    assert!(
        !passed,
        "wait_for_first_diagnostics must return false when no notification arrives"
    );

    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        let _ = client.shutdown().await;
        let _ = client.wait_for_child_exit(Duration::from_secs(5)).await;
    })
    .await;
    let _ = harness.shutdown().await;
}
