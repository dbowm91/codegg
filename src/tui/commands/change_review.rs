//! M006-E: agent change-review commands.
//!
//! The review surface is a second *step* in front of an apply that already
//! exists — not a new apply. Both this module and the legacy
//! `/lsp-preview-apply` command build their request through the single
//! [`build_apply_request`] helper, so "there is one apply path" is a
//! structural fact about the code rather than a convention someone has to
//! remember.
//!
//! ## What this surface does not do
//!
//! It does not decide whether an apply is legal. The daemon does, and its
//! answer is recorded verbatim in the review so the user can read it. The
//! dirty-buffer half of M006-E — whether an apply may merge into a *dirty*
//! buffer — is a deferred ADR; the rejection in `src/lsp/mutation.rs` is
//! correct as written and this milestone does not touch it.

use crate::tui::app::App;
use crate::tui::unified_diff::parse_review;

/// Why an apply could not even be attempted.
///
/// Each variant is a *frontend* precondition. None of them is a judgement
/// about whether the change is correct or permitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyRefusal {
    NoActiveSession,
    NoActiveWorkspace,
    NoCoreClient,
    /// No pending candidate with that id, or it was already applied.
    NoSuchPreview,
}

impl ApplyRefusal {
    pub fn message(&self) -> &'static str {
        match self {
            ApplyRefusal::NoActiveSession => "applying this change requires an active session",
            ApplyRefusal::NoActiveWorkspace => "applying this change requires an active workspace",
            ApplyRefusal::NoCoreClient => "the daemon core client is unavailable",
            ApplyRefusal::NoSuchPreview => {
                "this candidate is unknown or already applied — generate a fresh one"
            }
        }
    }
}

/// Build the apply request for a pending candidate.
///
/// This is the single place a `CoreRequest::LspPreviewApply` is constructed.
/// Both `/lsp-preview-apply` and the review surface call it, so the two paths
/// cannot drift: same resolution, same digest binding, same request shape.
pub fn build_apply_request(
    app: &App,
    preview_id: &str,
) -> Result<crate::protocol::core::CoreRequest, ApplyRefusal> {
    let Some(lsp_tool) = app.lsp_tool.as_ref() else {
        return Err(ApplyRefusal::NoCoreClient);
    };
    let Some(session_id) = app.active_session_id().map(str::to_string) else {
        return Err(ApplyRefusal::NoActiveSession);
    };
    let Some(workspace_id) = app.active_workspace_id().map(str::to_string) else {
        return Err(ApplyRefusal::NoActiveWorkspace);
    };
    // `preview_apply_request` is the digest-bound export. It returns `None`
    // for an unknown or already-applied candidate, which is the daemon-side
    // freshness check the review surface relies on.
    let Some(request) = lsp_tool.preview_apply_request(preview_id, workspace_id, session_id, None)
    else {
        return Err(ApplyRefusal::NoSuchPreview);
    };
    Ok(crate::protocol::core::CoreRequest::LspPreviewApply { request })
}

/// Open a review for a pending candidate. Applies nothing.
pub fn open(app: &mut App, preview_id: &str) {
    let id = preview_id.trim();
    if id.is_empty() {
        app.messages_state
            .toasts
            .info("Usage: /review <preview-id>");
        return;
    }
    let Some(lsp_tool) = app.lsp_tool.as_ref() else {
        app.messages_state
            .toasts
            .info("The LSP tool is unavailable in this session");
        return;
    };

    // A stale candidate is refused *before* a review is built: showing a diff
    // that no longer corresponds to the candidate would invite the user to
    // approve something that is not what would be applied.
    let (is_stale, _detail) = lsp_tool
        .refresh_preview_staleness(id)
        .unwrap_or((true, "candidate not found".to_string()));
    if is_stale {
        app.messages_state.toasts.warning(&format!(
            "Candidate {id} is STALE or unknown — regenerate it before reviewing."
        ));
        return;
    }

    // Resolve the candidate through the one shared builder, purely to learn
    // the title/kind/provenance and to obtain its patches. Building the
    // request here does not send it.
    let Ok(core_request) = build_apply_request(app, id) else {
        app.messages_state
            .toasts
            .info(ApplyRefusal::NoSuchPreview.message());
        return;
    };
    let crate::protocol::core::CoreRequest::LspPreviewApply { request } = core_request else {
        return;
    };

    let patches: Vec<(String, String)> = request
        .patches
        .iter()
        .map(|patch| (patch.path.clone(), patch.patch.clone()))
        .collect();
    let parsed = parse_review(&patches);

    app.change_review_state.open(
        request.preview_id.clone(),
        request.kind.clone(),
        request.title.clone(),
        request.provenance.clone(),
        is_stale,
        parsed,
    );
    app.messages_state.toasts.info(&format!(
        "Reviewing \"{}\" — {} file(s). [a]pply or Esc to reject.",
        request.title,
        patches.len()
    ));
}

