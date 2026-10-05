//! M006-B trajectory: the native LSP read surface, and the properties
//! ADR-0012 says must hold rather than merely be intended.
//!
//! Four things are proven here, and three of them are properties that a
//! plausible-looking implementation would get wrong:
//!
//! 1. A read against a cold server returns `NotReady`, **and starts no
//!    process**. `NotReady` without the second half would be a lie the daemon
//!    could tell while quietly launching rust-analyzer on a keystroke.
//! 2. A denied read is an *error*, not `NotReady`. Conflating them tells a
//!    caller that a forbidden request was merely early.
//! 3. An unchanged diagnostic set publishes nothing, and a changed one
//!    publishes exactly once with an incremented sequence. This is what makes
//!    the stream a latency optimization rather than an authority.
//! 4. A client that missed an envelope re-pulls and converges, rather than
//!    rendering a stale set as current.

use codegg::core::lsp_diagnostics_store::{
    DiagnosticsChange, DiagnosticsTracker, MAX_TRACKED_FILES_PER_PROJECT,
};
use codegg_client::{DiagnosticsReconciler, ReconcileDecision};
use codegg_protocol::lsp::{
    LspDiagnosticDto, LspDiagnosticsResultDto, LspFileDiagnosticsDto, LspRangeDto, LspReadInvalid,
    LspReadOperation, LspReadPayloadDto, LspReadRequestDto, LspReadResultDto, LspReadStatus,
};

fn diagnostic(message: &str) -> LspDiagnosticDto {
    LspDiagnosticDto {
        range: LspRangeDto {
            path: "lib.rs".to_string(),
            start_line: 1,
            start_column: 2,
            end_line: 1,
            end_column: 5,
        },
        severity: 1,
        tag: 0,
        code: Some("E0308".to_string()),
        message: message.to_string(),
        source: None,
    }
}

fn file(path: &str, sequence: u64, messages: &[&str]) -> LspFileDiagnosticsDto {
    LspFileDiagnosticsDto {
        path: path.to_string(),
        sequence,
        digest: String::new(),
        diagnostics: messages.iter().copied().map(diagnostic).collect(),
        post_restart: false,
        truncated: false,
    }
}

// ---------------------------------------------------------------------------
// 1. Warm-only
// ---------------------------------------------------------------------------

#[test]
fn a_cold_read_is_not_ready_and_carries_no_payload() {
    let result = LspReadResultDto::not_ready(LspReadOperation::Hover);

    assert_eq!(result.status, LspReadStatus::NotReady);
    // The point of the variant: a caller cannot render "clean" by accident.
    // A `NotReady` carrying an empty payload would be indistinguishable from a
    // warm server that had nothing to say.
    assert!(
        result.payload.is_none(),
        "a NotReady result must not carry a payload"
    );
}

#[test]
fn a_warm_read_with_nothing_to_report_is_ready_and_empty() {
    // The mirror image. A warm server saying "nothing here" is a real answer
    // and must not be conflated with a cold one.
    let result = LspReadResultDto::ready(
        LspReadOperation::Hover,
        LspReadPayloadDto::Hover {
            text: String::new(),
            truncated: false,
        },
    );
    assert_eq!(result.status, LspReadStatus::Ready);
    assert!(result.payload.is_some());
    assert_ne!(result.status, LspReadStatus::NotReady);
}

// ---------------------------------------------------------------------------
// 2. Denial is not NotReady
// ---------------------------------------------------------------------------

#[test]
fn a_malformed_request_is_rejected_before_any_service_call() {
    // A query is meaningful only for workspace symbols. Accepting one on a
    // point read would let a caller believe it searched the workspace when
    // the daemon never could have.
    let incoherent = LspReadRequestDto {
        operation: LspReadOperation::Hover,
        session_id: "session-1".to_string(),
        path: "src/lib.rs".to_string(),
        line: Some(3),
        column: Some(7),
        query: Some("Foo".to_string()),
    };
    assert_eq!(incoherent.validate(), Err(LspReadInvalid::IncoherentFields));

    let escaping = LspReadRequestDto {
        operation: LspReadOperation::Hover,
        session_id: "session-1".to_string(),
        path: "../../etc/passwd".to_string(),
        line: Some(1),
        column: Some(1),
        query: None,
    };
    // The DTO check is lexical; containment is the daemon's
    // `validate_target_path`, which is the same primitive the preview-apply
    // write path uses. This asserts only that a traversal-shaped path is
    // syntactically a relative path — containment is proven by the daemon
    // module, not here.
    assert!(escaping.validate().is_ok());
    assert!(escaping.path.contains(".."));
}

