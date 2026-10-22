//! M003 trajectory: the authorized LSP completion read contract
//! (ADR-0021, Option B).
//!
//! Properties proven here, each of which a plausible-looking implementation
//! would get wrong:
//!
//! 1. A warmed server returns bounded typed candidates with explicit
//!    supported/unsupported edit metadata — snippets, additional edits,
//!    commands, out-of-snapshot ranges and oversized edits stay visible with
//!    a reason and are never silently downgraded to label insertion.
//! 2. A cold read returns `NotReady` **and starts no process**.
//! 3. A denied read (no membership, wrong project) is an *error*, not
//!    `NotReady`.
//! 4. A caller behind the daemon is `Stale`; a synchronized draft is `Ready`
//!    again; an old revision never becomes applicable later.
//! 5. A document the server never saw is `Unsynced`, even with a warm server.
//! 6. Scope confusion (path/attachment mismatch, cross-workspace attachment)
//!    fails closed.
//! 7. No completion read changes document text or disk bytes.
//! 8. Reconnect is an explicit re-query, never a replayed acceptance.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::lsp::service::LspService;
use codegg_core::identity::ProjectId;
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::transport_auth::PersonalTokenStore;
use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::document::{
    DocumentScope, DocumentSnapshotDto, DocumentTextEditDto, DocumentTextRangeDto,
    DocumentTransactionDto,
};
use codegg_protocol::lsp::{
    LspCompletionApplicabilityDto, LspCompletionRequestDto, LspCompletionResultDto,
    LspCompletionStatus, LspCompletionUnsupportedReason,
};
use egglsp::{LspConfig, LspRule};
use serde_json::{json, Value};
use tempfile::TempDir;

const MEMBER: &str = "client-member";

fn fake_server() -> PathBuf {
    option_env!("CARGO_BIN_EXE_codegg-lsp-test-server")
        .map(PathBuf::from)
        .expect("build with lsp-test-support")
}

fn init_step() -> Value {
    json!({
        "type": "ExpectRequest",
        "method": "initialize",
        "then": [{"type": "RespondResult", "result": {"capabilities": {"completionProvider": {}, "hoverProvider": true}}}]
    })
}

fn initialized_step() -> Value {
    json!({"type": "ExpectNotification", "method": "initialized", "then": []})
}

fn did_open_step() -> Value {
    json!({"type": "ExpectNotification", "method": "textDocument/didOpen", "then": []})
}

fn did_change_step() -> Value {
    json!({"type": "ExpectNotification", "method": "textDocument/didChange", "then": []})
}

fn completion_step(result: Value) -> Value {
    json!({
        "type": "ExpectRequest",
        "method": "textDocument/completion",
        "then": [{"type": "RespondResult", "result": result}]
    })
}

fn shutdown_steps() -> Vec<Value> {
    vec![
        json!({
            "type": "ExpectRequest",
            "method": "shutdown",
            "then": [{"type": "RespondResult", "result": null}]
        }),
        json!({"type": "ExpectNotification", "method": "exit", "then": []}),
    ]
}

fn scenario(mut middle: Vec<Value>) -> Value {
    let mut steps = vec![init_step(), initialized_step(), did_open_step()];
    steps.append(&mut middle);
    steps.extend(shutdown_steps());
    json!({
        "name": "lsp-completion",
        "steps": steps,
        "exit": {"type": "ExitCode", "code": 0},
        "strict": true
    })
}

fn service_for(root: &Path, scenario_value: &Value) -> (Arc<LspService>, PathBuf) {
    let scenario_path = root.join("scenario.json");
    let transcript = root.join("transcript.jsonl");
    std::fs::write(&scenario_path, serde_json::to_vec(scenario_value).unwrap()).unwrap();
    let env = HashMap::from([
        (
            "CODEGG_FAKE_LSP_SCENARIO".into(),
            scenario_path.display().to_string(),
        ),
        (
            "CODEGG_FAKE_LSP_TRANSCRIPT".into(),
            transcript.display().to_string(),
        ),
    ]);
    let config = LspConfig::Rules(HashMap::from([(
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
    )]));
    (LspService::new_arc(config), transcript)
}

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    transcript: PathBuf,
    service: Arc<LspService>,
    daemon: CoreDaemon,
    pool: sqlx::SqlitePool,
    workspace_id: String,
    project_id: String,
    session_id: String,
}

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

async fn human_token(team: &TeamStore, name: &str, client_id: &str) -> AuthenticatedPrincipal {
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .unwrap();
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .unwrap();
    tokens
        .verify_for_client(&plaintext, client_id)
        .await
        .unwrap()
}

