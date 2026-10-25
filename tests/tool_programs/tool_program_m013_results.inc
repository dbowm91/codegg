//! M013 typed result and artifact integrity tests.
//!
//! Covers closure criteria related to F08 / H1-H5:
//! - H1: Result digest authenticates the complete semantic result record.
//! - H2: Real call artifacts are present, bounded, resolvable, digest-verifiable.
//! - H3: Real child artifacts are present, bounded, resolvable, digest-verifiable.
//! - H4: Large final output uses a real artifact handle and bounded projection.
//! - H5: Tampering any semantic field causes digest failure; corruption fails closed.

use codegg::tool::tool_program::ToolProgramTool;
use codegg::tool::tool_program_result::{
    result_to_json, ChildArtifactHandle, ProgramArtifactHandle, ToolProgramResultError,
    ToolProgramResultStore,
};
use codegg::tool::Tool;
use codegg_core::tool_program::{ProgramResult, ProgramStatus};

fn make_completed_result() -> ProgramResult {
    ProgramResult {
        status: ProgramStatus::Completed,
        output: Some(codegg_core::tool_program::ProgramValue::String("ok".into())),
        error_message: None,
        failure_class: None,
        steps_used: 1,
        calls_completed: 1,
        calls_total: 1,
        iterations_used: 0,
        bytes_used: 0,
    }
}

fn call_artifact(success: bool) -> ProgramArtifactHandle {
    ProgramArtifactHandle {
        tool_name: Some("read".into()),
        preview: "preview-text".into(),
        success,
        artifact_id: Some("ctx://artifacts/abc".into()),
        digest: Some("sha256:deadbeef".into()),
        absence_reason: None,
    }
}

fn child_artifact(job_id: &str) -> ChildArtifactHandle {
    ChildArtifactHandle {
        job_id: job_id.to_string(),
        attempt_id: None,
        run_id: None,
        status: "completed".into(),
        artifact_id: Some("ctx://artifacts/child-abc".into()),
        digest: Some("sha256:childbeef".into()),
        absence_reason: None,
    }
}

/// M013 H1: tampering call_artifacts must change the result digest.
#[tokio::test(flavor = "current_thread")]
async fn h1_tampering_call_artifacts_changes_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let program_id = "tp-m013-h1-call";
    let attempt_id = "attempt-1";

    let result = make_completed_result();
    let _record_a = store
        .persist(
            program_id,
            attempt_id,
            "native_only",
            result.clone(),
            vec![call_artifact(true)],
            vec![],
            None,
        )
        .expect("persist");

    // Reload, tamper with call_artifacts (append a fake one), then save and reload.
    let mut loaded = store.load(program_id).expect("load").expect("present");
    loaded.call_artifacts.push(ProgramArtifactHandle {
        tool_name: Some("injected".into()),
        preview: "tampered".into(),
        success: false,
        artifact_id: Some("ctx://artifacts/tampered".into()),
        digest: Some("sha256:tampered".into()),
        absence_reason: None,
    });
    let tampered_json = serde_json::to_string(&loaded).expect("serialize tampered");
    let path = temp
        .path()
        .join(".codegg")
        .join("tool_program_results")
        .join(format!("{program_id}.json"));
    std::fs::write(&path, tampered_json).expect("write tampered");

    let reload = store.load(program_id);
    let tamper_check = match reload {
        Ok(Some(_)) => Err("reload returned Ok(Some) — digest was not verified".to_string()),
        Ok(None) => Err("reload returned Ok(None) — corrupt record vanished".to_string()),
        Err(ToolProgramResultError::DigestMismatch { .. }) => Ok(()),
        Err(other) => Err(format!("unexpected error: {other:?}")),
    };
    tamper_check.expect("tampered call_artifacts must fail digest verification");
}

/// M013 H1: tampering child_artifacts must change the result digest.
#[tokio::test(flavor = "current_thread")]
async fn h1_tampering_child_artifacts_changes_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let program_id = "tp-m013-h1-child";
    let attempt_id = "attempt-1";

    let result = make_completed_result();
    store
        .persist(
            program_id,
            attempt_id,
            "native_only",
            result.clone(),
            vec![],
            vec![child_artifact("job-1")],
            None,
        )
        .expect("persist");

    let mut loaded = store.load(program_id).expect("load").expect("present");
    loaded.child_artifacts.push(child_artifact("job-injected"));
    let tampered_json = serde_json::to_string(&loaded).expect("serialize tampered");
    let path = temp
        .path()
        .join(".codegg")
        .join("tool_program_results")
        .join(format!("{program_id}.json"));
    std::fs::write(&path, tampered_json).expect("write tampered");

    let reload = store.load(program_id);
    let tamper_check = match reload {
        Ok(Some(_)) => Err("reload returned Ok(Some) — digest was not verified".to_string()),
        Ok(None) => Err("reload returned Ok(None) — corrupt record vanished".to_string()),
        Err(ToolProgramResultError::DigestMismatch { .. }) => Ok(()),
        Err(other) => Err(format!("unexpected error: {other:?}")),
    };
    tamper_check.expect("tampered child_artifacts must fail digest verification");
}

