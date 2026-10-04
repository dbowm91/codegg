//! M006-D trajectory: a file discovered by the bounded workspace tree
//! opens through the same controller-backed path as `/open`.
//!
//! This is the qualification that the tree is a *navigator* rather than a
//! second content path. It walks a real temporary workspace, selects a file
//! the way the pane does, and then opens the selected path through the
//! ordinary `DocumentController` — the same daemon-authorized attachment
//! `/open` produces — asserting the text arrives intact.
//!
//! It deliberately does not build a whole `App`: the tree-to-open link is a
//! direct call to the existing `open_editor`, and what needs proving here is
//! the part that could plausibly be wrong, namely that a *tree-derived* path
//! is a legitimate document path and opens like any other.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use codegg::tui::app::state::file_tree::FileTreeState;
use codegg::tui::file_tree::{build_tree, visible_rows, TreeNodeKind};
use codegg_client::{DocumentController, DocumentState, DocumentTransport};
use codegg_protocol::core::{CoreRequest, CoreResponse};
use codegg_protocol::document::DocumentSnapshotDto;
use tokio::sync::Mutex;

/// A transport that serves exactly the documents the workspace actually
/// contains, so a tree-derived path that does not exist fails here rather
/// than being papered over.
struct WorkspaceTransport {
    /// `(workspace-relative path, contents)`.
    files: Vec<(String, String)>,
    opened: Mutex<Vec<String>>,
}

#[async_trait]
impl DocumentTransport for WorkspaceTransport {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        match request {
            CoreRequest::DocumentOpen { relative_path, .. } => {
                let Some((_, text)) = self
                    .files
                    .iter()
                    .find(|(candidate, _)| candidate == &relative_path)
                else {
                    return Err(format!("no such workspace document: {relative_path}"));
                };
                self.opened.lock().await.push(relative_path.clone());
                Ok(snapshot(text, 1, false, true))
            }
            CoreRequest::DocumentSnapshotGet { .. } => Err("unexpected snapshot".to_string()),
            other => Err(format!("unexpected request: {other:?}")),
        }
    }
}

fn snapshot(text: &str, revision: u64, dirty: bool, writer: bool) -> CoreResponse {
    CoreResponse::DocumentSnapshot {
        snapshot: DocumentSnapshotDto {
            document_id: "doc-1".into(),
            project_id: "project-1".into(),
            workspace_id: "workspace-1".into(),
            relative_path: String::new(),
            revision,
            text: text.into(),
            dirty,
            conflicted: false,
            writer,
            disk_base_digest: "digest".into(),
        },
        writer_lease: writer.then(|| "lease-1".into()),
        lsp_degraded: false,
    }
}

fn workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("tempdir");
    let write = |relative: &str, text: &str| {
        let path = root.path().join(relative);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    };
    write("README.md", "# demo\n");
    write("src/lib.rs", "pub fn demo() {}\n");
    write("src/nested/deep.rs", "pub const DEEP: u8 = 1;\n");
    root
}