fn register_client(daemon: &CoreDaemon, client_id: &str, principal: AuthenticatedPrincipal) {
    daemon.clients.register_with_principal(
        client_id.to_owned(),
        format!("{client_id}-name"),
        None,
        principal,
    );
}

async fn local_call(daemon: &CoreDaemon, name: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request(new_request(format!("req-{name}"), request)))
        .await
        .unwrap()
}

async fn call(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{client}-{}", uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

/// One workspace root with a Rust project, a daemon bound to the fake LSP
/// server, and a session whose member client may read.
async fn fixture(file_content: &str, scenario_value: &Value) -> Fixture {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='completion'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), file_content).unwrap();

    let (service, transcript) = service_for(&root, scenario_value);
    let pool = test_pool().await;
    let daemon = CoreDaemon::with_deps(
        CoreRuntimeDeps::new(Some(pool.clone()), None, None).with_lsp_service(service.clone()),
    );

    let workspace_id = match local_call(
        &daemon,
        "ws",
        CoreRequest::WorkspaceRegister {
            root: root.display().to_string(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project_id = match local_call(
        &daemon,
        "proj",
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: "completion".to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_owned(),
            },
        },
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project register failed: {other:?}"),
    };
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES (?, ?, 1, 1, '[]')",
    )
    .bind(&project_id)
    .bind(root.display().to_string())
    .execute(&pool)
    .await
    .unwrap();
    let session_id = format!("session-{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO session (id, project_id, workspace_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, 's', ?, 't', 'v', 1, 1)",
    )
    .bind(&session_id)
    .bind(&project_id)
    .bind(&workspace_id)
    .bind(root.display().to_string())
    .execute(&pool)
    .await
    .unwrap();

    let team = TeamStore::new(pool.clone());
    let member = human_token(&team, "Member", MEMBER).await;
    let principal = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &ProjectId::parse(&project_id).expect("registered project id parses"),
        &principal.id,
        ProjectRole::Contributor,
    )
    .await
    .unwrap();
    register_client(&daemon, MEMBER, member);

    Fixture {
        _temp: temp,
        root,
        transcript,
        service,
        daemon,
        pool,
        workspace_id,
        project_id,
        session_id,
    }
}

async fn open_lib(fixture: &Fixture) -> DocumentSnapshotDto {
    match call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::DocumentOpen {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "src/lib.rs".to_string(),
            scope: DocumentScope::Workspace,
        },
    )
    .await
    {
        CoreResponse::DocumentSnapshot { snapshot, .. } => snapshot,
        other => panic!("document open failed: {other:?}"),
    }
}

async fn complete(
    fixture: &Fixture,
    client: &str,
    path: &str,
    document_id: &str,
    revision: u64,
    line: u32,
    character: u32,
    max_candidates: u32,
) -> CoreResponse {
    call(
        &fixture.daemon,
        client,
        CoreRequest::LspCompletionGet {
            request: LspCompletionRequestDto {
                session_id: fixture.session_id.clone(),
                path: path.to_string(),
                document_id: document_id.to_string(),
                revision,
                line,
                character,
                trigger_character: None,
                max_candidates,
            },
        },
    )
    .await
}

fn as_completion(response: CoreResponse) -> LspCompletionResultDto {
    match response {
        CoreResponse::LspCompletionResult { result } => result,
        other => panic!("expected completion result, got {other:?}"),
    }
}

fn error_code(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error response, got {other:?}"),
    }
}

fn applicable(result: &LspCompletionResultDto, index: usize) -> (u32, u32, u32, u32, &str) {
    match &result.candidates[index].applicability {
        LspCompletionApplicabilityDto::Applicable { edit } => (
            edit.start_line,
            edit.start_character,
            edit.end_line,
            edit.end_character,
            edit.new_text.as_str(),
        ),
        LspCompletionApplicabilityDto::NonApplicable { reason } => {
            panic!("candidate {} must be applicable, got {reason:?}", index)
        }
    }
}

fn non_applicable(result: &LspCompletionResultDto, index: usize) -> LspCompletionUnsupportedReason {
    match &result.candidates[index].applicability {
        LspCompletionApplicabilityDto::NonApplicable { reason } => *reason,
        LspCompletionApplicabilityDto::Applicable { .. } => {
            panic!("candidate {index} must be non-applicable")
        }
    }
}

// ---------------------------------------------------------------------------
// 1. Warmed server: bounded typed completion with explicit applicability
// ---------------------------------------------------------------------------

const LIB_RS: &str = "fn main() {\n    prin\n}\n";