/// M013 H1: tampering output_artifact must change the result digest.
#[tokio::test(flavor = "current_thread")]
async fn h1_tampering_output_artifact_changes_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let program_id = "tp-m013-h1-output";
    let attempt_id = "attempt-1";

    let result = make_completed_result();
    store
        .persist(
            program_id,
            attempt_id,
            "native_only",
            result.clone(),
            vec![],
            vec![],
            Some("ctx://artifacts/output-original".into()),
        )
        .expect("persist");

    let mut loaded = store.load(program_id).expect("load").expect("present");
    loaded.output_artifact = Some("ctx://artifacts/output-tampered".into());
    let tampered_json = serde_json::to_string(&loaded).expect("serialize tampered");
    let path = temp
        .path()
        .join(".codegg")
        .join("tool_program_results")
        .join(format!("{program_id}.json"));
    std::fs::write(&path, tampered_json).expect("write tampered");

    let reload = store.load(program_id);
    let tamper_check = match reload {
        Ok(Some(_)) => Err("reload returned Ok(Some) — digest was not verified".to_string()),
        Ok(None) => Err("reload returned Ok(None) — corrupt record vanished".to_string()),
        Err(ToolProgramResultError::DigestMismatch { .. }) => Ok(()),
        Err(other) => Err(format!("unexpected error: {other:?}")),
    };
    tamper_check.expect("tampered output_artifact must fail digest verification");
}

/// M013 H2: call artifacts survive round trip with the recorded digest.
#[tokio::test(flavor = "current_thread")]
async fn h2_call_artifact_round_trip_preserves_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let program_id = "tp-m013-h2-round-trip";
    let attempt_id = "attempt-1";

    let result = make_completed_result();
    let artifact = call_artifact(true);
    let record = store
        .persist(
            program_id,
            attempt_id,
            "native_only",
            result,
            vec![artifact.clone()],
            vec![],
            None,
        )
        .expect("persist");

    let loaded = store.load(program_id).expect("load").expect("present");
    assert_eq!(loaded.call_artifacts.len(), 1);
    assert_eq!(loaded.call_artifacts[0].digest, artifact.digest);
    assert_eq!(loaded.call_artifacts[0].artifact_id, artifact.artifact_id);
    assert_eq!(loaded.call_artifacts[0].preview, artifact.preview);
    assert_eq!(loaded.call_artifacts[0].tool_name, artifact.tool_name);
    assert_eq!(loaded.call_artifacts[0].success, artifact.success);
    assert_eq!(loaded.result_digest, record.result_digest);
}

/// M013 H3: child artifacts survive round trip.
#[tokio::test(flavor = "current_thread")]
async fn h3_child_artifact_round_trip_preserves_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let program_id = "tp-m013-h3-round-trip";
    let attempt_id = "attempt-1";

    let result = make_completed_result();
    let artifact = child_artifact("job-child-42");
    let record = store
        .persist(
            program_id,
            attempt_id,
            "native_only",
            result,
            vec![],
            vec![artifact.clone()],
            None,
        )
        .expect("persist");

    let loaded = store.load(program_id).expect("load").expect("present");
    assert_eq!(loaded.child_artifacts.len(), 1);
    assert_eq!(loaded.child_artifacts[0].job_id, artifact.job_id);
    assert_eq!(loaded.child_artifacts[0].digest, artifact.digest);
    assert_eq!(loaded.result_digest, record.result_digest);
}

/// M013 H1: digest differs when only the result payload changes.
#[tokio::test(flavor = "current_thread")]
async fn h1_result_payload_change_changes_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let program_id = "tp-m013-h1-payload";
    let attempt_id = "attempt-1";

    let result_a = make_completed_result();
    let record_a = store
        .persist(
            program_id,
            attempt_id,
            "native_only",
            result_a,
            vec![],
            vec![],
            None,
        )
        .expect("persist a");

    let mut result_b = make_completed_result();
    result_b.output = Some(codegg_core::tool_program::ProgramValue::String(
        "different".into(),
    ));
    let record_b = store
        .persist(
            program_id,
            attempt_id,
            "native_only",
            result_b,
            vec![],
            vec![],
            None,
        )
        .expect("persist b");

    assert_ne!(
        record_a.result_digest, record_b.result_digest,
        "different output must change the result digest"
    );
}