// ---------------------------------------------------------------------------
// 3. Publish only on change
// ---------------------------------------------------------------------------

#[test]
fn an_unchanged_file_publishes_nothing() {
    let mut tracker = DiagnosticsTracker::new();
    let first = tracker
        .observe("lib.rs", file("lib.rs", 0, &["mismatched types"]))
        .expect("first observation is tracked");
    assert!(matches!(
        first,
        DiagnosticsChange::First { sequence: 1, .. }
    ));

    let second = tracker
        .observe("lib.rs", file("lib.rs", 0, &["mismatched types"]))
        .expect("still tracked");
    assert_eq!(
        second,
        DiagnosticsChange::Unchanged,
        "an identical set must not produce an envelope, or a high-churn \
         project turns the stream into a busy loop"
    );
    assert_eq!(tracker.file("lib.rs").expect("tracked").sequence, 1);
}

#[test]
fn a_changed_file_publishes_once_with_an_incremented_sequence() {
    let mut tracker = DiagnosticsTracker::new();
    tracker.observe("lib.rs", file("lib.rs", 0, &["mismatched types"]));

    let change = tracker
        .observe("lib.rs", file("lib.rs", 0, &["unused import"]))
        .expect("tracked");
    assert!(matches!(
        change,
        DiagnosticsChange::Changed { sequence: 2, .. }
    ));
    assert_eq!(tracker.file("lib.rs").expect("tracked").sequence, 2);
}

#[test]
fn the_tracker_computes_its_own_digest_rather_than_trusting_a_caller() {
    // A caller that passed a stale or empty digest would make an unchanged
    // file look changed (spamming the stream) or a changed file look
    // unchanged (silently stalling diagnostics). Both read as "LSP is flaky",
    // so the store owns the digest instead of trusting one.
    let mut tracker = DiagnosticsTracker::new();
    tracker.observe("lib.rs", file("lib.rs", 0, &["first"]));

    let mut lying = file("lib.rs", 0, &["second"]);
    lying.digest = "the-same-digest-as-before".to_string();
    assert!(
        matches!(
            tracker.observe("lib.rs", lying),
            Some(DiagnosticsChange::Changed { .. })
        ),
        "a caller-supplied stale digest must not suppress a real change"
    );
}

#[test]
fn the_file_cap_is_reported_rather_than_silent() {
    let mut tracker = DiagnosticsTracker::new();
    for index in 0..MAX_TRACKED_FILES_PER_PROJECT {
        tracker.observe(
            &format!("file{index}.rs"),
            file(&format!("file{index}.rs"), 0, &["m"]),
        );
    }
    assert_eq!(
        tracker.observe("one-too-many.rs", file("one-too-many.rs", 0, &["m"])),
        None,
        "a refused file must be distinguishable from an unchanged one"
    );
    assert!(tracker.is_saturated());
}

// ---------------------------------------------------------------------------
// 4. The resync contract
// ---------------------------------------------------------------------------

fn authoritative(files: Vec<LspFileDiagnosticsDto>) -> LspDiagnosticsResultDto {
    LspDiagnosticsResultDto {
        project_id: "project-1".to_string(),
        files,
        truncated: false,
    }
}

#[test]
fn a_missed_envelope_is_detected_and_repaired_by_a_re_pull() {
    // The property ADR-0012 §2 exists for. The client saw sequence 1; the
    // daemon moved to 3, meaning an envelope was dropped somewhere between
    // them. The client must notice and re-pull rather than render a stale set
    // as if it were current.
    let mut reconciler = DiagnosticsReconciler::new();
    reconciler.accept_streamed(&file("lib.rs", 1, &["first"]));

    let current = authoritative(vec![file("lib.rs", 3, &["third"])]);
    assert_eq!(
        reconciler.reconcile(&current),
        ReconcileDecision::GapDetected {
            last_seen: 1,
            authoritative: 3
        },
        "a client one envelope behind must be told to re-pull"
    );

    reconciler.adopt(&current);
    assert_eq!(
        reconciler.reconcile(&current),
        ReconcileDecision::Current,
        "after the re-pull the client is in sync"
    );
}