fn main_completion_list(huge_edit: &str) -> Value {
    json!({
        "isIncomplete": true,
        "items": [
            {"label": "println!", "kind": 3, "detail": "macro println!",
             "textEdit": {"range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 8}}, "newText": "println!"}},
            {"label": "print", "kind": 3},
            {"label": "eprintln!", "kind": 3, "insertText": "eprintln!()"},
            {"label": "format!", "kind": 3, "insertTextFormat": 2},
            {"label": "use_log", "kind": 9,
             "additionalTextEdits": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "use log;"}]},
            {"label": "fix_import", "kind": 3,
             "command": {"title": "fix", "command": "do-fix"}},
            {"label": "far_away", "kind": 3,
             "textEdit": {"range": {"start": {"line": 99, "character": 0}, "end": {"line": 99, "character": 1}}, "newText": "y"}},
            {"label": "huge", "kind": 3,
             "textEdit": {"range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 8}}, "newText": huge_edit}},
            {"label": "both", "kind": 3, "insertText": "WRONG",
             "textEdit": {"range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 8}}, "newText": "println!"}}
        ]
    })
}

#[tokio::test(flavor = "current_thread")]
async fn a_warmed_server_returns_typed_candidates_with_explicit_applicability() {
    let huge_edit = "x".repeat(9000);
    let list = main_completion_list(&huge_edit);
    // Two completion steps: the first answer plus the reconnect re-query.
    let fixture = fixture(
        LIB_RS,
        &scenario(vec![completion_step(list.clone()), completion_step(list)]),
    )
    .await;
    let disk_before = std::fs::read(fixture.root.join("src/lib.rs")).unwrap();
    let snapshot = open_lib(&fixture).await;

    let result = as_completion(
        complete(
            &fixture,
            MEMBER,
            "src/lib.rs",
            &snapshot.document_id,
            snapshot.revision,
            1,
            8,
            100,
        )
        .await,
    );
    assert_eq!(result.status, LspCompletionStatus::Ready);
    assert_eq!(result.document_id, snapshot.document_id);
    assert_eq!(result.revision, snapshot.revision);
    assert_eq!(result.synced_revision, Some(snapshot.revision));
    assert!(!result.truncated);
    assert!(result.incomplete, "the server's is_incomplete must survive");

    let labels: Vec<_> = result
        .candidates
        .iter()
        .map(|candidate| candidate.label.as_str())
        .collect();
    assert_eq!(
        labels,
        [
            "println!",
            "print",
            "eprintln!",
            "format!",
            "use_log",
            "fix_import",
            "far_away",
            "huge",
            "both"
        ],
        "server order is preserved, no client-side sort"
    );

    // An explicit in-snapshot textEdit is applicable with its range.
    assert_eq!(applicable(&result, 0), (1, 4, 1, 8, "println!"));
    // A bare label inserts at the cursor: the spec default, not a guess.
    assert_eq!(applicable(&result, 1), (1, 8, 1, 8, "print"));
    // insertText beats the label for cursor insertion.
    assert_eq!(applicable(&result, 2), (1, 8, 1, 8, "eprintln!()"));
    // The server's range beats insertText: label vs textEdit is no contest.
    assert_eq!(applicable(&result, 8), (1, 4, 1, 8, "println!"));

    // Nothing unsafe is silently downgraded: each stays visible with its
    // reason.
    assert_eq!(
        non_applicable(&result, 3),
        LspCompletionUnsupportedReason::Snippet
    );
    assert_eq!(
        non_applicable(&result, 4),
        LspCompletionUnsupportedReason::AdditionalEdits
    );
    assert_eq!(
        non_applicable(&result, 5),
        LspCompletionUnsupportedReason::Command
    );
    assert_eq!(
        non_applicable(&result, 6),
        LspCompletionUnsupportedReason::OutOfRange
    );
    assert_eq!(
        non_applicable(&result, 7),
        LspCompletionUnsupportedReason::EditTooLarge
    );

    // A reconnect is an explicit re-query on a new transport, never a replay:
    // the same request from a fresh client answers Ready again. The fresh
    // principal needs its own grant: authorization follows the principal,
    // not the transport.
    let team = TeamStore::new(fixture.pool.clone());
    let member = human_token(&team, "Member2", "client-member-2").await;
    let principal = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &ProjectId::parse(&fixture.project_id).expect("project id parses"),
        &principal.id,
        ProjectRole::Contributor,
    )
    .await
    .unwrap();
    register_client(&fixture.daemon, "client-member-2", member);
    let again = as_completion(
        complete(
            &fixture,
            "client-member-2",
            "src/lib.rs",
            &snapshot.document_id,
            snapshot.revision,
            1,
            8,
            100,
        )
        .await,
    );
    assert_eq!(again.status, LspCompletionStatus::Ready);

    // No read changed document text or disk bytes.
    let disk_after = std::fs::read(fixture.root.join("src/lib.rs")).unwrap();
    assert_eq!(disk_before, disk_after);
    assert_eq!(again.revision, snapshot.revision);

    fixture.service.shutdown_all().await;
    let transcript = std::fs::read_to_string(&fixture.transcript).unwrap();
    assert!(
        transcript.contains("textDocument/completion"),
        "the query must actually reach the server: {transcript}"
    );
}

// ---------------------------------------------------------------------------
// 2. UTF-16 surrogate boundaries at the daemon boundary
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn surrogate_split_ranges_are_out_of_range_at_the_daemon() {
    // "// " is 3 units, the emoji is 2, "x" is 1: x spans units 5..6.
    let content = "// \u{1F600}x\nfn main() {}\n";
    let list = json!({
        "isIncomplete": false,
        "items": [
            {"label": "y", "kind": 3,
             "textEdit": {"range": {"start": {"line": 0, "character": 5}, "end": {"line": 0, "character": 6}}, "newText": "y"}},
            {"label": "split", "kind": 3,
             "textEdit": {"range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 4}}, "newText": "z"}}
        ]
    });
    let fixture = fixture(content, &scenario(vec![completion_step(list)])).await;
    let snapshot = open_lib(&fixture).await;

    let result = as_completion(
        complete(
            &fixture,
            MEMBER,
            "src/lib.rs",
            &snapshot.document_id,
            snapshot.revision,
            0,
            6,
            100,
        )
        .await,
    );
    assert_eq!(result.status, LspCompletionStatus::Ready);
    assert_eq!(result.candidates.len(), 2);
    assert_eq!(applicable(&result, 0), (0, 5, 0, 6, "y"));
    assert_eq!(
        non_applicable(&result, 1),
        LspCompletionUnsupportedReason::OutOfRange
    );

    fixture.service.shutdown_all().await;
}

// ---------------------------------------------------------------------------
// 3. Count and byte caps are measured facts
// ---------------------------------------------------------------------------

fn many_items(count: usize, edit: &str) -> Value {
    let items: Vec<Value> = (0..count)
        .map(|index| {
            json!({"label": format!("item-{index:03}"), "kind": 3,
                   "textEdit": {"range": {"start": {"line": 1, "character": 4}, "end": {"line": 1, "character": 8}}, "newText": edit}})
        })
        .collect();
    json!({"isIncomplete": false, "items": items})
}

#[tokio::test(flavor = "current_thread")]
async fn one_hundred_one_candidates_truncate_to_one_hundred() {
    let fixture = fixture(
        LIB_RS,
        &scenario(vec![completion_step(many_items(101, "x"))]),
    )
    .await;
    let snapshot = open_lib(&fixture).await;

    let result = as_completion(
        complete(
            &fixture,
            MEMBER,
            "src/lib.rs",
            &snapshot.document_id,
            snapshot.revision,
            1,
            8,
            100,
        )
        .await,
    );
    assert_eq!(result.status, LspCompletionStatus::Ready);
    assert_eq!(result.candidates.len(), 100);
    assert!(result.truncated, "the 101st item must be reported");
    assert_eq!(result.candidates[0].label, "item-000");
    assert_eq!(result.candidates[99].label, "item-099");

    fixture.service.shutdown_all().await;
}

#[tokio::test(flavor = "current_thread")]
async fn the_result_byte_budget_truncates_before_the_count_cap() {
    // 100 edits of 3000 bytes each exceed the 256 KiB result budget while
    // every edit stays under the 8 KiB per-edit bound.
    let fixture = fixture(
        LIB_RS,
        &scenario(vec![completion_step(many_items(100, &"y".repeat(3000)))]),
    )
    .await;
    let snapshot = open_lib(&fixture).await;

    let result = as_completion(
        complete(
            &fixture,
            MEMBER,
            "src/lib.rs",
            &snapshot.document_id,
            snapshot.revision,
            1,
            8,
            100,
        )
        .await,
    );
    assert_eq!(result.status, LspCompletionStatus::Ready);
    assert!(result.truncated, "the byte budget must fire");
    assert!(
        result.candidates.len() < 100,
        "byte truncation cuts below the count cap, got {}",
        result.candidates.len()
    );
    assert!(!result.candidates.is_empty());

    fixture.service.shutdown_all().await;
}

// ---------------------------------------------------------------------------
// 4. Cold server: NotReady and zero starts
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn a_cold_read_is_not_ready_and_starts_no_process() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='cold'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), LIB_RS).unwrap();

    // A service that cannot launch anything: a cold read must stay cold.
    let service = LspService::new_arc(LspConfig::Disabled(true));
    let pool = test_pool().await;
    let daemon = CoreDaemon::with_deps(
        CoreRuntimeDeps::new(Some(pool.clone()), None, None).with_lsp_service(service.clone()),
    );
    let workspace_id = match local_call(
        &daemon,
        "ws",
        CoreRequest::WorkspaceRegister {
            root: root.display().to_string(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project_id = match local_call(
        &daemon,
        "proj",
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: "cold".to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_owned(),
            },
        },
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project register failed: {other:?}"),
    };
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES (?, ?, 1, 1, '[]')",
    )
    .bind(&project_id)
    .bind(root.display().to_string())
    .execute(&pool)
    .await
    .unwrap();
    let session_id = format!("session-{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO session (id, project_id, workspace_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, 's', ?, 't', 'v', 1, 1)",
    )
    .bind(&session_id)
    .bind(&project_id)
    .bind(&workspace_id)
    .bind(root.display().to_string())
    .execute(&pool)
    .await
    .unwrap();
    let team = TeamStore::new(pool.clone());
    let member = human_token(&team, "Member", MEMBER).await;
    let principal = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &ProjectId::parse(&project_id).expect("registered project id parses"),
        &principal.id,
        ProjectRole::Contributor,
    )
    .await
    .unwrap();
    register_client(&daemon, MEMBER, member);

    let snapshot = match call(
        &daemon,
        MEMBER,
        CoreRequest::DocumentOpen {
            project_id: project_id.clone(),
            workspace_id: workspace_id.clone(),
            relative_path: "src/lib.rs".to_string(),
            scope: DocumentScope::Workspace,
        },
    )
    .await
    {
        CoreResponse::DocumentSnapshot { snapshot, .. } => snapshot,
        other => panic!("document open failed: {other:?}"),
    };

    let response = call(
        &daemon,
        MEMBER,
        CoreRequest::LspCompletionGet {
            request: LspCompletionRequestDto {
                session_id: session_id.clone(),
                path: "src/lib.rs".to_string(),
                document_id: snapshot.document_id.clone(),
                revision: snapshot.revision,
                line: 1,
                character: 8,
                trigger_character: None,
                max_candidates: 100,
            },
        },
    )
    .await;
    let result = as_completion(response);
    assert_eq!(result.status, LspCompletionStatus::NotReady);
    assert!(result.candidates.is_empty());
    assert!(
        service.client_keys().await.is_empty(),
        "a cold completion read must not start a language server"
    );
    service.shutdown_all().await;
}

// ---------------------------------------------------------------------------
// 5. Stale callers, synchronized drafts, overlapping requests
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn stale_revisions_cannot_become_applicable_and_drafts_resync() {
    let list = json!({
        "isIncomplete": false,
        "items": [{"label": "print", "kind": 3}]
    });
    // One query only: the synchronized draft. Stale answers never query.
    let fixture = fixture(
        LIB_RS,
        &scenario(vec![did_change_step(), completion_step(list)]),
    )
    .await;
    let snapshot = open_lib(&fixture).await;
    let revision = snapshot.revision;

    // A caller from the future is stale: the echo tells it where HEAD is.
    let future = as_completion(
        complete(
            &fixture,
            MEMBER,
            "src/lib.rs",
            &snapshot.document_id,
            revision + 99,
            1,
            8,
            100,
        )
        .await,
    );
    assert_eq!(future.status, LspCompletionStatus::Stale);
    assert_eq!(future.revision, revision);
    assert!(future.candidates.is_empty());

    // A real edit advances the daemon revision and re-pushes to the server.
    let lease = match call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::DocumentWriterAcquire {
            project_id: fixture.project_id.clone(),
            document_id: snapshot.document_id.clone(),
        },
    )
    .await
    {
        CoreResponse::DocumentWriterLease { writer_lease, .. } => writer_lease,
        other => panic!("writer acquire failed: {other:?}"),
    };
    // "    prin" lives at bytes 15..19 of the initial content.
    match call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::DocumentChange {
            project_id: fixture.project_id.clone(),
            document_id: snapshot.document_id.clone(),
            writer_lease: lease,
            base_revision: revision,
            change_id: "chg-1".to_string(),
            transaction: DocumentTransactionDto {
                edits: vec![DocumentTextEditDto {
                    range: DocumentTextRangeDto { start: 15, end: 19 },
                    insert: "print".to_string(),
                }],
            },
        },
    )
    .await
    {
        CoreResponse::DocumentChanged { revision: next, .. } => {
            assert_eq!(next, revision + 1);
        }
        other => panic!("document change failed: {other:?}"),
    }

    // The synchronized draft is Ready again, bound to the new revision.
    let synced = as_completion(
        complete(
            &fixture,
            MEMBER,
            "src/lib.rs",
            &snapshot.document_id,
            revision + 1,
            1,
            9,
            100,
        )
        .await,
    );
    assert_eq!(synced.status, LspCompletionStatus::Ready);
    assert_eq!(synced.revision, revision + 1);
    assert_eq!(synced.synced_revision, Some(revision + 1));

    // The overlapping old request arriving late is Stale, never applicable:
    // a response for a superseded revision cannot become a suggestion.
    let late = as_completion(
        complete(
            &fixture,
            MEMBER,
            "src/lib.rs",
            &snapshot.document_id,
            revision,
            1,
            8,
            100,
        )
        .await,
    );
    assert_eq!(late.status, LspCompletionStatus::Stale);
    assert_eq!(late.revision, revision + 1);
    assert!(late.candidates.is_empty());

    // The edit never touched the disk: completion-adjacent writes do not
    // happen, and the change itself stayed a dirty buffer.
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("src/lib.rs")).unwrap(),
        LIB_RS
    );

    fixture.service.shutdown_all().await;
}

