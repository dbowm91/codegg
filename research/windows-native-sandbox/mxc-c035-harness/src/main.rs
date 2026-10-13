#[cfg(windows)]
use mxc_sdk::v1::configs::{CaptureDenials, CaptureDenialsMode, ProcessContainerConfig};
#[cfg(windows)]
use mxc_sdk::v1::policy::FilesystemPolicy;
#[cfg(windows)]
use mxc_sdk::v1::{self, ContainerRequest};
use serde_json::json;
#[cfg(windows)]
use serde_json::Value;
use serde_json::Value as JsonValue;
use std::fs;
#[cfg(windows)]
use std::io::{Seek, SeekFrom};
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::io::AsRawHandle;

#[cfg(windows)]
fn make_inheritable(file: &fs::File) -> std::io::Result<()> {
    use windows::Win32::Foundation::{SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT};
    let handle = HANDLE(file.as_raw_handle() as *mut std::ffi::c_void);
    unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT.0, HANDLE_FLAG_INHERIT) }
        .map_err(std::io::Error::other)
}

#[cfg(windows)]
fn read_child_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    if bytes.len() % 2 != 0 {
        return None;
    }
    let words = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect::<Vec<_>>();
    serde_json::from_str(&String::from_utf16(&words).ok()?).ok()
}

#[cfg(windows)]
fn sha256(path: &Path) -> String {
    Command::new("certutil.exe")
        .args(["-hashfile", path.to_string_lossy().as_ref(), "SHA256"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .nth(1)
                .unwrap_or("unavailable")
                .replace(' ', "")
        })
        .unwrap_or_else(|| "unavailable".into())
}

fn main() {
    if !cfg!(windows) {
        println!(
            "C035_REPORT={}",
            json!({"schema":"codegg.mxc_c035.v1","qualified":false,"reason":"Windows-only"})
        );
        return;
    }
    #[cfg(windows)]
    run();
}

fn evidence_matches(
    evidence: Option<&JsonValue>,
    nonce: &str,
    pid: Option<&JsonValue>,
    field: &str,
    expected: &str,
) -> bool {
    evidence.is_some_and(|value| {
        value["nonce"] == nonce
            && value[field] == expected
            && value["pid"].as_u64().is_some_and(|observed| observed > 0)
            && pid.is_none_or(|expected_pid| value["pid"] == *expected_pid)
    })
}