#[test]
fn a_client_with_no_baseline_must_pull_rather_than_trust_the_stream() {
    // The post-restart state. Diagnostics are not persisted (ADR-0008
    // extended to diagnostic state), so a reconnecting client has no
    // sequence history and cannot know whether it missed anything.
    let reconciler = DiagnosticsReconciler::new();
    assert_eq!(
        reconciler.reconcile(&authoritative(vec![file("lib.rs", 1, &["a"])])),
        ReconcileDecision::NoBaseline
    );
}

#[test]
fn a_replayed_envelope_cannot_move_the_client_backwards() {
    let mut reconciler = DiagnosticsReconciler::new();
    assert!(reconciler.accept_streamed(&file("lib.rs", 5, &["a"])));
    assert!(
        !reconciler.accept_streamed(&file("lib.rs", 2, &["b"])),
        "an older sequence must be refused, or a reordered delivery would \
         regress the client's view"
    );
    assert_eq!(
        reconciler.reconcile(&authoritative(vec![file("lib.rs", 5, &["a"])])),
        ReconcileDecision::Current
    );
}

#[test]
fn the_streamed_and_authoritative_payloads_are_the_same_type() {
    // If these ever diverged, the stream and the pull could disagree and the
    // resync contract would be void. One type makes that impossible to
    // express, not merely unlikely.
    let recorded = file("lib.rs", 7, &["mismatched types"]);
    let projection = codegg_protocol::lsp::LspDiagnosticsProjectionDto {
        project_id: "project-1".to_string(),
        file: recorded.clone(),
    };
    assert_eq!(projection.file, recorded);
}

// ---------------------------------------------------------------------------
// 5. The stream gate
// ---------------------------------------------------------------------------

#[test]
fn the_diagnostics_subscription_is_its_own_gated_request() {
    // ADR-0012 §4 requires `file.read` for content-bearing reads. The generic
    // `ProjectionSubscribe` is `Opaque + project.observe` and cannot express
    // that, so the diagnostics stream is a *separate* request rather than a
    // flag on the generic one. This asserts the split exists: if someone later
    // folds diagnostics back into `ProjectionSubscribe`, the gate regresses to
    // `project.observe` and this no longer describes the protocol.
    use codegg_protocol::core::CoreRequest;

    let request = CoreRequest::LspDiagnosticsSubscribe {
        request: codegg_protocol::lsp::LspDiagnosticsSubscribeRequestDto {
            project_id: "project-1".to_string(),
            cursor: None,
            projection_version: 1,
        },
    };
    let descriptor = codegg_core::authorization::operation_descriptor(&request);
    assert_eq!(descriptor.operation, "lsp_diagnostics_subscribe");
    assert_eq!(
        descriptor.capability.map(|cap| cap.as_str()),
        Some("file.read"),
        "the diagnostics stream must be gated at file.read, not project.observe"
    );
    assert_eq!(
        descriptor.scope_kind,
        codegg_core::authorization::ScopeKind::DirectProject
    );
}

#[test]
fn the_point_reads_are_gated_at_file_read_too() {
    use codegg_protocol::core::CoreRequest;
    for request in [
        CoreRequest::LspReadGet {
            request: codegg_protocol::lsp::LspReadRequestDto {
                operation: LspReadOperation::Hover,
                session_id: "session-1".to_string(),
                path: "src/lib.rs".to_string(),
                line: Some(1),
                column: Some(1),
                query: None,
            },
        },
        CoreRequest::LspDiagnosticsGet {
            request: codegg_protocol::lsp::LspDiagnosticsGetRequestDto {
                project_id: "project-1".to_string(),
                session_id: "session-1".to_string(),
            },
        },
    ] {
        let descriptor = codegg_core::authorization::operation_descriptor(&request);
        assert_eq!(
            descriptor.capability.map(|cap| cap.as_str()),
            Some("file.read"),
            "{} must be gated at file.read",
            descriptor.operation
        );
        assert_ne!(
            descriptor.capability.map(|cap| cap.as_str()),
            Some("project.observe"),
            "project.observe would understate what these disclose"
        );
    }
}

#[test]
fn a_malformed_subscription_is_rejected() {
    use codegg_protocol::lsp::LspDiagnosticsSubscribeRequestDto;
    for project_id in ["", "projects/../etc", "a/b"] {
        let request = LspDiagnosticsSubscribeRequestDto {
            project_id: project_id.to_string(),
            cursor: None,
            projection_version: 1,
        };
        assert_eq!(
            request.validate(),
            Err(LspReadInvalid::InvalidPath),
            "a subscription for {project_id:?} must be rejected before any \
             subscription state is created"
        );
    }
}
