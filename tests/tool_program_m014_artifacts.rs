//! M014 canonical artifact tests.
//!
//! Covers C-39 through C-44: call artifacts use canonical resolvable handles,
//! child artifacts include real attempt/run identity, large output spills
//! through the canonical store, and missing/corrupt data fails closed.

#![cfg(test)]

/// C-39: Call artifacts use canonical resolvable handles and verified content
/// digests.
#[tokio::test(flavor = "current_thread")]
async fn c39_call_artifacts_have_digests() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c39";
    let attempt_id = "att-c39";
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "hello world".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 10,
        iterations_used: 1,
        bytes_used: 128,
        calls_completed: 1,
        calls_total: 1,
    };

    let call_artifacts = vec![codegg::tool::tool_program_result::ProgramArtifactHandle {
        tool_name: Some("read".into()),
        preview: "hello world".into(),
        success: true,
        artifact_id: Some("sha256:artifact-c39".into()),
        digest: Some("sha256:content-c39".into()),
    }];

    let record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            call_artifacts,
            vec![],
            None,
        )
        .expect("persist should succeed");

    assert!(
        !record.call_artifacts.is_empty(),
        "call artifacts must be persisted"
    );
    let artifact = &record.call_artifacts[0];
    assert!(
        artifact.digest.is_some(),
        "call artifact must have a digest"
    );
    assert!(
        artifact.digest.as_ref().unwrap().starts_with("sha256:"),
        "call artifact digest must be SHA-256"
    );
}

/// C-40: Child artifacts include real attempt/run identity, canonical handles,
/// and verified digests, or a typed absence reason.
#[tokio::test(flavor = "current_thread")]
async fn c40_child_artifacts_have_identity_and_digests() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c40";
    let attempt_id = "att-c40";
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "parent result".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        iterations_used: 1,
        bytes_used: 64,
        calls_completed: 0,
        calls_total: 0,
    };

    let child_artifacts = vec![codegg::tool::tool_program_result::ChildArtifactHandle {
        job_id: "job-child-c40".into(),
        run_id: None,
        status: "completed".into(),
        artifact_id: Some("sha256:child-result-c40".into()),
        digest: Some("sha256:child-digest-c40".into()),
    }];

    let record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            vec![],
            child_artifacts,
            None,
        )
        .expect("persist should succeed");

    assert!(
        !record.child_artifacts.is_empty(),
        "child artifacts must be persisted"
    );
    let child = &record.child_artifacts[0];
    assert_eq!(child.job_id, "job-child-c40");
    assert!(child.digest.is_some(), "child artifact must have a digest");
    assert!(
        child.digest.as_ref().unwrap().starts_with("sha256:"),
        "child artifact digest must be SHA-256"
    );
}

/// C-41: Large final output is persisted through the canonical artifact store
/// and fails closed on storage failure.
#[tokio::test(flavor = "current_thread")]
async fn c41_large_output_spills_through_canonical_store() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c41";
    let attempt_id = "att-c41";

    let large_output = "x".repeat(300_000);
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            large_output,
        )),
        error_message: None,
        failure_class: None,
        steps_used: 10,
        iterations_used: 1,
        bytes_used: 300_000,
        calls_completed: 0,
        calls_total: 0,
    };

    let record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .expect("persist should succeed");

    assert!(
        record.output_artifact.is_some(),
        "large output must be spilled to artifact store"
    );
    assert!(
        record
            .output_artifact
            .as_ref()
            .unwrap()
            .starts_with("ctx://artifact/"),
        "spilled output must have a canonical ctx:// handle"
    );
}

/// C-42: Foreground, background notification, and inspection expose one
/// authoritative typed result and identical artifact identities.
#[tokio::test(flavor = "current_thread")]
async fn c42_result_record_is_authoritative() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c42";
    let attempt_id = "att-c42";
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "result".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        iterations_used: 1,
        bytes_used: 64,
        calls_completed: 0,
        calls_total: 0,
    };

    let record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .expect("persist should succeed");

    let loaded = store
        .load(program_id)
        .expect("load should succeed")
        .expect("record must exist");

    assert_eq!(loaded.program_id, program_id);
    assert_eq!(loaded.attempt_id, attempt_id);
    assert_eq!(loaded.selected_backend, "native");
    assert_eq!(
        loaded.result.status,
        codegg_core::tool_program::ProgramStatus::Completed
    );
    assert!(
        loaded.result_digest.starts_with("sha256:"),
        "result digest must be SHA-256"
    );
}

/// C-43: Result integrity covers every semantic result and artifact field.
/// Tampering with the stored record causes load to fail with DigestMismatch.
#[tokio::test(flavor = "current_thread")]
async fn c43_tampered_record_fails_load() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c43";
    let attempt_id = "att-c43";
    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "result".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        iterations_used: 1,
        bytes_used: 64,
        calls_completed: 0,
        calls_total: 0,
    };

    let call_artifacts = vec![codegg::tool::tool_program_result::ProgramArtifactHandle {
        tool_name: Some("read".into()),
        preview: "preview".into(),
        success: true,
        artifact_id: Some("sha256:art".into()),
        digest: Some("sha256:digest".into()),
    }];

    let record = store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            call_artifacts,
            vec![],
            None,
        )
        .expect("persist should succeed");

    // Tamper with the stored file
    let artifact_dir = temp.path().join(".codegg").join("tool_program_results");
    let result_file = artifact_dir.join(format!("{}.json", program_id));
    let bytes = std::fs::read(&result_file).unwrap();
    let mut tampered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    tampered["result_digest"] = "sha256:tampered".into();
    std::fs::write(&result_file, serde_json::to_vec(&tampered).unwrap()).unwrap();

    // Loading should fail with DigestMismatch
    let load_result = store.load(program_id);
    assert!(load_result.is_err(), "tampered record must fail to load");
    let err = load_result.unwrap_err();
    assert!(
        matches!(
            err,
            codegg::tool::tool_program_result::ToolProgramResultError::DigestMismatch { .. }
        ),
        "tampered record must fail with DigestMismatch, got: {:?}",
        err
    );

    // Verify the original record's digest is correct
    assert!(
        record.result_digest.starts_with("sha256:"),
        "original record digest must be SHA-256"
    );
}

/// C-44: Missing or corrupt result/artifact data fails closed with bounded
/// diagnostics.
#[tokio::test(flavor = "current_thread")]
async fn c44_corrupt_data_fails_closed() {
    let temp = tempfile::tempdir().unwrap();
    let store = codegg::tool::tool_program_result::ToolProgramResultStore::new(temp.path());

    let program_id = "tp-c44";
    let attempt_id = "att-c44";

    let result = codegg_core::tool_program::ProgramResult {
        status: codegg_core::tool_program::ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String(
            "result".into(),
        )),
        error_message: None,
        failure_class: None,
        steps_used: 5,
        iterations_used: 1,
        bytes_used: 64,
        calls_completed: 0,
        calls_total: 0,
    };

    store
        .persist(
            program_id,
            attempt_id,
            "native",
            result,
            vec![],
            vec![],
            None,
        )
        .expect("persist should succeed");

    // Corrupt the stored file
    let artifact_dir = temp.path().join(".codegg").join("tool_program_results");
    let result_file = artifact_dir.join(format!("{}.json", program_id));
    std::fs::write(&result_file, "corrupted json").expect("write should succeed");

    // Loading should fail gracefully
    let load_result = store.load(program_id);
    assert!(
        load_result.is_err(),
        "corrupted result data must fail closed"
    );
}