// ---------------------------------------------------------------------------
// 6. Unsynced: warm server, document it never saw
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn a_document_the_server_never_saw_is_unsynced_despite_a_warm_server() {
    // One completion step for the Rust control below; the Unsynced answer
    // itself must never query the server.
    let rs_list = json!({
        "isIncomplete": false,
        "items": [{"label": "print", "kind": 3}]
    });
    let fixture = fixture(LIB_RS, &scenario(vec![completion_step(rs_list)])).await;
    std::fs::write(fixture.root.join("notes.txt"), "plain notes\n").unwrap();

    // A file type with no server rule fails its push: degraded, no witness.
    let degraded = match call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::DocumentOpen {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            relative_path: "notes.txt".to_string(),
            scope: DocumentScope::Workspace,
        },
    )
    .await
    {
        CoreResponse::DocumentSnapshot {
            snapshot,
            lsp_degraded,
            ..
        } => {
            assert!(lsp_degraded, "the txt push must fail");
            snapshot
        }
        other => panic!("document open failed: {other:?}"),
    };

    // Warm the server through the Rust document.
    let rs = open_lib(&fixture).await;

    // The server is warm now, but it never saw notes.txt: Unsynced, with the
    // binding echoed so the client can wait rather than guess.
    let result = as_completion(
        complete(
            &fixture,
            MEMBER,
            "notes.txt",
            &degraded.document_id,
            degraded.revision,
            0,
            0,
            100,
        )
        .await,
    );
    assert_eq!(result.status, LspCompletionStatus::Unsynced);
    assert_eq!(result.document_id, degraded.document_id);
    assert_eq!(result.revision, degraded.revision);
    assert_eq!(result.synced_revision, None);
    assert!(result.candidates.is_empty());
    // The Rust control answers Ready against the same warm server, proving
    // the Unsynced verdict is about the document, not the server.
    let control = as_completion(
        complete(
            &fixture,
            MEMBER,
            "src/lib.rs",
            &rs.document_id,
            rs.revision,
            1,
            8,
            100,
        )
        .await,
    );
    assert_eq!(control.status, LspCompletionStatus::Ready);
    assert_eq!(control.synced_revision, Some(rs.revision));

    fixture.service.shutdown_all().await;
    let transcript = std::fs::read_to_string(&fixture.transcript).unwrap();
    assert_eq!(
        transcript.matches("textDocument/completion").count(),
        1,
        "only the Rust control may query; the Unsynced answer must not: {transcript}"
    );
    assert!(
        !transcript.contains("notes.txt"),
        "the server must never hear about the unsynced document: {transcript}"
    );
}