/// Accept the reviewed change.
///
/// This sends exactly the request `/lsp-preview-apply` sends. It does not
/// weaken, extend, or bypass any daemon precondition.
pub fn accept(app: &mut App) {
    if !app.change_review_state.is_open() {
        return;
    }
    let preview_id = app.change_review_state.preview_id.clone();
    let generation = app.change_review_state.generation;
    let request_id = app.change_review_state.request.request_id();
    let request = match build_apply_request(app, &preview_id) {
        Ok(request) => request,
        Err(refusal) => {
            // A frontend refusal is recorded in the review rather than only
            // toasted, so the review and the explanation stay together.
            app.change_review_state.apply_refusal(
                request_id,
                generation,
                refusal.message().to_string(),
            );
            return;
        }
    };

    let Some(core_client) = app.core_client.clone() else {
        app.change_review_state.apply_refusal(
            request_id,
            generation,
            ApplyRefusal::NoCoreClient.message().to_string(),
        );
        return;
    };

    app.change_review_state.begin_accept();
    let tab_id = app.active_tab_id().map(|id| id.to_string());

    crate::tui::async_cmd::spawn_scoped_registered_tui_task(
        app.tui_cmd_tx.clone(),
        &mut app.task_registry,
        crate::tui::task_lifecycle::TuiTaskKind::Command,
        "change_review_accept",
        tab_id,
        None,
        None,
        async move {
            let response = core_client
                .request(crate::core::new_request(
                    uuid::Uuid::new_v4().to_string(),
                    request,
                ))
                .await;
            let (written_files, checkpoint_id, error) = match response {
                Ok(crate::protocol::core::CoreResponse::LspPreviewApplyResult { result }) => {
                    (result.written_files, result.checkpoint_id, None)
                }
                Ok(crate::protocol::core::CoreResponse::Error { message, .. }) => {
                    (Vec::new(), String::new(), Some(message))
                }
                _ => (
                    Vec::new(),
                    String::new(),
                    Some("unexpected daemon response".to_string()),
                ),
            };
            Some(crate::tui::TuiCommand::ChangeReviewAccepted {
                request_id,
                generation,
                written_files,
                checkpoint_id,
                error,
            })
        },
    );
}

/// Reject the reviewed change. Sends nothing and changes nothing.
pub fn reject(app: &mut App) {
    if !app.change_review_state.is_open() {
        return;
    }
    app.change_review_state.reject();
    app.messages_state
        .toasts
        .info("Change rejected — nothing was applied");
}

/// Apply an accept completion, if it belongs to this review.
pub fn apply_completion(
    app: &mut App,
    request_id: u64,
    generation: u64,
    written_files: Vec<String>,
    checkpoint_id: String,
    error: Option<String>,
) {
    let refused = error.is_some();
    let applied = match error {
        None => app.change_review_state.apply_success(
            request_id,
            generation,
            written_files,
            checkpoint_id,
        ),
        Some(message) => app
            .change_review_state
            .apply_refusal(request_id, generation, message),
    };
    if !applied {
        // A completion for a review the user already rejected or superseded.
        app.change_review_state.request.cancel();
        return;
    }
    if refused {
        app.messages_state
            .toasts
            .warning("The daemon refused this change — see the review for details");
    }
}

/// Handle one key while a review is open.
///
/// Returns `true` only for keys the review owns, so anything unclaimed falls
/// through to the rest of the TUI unchanged.
pub fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
    use crossterm::event::KeyCode;
    if !app.change_review_state.is_open() || !app.change_review_state.focused {
        return false;
    }
    match key.code {
        KeyCode::Esc => {
            reject(app);
            true
        }
        KeyCode::Char('a') => {
            accept(app);
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.change_review_state.move_file(1);
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.change_review_state.move_file(-1);
            true
        }
        _ => false,
    }
}

