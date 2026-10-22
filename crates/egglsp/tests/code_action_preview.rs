//! The warm-only code-action preview: **one** request, **many** previews.
//!
//! The design this file defends is the one the editor's code-action read
//! rests on. The alternatives were a summaries-then-a-second-request flow
//! (whose action *index* is only meaningful for the list the server returned
//! in that moment) and a per-action request (one round trip per action the
//! user might not pick). One request, with every action's edit previewed
//! locally, removes the index and removes the round trips.
//!
//! The assertions are deliberately narrow: the *number of requests the
//! server saw* is read out of the transcript, so "one request" is a
//! measurement rather than a claim about the code's shape.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egglsp::lsp_types::{Position, Range};
use egglsp::{LspConfig, LspError, LspOperations, LspRule, LspService};
use serde_json::json;
use tempfile::TempDir;
use tokio::time::sleep;
use url::Url;

#[path = "common/harness.rs"]
mod harness;

use harness::FakeLspHarness;

const INITIAL_SOURCE_TEXT: &str = "pub fn harness_marker() {}\n";

struct CodeActionHarness {
    #[allow(dead_code)]
    tempdir: TempDir,
    root: PathBuf,
    source_path: PathBuf,
    transcript_path: PathBuf,
    service: Arc<LspService>,
}

impl CodeActionHarness {
    fn start<F>(build_scenario: F) -> Result<Self, LspError>
    where
        F: FnOnce(&str) -> serde_json::Value,
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
name = "egglsp-code-action-test"
version = "0.1.0"
edition = "2021"
"#,
        )?;
        std::fs::write(&source_path, INITIAL_SOURCE_TEXT)?;

        let source_uri = path_to_uri(&source_path);
        let scenario = build_scenario(&source_uri);
        std::fs::write(&scenario_path, serde_json::to_string_pretty(&scenario)?)?;

        let service = LspService::new_arc(make_service_config(&scenario_path, &transcript_path));

        Ok(Self {
            tempdir,
            root,
            source_path,
            transcript_path,
            service,
        })
    }

    /// How many `textDocument/codeAction` requests the server actually saw.
    ///
    /// Read out of the transcript rather than counted in the client, so the
    /// number is the server's observation and not the client's intention.
    async fn code_action_request_count(&self) -> usize {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(contents) = std::fs::read_to_string(&self.transcript_path) {
                let count = contents.matches("textDocument/codeAction").count();
                if count > 0 {
                    return count;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return 0;
            }
            sleep(Duration::from_millis(20)).await;
        }
    }

    /// The whole transcript, for a failure message.
    fn transcript(&self) -> String {
        std::fs::read_to_string(&self.transcript_path)
            .unwrap_or_else(|err| format!("(transcript unavailable: {err})"))
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
            restart: None,
        },
    );

    LspConfig::Rules(rules)
}

fn path_to_uri(path: &Path) -> String {
    Url::from_file_path(path)
        .expect("invalid file path")
        .to_string()
}

fn init_result_capabilities() -> serde_json::Value {
    json!({
        "capabilities": {
            "codeActionProvider": true,
            "hoverProvider": true
        }
    })
}

fn cursor_range() -> Range {
    let point = Position {
        line: 0,
        character: 4,
    };
    Range {
        start: point,
        end: point,
    }
}