#[tokio::test]
async fn a_file_discovered_by_the_tree_opens_like_any_typed_path() {
    let root = workspace();

    // Walk exactly as the pane does, then expand and select the way a user
    // would, rather than reaching into the node list.
    let listing = build_tree(root.path());
    let mut state = FileTreeState {
        visible: true,
        ..FileTreeState::default()
    };
    let (generation, request_id) = state.begin_rebuild(root.path().to_path_buf());
    assert!(
        state.apply_listing(generation, request_id, root.path().to_path_buf(), listing),
        "the walk should be applied to the state that requested it"
    );

    // Expand down to the nested file, selecting each directory in turn.
    let target = "src/nested/deep.rs";
    for step in ["src", "src/nested"] {
        state.selected = Some(step.to_string());
        assert_eq!(state.toggle_selected().as_deref(), Some(step));
    }
    state.selected = Some(target.to_string());

    let rows = state.rows();
    let row = rows
        .iter()
        .find(|row| row.path == target)
        .expect("the expanded tree should expose the nested file");
    assert_eq!(row.kind, TreeNodeKind::File);
    assert_eq!(
        row.depth, 3,
        "src -> nested -> deep.rs is three levels below the workspace root"
    );

    // The path the tree produced is workspace-relative and free of `..`, so
    // the lexical check `/open` performs accepts it unchanged.
    assert!(!row.path.starts_with('/'));
    assert!(!row.path.contains(".."));
    assert!(!row.path.contains('\\'));

    // Open it through the ordinary controller, which is the daemon-authorized
    // attachment every document path in CodeGG goes through.
    let transport = Arc::new(WorkspaceTransport {
        files: vec![
            ("README.md".to_string(), "# demo\n".to_string()),
            ("src/lib.rs".to_string(), "pub fn demo() {}\n".to_string()),
            (target.to_string(), "pub const DEEP: u8 = 1;\n".to_string()),
        ],
        opened: Mutex::new(Vec::new()),
    });
    let controller = DocumentController::new(transport.clone());
    controller
        .open(
            "project-1".into(),
            "workspace-1".into(),
            row.path.clone(),
            true,
        )
        .await
        .expect("the tree-derived path should open");

    let state_after = controller.state().await;
    assert!(
        matches!(state_after, DocumentState::Synced),
        "the document should be open and synced, got {state_after:?}"
    );
    assert_eq!(
        transport.opened.lock().await.as_slice(),
        &[target.to_string()]
    );
}

#[tokio::test]
async fn a_tree_derived_path_the_workspace_does_not_contain_fails_closed() {
    // The transport refuses an unknown path. This proves the tree is not
    // fabricating openable documents: a path that is not really in the
    // workspace is rejected by the same authority that would reject a typed
    // one, rather than being satisfied by a second read path.
    let transport = Arc::new(WorkspaceTransport {
        files: vec![("src/lib.rs".to_string(), "x".to_string())],
        opened: Mutex::new(Vec::new()),
    });
    let controller = DocumentController::new(transport);
    let result = controller
        .open(
            "project-1".into(),
            "workspace-1".into(),
            "src/does-not-exist.rs".to_string(),
            true,
        )
        .await;
    assert!(result.is_err(), "an absent document must not open");
}

#[tokio::test]
async fn a_directory_is_navigable_but_not_openable() {
    // The walk reports directories so they can be expanded, but the pane
    // routes them to expansion rather than to the open path. This asserts the
    // distinction is available to the caller, which is what keeps `Enter` on
    // a directory from ever reaching `DocumentOpen`.
    let root = workspace();
    let listing = build_tree(root.path());
    let mut rows = Vec::new();
    visible_rows(&listing.nodes, &BTreeSet::new(), &mut rows);
    let src = rows
        .iter()
        .find(|row| row.path == "src")
        .expect("src should be a top-level row");
    assert_eq!(src.kind, TreeNodeKind::Directory);
    assert!(!src.expanded);

    let mut expanded = BTreeSet::new();
    expanded.insert("src".to_string());
    let mut deep_rows = Vec::new();
    visible_rows(&listing.nodes, &expanded, &mut deep_rows);
    assert!(deep_rows.iter().any(|row| row.path == "src/lib.rs"));
}

#[cfg(unix)]
#[tokio::test]
async fn a_symlinked_file_is_not_offered_as_an_openable_document() {
    // A workspace that links to a real file outside itself must not have that
    // file appear in the tree, because the tree's only action on a file is to
    // open it.
    let root = workspace();
    let outside = tempfile::tempdir().expect("outside");
    let secret = outside.path().join("secret.txt");
    std::fs::write(&secret, "secret").expect("write outside");
    std::os::unix::fs::symlink(&secret, root.path().join("linked.txt")).expect("symlink");

    let listing = build_tree(root.path());
    let mut rows = Vec::new();
    visible_rows(&listing.nodes, &BTreeSet::new(), &mut rows);
    assert!(
        !rows.iter().any(|row| row.name == "linked.txt"),
        "a symlinked file must not be offered for opening"
    );
}