/// M013 H5: selected_backend tampering fails digest verification.
#[tokio::test(flavor = "current_thread")]
async fn h1_selected_backend_tampering_changes_digest() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    let program_id = "tp-m013-h1-backend";
    let attempt_id = "attempt-1";

    let result = make_completed_result();
    store
        .persist(
            program_id,
            attempt_id,
            "native_only",
            result,
            vec![],
            vec![],
            None,
        )
        .expect("persist");

    let mut loaded = store.load(program_id).expect("load").expect("present");
    loaded.selected_backend = "hosted".into();
    let tampered_json = serde_json::to_string(&loaded).expect("serialize tampered");
    let path = temp
        .path()
        .join(".codegg")
        .join("tool_program_results")
        .join(format!("{program_id}.json"));
    std::fs::write(&path, tampered_json).expect("write tampered");

    let reload = store.load(program_id);
    let tamper_check = match reload {
        Ok(Some(_)) => Err("reload returned Ok(Some) — digest was not verified".to_string()),
        Ok(None) => Err("reload returned Ok(None) — corrupt record vanished".to_string()),
        Err(ToolProgramResultError::DigestMismatch { .. }) => Ok(()),
        Err(other) => Err(format!("unexpected error: {other:?}")),
    };
    tamper_check.expect("tampered selected_backend must fail digest verification");
}

/// Present-keys-only JSON-Schema check mirroring the broker validator
/// (`src/tool/broker.rs::validate_json_schema`): object/array/scalar
/// types, `properties`, `required`, and `enum`. Absent keys are never
/// failures, which is why omitting a missing `artifact_handle` validates
/// while emitting `null` does not.
fn check_schema_against_value(schema: &serde_json::Value, value: &serde_json::Value, path: &str) {
    if let Some(expected) = schema.get("type").and_then(|kind| kind.as_str()) {
        let valid = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => true,
        };
        assert!(valid, "{path}: expected JSON type {expected}, got {value}");
    }
    if let Some(allowed) = schema.get("enum").and_then(|values| values.as_array()) {
        assert!(
            allowed.iter().any(|candidate| candidate == value),
            "{path}: value {value} is not in the allowed enum"
        );
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(|keys| keys.as_array()) {
            for key in required.iter().filter_map(|key| key.as_str()) {
                assert!(
                    object.contains_key(key),
                    "{path}: missing required property {key}"
                );
            }
        }
        if let Some(properties) = schema.get("properties").and_then(|props| props.as_object()) {
            for (key, child_schema) in properties {
                if let Some(child) = object.get(key) {
                    check_schema_against_value(child_schema, child, &format!("{path}.{key}"));
                }
            }
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array()) {
        for (index, item) in values.iter().enumerate() {
            check_schema_against_value(items, item, &format!("{path}[{index}]"));
        }
    }
}

/// F1: a small-output program (no artifact handle persisted) must project
/// to JSON that validates against the `tool_program` output schema.
#[test]
fn small_output_result_projection_passes_output_schema() {
    let temp = tempfile::tempdir().unwrap();
    let store = ToolProgramResultStore::new(temp.path());
    // Small output: no artifact persisted, so no handle and no digest —
    // only the absence reason, exactly as the executor records it.
    let small_output_call = ProgramArtifactHandle {
        tool_name: Some("read".into()),
        preview: "small output".into(),
        success: true,
        artifact_id: None,
        digest: None,
        absence_reason: Some("nested call output remained below artifact threshold".into()),
    };
    let record = store
        .persist(
            "tp-m013-f1-small-output",
            "attempt-1",
            "native_only",
            make_completed_result(),
            vec![small_output_call],
            vec![],
            None,
        )
        .expect("persist");
    let projected = result_to_json(&record);
    let artifacts = projected["program_artifacts"]
        .as_array()
        .expect("program_artifacts must be an array");
    assert_eq!(artifacts.len(), 1);
    assert!(
        artifacts[0].get("artifact_handle").is_none(),
        "missing handle must be omitted, not null"
    );
    let tool = ToolProgramTool::new();
    let contract = tool.contract("tool_program", tool.parameters());
    let schema = contract.output_schema.expect("tool_program output schema");
    check_schema_against_value(&schema, &projected, "$");
}