/// A scenario that offers one editable action and one command-only action.
fn scenario_with_two_actions(source_uri: &str) -> serde_json::Value {
    json!({
        "name": "code_action_previews_one_request_many_previews",
        "steps": [
            {
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{
                    "type": "RespondResult",
                    "result": init_result_capabilities()
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
                            "value": {"uri": source_uri}
                        }
                    }
                },
                "then": []
            },
            {
                "type": "ExpectRequest",
                "method": "textDocument/codeAction",
                "params": {
                    "type": "ObjectContains",
                    "value": {
                        "textDocument": {
                            "type": "ObjectContains",
                            "value": {"uri": source_uri}
                        },
                        "range": {
                            "type": "ObjectContains",
                            "value": {
                                "start": {"line": 0, "character": 4},
                                "end": {"line": 0, "character": 4}
                            }
                        }
                    }
                },
                "then": [{
                    "type": "RespondResult",
                    "result": [
                        {
                            "title": "Add the missing import",
                            "kind": "quickfix",
                            "edit": {
                                "changes": {
                                    source_uri: [{
                                        "range": {
                                            "start": {"line": 0, "character": 0},
                                            "end": {"line": 0, "character": 3}
                                        },
                                        "newText": "pub "
                                    }]
                                }
                            }
                        },
                        {
                            "title": "Run the test suite",
                            "kind": "source",
                            "command": {
                                "title": "Run the test suite",
                                "command": "cargo",
                                "arguments": ["test"]
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
    })
}

#[tokio::test]
async fn one_request_yields_many_previews_and_a_command_only_action_yields_none() {
    let harness = CodeActionHarness::start(scenario_with_two_actions).expect("harness");

    // Warm the client the way the daemon does: the key is resolved once, and
    // the code-action read is taken against that same key.
    let (key, _root) = harness
        .service
        .get_or_create_client_for_file(&harness.source_path)
        .await
        .expect("the client starts");

    let operations = LspOperations::new(harness.service.clone());
    let entries = operations
        .code_action_previews_for_warm_client(
            &key,
            &harness.source_path,
            cursor_range(),
            None,
            50,
            Some(harness.root.as_path()),
        )
        .await
        .expect("the code-action read succeeds");

    assert_eq!(
        entries.len(),
        2,
        "both offered actions are listed, so the reader sees what the server offered"
    );

    let editable = &entries[0];
    assert_eq!(editable.title, "Add the missing import");
    assert_eq!(editable.kind.as_deref(), Some("quickfix"));
    assert!(editable.has_edit);
    assert!(!editable.has_command);
    let preview = editable
        .preview
        .as_ref()
        .expect("an editable action with a previewable edit gets one");
    assert_eq!(preview.affected_files.len(), 1);
    assert!(!preview.truncated, "a small edit is never truncated");

    let command_only = &entries[1];
    assert_eq!(command_only.title, "Run the test suite");
    assert!(!command_only.has_edit);
    assert!(
        command_only.has_command,
        "the action carried a command, and the fact is carried rather than guessed"
    );
    assert!(
        command_only.preview.is_none(),
        "a command-bearing action never becomes a preview: the surface never runs commands"
    );

    let requests = harness.code_action_request_count().await;
    assert_eq!(
        requests,
        1,
        "one request, and the extra previews were computed locally\n{}",
        harness.transcript()
    );
}

#[tokio::test]
async fn a_mixed_action_carries_a_command_and_gets_no_preview() {
    // The LSP specification has the client apply an action's edit *and then*
    // its command. Previewing the edit of a mixed action would be previewing
    // half of what the action means, so the mixed action is refused too.
    let harness = CodeActionHarness::start(|source_uri| {
        let mut scenario = scenario_with_two_actions(source_uri);
        let result = scenario["steps"][3]["then"][0]["result"]
            .as_array_mut()
            .expect("the step has a result array");
        result[0]["command"] = json!({
            "title": "Add the missing import",
            "command": "cargo",
            "arguments": ["fix"]
        });
        scenario
    })
    .expect("harness");

    let (key, _root) = harness
        .service
        .get_or_create_client_for_file(&harness.source_path)
        .await
        .expect("the client starts");

    let operations = LspOperations::new(harness.service.clone());
    let entries = operations
        .code_action_previews_for_warm_client(
            &key,
            &harness.source_path,
            cursor_range(),
            None,
            50,
            Some(harness.root.as_path()),
        )
        .await
        .expect("the code-action read succeeds");

    let mixed = &entries[0];
    assert!(mixed.has_edit, "the action did carry an edit");
    assert!(mixed.has_command, "and it also carried a command");
    assert!(
        mixed.preview.is_none(),
        "a mixed action is not half-applied: it gets no preview at all"
    );
}