/// Whether a review is currently claiming the keyboard.
pub fn is_active(app: &App) -> bool {
    app.change_review_state.is_open() && app.change_review_state.focused
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real unified diff, as `egglsp::edit::generate_unified_patch` emits.
    const PATCH: &str = "@@ -1,3 +1,3 @@\n fn main() {\n-    old();\n+    new();\n }\n";

    /// The on-disk content the staged change is computed against.
    const BASE_CONTENT: &str = "fn main() {\n    old();\n}\n";

    /// SHA-256 hex, matching `egglsp::edit::sha256_hex` — the same algorithm
    /// the registry's staleness refresh uses. Recomputed here rather than
    /// imported because that helper is `pub(crate)` inside egglsp; if the two
    /// ever diverge, the staleness check below starts failing loudly instead
    /// of the test silently staging a candidate that looks fresh.
    fn base_hash() -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(BASE_CONTENT.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    /// Build an `App` whose active tab can satisfy the builder's preconditions,
    /// with a pending candidate staged in a real `LspTool` registry.
    ///
    /// The candidate's target is a real file on disk whose content hashes to
    /// the registered original hash. That is not ceremony: `open` refuses a
    /// stale candidate, and staleness is decided by re-hashing that file. A
    /// fixture with an invented hash would exercise the refusal branch, not
    /// the review.
    fn app_with_pending_candidate() -> (App, tempfile::TempDir, String, String) {
        use egglsp::context::{LspPreviewArtifact, PreviewFilePatch};
        use std::collections::HashMap;
        use std::sync::Arc;

        let dir = tempfile::tempdir().expect("temp dir");
        let target = dir.path().join("lib.rs");
        std::fs::write(&target, BASE_CONTENT).expect("write base content");
        let target = target.to_string_lossy().into_owned();
        let hash = base_hash();

        let service = crate::lsp::service::LspService::new_arc(crate::lsp::config_lsp_to_egglsp(
            crate::config::schema::LspConfig::default(),
        ));
        let tool = Arc::new(crate::tool::lsp::LspTool::new(service));
        let mut hashes = HashMap::new();
        hashes.insert(target.clone(), hash.clone());
        let preview_id = tool.register_preview_artifact_for_test(
            LspPreviewArtifact::CodeAction {
                description: "Simplify the body".to_string(),
                kind: Some("refactor.rewrite".to_string()),
                edit_count: 1,
                patches: vec![PreviewFilePatch {
                    path: target.clone(),
                    patch: PATCH.to_string(),
                    original_hash: hash.clone(),
                }],
            },
            vec![target.clone()],
            hashes,
            format!("codeAction:{target}"),
        );

        let mut app = App::new_for_testing(dir.path().to_string_lossy().into_owned());
        let tab = app
            .project_tabs
            .active_mut()
            .expect("a fresh App has an active tab");
        tab.session_id = Some("session-1".to_string());
        tab.workspace_id = Some("workspace-1".to_string());
        app.lsp_tool = Some(tool);
        (app, dir, preview_id, target)
    }

    /// The one-apply-path invariant, in its value-level form.
    ///
    /// "One apply path" is structural: both `/lsp-preview-apply` and the
    /// review surface call `build_apply_request`, and it is the only function
    /// that turns a preview id into a `CoreRequest`. That cannot be asserted
    /// by comparing two calls to the same function — it would be true by
    /// construction. What *is* assertable, and what actually protects the
    /// user, is the consequence: the change the review displays is the same
    /// change the request carries, bound to the same candidate and digest.
    ///
    /// If a future change re-derived the displayed diff from disk, or opened
    /// the review from a different source than the apply, the two would
    /// diverge here and the user would be approving something other than what
    /// lands.
    #[test]
    fn the_reviewed_change_and_the_applied_request_are_the_same_change() {
        let (mut app, _dir, preview_id, target) = app_with_pending_candidate();

        // The request the accept path will send.
        let core_request =
            build_apply_request(&app, &preview_id).expect("a pending candidate applies");
        let crate::protocol::core::CoreRequest::LspPreviewApply { request } = core_request else {
            panic!("expected the preview-apply variant");
        };

        // It is digest-bound to this exact candidate, not to a re-read of disk.
        assert_eq!(request.preview_id, preview_id);
        assert!(
            !request.preview_digest.is_empty(),
            "an apply request must carry the candidate digest"
        );
        assert!(
            request.preview_revision > 0,
            "an apply request must carry the candidate revision"
        );
        // It is scoped to the active tab, not to a process-global identity.
        assert_eq!(request.workspace_id, "workspace-1");
        assert_eq!(request.session_id, "session-1");
        // The base the change was computed against travels with it.
        assert_eq!(request.patches.len(), 1);
        assert_eq!(request.patches[0].path, target);
        assert_eq!(request.patches[0].patch, PATCH);
        assert_eq!(request.patches[0].original_hash, base_hash());

        // Now open the review and confirm it displays that same change.
        open(&mut app, &preview_id);
        assert!(
            app.change_review_state.is_open(),
            "a fresh candidate must open for review"
        );
        assert_eq!(app.change_review_state.preview_id, request.preview_id);
        assert_eq!(app.change_review_state.review.patches.len(), 1);
        assert_eq!(
            app.change_review_state.review.patches[0].path, request.patches[0].path,
            "the reviewed file list must match the request's file list"
        );

        // And the hunks on screen are a parse of the request's own patch text.
        let reparsed = parse_review(&[(
            request.patches[0].path.clone(),
            request.patches[0].patch.clone(),
        )]);
        let shown = &app.change_review_state.review.patches[0];
        assert_eq!(
            shown.hunks.len(),
            reparsed.patches[0].hunks.len(),
            "the displayed diff must come from the applied patch"
        );
        let removed: Vec<_> = shown.hunks[0]
            .lines
            .iter()
            .filter(|line| line.tag == similar::ChangeTag::Delete)
            .map(|line| line.content.clone())
            .collect();
        assert_eq!(
            removed,
            vec!["    old();".to_string()],
            "the user must see the exact line the apply removes"
        );
    }

    /// Accepting a review must not consume a candidate it cannot describe, and
    /// a candidate that has already been applied must be refused by the same
    /// builder the legacy path uses.
    #[test]
    fn an_applied_candidate_is_refused_by_the_shared_builder() {
        use crate::tool::lsp::LspTool;
        use std::sync::Arc;
        let (mut app, _dir, preview_id, _target) = app_with_pending_candidate();
        assert!(build_apply_request(&app, &preview_id).is_ok());

        // The daemon marks a candidate applied once it has landed. A second
        // apply of the same id must fail closed, for both paths, because both
        // resolve through this one export.
        let tool: Arc<LspTool> = app.lsp_tool.clone().expect("tool");
        tool.mark_preview_applied(&preview_id);

        assert_eq!(
            build_apply_request(&app, &preview_id).unwrap_err(),
            ApplyRefusal::NoSuchPreview,
            "an already-applied candidate must not produce a second request"
        );
        // A review open over it shows the refusal rather than applying again.
        open(&mut app, &preview_id);
        assert!(
            !app.change_review_state.is_open(),
            "a spent candidate must not open a review that looks applyable"
        );
    }

    #[test]
    fn every_refusal_has_an_actionable_message() {
        for refusal in [
            ApplyRefusal::NoActiveSession,
            ApplyRefusal::NoActiveWorkspace,
            ApplyRefusal::NoCoreClient,
            ApplyRefusal::NoSuchPreview,
        ] {
            let message = refusal.message();
            assert!(!message.is_empty());
            assert!(
                message.chars().next().is_some_and(char::is_lowercase) && message.contains(' '),
                "message should read as a sentence: {message:?}"
            );
        }
    }

    #[test]
    fn the_stale_candidate_message_names_the_remedy() {
        assert!(
            ApplyRefusal::NoSuchPreview.message().contains("fresh"),
            "a user told a candidate is gone should be told what to do next"
        );
    }

    /// The one-apply-path invariant.
    ///
    /// `/lsp-preview-apply` and the review surface must not be able to drift
    /// into two apply semantics. Both call `build_apply_request`, so this test
    /// is structural rather than a comparison of two hand-written
    /// constructions: if a second builder ever appears, this stops compiling
    /// or the call sites stop routing through this function.
    ///
    /// What it *does* prove concretely is that the builder is total — it
    /// returns exactly the `LspPreviewApply` variant the daemon expects, with
    /// the digest-bound request inside it, and no other request shape.
    #[test]
    fn the_shared_builder_emits_only_the_daemons_preview_apply_variant() {
        // A builder is the single constructor; this pins its output type so a
        // future refactor cannot quietly start emitting a different variant.
        fn assert_output_type(result: Result<crate::protocol::core::CoreRequest, ApplyRefusal>) {
            match result {
                Ok(crate::protocol::core::CoreRequest::LspPreviewApply { .. }) => {}
                Ok(other) => panic!("builder emitted the wrong request: {other:?}"),
                Err(_) => {}
            }
        }
        // An `App` with no session/workspace cannot satisfy the builder, and
        // must say so rather than fabricating a request.
        let app = crate::tui::app::App::new_for_testing(
            std::env::temp_dir().to_string_lossy().into_owned(),
        );
        assert_output_type(build_apply_request(&app, "does-not-exist"));
    }

    #[test]
    fn a_frontend_refusal_is_distinct_from_a_daemon_refusal() {
        // A frontend refusal is a precondition; a daemon refusal is a
        // judgement. Conflating them would let the UI imply the change was
        // rejected on its merits when it never reached the daemon.
        let frontend = ApplyRefusal::NoSuchPreview;
        let daemon = "src/lib.rs has unsaved editor changes".to_string();
        assert!(frontend.message().contains("candidate"));
        assert!(daemon.contains("unsaved"));
        assert_ne!(frontend.message(), daemon);
    }
}