#[cfg(windows)]
fn run() {
    let backends = v1::available_backends();
    let helper = std::env::var_os("C035_NATIVE_HELPER").map(PathBuf::from);
    let temp = std::env::var_os("RUNNER_TEMP").map(PathBuf::from);
    let (Some(helper), Some(temp)) = (helper, temp) else {
        report(
            json!({"schema":"codegg.mxc_c035.v1","qualified":false,"gate":"setup","reason":"helper or RUNNER_TEMP missing"}),
        );
        return;
    };
    let nonce = format!(
        "{:x}-{:x}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let root = temp.join(format!("codegg-c035-{nonce}"));
    let stage = root.join("stage");
    let capture = root.join("capture");
    let host = root.join("host");
    if let Err(error) = fs::create_dir_all(&stage)
        .and_then(|_| fs::create_dir_all(&capture))
        .and_then(|_| fs::create_dir_all(&host))
        .and_then(|_| fs::create_dir_all(stage.join("localappdata")))
        .and_then(|_| fs::create_dir_all(stage.join("appdata")))
        .and_then(|_| fs::create_dir_all(stage.join("profile")))
    {
        report(
            json!({"schema":"codegg.mxc_c035.v1","qualified":false,"gate":"setup","reason":error.to_string()}),
        );
        return;
    }
    let early_entry = root.join("early-exit-entry.json");
    let early_exit_regression_passed = Command::new(&helper)
        .arg("--exit-before-entry")
        .output()
        .is_ok_and(|output| output.status.code() == Some(23) && !early_entry.exists());
    let secret = host.join("synthetic-c035-secret.txt");
    if let Err(error) = fs::write(&secret, b"synthetic-c035-secret") {
        report(
            json!({"schema":"codegg.mxc_c035.v1","qualified":false,"gate":"fixture","reason":error.to_string()}),
        );
        let _ = fs::remove_dir_all(&root);
        return;
    }
    let acl_before = Command::new("icacls.exe")
        .arg(&secret)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let grant = Command::new("icacls.exe")
        .args([
            secret.to_string_lossy().as_ref(),
            "/grant",
            "*S-1-1-0:R",
            "*S-1-5-32-545:R",
            "*S-1-5-11:R",
        ])
        .output();
    let acl_granted = grant.is_ok_and(|o| o.status.success());
    let acl_at_launch = Command::new("icacls.exe")
        .arg(&secret)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let secret_hash_before = sha256(&secret);
    let mut file = match fs::File::open(&secret) {
        Ok(file) => file,
        Err(error) => {
            report(
                json!({"schema":"codegg.mxc_c035.v1","qualified":false,"gate":"fixture","reason":error.to_string()}),
            );
            let _ = fs::remove_dir_all(&root);
            return;
        }
    };
    if let Err(error) = make_inheritable(&file) {
        report(
            json!({"schema":"codegg.mxc_c035.v1","qualified":false,"gate":"fixture","reason":error.to_string()}),
        );
        let _ = fs::remove_dir_all(&root);
        return;
    }
    let entry_control = root.join("control-entry.json");
    let proof_control = root.join("control-proof.json");
    let control = Command::new(&helper)
        .args([
            "--control",
            secret.to_string_lossy().as_ref(),
            helper.to_string_lossy().as_ref(),
            entry_control.to_string_lossy().as_ref(),
            proof_control.to_string_lossy().as_ref(),
            nonce.as_str(),
        ])
        .output();
    let control_status = control
        .as_ref()
        .ok()
        .map(|o| o.status.to_string())
        .unwrap_or_else(|| "launch-error".into());
    let control_entry = read_child_json(&entry_control);
    let control_proof = read_child_json(&proof_control);
    let control_read_valid = control.as_ref().is_ok_and(|output| output.status.success())
        && evidence_matches(control_entry.as_ref(), &nonce, None, "phase", "entry")
        && control_proof.as_ref().is_some_and(|proof| {
            evidence_matches(
                Some(proof),
                &nonce,
                control_entry.as_ref().map(|entry| &entry["pid"]),
                "marker",
                "child-proof",
            ) && proof["secret_match"] == true
                && proof["host_path_read_ok"] == true
                && proof["host_path_secret_match"] == true
                && proof["handle_inheritable"] == true
        });
    let _ = file.seek(SeekFrom::Start(0));
    let handle_value = file.as_raw_handle() as usize;
    let child_entry = stage.join("mxc-entry.json");
    let child_proof = stage.join(format!("child-proof-{nonce}.json"));
    let stage_was_absent_before = !child_entry.exists() && !child_proof.exists();
    let mut readonly =
        vec![std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()) + r"\System32"];
    readonly.push(helper.to_string_lossy().into_owned());
    let policy = FilesystemPolicy {
        readwrite_paths: vec![stage.to_string_lossy().into_owned()],
        readonly_paths: readonly,
        denied_paths: vec![host.to_string_lossy().into_owned()],
        clear_policy_on_exit: Some(true),
    };
    let command = format!(
        "\"{}\" --child \"{}\" \"{}\" {} {} \"{}\"",
        helper.display(),
        child_entry.display(),
        child_proof.display(),
        nonce,
        handle_value,
        secret.display()
    );
    let capture_path = capture.join(format!("mxc-{nonce}")).join("denials.json");
    let _ = fs::create_dir_all(capture_path.parent().unwrap());
    let mut denial_capture = CaptureDenials::default();
    denial_capture.mode = CaptureDenialsMode::Block;
    denial_capture.output_path = Some(capture_path.to_string_lossy().into_owned());
    denial_capture.retain_etl = false;
    let mut container = ProcessContainerConfig::default();
    container.capture_denials = Some(denial_capture);
    let request = ContainerRequest {
        containment: v1::Containment::ProcessContainer(container),
        filesystem: Some(policy),
        working_directory: Some(stage.to_string_lossy().into_owned()),
        timeout_ms: Some(20_000),
        environment: Some(vec![
            (
                "SystemRoot".into(),
                std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()),
            ),
            (
                "C035_NATIVE_HELPER".into(),
                helper.to_string_lossy().into_owned(),
            ),
            (
                "USERPROFILE".into(),
                stage.join("profile").to_string_lossy().into_owned(),
            ),
            (
                "LOCALAPPDATA".into(),
                stage.join("localappdata").to_string_lossy().into_owned(),
            ),
            (
                "APPDATA".into(),
                stage.join("appdata").to_string_lossy().into_owned(),
            ),
            (
                "SystemDrive".into(),
                std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into()),
            ),
            ("TEMP".into(), stage.to_string_lossy().into_owned()),
            ("TMP".into(), stage.to_string_lossy().into_owned()),
        ]),
        inherit_default_environment: Some(false),
        ..ContainerRequest::new(command)
    };
    let execution = v1::run(request, Default::default());
    let (mxc_outcome, mxc_stdout, mxc_stderr) = match execution {
        Ok(result) => (
            format!("{:?}", result.outcome),
            String::from_utf8_lossy(&result.stdout).into_owned(),
            String::from_utf8_lossy(&result.stderr).into_owned(),
        ),
        Err(error) => (format!("error: {error}"), String::new(), String::new()),
    };
    let mxc_entry = read_child_json(&child_entry);
    let mxc_proof = read_child_json(&child_proof);
    let mxc_entry_valid = evidence_matches(mxc_entry.as_ref(), &nonce, None, "phase", "entry");
    let mxc_proof_valid = mxc_proof.as_ref().is_some_and(|proof| {
        evidence_matches(
            Some(proof),
            &nonce,
            mxc_entry.as_ref().map(|entry| &entry["pid"]),
            "marker",
            "child-proof",
        )
    });
    let denial_bytes = fs::read(&capture_path).ok();
    let denial_doc: Option<Value> = denial_bytes
        .as_deref()
        .and_then(|b| serde_json::from_slice(b).ok());
    let acl_after = Command::new("icacls.exe")
        .arg(&secret)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    let secret_hash_after = sha256(&secret);
    let baseline_ok = std::env::var("C035_C033_BASELINE_OK").as_deref() == Ok("True");
    let gate_b_passed = control_read_valid
        && mxc_proof_valid
        && mxc_proof.as_ref().is_some_and(|proof| {
            proof["secret_match"] == false
                && proof["probe_exception"] == 0
                && proof["host_path_read_ok"] == false
                && proof["host_path_read_error"] == 5
                && proof["host_path_secret_match"] == false
                && (proof["handle_present"] == false
                    || (proof["handle_inheritable"] == true && proof["read_error"] == 5))
        });
    let gate_c_passed = stage_was_absent_before
        && mxc_outcome == "Exited(0)"
        && mxc_proof_valid
        && mxc_proof.as_ref().is_some_and(|proof| {
            proof["token_observed"] == true
                && proof["group_sids"].is_array()
                && proof["integrity_sid"].is_string()
                && proof["token_type"].is_number()
        });
    let gate_d_passed = denial_doc.is_some();
    let qualified = evidence_matches(control_entry.as_ref(), &nonce, None, "phase", "entry")
        && mxc_entry_valid
        && gate_b_passed
        && gate_c_passed
        && gate_d_passed
        && early_exit_regression_passed
        && baseline_ok
        && secret_hash_before == secret_hash_after
        && acl_granted
        && acl_at_launch == acl_after;
    let secret_acl_stable_during_mxc = acl_at_launch == acl_after;
    let failed_gates = [
        (!mxc_entry_valid).then_some("A-entry"),
        (!gate_b_passed).then_some("B-handle-boundary"),
        (!gate_c_passed).then_some("C-stage-token"),
        (!gate_d_passed).then_some("D-denial-capture"),
        (!early_exit_regression_passed).then_some("pre-entry-regression"),
        (!baseline_ok).then_some("C033-baseline"),
        (!secret_acl_stable_during_mxc || secret_hash_before != secret_hash_after)
            .then_some("fixture-integrity"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    let report = json!({
        "schema":"codegg.mxc_c035.v1", "qualified":qualified, "qualification":"test-only evidence; never production admission",
        "run_nonce":nonce, "host_architecture":format!("{:?}", std::env::consts::ARCH), "backends":format!("{backends:?}"),
        "helper_sha256":sha256(&helper), "helper_path":"redacted", "secret_sha256_before":secret_hash_before,
        "secret_sha256_after":secret_hash_after, "synthetic_acl_grant_succeeded":acl_granted,
        "synthetic_acl_before_grant":acl_before, "synthetic_acl_at_launch":acl_at_launch, "synthetic_acl_after":acl_after,
        "synthetic_acl_stable_during_mxc":secret_acl_stable_during_mxc,
        "control_status":control_status.clone(), "control_entry":control_entry.clone(), "control_proof":control_proof.clone(), "control_read_valid":control_read_valid,
        "pre_entry_nonzero_is_not_denial":early_exit_regression_passed,
        "c033_baseline_passed":baseline_ok,
        "mxc_outcome":mxc_outcome.clone(), "requested_backend":"ProcessContainer", "mxc_entry":mxc_entry.clone(), "mxc_proof":mxc_proof.clone(),
        "stage_was_absent_before":stage_was_absent_before,
        "mxc_stdout":mxc_stdout, "mxc_stderr":mxc_stderr,
        "mxc_denial_capture_present":denial_doc.is_some(), "mxc_denial_document":denial_doc,
        "gate_a": if mxc_entry_valid {"child-entered"} else {"failed-or-unavailable"},
        "gate_b": if !control_read_valid {"invalid-uncontained-positive-control"} else if mxc_proof.as_ref().is_some_and(|p| p["secret_match"] == true || p["host_path_secret_match"] == true) {"security-failure-secret-readable"} else if mxc_proof.as_ref().is_some_and(|p| p["host_path_read_error"] == 5 && p["probe_exception"] != 0) {"host-path-denied; inherited-handle-probe-exception-unqualified"} else if gate_b_passed && mxc_proof.as_ref().is_some_and(|p| p["handle_present"] == false) {"running-child-attempted-but-handle-not-present"} else if gate_b_passed {"running-child-attempted-and-access-denied"} else {"failed-or-unavailable"},
        "gate_c": if gate_c_passed {"stage-proof-and-token-evidence-present"} else if mxc_proof_valid {"stage-proof-present; token-evidence-incomplete"} else {"failed-or-unavailable"},
        "gate_d": if denial_doc.is_some() {"per-invocation-capture-present"} else {"capture-missing"},
        "failed_gates": failed_gates,
        "unknowns":["bounded descendant Job cleanup is not independently observable in the MXC SDK", "C033's shell/Python/Cargo smoke remains separately evidenced by the same-run baseline step"],
        "network":"not tested"
    });
    println!(
        "C035_CONTROLS={}",
        json!({"status":control_status,"entry":control_entry,"proof":control_proof})
    );
    println!(
        "C035_MXC={}",
        json!({"outcome":mxc_outcome,"entry":mxc_entry,"proof":mxc_proof})
    );
    println!(
        "C035_HANDLES={}",
        json!({"control":control_proof,"mxc":mxc_proof})
    );
    println!("C035_STAGE={}", child_proof.display());
    println!("C035_IDENTITY={}", mxc_proof.as_ref().map(|p| json!({"user":p["token_user"],"groups_count":p["group_count"],"integrity":p["integrity_sid"],"complete":false})).unwrap_or(Value::Null));
    println!("C035_REPORT={report}");
    if let Some(path) = std::env::var_os("C035_REPORT_JSON") {
        let _ = fs::write(path, serde_json::to_vec_pretty(&report).unwrap_or_default());
    }
    let _ = file;
    let _ = fs::remove_dir_all(root);
}

#[cfg(windows)]
fn report(value: Value) {
    println!("C035_REPORT={value}");
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn child_evidence_requires_entry_nonce_and_pid_correlation() {
        assert!(!super::evidence_matches(None, "n", None, "phase", "entry"));
        assert!(!super::evidence_matches(
            Some(&json!({"nonce":"wrong","pid":1,"phase":"entry"})),
            "n",
            None,
            "phase",
            "entry"
        ));
        assert!(!super::evidence_matches(
            Some(&json!({"nonce":"n","pid":2,"marker":"child-proof"})),
            "n",
            Some(&json!(1)),
            "marker",
            "child-proof"
        ));
        assert!(super::evidence_matches(
            Some(&json!({"nonce":"n","pid":1,"marker":"child-proof"})),
            "n",
            Some(&json!(1)),
            "marker",
            "child-proof"
        ));
    }
}
