//! End-to-end managed document synchronization against the fake stdio LSP.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use codegg::lsp::service::LspService;
use egglsp::{LspConfig, LspRule};
use serde_json::json;
use tempfile::TempDir;

const INITIAL: &str = "pub fn original() {}\n";
const EDITED: &str = "pub fn unsaved_editor_text() {}\n";

fn fake_server() -> PathBuf {
    option_env!("CARGO_BIN_EXE_codegg-lsp-test-server")
        .map(PathBuf::from)
        .expect("build with lsp-test-support")
}

fn config(scenario: &Path, transcript: &Path) -> LspConfig {
    let env = HashMap::from([
        (
            "CODEGG_FAKE_LSP_SCENARIO".into(),
            scenario.display().to_string(),
        ),
        (
            "CODEGG_FAKE_LSP_TRANSCRIPT".into(),
            transcript.display().to_string(),
        ),
    ]);
    LspConfig::Rules(HashMap::from([(
        "rust-analyzer".into(),
        LspRule::Active {
            command: vec![fake_server().display().to_string()],
            extensions: Some(vec!["rs".into()]),
            disabled: None,
            env: Some(env),
            initialization: None,
            workspace_configuration: None,
            restart: None,
        },
    )]))
}

#[tokio::test(flavor = "current_thread")]
async fn managed_document_replay_and_disk_refresh_keep_unsaved_text() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='doc-sync'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    let file = root.join("src/lib.rs");
    std::fs::write(&file, INITIAL).unwrap();
    let scenario_path = root.join("scenario.json");
    let transcript = root.join("transcript.jsonl");
    std::fs::write(&scenario_path, serde_json::to_vec(&json!({
        "name": "managed_document_sync",
        "steps": [
            {"type":"ExpectRequest","method":"initialize","id":{"type":"Number"},"params":{"type":"ObjectContains","value":{"processId":{"type":"Number"},"rootUri":{"type":"String"},"initializationOptions":{"type":"Null"}}},"then":[{"type":"RespondResult","result":{"capabilities":{"hoverProvider":true}}}]},
            {"type":"ExpectNotification","method":"initialized","then":[]},
            {"type":"ExpectNotification","method":"textDocument/didOpen","then":[]},
            {"type":"ExpectNotification","method":"textDocument/didChange","then":[]},
            {"type":"ExpectNotification","method":"textDocument/didClose","then":[]},
            {"type":"ExpectRequest","method":"shutdown","then":[{"type":"RespondResult","result":null}]},
            {"type":"ExpectNotification","method":"exit","then":[]}
        ],
        "exit":{"type":"ExitCode","code":0},
        "strict":true
    })).unwrap()).unwrap();

    let service = LspService::new_arc(config(&scenario_path, &transcript));
    service
        .set_managed_document(&file, INITIAL, false)
        .await
        .unwrap();
    service
        .set_managed_document(&file, EDITED, true)
        .await
        .unwrap();
    assert!(service.is_managed_document_dirty(&file).await);
    // Disk still contains INITIAL; semantic preparation must not issue a
    // disk-derived didChange over the canonical editor snapshot.
    service.ensure_file_open_from_disk(&file).await.unwrap();
    assert!(service.is_managed_document_dirty(&file).await);
    if let Err(error) = service.close_file(&file).await {
        panic!(
            "close failed: {error}; transcript: {}",
            std::fs::read_to_string(&transcript).unwrap_or_default()
        );
    }
    service.shutdown_all().await;
    let transcript = std::fs::read_to_string(transcript).unwrap();
    assert_eq!(
        transcript.matches("textDocument/didChange").count(),
        1,
        "{transcript}"
    );
    assert!(transcript.contains("unsaved_editor_text"), "{transcript}");
}