// ---------------------------------------------------------------------------
// 7. Scope confusion and authorization fail closed
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn scope_confusion_and_denied_callers_fail_closed() {
    let fixture = fixture(LIB_RS, &scenario(vec![])).await;
    let snapshot = open_lib(&fixture).await;

    // Path and attachment locators disagree: a client bug, rejected before
    // any server contact.
    let mismatch = complete(
        &fixture,
        MEMBER,
        "src/other.rs",
        &snapshot.document_id,
        snapshot.revision,
        1,
        8,
        100,
    )
    .await;
    assert_eq!(error_code(&mismatch), "invalid_lsp_read");

    // Unknown attachment: indistinguishable from absent.
    let unknown = complete(&fixture, MEMBER, "src/lib.rs", "doc-nope", 0, 1, 8, 100).await;
    assert_eq!(error_code(&unknown), "lsp_read_not_found");

    // Malformed shapes never reach a server either.
    let empty_path = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspCompletionGet {
            request: LspCompletionRequestDto {
                session_id: fixture.session_id.clone(),
                path: String::new(),
                document_id: snapshot.document_id.clone(),
                revision: snapshot.revision,
                line: 1,
                character: 8,
                trigger_character: None,
                max_candidates: 100,
            },
        },
    )
    .await;
    assert_eq!(error_code(&empty_path), "invalid_lsp_read");
    let zero_cap = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspCompletionGet {
            request: LspCompletionRequestDto {
                session_id: fixture.session_id.clone(),
                path: "src/lib.rs".to_string(),
                document_id: snapshot.document_id.clone(),
                revision: snapshot.revision,
                line: 1,
                character: 8,
                trigger_character: None,
                max_candidates: 0,
            },
        },
    )
    .await;
    assert_eq!(error_code(&zero_cap), "invalid_lsp_read");

    // A caller with no membership is denied: an error, never NotReady.
    let team = TeamStore::new(fixture.pool.clone());
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&fixture.daemon, "client-outsider", outsider);
    let denied = complete(
        &fixture,
        "client-outsider",
        "src/lib.rs",
        &snapshot.document_id,
        snapshot.revision,
        1,
        8,
        100,
    )
    .await;
    assert_eq!(
        error_code(&denied),
        "authorization_denied",
        "an unauthorized read is denied, not NotReady: {denied:?}"
    );

    // A member of another project is denied on this session too. The second
    // project lives under its own workspace: project registration is
    // idempotent per workspace, so reusing this workspace would collapse
    // back onto the member's own project and prove nothing.
    let temp_b = TempDir::new().unwrap();
    let root_b = temp_b.path().to_path_buf();
    let workspace_b = match local_call(
        &fixture.daemon,
        "ws-b",
        CoreRequest::WorkspaceRegister {
            root: root_b.display().to_string(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("second workspace register failed: {other:?}"),
    };
    let other_project = match local_call(
        &fixture.daemon,
        "proj-b",
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_b.clone(),
                display_name: "other".to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_owned(),
            },
        },
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("second project register failed: {other:?}"),
    };
    assert_ne!(
        other_project, fixture.project_id,
        "the wrong-project probe needs a genuinely different project"
    );
    let foreign = human_token(&team, "Foreign", "client-foreign").await;
    let foreign_principal = team
        .get_principal(foreign.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &ProjectId::parse(&other_project).expect("second project id parses"),
        &foreign_principal.id,
        ProjectRole::Contributor,
    )
    .await
    .unwrap();
    register_client(&fixture.daemon, "client-foreign", foreign);
    let cross_project = complete(
        &fixture,
        "client-foreign",
        "src/lib.rs",
        &snapshot.document_id,
        snapshot.revision,
        1,
        8,
        100,
    )
    .await;
    assert_eq!(
        error_code(&cross_project),
        "authorization_denied",
        "a wrong-project read is denied, not NotReady: {cross_project:?}"
    );

    fixture.service.shutdown_all().await;
    let transcript = std::fs::read_to_string(&fixture.transcript).unwrap();
    assert!(
        !transcript.contains("textDocument/completion"),
        "denials and incoherent requests must never query the server: {transcript}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_cross_workspace_attachment_is_not_found() {
    let fixture = fixture(LIB_RS, &scenario(vec![])).await;
    let snapshot = open_lib(&fixture).await;

    // A second workspace with its own document under the same daemon.
    let temp_b = TempDir::new().unwrap();
    let root_b = temp_b.path().to_path_buf();
    std::fs::create_dir_all(root_b.join("src")).unwrap();
    std::fs::write(
        root_b.join("Cargo.toml"),
        "[package]\nname='other'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root_b.join("src/lib.rs"), LIB_RS).unwrap();
    let workspace_b = match local_call(
        &fixture.daemon,
        "ws-b",
        CoreRequest::WorkspaceRegister {
            root: root_b.display().to_string(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("second workspace register failed: {other:?}"),
    };
    let project_b = match local_call(
        &fixture.daemon,
        "proj-b2",
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_b.clone(),
                display_name: "other".to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_owned(),
            },
        },
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("second project register failed: {other:?}"),
    };
    // Opening under B needs a member of B: direct-project reads do not leak
    // across projects even for a member elsewhere.
    let team_b = TeamStore::new(fixture.pool.clone());
    let member_b = human_token(&team_b, "MemberB", "client-member-b").await;
    let principal_b = team_b
        .get_principal(member_b.principal_id())
        .await
        .unwrap()
        .unwrap();
    team_b
        .create_membership(
            &ProjectId::parse(&project_b).expect("second project id parses"),
            &principal_b.id,
            ProjectRole::Contributor,
        )
        .await
        .unwrap();
    register_client(&fixture.daemon, "client-member-b", member_b);
    let doc_b = match call(
        &fixture.daemon,
        "client-member-b",
        CoreRequest::DocumentOpen {
            project_id: project_b.clone(),
            workspace_id: workspace_b.clone(),
            relative_path: "src/lib.rs".to_string(),
            scope: DocumentScope::Workspace,
        },
    )
    .await
    {
        CoreResponse::DocumentSnapshot { snapshot, .. } => snapshot,
        other => panic!("second document open failed: {other:?}"),
    };

    // The member may read session A, but attachment B lives under another
    // workspace: not found, never served across the boundary.
    let cross = complete(
        &fixture,
        MEMBER,
        "src/lib.rs",
        &doc_b.document_id,
        0,
        1,
        8,
        100,
    )
    .await;
    assert_eq!(error_code(&cross), "lsp_read_not_found");
    // Both attachments opened at revision 0 under their own workspaces; the
    // verdict is about the workspace binding, not the content.
    assert_eq!(snapshot.revision, doc_b.revision);

    fixture.service.shutdown_all().await;
}

// ---------------------------------------------------------------------------
// 8. Wire shape: new variant, no silent reinterpretation, loud old daemons
// ---------------------------------------------------------------------------

#[test]
fn the_completion_variant_has_its_own_wire_tag() {
    let request = LspCompletionRequestDto {
        session_id: "session-1".to_string(),
        path: "src/lib.rs".to_string(),
        document_id: "doc-1".to_string(),
        revision: 4,
        line: 1,
        character: 8,
        trigger_character: None,
        max_candidates: 100,
    };
    let wire = serde_json::to_value(&CoreRequest::LspCompletionGet { request }).unwrap();
    assert_eq!(
        wire.get("type").and_then(Value::as_str),
        Some("lsp_completion_get")
    );
}

#[test]
fn unknown_envelope_tags_fail_loudly_which_is_how_old_daemons_reject() {
    // An old daemon's `CoreRequest` enum has no `lsp_completion_get` variant,
    // so the envelope fails to deserialize and the client sees a transport
    // error mapped to `Rejected` — never an empty success. The mechanism is
    // that unknown tags are loud, proven here in both directions.
    let unknown: Result<CoreRequest, _> =
        serde_json::from_str(r#"{"type":"lsp_nope","request":{"session_id":"s"}}"#);
    assert!(
        unknown.is_err(),
        "an unknown request tag must not parse: {unknown:?}"
    );
    let known: Result<CoreRequest, _> = serde_json::from_str(
        r#"{"type":"lsp_completion_get","request":{"session_id":"s","path":"p","document_id":"d","revision":0,"line":0,"character":0,"max_candidates":10}}"#,
    );
    assert!(
        known.is_ok(),
        "the new tag must parse on a new daemon: {known:?}"
    );
}

#[test]
fn unexpected_completion_wire_payloads_fail_loudly() {
    // Unknown status strings are not a status.
    let bad_status: Result<LspCompletionResultDto, _> = serde_json::from_str(
        r#"{"document_id":"doc-1","revision":9,"status":"warming","candidates":[]}"#,
    );
    assert!(bad_status.is_err());
    // A missing identity is not a result.
    let no_identity: Result<LspCompletionResultDto, _> =
        serde_json::from_str(r#"{"revision":9,"status":"ready","candidates":[]}"#);
    assert!(no_identity.is_err());
    // A candidate without applicability is not a candidate.
    let no_applicability: Result<LspCompletionResultDto, _> = serde_json::from_str(
        r#"{"document_id":"doc-1","revision":9,"status":"ready","candidates":[{"label":"x"}]}"#,
    );
    assert!(no_applicability.is_err());
    // A wrongly typed cap is not a request.
    let bad_cap: Result<LspCompletionRequestDto, _> = serde_json::from_str(
        r#"{"session_id":"s","path":"p","document_id":"d","revision":0,"line":0,"character":0,"max_candidates":"many"}"#,
    );
    assert!(bad_cap.is_err());
}
